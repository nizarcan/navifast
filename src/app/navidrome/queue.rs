//! Queue commands keep the shown order authoritative while album reads finish.

use super::{App, Request, player};
use crate::navidrome::{
    api::{Album, Song},
    queue::{Advance, Id},
};

#[derive(Clone, Debug)]
pub enum Action {
    Songs {
        songs: Vec<Song>,
        next: bool,
    },
    Album {
        id: String,
        name: String,
        next: bool,
    },
    Play(Id),
    Remove(Id),
    Move {
        id: Id,
        delta: isize,
    },
    Clear,
}

impl App {
    pub(crate) fn queue_item(&self, uri: &str, next: bool) -> Option<Action> {
        use super::favorites::Item;
        match self.favorite_item(uri)? {
            Item::Song(song) => Some(Action::Songs {
                songs: vec![song],
                next,
            }),
            Item::Album(album) => Some(Action::Album {
                id: album.id,
                name: album.name,
                next,
            }),
            Item::Artist(_) => None,
        }
    }

    pub(super) fn queue_action(&mut self, action: Action) {
        self.edit_session();
        match action {
            Action::Songs { songs, next } => {
                self.transport.queue.add(songs, next);
                self.resume_queue_wait();
            }
            Action::Album { id, name, next } => {
                let token = self.transport.queue.reserve(id.clone(), name, next);
                self.resume_queue_wait();
                if let Some(client) = &self.client {
                    self.backend.send(Request::QueueAlbum {
                        session: self.session,
                        token,
                        client: client.clone(),
                        id,
                    });
                } else if self.demo {
                    let result = self
                        .albums
                        .iter()
                        .find(|a| a.id == id)
                        .cloned()
                        .ok_or_else(|| "Album unavailable.".into());
                    self.queued_album(token, result);
                }
            }
            Action::Play(id) => {
                if let Some(at) = self
                    .transport
                    .queue
                    .upcoming()
                    .iter()
                    .position(|e| e.id == id)
                {
                    for entry in self
                        .transport
                        .queue
                        .upcoming()
                        .iter()
                        .take(at)
                        .filter(|e| e.pending())
                    {
                        self.backend.send(Request::CancelQueueAlbum(entry.id));
                    }
                }
                if let Some(advance) = self.transport.queue.play(id) {
                    self.advance_queue(advance, false);
                }
            }
            Action::Remove(id) => {
                if self
                    .transport
                    .queue
                    .upcoming()
                    .iter()
                    .any(|entry| entry.id == id && entry.automatic())
                {
                    self.cancel_autoplay(false);
                }
                self.transport.queue.remove(id);
                self.backend.send(Request::CancelQueueAlbum(id));
                self.resume_queue_wait();
            }
            Action::Move { id, delta } => {
                self.transport.queue.move_by(id, delta);
                self.resume_queue_wait();
            }
            Action::Clear => {
                self.cancel_autoplay(true);
                self.backend.send(Request::ClearQueueAlbums);
                self.transport.queue.clear_manual();
                if self.transport.waiting_for_queue {
                    // Clear does not change the current occurrence, even while an
                    // unresolved album was holding Next. Resume its exact position.
                    let paused = self.transport.paused
                        || self
                            .transport
                            .song()
                            .is_some_and(|s| self.transport.position >= s.duration);
                    self.load(self.transport.position, paused);
                }
            }
        }
    }

    pub(super) fn advance_queue(&mut self, advance: Advance, paused: bool) {
        match advance {
            Advance::Ready => {
                if self
                    .transport
                    .queue
                    .current()
                    .is_some_and(|entry| entry.automatic())
                {
                    self.playing_album = None;
                    self.playing_playlist = None;
                }
                self.load(0.0, paused);
            }
            Advance::Pending => {
                self.pending_play = None;
                self.transport.waiting_for_queue = true;
                self.transport.loading = true;
                self.transport.paused = paused;
                self.transport.error = None;
                self.transport.generation = player::next_generation();
                self.transport.information = None;
                self.player.send(player::Command::Stop);
                self.tap.clear();
            }
            Advance::Empty => {
                if self.transport.waiting_for_queue {
                    self.transport.waiting_for_queue = false;
                    self.transport.loading = false;
                    self.transport.paused = true;
                }
            }
        }
    }

    fn resume_queue_wait(&mut self) {
        if self.transport.waiting_for_queue
            || self.autoplay.waiting && !self.transport.queue.upcoming().is_empty()
        {
            self.autoplay.waiting = false;
            let advance = self.transport.queue.advance();
            if advance == Advance::Empty && self.wait_for_autoplay(self.transport.paused) {
                self.transport.waiting_for_queue = false;
            } else {
                self.advance_queue(advance, self.transport.paused);
            }
        }
    }

    pub(super) fn queued_album(&mut self, token: Id, result: Result<Album, String>) {
        let (songs, error) = match result {
            Ok(mut album) => {
                album.song.sort_by_key(|s| (s.disc_number, s.track));
                for song in &mut album.song {
                    if song.cover_art.is_none() {
                        song.cover_art.clone_from(&album.cover_art);
                    }
                }
                let error = album
                    .song
                    .is_empty()
                    .then(|| "This album has no playable songs.".to_owned());
                (album.song, error)
            }
            Err(error) => (Vec::new(), Some(error)),
        };
        if self.transport.queue.resolve(token, songs) {
            self.dirty_session();
            if let Some(error) = error {
                self.presentation.toast_error(error);
            }
            self.resume_queue_wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::{Action as TransportAction, tests::with_app};

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter()
            .map(|id| Song {
                id: (*id).into(),
                duration: 60.0,
                ..Default::default()
            })
            .collect()
    }

    fn finish(app: &mut App) {
        let paused = app.transport.paused;
        if app
            .transport
            .event(player::Event::Finished(app.transport.generation))
        {
            app.finish_track(paused);
        }
    }

    #[test]
    fn playback_modes_preserve_paused_position_and_explicit_next_ignores_repeat_one() {
        use crate::player::RepeatMode;
        with_app(|app, _| {
            app.play_search_songs(songs(&["dup", "dup", "tail"]), 0);
            app.apply(TransportAction::Pause(true));
            app.apply(TransportAction::Seek(21.0));
            let before = (
                app.transport.queue.current().unwrap().id,
                app.transport.generation,
            );
            app.apply(TransportAction::Shuffle(true));
            app.apply(TransportAction::Repeat(RepeatMode::Context));
            assert_eq!(
                (
                    app.transport.queue.current().unwrap().id,
                    app.transport.generation
                ),
                before
            );
            assert_eq!(app.transport.position, 21.0);
            assert!(app.transport.paused);
            app.queue_action(Action::Songs {
                songs: songs(&["manual"]),
                next: false,
            });
            let next = app.transport.queue.upcoming()[0].id;
            app.apply(TransportAction::Repeat(RepeatMode::Track));
            app.apply(TransportAction::Next);
            assert_eq!(app.transport.queue.current().unwrap().id, next);
            assert_eq!(app.transport.song().unwrap().id, "manual");
            assert!(app.transport.paused);
            let generation = app.transport.generation;
            finish(app);
            assert_eq!(app.transport.queue.current().unwrap().id, next);
            assert!(app.transport.generation > generation);
            assert_eq!(app.transport.position, 0.0);
            assert!(app.transport.paused);
            let next = app.transport.queue.upcoming()[0].id;
            app.apply(TransportAction::Repeat(RepeatMode::Off));
            finish(app);
            assert_eq!(app.transport.queue.current().unwrap().id, next);
        });
    }

    #[test]
    fn one_track_repeat_and_control_snapshots_report_the_actual_modes() {
        use crate::player::RepeatMode;
        with_app(|app, host| {
            app.play_search_songs(songs(&["only"]), 0);
            finish(app);
            assert!(app.transport.paused);
            assert_eq!(app.transport.position, 60.0);
            app.apply(TransportAction::Repeat(RepeatMode::Context));
            let next = app.transport.queue.upcoming()[0].id;
            finish(app);
            assert_eq!(app.transport.queue.current().unwrap().id, next);
            assert_eq!(app.transport.position, 0.0);
            assert!(app.transport.paused);
            let current = app.transport.queue.current().unwrap().id;
            app.commands = Some(std::sync::Arc::new(std::sync::Mutex::new(vec![
                crate::single_instance::ControlCommand::SetShuffle(true),
                crate::single_instance::ControlCommand::SetRepeat(RepeatMode::Track),
            ])));
            assert!(app.poll_controls(host));
            assert!(app.transport.queue.shuffled());
            assert_eq!(app.transport.repeat, RepeatMode::Track);
            assert_eq!(app.transport.queue.current().unwrap().id, current);
            app.now_slot = Some(std::sync::Arc::new(std::sync::Mutex::new(String::new())));
            app.sync_media();
            let snapshot = app.now_slot.as_ref().unwrap().lock().unwrap().clone();
            let fields: Vec<_> = snapshot.split('\t').collect();
            assert_eq!(&fields[7..9], ["true", "track"]);
        });
    }

    #[test]
    fn catalogue_queue_actions_do_not_depend_on_a_successful_favorites_read() {
        with_app(|app, _| {
            app.albums.push(Album {
                id: "album".into(),
                name: "Album".into(),
                ..Default::default()
            });
            assert!(!app.presentation.navidrome_favorites.ready);
            assert!(matches!(
                app.queue_item(
                    &crate::navidrome::api::FavoriteKind::Album.uri("album"),
                    false
                ),
                Some(Action::Album { next: false, .. })
            ));
            app.play_search_songs(songs(&["track"]), 0);
            assert!(matches!(
                app.queue_item(
                    &crate::navidrome::api::FavoriteKind::Song.uri("track"),
                    true
                ),
                Some(Action::Songs { next: true, .. })
            ));
        });
    }

    #[test]
    fn paused_reorders_and_skips_follow_the_visible_occurrence() {
        with_app(|app, _| {
            app.play_search_songs(songs(&["a", "a", "tail"]), 0);
            app.apply(TransportAction::Pause(true));
            app.queue_action(Action::Songs {
                songs: songs(&["same", "same"]),
                next: false,
            });
            let second = app.transport.queue.upcoming()[1].id;
            let generation = app.transport.generation;
            app.queue_action(Action::Move {
                id: second,
                delta: -1,
            });
            assert_eq!(app.transport.generation, generation);
            app.apply(TransportAction::Next);
            assert_eq!(app.transport.queue.current().unwrap().id, second);
            assert!(app.transport.paused);
            app.apply(TransportAction::Seek(15.0));
            assert_eq!(app.transport.position, 15.0);
            assert!(app.transport.paused);
            assert!(!app.transport.event(player::Event::Finished(generation)));
            app.queue_action(Action::Clear);
            assert_eq!(app.transport.queue.current().unwrap().id, second);
            assert_eq!(app.transport.queue.upcoming().len(), 2);
        });
    }

    #[test]
    fn next_waits_for_album_order_and_clear_and_signout_reject_late_reads() {
        with_app(|app, _| {
            app.play_search_songs(songs(&["playing", "tail"]), 0);
            app.apply(TransportAction::Seek(12.0));
            let token = app
                .transport
                .queue
                .reserve("album".into(), "Album".into(), false);
            app.queue_action(Action::Songs {
                songs: songs(&["later"]),
                next: false,
            });
            app.apply(TransportAction::Pause(true));
            app.apply(TransportAction::Next);
            assert!(app.transport.waiting_for_queue);
            app.queued_album(
                token,
                Ok(Album {
                    song: songs(&["first", "second"]),
                    ..Default::default()
                }),
            );
            assert_eq!(app.transport.song().unwrap().id, "first");
            assert_eq!(app.transport.queue.current().unwrap().id, token);
            assert!(app.transport.paused);
            let token = app
                .transport
                .queue
                .reserve("album".into(), "Album".into(), true);
            app.apply(TransportAction::Seek(10.0));
            app.apply(TransportAction::Next);
            app.queue_action(Action::Clear);
            assert_eq!(app.transport.song().unwrap().id, "first");
            assert_eq!(app.transport.position, 10.0);
            assert!(app.transport.paused);
            app.queued_album(
                token,
                Ok(Album {
                    song: songs(&["late"]),
                    ..Default::default()
                }),
            );
            assert_eq!(app.transport.queue.upcoming().len(), 1);
            app.apply(TransportAction::SignOut);
            app.queued_album(
                token,
                Ok(Album {
                    song: songs(&["late"]),
                    ..Default::default()
                }),
            );
            assert!(app.transport.queue.upcoming().is_empty());
            assert!(app.transport.song().is_none());
        });
    }

    #[test]
    fn failed_fetch_releases_reserved_head_without_losing_later_additions() {
        with_app(|app, _| {
            app.play_search_songs(songs(&["playing", "tail"]), 0);
            let token = app
                .transport
                .queue
                .reserve("missing".into(), "Missing".into(), false);
            app.queue_action(Action::Songs {
                songs: songs(&["later"]),
                next: false,
            });
            app.apply(TransportAction::Next);
            app.queued_album(token, Err("The album could not be loaded.".into()));
            assert_eq!(app.transport.song().unwrap().id, "later");
            assert!(!app.transport.paused);
            assert!(!app.transport.waiting_for_queue);
            assert_eq!(app.transport.queue.upcoming().len(), 1);
            assert_eq!(app.presentation.toasts.len(), 1);
        });
    }
}
