//! An inspected library file stays independent of the currently playing occurrence.
use super::State;
use crate::navidrome::{api::Song, audio::Playback};

pub(crate) struct Inspector {
    pub song: Song,
    pub origin: Option<String>,
    session: u64,
}
impl Inspector {
    pub fn playback<'a>(&self, app: &'a super::App) -> Option<&'a Playback> {
        (self.session == app.session && app.transport.song().is_some_and(|s| s.id == self.song.id))
            .then_some(app.transport.information.as_ref())
            .flatten()
    }
}
impl State {
    pub(crate) fn inspect_song(&mut self, uri: &str, origin: Option<String>) {
        let Some(crate::app::navidrome::favorites::Item::Song(song)) = self.app.favorite_item(uri)
        else {
            return;
        };
        self.inspector = Some(Inspector {
            song,
            origin,
            session: self.app.session,
        });
        self.favorite_menu = None;
        self.queue_menu = None;
    }
    pub(crate) fn close_inspector(&mut self) {
        self.inspector = None;
    }
    #[cfg(feature = "navidrome-demo")]
    pub(super) fn demo_inspector(&mut self, flags: &[&str]) {
        use crate::navidrome::{audio, quality::Quality};
        let song = if flags.contains(&"inspector-other") {
            self.app.page_songs().and_then(|s| s.get(1))
        } else {
            self.app.transport.song()
        };
        let Some(song) = song else {
            return;
        };
        let uri = crate::navidrome::api::FavoriteKind::Song.uri(&song.id);
        self.inspect_song(&uri, Some("playing-track".into()));
        let Some(inspector) = &mut self.inspector else {
            return;
        };
        if !flags.contains(&"inspector-missing") {
            inspector.song.suffix = "flac".into();
            inspector.song.sampling_rate = Some(96_000);
            inspector.song.bit_depth = Some(24);
            inspector.song.channel_count = Some(2);
            inspector.song.bit_rate = Some(2700);
            inspector.song.replay_gain = Some(crate::navidrome::api::ReplayGain {
                track_gain: Some(-5.4),
                album_gain: Some(-4.1),
                track_peak: Some(0.98),
                album_peak: Some(1.0),
            });
        }
        if flags.contains(&"inspector-error") {
            self.app.transport.error =
                Some("The audio stream was interrupted. Press Play to retry.".into());
        }
        if flags.contains(&"inspector-loading")
            || flags.contains(&"inspector-missing")
            || flags.contains(&"inspector-error")
        {
            self.app.transport.information = None;
        } else {
            let mp3 = flags.contains(&"inspector-mp3");
            self.app.transport.information = Some(audio::Playback {
                quality: if mp3 {
                    Quality::Mp3Standard
                } else {
                    Quality::Original
                },
                decoded: audio::Decoded {
                    codec: if mp3 { "mp3" } else { "flac" }.into(),
                    sample_rate: if mp3 { 48_000 } else { 96_000 },
                    channels: 2,
                    bit_depth: (!mp3).then_some(24),
                },
                output: audio::Output {
                    sample_rate: 48_000,
                    channels: 2,
                    sample_format: "f32".into(),
                },
            });
        }
    }
}
