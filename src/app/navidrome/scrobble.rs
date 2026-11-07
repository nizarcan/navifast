//! Account and preference guards around worker-generated listening reports.
use super::{App, player};
use crate::navidrome::{
    api::RequestError,
    backend::{Event, Request},
    scrobble::{Context, Report},
};

#[derive(Clone, Debug)]
pub enum Action {
    Enabled(bool),
    Retry(u64),
    Dismiss(u64),
}

pub struct Failure {
    pub report: Report,
    pub message: String,
    pub retryable: bool,
}

#[derive(Default)]
pub struct State {
    pub epoch: u64,
    play: Option<(u64, u64)>,
    next_play: u64,
    request: u64,
    pub failures: std::collections::BTreeMap<u64, Failure>,
}

impl State {
    pub fn context(&mut self, session: u64, occurrence: u64, enabled: bool) -> Context {
        if self.play.is_none_or(|(entry, _)| entry != occurrence) {
            self.next_play = self.next_play.wrapping_add(1);
            self.play = Some((occurrence, self.next_play));
        }
        Context {
            session,
            epoch: self.epoch,
            occurrence: self.play.unwrap().1,
            enabled,
        }
    }
    pub fn reserve(&mut self, session: u64, enabled: bool) -> Context {
        self.next_play = self.next_play.wrapping_add(1);
        Context {
            session,
            epoch: self.epoch,
            occurrence: self.next_play,
            enabled,
        }
    }
    pub fn activate(&mut self, occurrence: u64, context: Context) {
        self.play = Some((occurrence, context.occurrence));
    }
    pub fn new_play(&mut self) {
        self.play = None;
    }
    pub fn reset(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.play = None;
        self.failures.clear();
    }
}

impl App {
    pub(super) fn scrobble_action(&mut self, action: Action) {
        match action {
            Action::Enabled(enabled) => {
                self.cancel_preparation();
                self.presentation.settings.scrobbling = enabled;
                self.scrobbles.epoch = self.scrobbles.epoch.wrapping_add(1);
                self.scrobbles.failures.clear();
                self.backend.send(Request::CancelScrobbles);
                self.sync_command(crate::navidrome::sync::worker::Action::Scrobbling(enabled));
                self.player.send(player::Command::Scrobbling {
                    epoch: self.scrobbles.epoch,
                    enabled,
                });
                self.mark_dirty();
            }
            Action::Dismiss(request) => {
                self.scrobbles.failures.remove(&request);
            }
            Action::Retry(request) if self.presentation.settings.scrobbling => {
                if self
                    .scrobbles
                    .failures
                    .get(&request)
                    .is_some_and(|f| f.retryable)
                    && let Some(failure) = self.scrobbles.failures.remove(&request)
                {
                    self.send_scrobble(failure.report);
                }
            }
            Action::Retry(_) => {}
        }
    }

    pub(super) fn listening_report(&mut self, context: Context, report: Report) {
        if self.connected
            && self.presentation.settings.scrobbling
            && context.session == self.session
            && context.epoch == self.scrobbles.epoch
        {
            self.send_scrobble(report);
        }
    }

    fn send_scrobble(&mut self, report: Report) {
        self.scrobbles.request = self.scrobbles.request.wrapping_add(1);
        if !self.demo && self.client.is_some() && report.submission {
            let key = format!(
                "{:016x}:{}:{}",
                self.sync.boot, report.occurrence, report.time
            );
            self.enqueue_sync(crate::navidrome::sync::Work::Listen { key, report });
            return;
        }
        if self.offline() {
            return;
        }
        if let Some(client) = &self.client {
            self.backend.send(Request::Scrobble {
                session: self.session,
                epoch: self.scrobbles.epoch,
                request: self.scrobbles.request,
                client: client.clone(),
                report,
            });
        }
    }

    pub(super) fn scrobble_event(&mut self, event: Event) {
        if let Event::Scrobbled {
            session,
            epoch,
            request,
            report,
            result: Err(error),
        } = event
            && session == self.session
            && epoch == self.scrobbles.epoch
            && self.presentation.settings.scrobbling
        {
            // Now-playing is ephemeral. Its next real start will try again; never
            // replay a failed notification after another song has started.
            let retryable = report.submission && !matches!(error, RequestError::Unconfirmed(_));
            if self.scrobbles.failures.len() == 8 {
                self.scrobbles.failures.pop_first();
            }
            self.scrobbles.failures.insert(
                request,
                Failure {
                    report,
                    message: error.to_string(),
                    retryable,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "navidrome-demo")]
impl App {
    pub(super) fn demo_scrobble_error(&mut self, uncertain: bool) {
        self.scrobble_event(Event::Scrobbled {
            session: self.session,
            epoch: self.scrobbles.epoch,
            request: 1,
            report: Report {
                id: "track0".into(),
                title: "Rosewood".into(),
                occurrence: 1,
                time: 1,
                submission: true,
            },
            result: Err(if uncertain {
                RequestError::Unconfirmed("The server did not reply.".into())
            } else {
                RequestError::Rejected("Your account could not submit this listen.".into())
            }),
        });
    }
}
