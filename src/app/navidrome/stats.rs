use super::{App, Request};
use crate::{model::Page, navidrome::stats::Summary};

#[derive(Default)]
pub struct State {
    pub summary: Option<Summary>,
    pub loading: bool,
    pub error: Option<String>,
    active: bool,
    serial: u64,
}

impl App {
    pub(super) fn sync_stats(&mut self) {
        let active = self.connected && self.presentation.page() == &Page::Stats;
        if active == self.stats.active {
            return;
        }
        self.stats.active = active;
        if active {
            if self.stats.summary.is_none() && self.stats.error.is_none() {
                self.refresh_stats();
            }
        } else {
            self.backend.send(Request::CancelStats);
            self.stats.serial = self.stats.serial.wrapping_add(1);
            self.stats.loading = false;
        }
    }

    pub(super) fn refresh_stats(&mut self) {
        if self.stats.loading {
            return;
        }
        self.stats.loading = true;
        self.stats.error = None;
        self.stats.serial = self.stats.serial.wrapping_add(1);
        if let Some(client) = &self.client {
            self.backend.send(Request::Stats {
                session: self.session,
                request: self.stats.serial,
                client: client.clone(),
            });
        }
        #[cfg(feature = "navidrome-demo")]
        if self.demo {
            self.configure_demo_stats(&[]);
        }
    }

    pub(super) fn stats_result(
        &mut self,
        session: u64,
        request: u64,
        result: Result<Summary, String>,
    ) {
        if session != self.session || request != self.stats.serial || !self.stats.active {
            return;
        }
        self.stats.loading = false;
        match result {
            Ok(summary) => {
                self.stats.summary = Some(summary);
                self.stats.error = None;
            }
            Err(error) => self.stats.error = Some(error),
        }
    }

    #[cfg(feature = "navidrome-demo")]
    pub(super) fn configure_demo_stats(&mut self, flags: &[&str]) {
        use crate::navidrome::stats::Aggregate;
        let mut songs = crate::demo::album(0).song;
        if flags.contains(&"empty") {
            for song in &mut songs {
                song.play_count = Some(0);
            }
        }
        if flags.contains(&"partial") {
            songs[0].duration = 0.;
            songs[1].play_count = None;
        }
        if flags.contains(&"unavailable") {
            for song in &mut songs {
                song.play_count = None;
            }
        }
        let mut aggregate = Aggregate::default();
        aggregate.page(songs).unwrap();
        self.stats.active = true;
        self.stats.loading = flags.contains(&"loading");
        self.stats.summary =
            (!self.stats.loading).then(|| aggregate.finish("2026-11-07 12:30 UTC".into()));
        self.stats.error = flags.contains(&"error").then(|| {
            "The server did not respond in time. The previous summary is still shown.".into()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::{Action, tests::with_app};

    #[test]
    fn failed_refresh_keeps_completed_summary_and_stale_results_cannot_replace_it() {
        with_app(|app, _| {
            app.stats.active = true;
            app.stats.serial = 3;
            app.stats.loading = true;
            app.stats_result(
                app.session,
                3,
                Ok(Summary {
                    total_plays: 42,
                    refreshed: "previous".into(),
                    ..Default::default()
                }),
            );
            app.stats.serial = 4;
            app.stats.loading = true;
            app.stats_result(app.session, 3, Ok(Summary::default()));
            assert!(app.stats.loading);
            app.stats_result(app.session, 4, Err("offline".into()));
            assert!(!app.stats.loading);
            assert_eq!(app.stats.summary.as_ref().unwrap().total_plays, 42);
            assert_eq!(app.stats.summary.as_ref().unwrap().refreshed, "previous");
            assert_eq!(app.stats.error.as_deref(), Some("offline"));
            app.presentation.open(Page::Home);
            app.sync_stats();
            app.stats_result(app.session, 4, Ok(Summary::default()));
            assert_eq!(app.stats.summary.as_ref().unwrap().total_plays, 42);
            let session = app.session;
            app.apply(Action::SignOut);
            app.stats_result(session, 0, Ok(Summary::default()));
            assert!(app.stats.summary.is_none());
        });
    }
}
