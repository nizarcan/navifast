use super::{App, Request};
use crate::navidrome::{api::Song, queue::Id};

#[derive(Default)]
pub struct State {
    serial: u64,
    request: Option<u64>,
    attempted: Option<Id>,
    pub waiting: bool,
    pub error: Option<String>,
}

impl State {
    pub fn loading(&self) -> bool {
        self.request.is_some()
    }
}

impl App {
    fn autoplay_scope(&self) -> Option<&str> {
        self.client
            .as_ref()
            .map(|client| client.scope.as_str())
            .or_else(|| {
                self.demo
                    .then_some("0000000000000000000000000000000000000000000000000000000000000000")
            })
    }
    pub(crate) fn autoplay_exclusions(
        &self,
    ) -> Option<&crate::navidrome::autoplay::exclusions::Exclusions> {
        self.preferences
            .autoplay_exclusions
            .get(self.autoplay_scope()?)
    }

    pub(crate) fn autoplay_excluded(&self, uri: &str) -> Option<bool> {
        self.autoplay_scope()?;
        let (kind, id) = exclusion_target(uri)?;
        Some(
            self.autoplay_exclusions()
                .is_some_and(|rules| rules.contains(kind, &id)),
        )
    }

    pub(crate) fn toggle_autoplay_exclusion(&mut self, uri: &str) {
        let Some((kind, id)) = exclusion_target(uri) else {
            return;
        };
        let Some(excluded) = self.autoplay_excluded(uri) else {
            return;
        };
        let name = self
            .favorite_item(uri)
            .map(|item| item.name().to_owned())
            .or_else(|| {
                use crate::navidrome::api::FavoriteKind;
                let history = &self.preferences.home_history;
                match kind {
                    FavoriteKind::Artist => history.artists.iter().find(|entry| entry.id == id),
                    FavoriteKind::Album => history
                        .collections
                        .iter()
                        .find(|entry| entry.id == id && entry.kind == super::home::Kind::Album),
                    FavoriteKind::Song => None,
                }
                .map(|entry| entry.name.clone())
            })
            .unwrap_or_else(|| id.clone());
        self.set_autoplay_exclusion(kind, &id, &name, !excluded);
    }

    pub(crate) fn set_autoplay_exclusion(
        &mut self,
        kind: crate::navidrome::api::FavoriteKind,
        id: &str,
        name: &str,
        excluded: bool,
    ) {
        let Some(scope) = self.autoplay_scope().map(str::to_owned) else {
            return;
        };
        if self
            .autoplay_exclusions()
            .is_some_and(|rules| rules.contains(kind, id))
            == excluded
        {
            return;
        }
        if let Err(error) = self
            .preferences
            .autoplay_exclusions
            .set(&scope, kind, id, name, excluded)
        {
            self.presentation.toast_error(error);
            return;
        }
        self.mark_dirty();
        // Invalidate reads made with old rules without touching explicit playback.
        let waiting = self.autoplay.waiting;
        let paused = self.transport.paused;
        self.cancel_autoplay(false);
        self.transport.paused = paused;
        self.prune_autoplay_exclusions();
        self.autoplay.attempted = None;
        self.poll_autoplay();
        if waiting && !self.wait_for_autoplay(paused) {
            self.transport.loading = false;
            self.transport.paused = true;
        }
        self.prepare_next();
    }

    pub(super) fn prune_autoplay_exclusions(&mut self) {
        let blocked = self.autoplay_exclusions().is_some_and(|rules| {
            self.transport
                .queue
                .upcoming()
                .iter()
                .filter(|entry| entry.automatic())
                .any(|entry| {
                    self.transport
                        .queue
                        .song(entry)
                        .is_some_and(|song| rules.blocks(song))
                })
        });
        if blocked {
            self.cancel_preparation();
            self.transport.queue.clear_automatic();
            self.dirty_session();
        }
    }

    pub(crate) fn set_autoplay(&mut self, enabled: bool) {
        if self.preferences.autoplay != enabled {
            self.preferences.autoplay = enabled;
            self.reset_autoplay();
            self.mark_dirty();
            self.poll_autoplay();
        }
    }

    pub(super) fn cancel_autoplay(&mut self, discard: bool) {
        self.autoplay.serial = self.autoplay.serial.wrapping_add(1);
        self.autoplay.request = None;
        self.autoplay.attempted = self.transport.queue.current().map(|entry| entry.id);
        self.autoplay.error = None;
        self.backend.send(Request::CancelAutoplay);
        if discard && self.transport.queue.clear_automatic() {
            self.dirty_session();
        }
        if self.autoplay.waiting {
            self.autoplay.waiting = false;
            self.transport.loading = false;
            self.transport.paused = true;
        }
    }

    pub(super) fn reset_autoplay(&mut self) {
        self.cancel_autoplay(true);
        self.autoplay.attempted = None;
    }

    fn autoplay_blocked(&self) -> bool {
        !self.connected
            || !self.preferences.autoplay
            || self.transport.repeat != crate::player::RepeatMode::Off
    }

    /// Refill only the final occurrence, at most once per occurrence, within 15s.
    pub(super) fn poll_autoplay(&mut self) -> bool {
        if self.autoplay_blocked() {
            if self.autoplay.loading() || self.transport.queue.automatic_len() > 0 {
                self.cancel_autoplay(true);
                return true;
            }
            return false;
        }
        if self.autoplay.loading()
            || self.transport.paused
            || !self.transport.queue.upcoming().is_empty()
        {
            return false;
        }
        let Some(entry) = self.transport.queue.current() else {
            return false;
        };
        let Some(song) = self.transport.song() else {
            return false;
        };
        if self.autoplay.attempted == Some(entry.id)
            || song.duration - self.transport.position > 15.
        {
            return false;
        }
        self.autoplay.attempted = Some(entry.id);
        self.autoplay.serial = self.autoplay.serial.wrapping_add(1);
        self.autoplay.request = Some(self.autoplay.serial);
        self.autoplay.error = None;
        if let Some(client) = &self.client {
            self.backend.send(Request::Autoplay {
                session: self.session,
                request: self.autoplay.serial,
                client: client.clone(),
                current: song.id.clone(),
                exclusions: self.autoplay_exclusions().cloned().unwrap_or_default(),
            });
        }
        true
    }

    pub(super) fn wait_for_autoplay(&mut self, paused: bool) -> bool {
        if !paused && !self.autoplay_blocked() && self.autoplay.loading() {
            self.autoplay.waiting = true;
            self.transport.paused = false;
            self.transport.loading = true;
            true
        } else {
            false
        }
    }

    pub(super) fn autoplay_result(
        &mut self,
        session: u64,
        request: u64,
        result: Result<Option<Song>, String>,
    ) {
        if session != self.session || self.autoplay.request != Some(request) {
            return;
        }
        self.autoplay.request = None;
        if self.autoplay_blocked() {
            self.cancel_autoplay(true);
            return;
        }
        let added = match result {
            Ok(Some(song))
                if !self
                    .autoplay_exclusions()
                    .is_some_and(|rules| rules.blocks(&song)) =>
            {
                self.transport.queue.add_automatic(song)
            }
            Ok(_) => {
                self.autoplay.error = Some("No eligible songs were found in this Autoplay sample. Check exclusions in Settings or toggle Autoplay to try again.".into());
                false
            }
            Err(error) => {
                self.autoplay.error = Some(error);
                false
            }
        };
        if added {
            self.dirty_session();
        }
        if self.autoplay.waiting {
            self.autoplay.waiting = false;
            self.transport.loading = false;
            if added {
                let advance = self.transport.queue.advance();
                self.advance_queue(advance, self.transport.paused);
            } else {
                self.transport.paused = true;
            }
        }
    }

    #[cfg(feature = "navidrome-demo")]
    pub(super) fn configure_demo_exclusions(&mut self, flags: &[&str]) {
        use crate::navidrome::api::FavoriteKind;
        if !flags.contains(&"exclusions-filled") && !flags.contains(&"exclusions-many") {
            return;
        }
        let scope = self.autoplay_scope().unwrap().to_owned();
        for (kind, id, name) in [
            (FavoriteKind::Song, "trk0", "Rosewood"),
            (FavoriteKind::Album, "alb1", "Mordechai"),
            (FavoriteKind::Artist, "art2", "Nils Frahm"),
        ] {
            self.preferences
                .autoplay_exclusions
                .set(&scope, kind, id, name, true)
                .unwrap();
        }
        if flags.contains(&"exclusions-many") {
            for i in 0..24 {
                self.preferences
                    .autoplay_exclusions
                    .set(
                        &scope,
                        FavoriteKind::Song,
                        &format!("extra{i:02}"),
                        &format!("Saved song {}", i + 1),
                        true,
                    )
                    .unwrap();
            }
        }
    }

    #[cfg(feature = "navidrome-demo")]
    pub(super) fn configure_demo_autoplay(&mut self, flags: &[&str]) {
        if !flags.iter().any(|flag| flag.starts_with("autoplay-")) {
            return;
        }
        if let Some(album) = self.albums.first().cloned() {
            let index = album.song.len().saturating_sub(1);
            self.play_album(album, index);
            self.transport.paused = true;
        }
        self.preferences.autoplay = true;
        self.autoplay.attempted = self.transport.queue.current().map(|entry| entry.id);
        if flags.contains(&"autoplay-loading") {
            self.autoplay.request = Some(self.autoplay.serial);
        } else if flags.contains(&"autoplay-error") {
            self.autoplay.error =
                Some("The server could not be reached. Playback will end normally.".into());
        } else if flags.contains(&"autoplay-empty") {
            self.autoplay.error = Some("No playable library songs were found.".into());
        } else if let Some(song) = self.albums.get(1).and_then(|album| album.song.first()) {
            self.transport.queue.add_automatic(song.clone());
        }
    }
}

fn exclusion_target(uri: &str) -> Option<(crate::navidrome::api::FavoriteKind, String)> {
    use crate::navidrome::api::FavoriteKind;
    let (kind, id) = uri.strip_prefix("navidrome:")?.split_once(':')?;
    let kind = match kind {
        "track" => FavoriteKind::Song,
        "album" => FavoriteKind::Album,
        "artist" => FavoriteKind::Artist,
        _ => return None,
    };
    Some((kind, super::decode(id).filter(|id| !id.is_empty())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::{Action, Preferences, player, queue, tests::with_app};

    fn song(id: &str) -> Song {
        Song {
            id: id.into(),
            duration: 60.,
            ..Default::default()
        }
    }

    fn start(app: &mut App) -> u64 {
        app.play_search_songs(vec![song("chosen")], 0);
        app.set_autoplay(true);
        app.transport.position = 50.;
        assert!(app.poll_autoplay());
        app.autoplay.request.unwrap()
    }

    fn finish(app: &mut App) {
        let paused = app.transport.paused;
        app.transport
            .event(player::Event::Finished(app.transport.generation));
        app.finish_track(paused);
    }

    fn ids(app: &App) -> Vec<&str> {
        app.transport
            .queue
            .upcoming()
            .iter()
            .filter_map(|e| app.transport.queue.song(e))
            .map(|s| s.id.as_str())
            .collect()
    }

    #[test]
    fn autoplay_defaults_off_and_requests_only_once_near_the_final_occurrence() {
        assert!(!serde_json::from_str::<Preferences>("{}").unwrap().autoplay);
        with_app(|app, _| {
            app.play_search_songs(vec![song("first"), song("last")], 0);
            app.transport.position = 59.;
            assert!(!app.poll_autoplay());
            app.set_autoplay(true);
            assert!(!app.poll_autoplay());
            app.apply(Action::Next);
            assert!(!app.poll_autoplay());
            app.transport.position = 50.;
            assert!(app.poll_autoplay());
            let request = app.autoplay.request.unwrap();
            for _ in 0..10 {
                assert!(!app.poll_autoplay());
            }
            app.autoplay_result(app.session, request, Ok(Some(song("random"))));
            assert_eq!(ids(app), ["random"]);
            assert!(!app.poll_autoplay());
            app.apply(Action::Repeat(crate::player::RepeatMode::Track));
            assert!(ids(app).is_empty());
            assert!(!app.poll_autoplay());
        });
    }

    #[test]
    fn autoplay_clear_disable_replacement_and_account_change_reject_late_results() {
        for action in 0..5 {
            with_app(|app, _| {
                let request = start(app);
                let session = app.session;
                match action {
                    0 => app.queue_action(queue::Action::Clear),
                    1 => app.set_autoplay(false),
                    2 => app.play_search_songs(vec![song("new")], 0),
                    3 => app.apply(Action::SignOut),
                    _ => app.set_client(
                        crate::navidrome::api::Client::new(
                            "http://127.0.0.1:9",
                            "other",
                            "dummy".into(),
                            &crate::settings::ProxyConfig::Off,
                        )
                        .unwrap(),
                    ),
                }
                app.autoplay_result(session, request, Ok(Some(song("stale"))));
                assert!(!ids(app).contains(&"stale"));
                assert!(!app.autoplay.loading());
                if action == 0 {
                    assert!(!app.poll_autoplay());
                }
            });
        }
    }

    #[test]
    fn autoplay_manual_additions_precede_the_suggestion_even_while_waiting() {
        with_app(|app, _| {
            let request = start(app);
            app.queue_action(queue::Action::Songs {
                songs: vec![song("manual")],
                next: false,
            });
            app.autoplay_result(app.session, request, Ok(Some(song("random"))));
            assert_eq!(ids(app), ["manual", "random"]);
            finish(app);
            assert_eq!(app.transport.song().unwrap().id, "manual");
            assert!(!app.transport.paused);
        });
        with_app(|app, _| {
            let request = start(app);
            finish(app);
            assert!(app.autoplay.waiting);
            app.queue_action(queue::Action::Songs {
                songs: vec![song("manual")],
                next: true,
            });
            assert_eq!(app.transport.song().unwrap().id, "manual");
            assert!(!app.autoplay.waiting);
            app.autoplay_result(app.session, request, Ok(Some(song("random"))));
            assert_eq!(app.transport.song().unwrap().id, "manual");
            assert_eq!(ids(app), ["random"]);
        });
    }

    #[test]
    fn autoplay_pending_album_keeps_priority_and_failed_album_can_resume_waiting() {
        with_app(|app, _| {
            let request = start(app);
            finish(app);
            // Keep the album read unresolved while testing the controller boundary.
            app.demo = false;
            app.queue_action(queue::Action::Album {
                id: "album".into(),
                name: "Album".into(),
                next: false,
            });
            assert!(app.transport.waiting_for_queue);
            assert!(!app.autoplay.waiting);
            let token = app.transport.queue.upcoming()[0].id;
            app.queued_album(token, Err("Unavailable".into()));
            assert!(app.autoplay.waiting);
            assert!(!app.transport.waiting_for_queue);
            app.demo = true;
            app.autoplay_result(app.session, request, Ok(Some(song("random"))));
            assert_eq!(app.transport.song().unwrap().id, "random");
            assert!(!app.transport.paused);
        });
    }

    #[test]
    fn autoplay_finish_preserves_pause_and_seek_cancels_the_wait_without_skipping() {
        with_app(|app, _| {
            let request = start(app);
            finish(app);
            assert!(app.transport.loading);
            app.apply(Action::Pause(true));
            app.autoplay_result(app.session, request, Ok(Some(song("random"))));
            assert_eq!(app.transport.song().unwrap().id, "random");
            assert!(app.transport.paused);
            assert_eq!(app.transport.position, 0.);
        });
        with_app(|app, _| {
            let request = start(app);
            finish(app);
            app.apply(Action::Seek(10.));
            assert!(!app.autoplay.waiting);
            app.autoplay_result(app.session, request, Ok(Some(song("random"))));
            assert_eq!(app.transport.song().unwrap().id, "chosen");
            assert_eq!(app.transport.position, 10.);
            assert_eq!(ids(app), ["random"]);
        });
    }

    #[test]
    fn autoplay_empty_or_failed_reads_stop_normally_without_a_retry_loop() {
        for result in [Ok(None), Err("Unavailable".into())] {
            with_app(|app, _| {
                let request = start(app);
                finish(app);
                app.autoplay_result(app.session, request, result);
                assert!(app.transport.paused);
                assert!(!app.transport.loading);
                assert!(!app.autoplay.waiting);
                assert!(app.autoplay.error.is_some());
                assert_eq!(app.transport.position, 60.);
                for _ in 0..10 {
                    assert!(!app.poll_autoplay());
                }
            });
        }
        with_app(|app, _| {
            app.play_search_songs(vec![song("short")], 0);
            app.set_autoplay(true);
            // A finished event can arrive before a near-end position update.
            finish(app);
            assert!(app.autoplay.waiting);
            let request = app.autoplay.request.unwrap();
            app.queue_action(queue::Action::Clear);
            app.autoplay_result(app.session, request, Ok(Some(song("stale"))));
            assert!(app.transport.paused);
            assert!(!app.transport.loading);
            assert!(ids(app).is_empty());
        });
    }
    #[test]
    fn exclusions_remove_only_automatic_suggestions_and_reject_old_reads() {
        use crate::navidrome::api::FavoriteKind;
        with_app(|app, _| {
            let request = start(app);
            app.queue_action(queue::Action::Songs {
                songs: vec![song("blocked")],
                next: false,
            });
            app.autoplay_result(app.session, request, Ok(Some(song("blocked"))));
            let generation = app.transport.generation;
            let position = app.transport.position;
            app.set_autoplay_exclusion(FavoriteKind::Song, "blocked", "Blocked", true);
            assert_eq!(ids(app), ["blocked"]); // The explicit occurrence stays.
            assert_eq!(app.transport.generation, generation);
            assert_eq!(app.transport.position, position);
            assert!(!app.transport.paused);
            app.apply(Action::Next);
            assert_eq!(app.transport.song().unwrap().id, "blocked");
        });
        with_app(|app, _| {
            let old = start(app);
            finish(app);
            assert!(app.autoplay.waiting);
            app.set_autoplay_exclusion(FavoriteKind::Song, "blocked", "Blocked", true);
            assert!(app.autoplay.waiting);
            let new = app.autoplay.request.unwrap();
            assert_ne!(old, new);
            app.autoplay_result(app.session, old, Ok(Some(song("stale"))));
            assert!(ids(app).is_empty());
            app.autoplay_result(app.session, new, Ok(Some(song("blocked"))));
            assert!(ids(app).is_empty());
            assert!(app.transport.paused);
            assert!(app.autoplay.error.is_some());
            assert!(!app.poll_autoplay());
        });
    }
    #[test]
    fn exclusions_survive_restart_and_switching_accounts_without_touching_playback() {
        use crate::navidrome::api::{Client, FavoriteKind};
        with_app(|app, _| {
            let client = |user| {
                Client::new(
                    "http://127.0.0.1:9",
                    user,
                    "dummy".into(),
                    &crate::settings::ProxyConfig::Off,
                )
                .unwrap()
            };
            app.client = Some(client("one"));
            app.set_autoplay_exclusion(FavoriteKind::Song, "shared", "Shared", true);
            let uri = FavoriteKind::Song.uri("shared");
            app.preferences =
                serde_json::from_str(&serde_json::to_string(&app.preferences).unwrap()).unwrap();
            assert_eq!(app.autoplay_excluded(&uri), Some(true));
            app.client = Some(client("two"));
            assert_eq!(app.autoplay_excluded(&uri), Some(false));
            app.client = Some(client("one"));
            assert_eq!(app.autoplay_excluded(&uri), Some(true));
            app.toggle_autoplay_exclusion(&uri);
            assert_eq!(app.autoplay_excluded(&uri), Some(false));
        });
    }
}
