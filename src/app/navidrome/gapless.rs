//! Prepare only the next resolved queue occurrence. Queue order stays authoritative.
use super::{App, player};

#[derive(Default)]
pub(super) struct State {
    prepared: Option<(u64, u64, u64)>, // current generation, next generation, occurrence
}

impl App {
    pub(super) fn cancel_preparation(&mut self) {
        if self.gapless.prepared.take().is_some() {
            self.player.send(player::Command::CancelPreparation);
        }
    }

    pub(super) fn prepare_next(&mut self) {
        let next = if self.transport.paused || self.transport.error.is_some() || self.demo {
            None
        } else if self.transport.repeat == crate::player::RepeatMode::Track {
            self.transport.queue.current()
        } else {
            self.transport.queue.upcoming().front()
        };
        let candidate = next.and_then(|entry| {
            self.transport
                .queue
                .song(entry)
                .map(|song| (entry.id, song))
        });
        let Some((occurrence, song)) = candidate else {
            self.cancel_preparation();
            return;
        };
        if self.gapless.prepared.is_some_and(|(from, _, entry)| {
            from == self.transport.generation && entry == occurrence
        }) {
            return;
        }
        let song = song.clone();
        self.cancel_preparation();
        let Some(client) = self.client.clone() else {
            return;
        };
        let generation = player::next_generation();
        let listening = self
            .scrobbles
            .reserve(self.session, self.presentation.settings.scrobbling);
        self.gapless.prepared = Some((self.transport.generation, generation, occurrence));
        self.player.send(player::Command::Prepare {
            quality: self.preferences.streaming_quality,
            from: self.transport.generation,
            generation,
            occurrence,
            listening,
            client,
            song: Box::new(song),
        });
    }

    pub(super) fn gapless_transition(
        &mut self,
        from: u64,
        generation: u64,
        occurrence: u64,
        listening: crate::navidrome::scrobble::Context,
    ) -> bool {
        if from != self.transport.generation {
            return false;
        }
        let repeat = self.transport.repeat == crate::player::RepeatMode::Track;
        let expected = if repeat {
            self.transport.queue.current()
        } else {
            self.transport.queue.upcoming().front()
        };
        if expected.is_none_or(|entry| entry.id != occurrence || entry.pending()) {
            // An already audible boundary raced a queue edit. Restore the user's
            // shown selection; the obsolete event must never pop the new head.
            self.load(self.transport.position, self.transport.paused);
            return false;
        }
        self.gapless.prepared = None;
        if !repeat {
            self.transport.queue.advance();
        }
        if self
            .transport
            .queue
            .current()
            .is_some_and(|entry| entry.automatic())
        {
            self.playing_album = None;
            self.playing_playlist = None;
        }
        self.scrobbles.activate(occurrence, listening);
        self.transport.generation = generation;
        self.transport.position = 0.0;
        self.transport.loading = false;
        self.transport.error = None;
        self.autoplay.waiting = false;
        self.remember_home_artists();
        self.sync_lyrics();
        self.dirty_session();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::api::Song;
    fn song(id: &str) -> Song {
        Song {
            id: id.into(),
            duration: 60.0,
            ..Default::default()
        }
    }

    #[test]
    fn audible_handoff_advances_one_occurrence_and_keeps_pause_intention() {
        super::super::tests::with_app(|app, _| {
            app.play_search_songs(vec![song("same"), song("same"), song("later")], 0);
            let from = app.transport.generation;
            let next = app.transport.queue.upcoming()[0].id;
            let context = app.scrobbles.reserve(app.session, true);
            let generation = player::next_generation();
            app.transport.paused = true;
            assert!(app.gapless_transition(from, generation, next, context));
            assert_eq!(app.transport.queue.current().unwrap().id, next);
            assert_eq!(app.transport.queue.upcoming().len(), 1);
            assert_eq!(app.transport.generation, generation);
            assert!(app.transport.paused);
            assert_eq!(app.transport.position, 0.0);
            assert_eq!(
                app.scrobbles.context(app.session, next, true).occurrence,
                context.occurrence
            );
            assert!(
                !app.gapless_transition(from, generation, next, context),
                "a duplicate boundary cannot advance twice"
            );
        });
    }
    #[test]
    fn next_seek_or_reordered_queue_reject_obsolete_handoffs() {
        super::super::tests::with_app(|app, _| {
            app.play_search_songs(vec![song("first"), song("second"), song("third")], 0);
            let from = app.transport.generation;
            let second = app.transport.queue.upcoming()[0].id;
            let context = app.scrobbles.reserve(app.session, true);
            let reserved = player::next_generation();
            app.apply(super::super::Action::Seek(30.0));
            assert_ne!(app.transport.generation, reserved);
            assert!(!app.gapless_transition(from, reserved, second, context));
            assert_eq!(app.transport.song().unwrap().id, "first");
            let from = app.transport.generation;
            app.transport.queue.remove(second);
            assert!(!app.gapless_transition(from, player::next_generation(), second, context));
            assert_eq!(app.transport.song().unwrap().id, "first");
            assert_eq!(app.transport.queue.upcoming().len(), 1);
            assert_eq!(app.transport.position, 30.0);
        });
    }
    #[test]
    fn repeat_one_reserves_a_new_listen_without_consuming_next() {
        super::super::tests::with_app(|app, _| {
            app.play_search_songs(vec![song("first"), song("second")], 0);
            app.transport.repeat = crate::player::RepeatMode::Track;
            let current = app.transport.queue.current().unwrap().id;
            let old = app.scrobbles.context(app.session, current, true);
            let new = app.scrobbles.reserve(app.session, true);
            assert_ne!(old.occurrence, new.occurrence);
            assert!(app.gapless_transition(
                app.transport.generation,
                player::next_generation(),
                current,
                new
            ));
            assert_eq!(app.transport.queue.current().unwrap().id, current);
            assert_eq!(app.transport.queue.upcoming().len(), 1);
        });
    }
}
