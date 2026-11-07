//! Sharing is explicit, account-scoped and serialized. Reads cannot undo writes.
use super::{App, favorites::Item};
use crate::{
    model::Dialog,
    navidrome::{
        api::RequestError,
        backend::Request,
        shares::{Outcome, Share, Task},
    },
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Action {
    Open(Option<String>),
    Expiry(u8),
    Create,
    Revoke(String),
    Refresh,
    RetryRevoke,
    DismissRevoke,
}

pub struct Target {
    pub id: String,
    pub name: String,
}

pub struct State {
    pub target: Option<Target>,
    pub revision: u64,
    pub days: u8,
    pub allowed: Option<bool>,
    pub list: Vec<Share>,
    pub error: Option<String>,
    pub copied: Option<String>,
    pub failed_revoke: Option<String>,
    pub creating: bool,
    pub uncertain_creation: bool,
    pub loading: bool,
    pub writing: bool,
    // Retain a confirmed create, or hide a revoked row, until the read catches up.
    added: HashMap<String, Share>,
    removed: HashMap<String, Share>,
    serial: u64,
    flight: Option<(u64, Task)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            target: None,
            revision: 0,
            days: 7,
            allowed: None,
            list: Vec::new(),
            error: None,
            copied: None,
            failed_revoke: None,
            creating: false,
            uncertain_creation: false,
            loading: false,
            writing: false,
            added: HashMap::new(),
            removed: HashMap::new(),
            serial: 0,
            flight: None,
        }
    }
}

impl State {
    pub fn copy_url(&self, id: &str) -> Option<&str> {
        self.list.iter().find(|s| s.id == id)?.url.as_deref()
    }
    pub fn busy(&self) -> bool {
        self.loading || self.writing
    }
    pub fn can_dismiss_revoke(&self) -> bool {
        self.failed_revoke
            .as_ref()
            .is_some_and(|id| !self.removed.contains_key(id))
    }
    fn begin(&mut self, task: Task) -> u64 {
        self.serial = self.serial.wrapping_add(1);
        self.loading = matches!(task, Task::List);
        self.writing = !self.loading;
        self.creating = matches!(task, Task::Create { .. });
        self.flight = Some((self.serial, task));
        self.error = None;
        self.serial
    }
    fn result(&mut self, token: u64, result: Result<Outcome, RequestError>) -> bool {
        if !self.flight.as_ref().is_some_and(|(id, _)| *id == token) {
            return false;
        }
        let (_, task) = self.flight.take().unwrap();
        self.revision = self.revision.wrapping_add(1);
        self.loading = false;
        self.writing = false;
        self.creating = false;
        match result {
            Ok(Outcome::Listed {
                allowed,
                mut shares,
            }) => {
                self.allowed = Some(allowed);
                // A permission check without getShares says nothing about whether
                // an uncertain write reached the server.
                if !allowed {
                    return false;
                }
                self.uncertain_creation = false;
                let ids: std::collections::HashSet<_> =
                    shares.iter().map(|s| s.id.as_str()).collect();
                if self
                    .failed_revoke
                    .as_ref()
                    .is_some_and(|id| !ids.contains(id.as_str()))
                {
                    self.failed_revoke = None;
                }
                self.added.retain(|id, _| !ids.contains(id.as_str()));
                self.removed.retain(|id, _| ids.contains(id.as_str()));
                shares.retain(|s| !self.removed.contains_key(&s.id));
                shares.extend(self.added.values().cloned());
                shares.sort_by(|a, b| a.id.cmp(&b.id));
                self.list = shares;
            }
            Ok(Outcome::Created(share)) => {
                self.list.retain(|s| s.id != share.id);
                self.added.insert(share.id.clone(), share.clone());
                self.list.insert(0, share);
                self.target = None;
                return true;
            }
            Ok(Outcome::Revoked) => {
                self.failed_revoke = None;
                return true;
            }
            Err(error) => {
                if matches!(task, Task::Create { .. })
                    && matches!(error, RequestError::Unconfirmed(_))
                {
                    self.uncertain_creation = true;
                }
                if let Task::Revoke(id) = task {
                    self.failed_revoke = Some(id.clone());
                    if !matches!(error, RequestError::Unconfirmed(_))
                        && let Some(share) = self.removed.remove(&id)
                    {
                        self.list.insert(0, share);
                    }
                }
                self.error = Some(error.to_string());
            }
        }
        false
    }
}

impl App {
    pub(crate) fn share_action(&mut self, action: Action) {
        if !self.connected {
            return;
        }
        if let Action::Open(uri) = action {
            if !self.shares.writing {
                self.shares.target = uri.and_then(|uri| match self.favorite_item(&uri)? {
                    Item::Song(s) => Some(Target {
                        id: s.id,
                        name: s.title,
                    }),
                    Item::Album(a) => Some(Target {
                        id: a.id,
                        name: a.name,
                    }),
                    Item::Artist(_) => None,
                });
                self.shares.copied = None;
            }
            self.presentation.dialog = Some(Dialog::Shares);
            if !self.shares.busy() {
                self.send_share_task(Task::List);
            }
            return;
        }
        if self.shares.busy() {
            return;
        }
        match action {
            Action::Expiry(days) if [1, 7, 30].contains(&days) => self.shares.days = days,
            Action::Create
                if self.shares.allowed == Some(true) && !self.shares.uncertain_creation =>
            {
                if let Some(target) = &self.shares.target {
                    self.send_share_task(Task::Create {
                        id: target.id.clone(),
                        name: target.name.clone(),
                        expires: jiff::Timestamp::now().as_millisecond()
                            + i64::from(self.shares.days) * 86_400_000,
                    });
                }
            }
            Action::Revoke(id)
                if self.shares.allowed == Some(true) && self.shares.failed_revoke.is_none() =>
            {
                self.revoke_share(id)
            }
            Action::RetryRevoke => {
                if let Some(id) = self.shares.failed_revoke.clone() {
                    self.revoke_share(id);
                }
            }
            Action::DismissRevoke if self.shares.can_dismiss_revoke() => {
                self.shares.failed_revoke = None;
                self.shares.error = None;
            }
            Action::Refresh => self.send_share_task(Task::List),
            _ => {}
        }
    }

    fn revoke_share(&mut self, id: String) {
        if let Some(index) = self.shares.list.iter().position(|s| s.id == id) {
            let share = self.shares.list.remove(index);
            self.shares.removed.insert(id.clone(), share);
        } else if !self.shares.removed.contains_key(&id) {
            return;
        }
        self.shares.revision = self.shares.revision.wrapping_add(1);
        self.shares.added.remove(&id);
        self.shares.copied = None;
        self.send_share_task(Task::Revoke(id));
    }

    fn send_share_task(&mut self, task: Task) {
        let request = self.shares.begin(task.clone());
        if let Some(client) = &self.client {
            self.backend.send(Request::Shares {
                session: self.session,
                request,
                client: client.clone(),
                task: task.clone(),
            });
        }
        #[cfg(feature = "navidrome-demo")]
        if self.demo {
            self.demo_share_task(request, task);
        }
    }

    pub(super) fn share_result(
        &mut self,
        session: u64,
        request: u64,
        result: Result<Outcome, RequestError>,
    ) {
        if session == self.session && self.shares.result(request, result) {
            self.send_share_task(Task::List);
        }
    }

    #[cfg(feature = "navidrome-demo")]
    fn demo_share_task(&mut self, request: u64, task: Task) {
        let result = match task {
            Task::List => Outcome::Listed {
                allowed: true,
                shares: self.shares.list.clone(),
            },
            Task::Create { name, expires, .. } => Outcome::Created(Share {
                id: "demo-link".into(),
                name,
                url: Some("https://music.example/share/evening-listen".into()),
                expires: Some(jiff::Timestamp::from_millisecond(expires).unwrap()),
                expiration_known: true,
            }),
            Task::Revoke(_) => Outcome::Revoked,
        };
        self.share_result(self.session, request, Ok(result));
    }

    #[cfg(feature = "navidrome-demo")]
    pub(super) fn configure_demo_shares(&mut self, flags: &[&str]) {
        if !flags.iter().any(|f| f.starts_with("share-")) {
            return;
        }
        self.share_action(Action::Open(Some(
            crate::navidrome::api::FavoriteKind::Album.uri("alb0"),
        )));
        self.shares.list = [
            ("listen", "Fragments", Some("2030-11-14T12:30:00Z")),
            ("evening", "Evening drive", Some("2020-11-01T12:30:00Z")),
            ("weekend", "Weekend music", None),
        ]
        .into_iter()
        .map(|(id, name, expires)| Share {
            id: id.into(),
            name: name.into(),
            url: Some(format!("https://music.example/share/{id}")),
            expires: expires.map(|v| v.parse().unwrap()),
            expiration_known: true,
        })
        .collect();
        if flags.contains(&"share-created") {
            self.shares.target = None;
        }
        if flags.contains(&"share-disabled") {
            self.shares.allowed = Some(false);
            self.shares.list.clear();
        }
        if flags.contains(&"share-loading") {
            self.shares.loading = true;
            self.shares.allowed = None;
            self.shares.list.clear();
        }
        if flags.contains(&"share-error") {
            self.shares.error =
                Some("Your account does not have permission for this operation.".into());
        }
        if flags.contains(&"share-revoke-error") {
            self.shares.error =
                Some("The server did not respond in time. Revocation is not confirmed.".into());
            self.shares.failed_revoke = Some("listen".into());
            let removed = self.shares.list.remove(0);
            self.shares.removed.insert(removed.id.clone(), removed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::{Action as Service, tests::with_app};

    fn share() -> Share {
        Share {
            id: "link".into(),
            name: "Album".into(),
            url: Some("https://music.example/share/link?secret=a%2Bb".into()),
            expires: None,
            expiration_known: true,
        }
    }

    #[test]
    fn successful_creates_and_revocations_survive_stale_lists() {
        let mut state = State::default();
        let token = state.begin(Task::Create {
            id: "album".into(),
            name: "Album".into(),
            expires: 1,
        });
        assert!(state.result(token, Ok(Outcome::Created(share()))));
        let token = state.begin(Task::List);
        state.result(
            token,
            Ok(Outcome::Listed {
                allowed: true,
                shares: vec![],
            }),
        );
        assert_eq!(state.list.len(), 1);
        let token = state.begin(Task::List);
        state.result(
            token,
            Ok(Outcome::Listed {
                allowed: true,
                shares: vec![share()],
            }),
        );
        assert!(state.added.is_empty());
        state.removed.insert("link".into(), state.list.remove(0));
        let token = state.begin(Task::Revoke("link".into()));
        state.result(token, Ok(Outcome::Revoked));
        let token = state.begin(Task::List);
        state.result(
            token,
            Ok(Outcome::Listed {
                allowed: true,
                shares: vec![share()],
            }),
        );
        assert!(state.list.is_empty());
        let token = state.begin(Task::List);
        state.result(
            token,
            Ok(Outcome::Listed {
                allowed: true,
                shares: vec![],
            }),
        );
        assert!(state.removed.is_empty());
    }

    #[test]
    fn losing_permission_cannot_confirm_an_uncertain_revocation() {
        let mut state = State {
            failed_revoke: Some("link".into()),
            ..Default::default()
        };
        state.removed.insert("link".into(), share());
        let token = state.begin(Task::List);
        state.result(
            token,
            Ok(Outcome::Listed {
                allowed: false,
                shares: Vec::new(),
            }),
        );
        assert_eq!(state.failed_revoke.as_deref(), Some("link"));
        assert!(state.removed.contains_key("link"));
        assert!(!state.can_dismiss_revoke());
        let token = state.begin(Task::List);
        state.result(
            token,
            Ok(Outcome::Listed {
                allowed: true,
                shares: Vec::new(),
            }),
        );
        assert!(state.failed_revoke.is_none());
        assert!(state.removed.is_empty());
    }

    #[test]
    fn failed_create_keeps_selection_and_uncertain_results_require_refresh() {
        let mut state = State {
            target: Some(Target {
                id: "album".into(),
                name: "Album".into(),
            }),
            days: 30,
            ..Default::default()
        };
        let token = state.begin(Task::Create {
            id: "album".into(),
            name: "Album".into(),
            expires: 1,
        });
        state.result(token, Err(RequestError::Unconfirmed("timeout".into())));
        assert!(state.uncertain_creation);
        assert_eq!(state.target.as_ref().unwrap().id, "album");
        assert_eq!(state.days, 30);
        let token = state.begin(Task::List);
        state.result(
            token - 1,
            Ok(Outcome::Listed {
                allowed: false,
                shares: vec![],
            }),
        );
        assert!(state.loading && state.uncertain_creation);
        state.result(
            token,
            Ok(Outcome::Listed {
                allowed: true,
                shares: vec![share()],
            }),
        );
        assert!(!state.uncertain_creation);
    }

    #[test]
    fn failed_revocation_restores_only_definite_rejections_and_signout_discards_late_results() {
        with_app(|app, _| {
            // Disable only the deterministic demo responder, preserving the fake backend.
            app.demo = false;
            app.shares.allowed = Some(true);
            for uncertain in [false, true] {
                app.shares.failed_revoke = None;
                app.shares.list = vec![share()];
                app.share_action(Action::Revoke("link".into()));
                assert!(app.shares.list.is_empty());
                assert!(app.shares.writing);
                let token = app.shares.serial;
                let error = if uncertain {
                    RequestError::Unconfirmed("timeout".into())
                } else {
                    RequestError::Rejected("denied".into())
                };
                app.share_result(app.session, token, Err(error));
                assert_eq!(app.shares.list.is_empty(), uncertain);
                assert_eq!(app.shares.failed_revoke.as_deref(), Some("link"));
            }
            let session = app.session;
            app.apply(Service::SignOut);
            app.share_result(session, 0, Ok(Outcome::Created(share())));
            assert!(app.shares.list.is_empty());
        });
    }
}
