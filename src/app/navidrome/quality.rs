//! Quality preferences and capability-aware playback changes.
use super::{App, Request};
use crate::navidrome::quality::{Capabilities, Quality};

#[derive(Default)]
pub struct State {
    pub capabilities: Option<Result<Capabilities, String>>,
    pub active: Quality,
    pub deferred: bool,
}
impl App {
    pub(crate) fn configure_audio_cache(&mut self, enabled: bool, limit_mib: u32, clear: bool) {
        self.preferences.audio_cache =
            crate::navidrome::cache::Config { enabled, limit_mib }.normalized();
        self.mark_dirty();
        if !self.demo {
            self.cache_revision = self.cache_revision.wrapping_add(1);
            self.cache_pending = true;
            self.audio_cache.error = None;
            self.backend.send(Request::Cache {
                root: self.dirs.cache.join("streams"),
                config: self.preferences.audio_cache,
                clear,
            });
        } else if clear || !enabled {
            self.audio_cache = Default::default();
        }
    }
    pub(crate) fn can_seek(&self) -> bool {
        self.quality.active == Quality::Original || self.transport.seekable
    }
    pub(crate) fn refresh_streaming_capabilities(&mut self) {
        self.quality.capabilities = None;
        if let Some(client) = &self.client {
            self.backend.send(Request::StreamingCapabilities {
                session: self.session,
                client: client.clone(),
            });
        }
    }
    pub(crate) fn set_streaming_quality(&mut self, quality: Quality) {
        if self.preferences.streaming_quality == quality {
            return;
        }
        if quality != Quality::Original && !matches!(self.quality.capabilities, Some(Ok(_))) {
            return;
        }
        self.cancel_preparation();
        self.preferences.streaming_quality = quality;
        self.mark_dirty();
        self.quality.deferred = false;
        if self.transport.song().is_none() {
            self.quality.active = quality;
            return;
        }
        let can_seek = self
            .quality
            .capabilities
            .as_ref()
            .and_then(|c| c.as_ref().ok())
            .copied()
            .unwrap_or_default()
            .can_seek(quality);
        if self.transport.position > 0.0 && !can_seek {
            // Keep the already selected format and position. Never silently
            // replace a compressed selection with an original-file fallback.
            self.quality.deferred = true;
        } else {
            self.quality.active = quality;
            self.load(self.transport.position, self.transport.paused);
        }
        self.prepare_next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::{Action, player};
    use crate::navidrome::api::Song;
    fn start(app: &mut App) {
        app.play_search_songs(
            vec![Song {
                id: "fixture".into(),
                duration: 60.0,
                ..Default::default()
            }],
            0,
        );
        app.apply(Action::Seek(15.0));
    }
    #[test]
    fn quality_changes_preserve_paused_position_and_reject_old_playback_answers() {
        super::super::tests::with_app(|app, _| {
            start(app);
            app.apply(Action::Pause(true));
            let old = app.transport.generation;
            app.set_streaming_quality(Quality::Mp3Standard);
            assert_ne!(app.transport.generation, old);
            assert_eq!(app.transport.position, 15.0);
            assert!(app.transport.paused);
            assert_eq!(app.quality.active, Quality::Mp3Standard);
            app.transport.event(player::Event::Ready {
                generation: old,
                seekable: false,
            });
            assert!(app.can_seek());
            let selected = app.transport.generation;
            app.apply(Action::Seek(20.0));
            app.transport.event(player::Event::Failed {
                generation: selected,
                message: "old quality error".into(),
            });
            assert_eq!(app.transport.position, 20.0);
            assert!(app.transport.error.is_none());
        });
    }
    #[test]
    fn unsupported_seek_defers_quality_until_next_track_without_a_raw_fallback() {
        super::super::tests::with_app(|app, _| {
            start(app);
            app.quality.capabilities = Some(Ok(Capabilities::default()));
            let generation = app.transport.generation;
            app.set_streaming_quality(Quality::Mp3Low);
            assert!(app.quality.deferred);
            assert_eq!(app.quality.active, Quality::Original);
            assert_eq!(app.transport.generation, generation);
            assert_eq!(app.transport.position, 15.0);
            app.load(0.0, true);
            assert_eq!(app.quality.active, Quality::Mp3Low);
            assert!(app.transport.paused);
            assert!(!app.can_seek());
            let generation = app.transport.generation;
            app.apply(Action::Seek(20.0));
            assert_eq!(app.transport.generation, generation);
            assert_eq!(app.transport.position, 0.0);
            app.set_streaming_quality(Quality::Original);
            assert!(app.can_seek());
        });
    }
}
