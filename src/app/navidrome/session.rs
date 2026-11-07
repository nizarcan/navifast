//! Checkpoint on edits/pause/exit and at most once per ten seconds of playback.
use super::{App, Request, Transport};
use crate::navidrome::session::{Restored, Session};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct State {
    token: u64,
    restoring: bool,
    writable: bool,
    dirty: Option<Instant>,
    last_write: Option<Instant>,
    position: f64,
    paused: bool,
}

impl App {
    pub(super) fn session_path(&self) -> std::path::PathBuf {
        self.dirs.state.join("navidrome-session.json")
    }

    pub(super) fn restore_session(&mut self) {
        if self.demo {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        self.listening_session.token = self.listening_session.token.wrapping_add(1);
        self.listening_session.restoring = true;
        self.listening_session.writable = false;
        self.backend.send(Request::RestoreSession {
            session: self.session,
            token: self.listening_session.token,
            path: self.session_path(),
            client: client.clone(),
        });
    }

    /// A new playback/queue intent wins even while its own metadata is loading.
    pub(super) fn edit_session(&mut self) {
        self.cancel_preparation();
        if self.listening_session.restoring {
            self.backend.send(Request::CancelSessionRestore);
        }
        self.listening_session.restoring = false;
        self.listening_session.writable = true;
        self.listening_session.token = self.listening_session.token.wrapping_add(1);
        self.dirty_session();
    }

    pub(super) fn dirty_session(&mut self) {
        self.listening_session
            .dirty
            .get_or_insert_with(Instant::now);
    }

    pub(super) fn forget_session(&mut self) {
        self.backend.send(Request::CancelSessionRestore);
        if !self.demo {
            self.backend.send(Request::SaveSession {
                path: self.session_path(),
                snapshot: None,
            });
        }
        let token = self.listening_session.token.wrapping_add(1);
        self.listening_session = State {
            token,
            ..Default::default()
        };
    }

    pub(super) fn session_restored(
        &mut self,
        session: u64,
        token: u64,
        result: Result<Option<Box<Restored>>, String>,
    ) {
        if session != self.session
            || token != self.listening_session.token
            || !self.listening_session.restoring
        {
            return;
        }
        self.listening_session.restoring = false;
        match result {
            Ok(Some(saved)) => {
                let generation = crate::navidrome::player::next_generation();
                self.transport = Transport {
                    queue: saved.queue,
                    repeat: saved.repeat,
                    collection_rows: saved.rows,
                    position: saved.position,
                    paused: true,
                    generation,
                    ..Default::default()
                };
                self.playing_album = saved.album;
                self.playing_playlist = saved.playlist;
                self.listening_session.writable = true;
                // Prime the decoder paused; no sound resumes until a playback action.
                self.load(self.transport.position, true);
                self.listening_session.paused = true;
                self.dirty_session();
            }
            Ok(None) => self.listening_session.writable = true,
            Err(_) => {
                // Keep the snapshot for another launch rather than save an empty queue.
                self.presentation.toast_error("Your saved queue could not be checked. Start playback or reopen Navifast to try again.");
            }
        }
    }

    fn can_checkpoint_session(&self) -> bool {
        !self.demo
            && self.connected
            && self.client.is_some()
            && self.listening_session.writable
            && !self.listening_session.restoring
            && self.pending_play.is_none()
            && self.presentation.navidrome_search.play_on_load.is_none()
    }

    pub(super) fn checkpoint_delay(&self) -> Option<Duration> {
        if !self.can_checkpoint_session() {
            return None;
        }
        self.listening_session
            .dirty
            .map(|at| Duration::from_millis(500).saturating_sub(at.elapsed()))
    }

    pub(super) fn checkpoint_session(&mut self, force: bool) {
        if !self.can_checkpoint_session() {
            return;
        }
        let state = &self.listening_session;
        let position_changed = (self.transport.position - state.position).abs() >= 0.5;
        let due = force
            || (self.transport.paused && !state.paused)
            || state
                .dirty
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(500))
            || (position_changed
                && state
                    .last_write
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(10)));
        if !due
            || (!force
                && state.dirty.is_none()
                && !position_changed
                && self.transport.paused == state.paused)
        {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        let mut snapshot = Session::new(client.scope.clone(), self.transport.queue.snapshot());
        snapshot.position = self.transport.position;
        snapshot.repeat = self.transport.repeat.api_name().into();
        snapshot.album = self.playing_album.as_ref().map(|a| a.id.clone());
        snapshot.playlist = self.playing_playlist.clone();
        snapshot.rows = self.transport.collection_rows.clone();
        self.backend.send(Request::SaveSession {
            path: self.session_path(),
            snapshot: Some(Box::new(snapshot)),
        });
        self.listening_session.dirty = None;
        self.listening_session.last_write = Some(Instant::now());
        self.listening_session.position = self.transport.position;
        self.listening_session.paused = self.transport.paused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::navidrome::{Action, tests::with_app},
        navidrome::api::{Client, Song},
    };
    fn restored() -> Box<Restored> {
        let mut queue = crate::navidrome::queue::Queue::default();
        queue.start(
            vec![Song {
                id: "saved".into(),
                duration: 90.0,
                ..Default::default()
            }],
            0,
        );
        Box::new(Restored {
            queue,
            position: 23.5,
            repeat: crate::player::RepeatMode::Track,
            album: None,
            playlist: None,
            rows: Vec::new(),
        })
    }
    #[test]
    fn restored_session_stays_paused_and_explicit_play_beats_late_restore() {
        with_app(|app, _| {
            app.listening_session.restoring = true;
            let token = app.listening_session.token;
            app.session_restored(app.session, token, Ok(Some(restored())));
            assert_eq!(app.transport.song().unwrap().id, "saved");
            assert_eq!(app.transport.position, 23.5);
            assert!(app.transport.paused);
            assert_eq!(app.transport.repeat, crate::player::RepeatMode::Track);
            app.listening_session.restoring = true;
            app.apply(Action::Play(1));
            let playing = app.transport.song().unwrap().id.clone();
            app.session_restored(app.session, token, Ok(Some(restored())));
            assert_eq!(app.transport.song().unwrap().id, playing);
            assert!(!app.transport.paused);
            app.listening_session.restoring = true;
            let token = app.listening_session.token;
            app.apply(Action::SignOut);
            app.session_restored(app.session.wrapping_sub(1), token, Ok(Some(restored())));
            assert!(app.transport.song().is_none());
        });
    }
    fn client(user: &str) -> Client {
        Client::new(
            "https://library.invalid",
            user,
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap()
    }
    #[test]
    fn account_switch_clears_old_audio_and_position_writes_are_bounded() {
        with_app(|app, _| {
            app.set_client(client("a"));
            app.apply(Action::Play(1));
            // Enable only persistence after demo playback; the worker never connects.
            app.demo = false;
            app.checkpoint_session(true);
            let first = app.listening_session.last_write.unwrap();
            app.transport.position = 2.0;
            app.checkpoint_session(false);
            assert_eq!(app.listening_session.last_write, Some(first));
            app.listening_session.last_write = Some(first - Duration::from_secs(11));
            app.checkpoint_session(false);
            let second = app.listening_session.last_write.unwrap();
            assert_eq!(app.listening_session.position, 2.0);
            app.transport.position = 2.25;
            app.apply(Action::Pause(true));
            assert!(
                app.checkpoint_delay().is_none(),
                "pause has already checkpointed"
            );
            app.checkpoint_session(false);
            assert!(app.listening_session.last_write.unwrap() > second);
            assert_eq!(app.listening_session.position, 2.25);
            app.apply(Action::Shuffle(true));
            assert!(
                app.checkpoint_delay().is_some(),
                "paused edits schedule a save without audio events"
            );
            app.listening_session.dirty = Some(Instant::now() - Duration::from_secs(1));
            app.checkpoint_session(false);
            assert!(app.checkpoint_delay().is_none());
            app.set_client(client("b"));
            assert!(app.transport.song().is_none());
            assert!(app.transport.paused);
            assert!(!app.listening_session.writable);
            app.demo = true;
        });
    }
    #[test]
    fn restore_failure_does_not_replace_the_saved_queue_with_empty_state() {
        with_app(|app, _| {
            app.listening_session.restoring = true;
            app.session_restored(
                app.session,
                app.listening_session.token,
                Err("offline".into()),
            );
            assert!(!app.listening_session.writable);
            app.apply(Action::Shuffle(true));
            assert!(app.listening_session.writable);
        });
    }
}
