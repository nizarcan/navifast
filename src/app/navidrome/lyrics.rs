//! One account-scoped lyrics document follows the playing song.
use super::{App, Request};
use crate::{
    model::Loadable,
    navidrome::lyrics::{Outcome, Source},
};

#[derive(Default)]
pub struct State {
    pub enabled: bool,
    pub song: Option<String>,
    pub data: Loadable<Outcome>,
    pub source: Source,
    request: u64,
}
impl App {
    pub(super) fn set_lrclib_lyrics(&mut self, enabled: bool) {
        if self.presentation.settings.lrclib_lyrics == enabled {
            return;
        }
        self.presentation.settings.lrclib_lyrics = enabled;
        self.mark_dirty();
        self.backend.send(Request::CancelLyrics);
        self.lyrics.request = self.lyrics.request.wrapping_add(1);
        // A server hit remains valid. External data disappears immediately on opt-out.
        if self.lyrics.source == Source::Lrclib
            || !matches!(
                self.lyrics.data,
                Loadable::Loaded(Outcome::Found(_) | Outcome::Instrumental)
            )
        {
            self.lyrics.data = Loadable::NotLoaded;
            self.lyrics.source = Source::Server;
            self.sync_lyrics();
        }
    }
    pub(super) fn enable_lyrics(&mut self, enabled: bool) {
        self.lyrics.enabled = enabled;
        if enabled {
            self.sync_lyrics();
        } else {
            self.backend.send(Request::CancelLyrics);
            self.lyrics.request = self.lyrics.request.wrapping_add(1);
            if matches!(self.lyrics.data, Loadable::Loading) {
                self.lyrics.data = Loadable::NotLoaded;
            }
        }
    }
    pub(super) fn retry_lyrics(&mut self) {
        self.lyrics.data = Loadable::NotLoaded;
        self.sync_lyrics();
    }
    pub(super) fn sync_lyrics(&mut self) {
        let id = self.transport.song().map(|song| song.id.as_str());
        if self.lyrics.song.as_deref() != id {
            self.backend.send(Request::CancelLyrics);
            self.lyrics.request = self.lyrics.request.wrapping_add(1);
            self.lyrics.song = id.map(str::to_owned);
            self.lyrics.data = Loadable::NotLoaded;
            self.lyrics.source = Source::Server;
        }
        if !self.lyrics.enabled || !matches!(self.lyrics.data, Loadable::NotLoaded) {
            return;
        }
        let Some(song) = self.transport.song() else {
            return;
        };
        if let Some(client) = &self.client {
            self.lyrics.request = self.lyrics.request.wrapping_add(1);
            self.backend.send(Request::Lyrics {
                session: self.session,
                request: self.lyrics.request,
                client: client.clone(),
                song: Box::new(song.clone()),
                external: self
                    .presentation
                    .settings
                    .lrclib_lyrics
                    .then(|| Box::new(self.proxy.clone())),
            });
            self.lyrics.data = Loadable::Loading;
        }
    }
    pub(super) fn lyrics_result(
        &mut self,
        session: u64,
        request: u64,
        song: &str,
        result: Result<(Source, Outcome), String>,
    ) {
        if session != self.session
            || request != self.lyrics.request
            || self.lyrics.song.as_deref() != Some(song)
            || !self.lyrics.enabled
        {
            return;
        }
        self.lyrics.data = match result {
            Ok((source, data)) => {
                self.lyrics.source = source;
                Loadable::Loaded(data)
            }
            Err(error) => Loadable::Failed(error),
        };
    }
}

#[cfg(feature = "navidrome-demo")]
impl App {
    pub(super) fn populate_demo_lyrics(&mut self, flags: &[&str]) {
        use crate::navidrome::lyrics::{Line, Lyrics};
        if flags.contains(&"lrclib-enabled") {
            self.presentation.settings.lrclib_lyrics = true;
        }
        if !flags.iter().any(|f| f.starts_with("lyrics")) {
            return;
        }
        self.lyrics.enabled = true;
        if flags.contains(&"lyrics-provider") {
            self.presentation.settings.lrclib_lyrics = true;
            self.lyrics.source = Source::Lrclib;
        }
        self.lyrics.song = self.transport.song().map(|s| s.id.clone());
        self.lyrics.data = if flags.contains(&"lyrics-loading") {
            Loadable::Loading
        } else if flags.contains(&"lyrics-error") {
            Loadable::Failed("The server did not respond in time.".into())
        } else if flags.contains(&"lyrics-missing") {
            Loadable::Loaded(Outcome::Missing)
        } else if flags.contains(&"lyrics-instrumental") {
            Loadable::Loaded(Outcome::Instrumental)
        } else if flags.contains(&"lyrics-ambiguous") {
            Loadable::Loaded(Outcome::Ambiguous)
        } else {
            let timed = !flags.contains(&"lyrics-plain") && !flags.contains(&"lyrics-malformed");
            let phrases = [
                "Morning gathers at the window",
                "Quiet footsteps cross the floor",
                "Every road becomes a river",
                "Every river finds the shore",
                "Leave a light beside the doorway",
                "Let the evening settle in",
                "Listen closely to the distance",
                "Hear a new refrain begin",
                "We can take the long way home",
                "Past the trees and city lights",
                "Keep a little room for wonder",
                "Carry daylight through the night",
            ];
            let mut lines: Vec<Line> = (0..if flags.contains(&"lyrics-long") {
                10_000
            } else {
                36
            })
                .map(|i| Line {
                    words: Vec::new(),
                    at_ms: timed.then_some(i as u64 * 10_000),
                    text: if flags.contains(&"lyrics-words-rtl") {
                        "مرحبا hello עולם, we can take the long way home together".into()
                    } else if flags.contains(&"lyrics-rtl") && i % 3 == 0 {
                        "مرحبا hello עולם, a line in logical order".into()
                    } else {
                        phrases[i % phrases.len()].into()
                    },
                })
                .collect();
            if flags.contains(&"lyrics-words") {
                for line in &mut lines {
                    let mut byte = 0;
                    line.words = line
                        .text
                        .split_inclusive(' ')
                        .enumerate()
                        .map(|(i, word)| {
                            let range = byte..byte + word.len();
                            byte = range.end;
                            let start_ms = line.at_ms.unwrap_or(0) + i as u64 * 800;
                            crate::navidrome::lyrics::Word {
                                range,
                                start_ms,
                                end_ms: Some(start_ms + 800),
                            }
                        })
                        .collect();
                }
            }
            if flags.contains(&"lyrics-end") {
                lines.truncate(18);
                self.transport.position = 175.;
            }
            Loadable::Loaded(Outcome::Found(Lyrics {
                lines,
                timed,
                invalid_timing: flags.contains(&"lyrics-malformed"),
            }))
        };
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::{Action, tests::with_app};
    #[test]
    fn opting_out_clears_external_text_and_rejects_pending_provider_replies() {
        with_app(|app, _| {
            app.apply(Action::Play(0));
            app.enable_lyrics(true);
            assert!(!app.presentation.settings.lrclib_lyrics);
            app.set_lrclib_lyrics(true);
            let id = app.lyrics.song.clone().unwrap();
            let request = app.lyrics.request;
            app.lyrics_result(
                app.session,
                request,
                &id,
                Ok((Source::Lrclib, Outcome::Instrumental)),
            );
            app.set_lrclib_lyrics(false);
            assert!(matches!(app.lyrics.data, Loadable::NotLoaded));
            app.lyrics_result(
                app.session,
                request,
                &id,
                Ok((Source::Lrclib, Outcome::Instrumental)),
            );
            assert!(matches!(app.lyrics.data, Loadable::NotLoaded));
            assert_eq!(app.lyrics.source, Source::Server);
        });
    }
    #[test]
    fn old_song_account_and_closed_panel_replies_are_ignored_while_seeks_keep_the_document() {
        with_app(|app, _| {
            app.apply(Action::Play(0));
            app.enable_lyrics(true);
            let id = app.lyrics.song.clone().unwrap();
            let request = app.lyrics.request;
            app.lyrics_result(
                app.session,
                request,
                &id,
                Ok((Source::Server, Outcome::Missing)),
            );
            assert!(matches!(
                app.lyrics.data,
                Loadable::Loaded(Outcome::Missing)
            ));
            app.apply(Action::Seek(30.0));
            app.apply(Action::Pause(true));
            assert_eq!(app.lyrics.request, request);
            assert!(matches!(
                app.lyrics.data,
                Loadable::Loaded(Outcome::Missing)
            ));
            app.apply(Action::Next);
            app.lyrics_result(
                app.session,
                request,
                &id,
                Ok((Source::Server, Outcome::Instrumental)),
            );
            assert!(matches!(app.lyrics.data, Loadable::NotLoaded));
            let id = app.lyrics.song.clone().unwrap();
            let request = app.lyrics.request;
            app.enable_lyrics(false);
            app.lyrics_result(
                app.session,
                request,
                &id,
                Ok((Source::Server, Outcome::Instrumental)),
            );
            assert!(matches!(app.lyrics.data, Loadable::NotLoaded));
            app.apply(Action::SignOut);
            app.lyrics_result(
                app.session.wrapping_sub(1),
                request,
                &id,
                Ok((Source::Server, Outcome::Instrumental)),
            );
            assert!(app.lyrics.song.is_none());
        });
    }
}
