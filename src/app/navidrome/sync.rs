//! Optimistic views over the durable worker journal, guarded by account and revision.
use super::App;
use crate::navidrome::{
    backend::Request,
    sync::{
        Journal, Work,
        worker::{Action, Command, Update},
    },
};

pub struct State {
    pub journal: Option<Journal>,
    pub error: Option<String>,
    pub loading: bool,
    pub failures: Vec<String>,
    pub listens: usize,
    waiting: std::collections::BTreeMap<String, Work>,
    revision: u64,
    // A boot nonce plus the playback occurrence makes distinct repeated songs durable.
    pub(super) boot: u64,
}

#[cfg(feature = "navidrome-demo")]
impl App {
    pub(super) fn demo_sync(&mut self, flags: &[&str]) {
        if !flags.contains(&"sync") {
            return;
        }
        use crate::navidrome::{ratings::Kind, scrobble::Report, sync::Status};
        let mut journal = Journal::new("demo".into());
        journal.last_sync = Some(1_794_051_000);
        if flags.contains(&"sync-pending") || flags.contains(&"sync-error") {
            journal
                .enqueue(Work::Rating {
                    kind: Kind::Song,
                    id: "track0".into(),
                    name: "Rosewood".into(),
                    value: 4,
                })
                .unwrap();
            journal
                .enqueue(Work::Listen {
                    key: "demo:1".into(),
                    report: Report {
                        id: "track1".into(),
                        title: "Cirrus".into(),
                        occurrence: 1,
                        time: 1,
                        submission: true,
                    },
                })
                .unwrap();
        }
        if flags.contains(&"sync-error") {
            for entry in journal.pending.values_mut() {
                entry.status = if matches!(entry.work, Work::Listen { .. }) {
                    Status::Uncertain("The server did not reply.".into())
                } else {
                    Status::Failed("This item is no longer available on the server.".into())
                };
            }
        }
        if flags.contains(&"sync-storage") {
            self.sync.error = Some("Changes could not be saved. Keep the app open, free disk space or fix folder permissions, then retry.".into());
        }
        self.sync.journal = Some(journal);
        self.sync.summarize();
    }
}
impl Default for State {
    fn default() -> Self {
        Self {
            journal: None,
            error: None,
            loading: false,
            failures: Vec::new(),
            listens: 0,
            waiting: Default::default(),
            revision: 0,
            boot: rand::random(),
        }
    }
}
impl App {
    pub(super) fn open_sync(&mut self) {
        self.sync = State::default();
        let Some(client) = self.client.clone() else {
            return;
        };
        if self.demo {
            return;
        }
        self.sync.loading = true;
        self.sync_command(Action::Open {
            root: self.dirs.state.join("sync"),
            client,
            scrobbling: self.presentation.settings.scrobbling,
        });
    }
    pub(super) fn sync_command(&mut self, action: Action) {
        if self.demo || self.client.is_none() {
            return;
        }
        self.sync.revision = self.sync.revision.wrapping_add(1);
        self.backend.send(Request::Sync(Command {
            session: self.session,
            revision: self.sync.revision,
            action,
        }));
    }
    pub(super) fn enqueue_sync(&mut self, work: Work) -> bool {
        let Some(journal) = &mut self.sync.journal else {
            if self.sync.waiting.len() >= 10_000 && !self.sync.waiting.contains_key(&work.key()) {
                self.sync.error =
                    Some("Too many changes are waiting. Sync pending changes first.".into());
                return false;
            }
            self.sync.waiting.insert(work.key(), work);
            return true;
        };
        if let Err(error) = journal.enqueue(work.clone()) {
            self.sync.error = Some(error);
            return false;
        }
        self.sync_command(Action::Enqueue(work));
        true
    }
    pub(crate) fn retry_sync(&mut self, key: Option<String>) {
        if self.sync.journal.is_none() {
            if let Some(client) = self.client.clone() {
                self.sync.loading = true;
                self.sync_command(Action::Open {
                    root: self.dirs.state.join("sync"),
                    client,
                    scrobbling: self.presentation.settings.scrobbling,
                });
            }
        } else {
            self.sync_command(Action::Retry(key));
        }
    }
    pub(crate) fn dismiss_sync(&mut self, key: String) {
        if let Some(journal) = &mut self.sync.journal
            && journal.pending.get(&key).is_some_and(|e| {
                matches!(
                    e.status,
                    crate::navidrome::sync::Status::Failed(_)
                        | crate::navidrome::sync::Status::Waiting(_)
                        | crate::navidrome::sync::Status::Uncertain(_)
                )
            })
            && let Some(entry) = journal.pending.remove(&key)
        {
            match entry.work {
                Work::Favorite { item, .. } => {
                    self.presentation
                        .navidrome_favorites
                        .discard(&item, journal.favorites.contains(&item));
                    self.refresh_favorites();
                }
                Work::Rating { kind, id, .. } => {
                    self.ratings.release(&kind.uri(&id));
                    if let Some(Work::Rating {
                        kind,
                        id,
                        name,
                        value,
                    }) = journal.ratings.get(&key)
                    {
                        self.ratings
                            .hold(*kind, id.clone(), name.clone(), *value, false);
                    }
                }
                Work::Listen { .. } => {}
            }
        }
        self.sync_command(Action::Dismiss(key));
    }
    fn apply_sync_value(&mut self, work: &Work, pending: bool) {
        match work {
            Work::Favorite { item, starred } => {
                self.presentation
                    .navidrome_favorites
                    .hold(item.as_ref().clone(), *starred, pending)
            }
            Work::Rating {
                kind,
                id,
                name,
                value,
            } => self
                .ratings
                .hold(*kind, id.clone(), name.clone(), *value, pending),
            Work::Listen { .. } => {}
        }
    }
    pub(super) fn sync_update(&mut self, session: u64, revision: u64, update: Update) {
        if session != self.session || revision < self.sync.revision {
            return;
        }
        self.sync.loading = false;
        self.sync.error = update.error;
        let Some(journal) = update.journal else {
            return;
        };
        if self
            .client
            .as_ref()
            .is_none_or(|client| client.scope != journal.scope)
        {
            return;
        }
        if let Some(previous) = self.sync.journal.take() {
            for (key, entry) in previous.pending {
                if !journal.pending.contains_key(&key) {
                    self.apply_sync_value(&entry.work, false);
                }
            }
        } else {
            self.presentation
                .navidrome_favorites
                .restore(journal.favorites.clone());
            for work in journal.ratings.values() {
                self.apply_sync_value(work, false);
            }
        }
        for entry in journal.pending.values() {
            self.apply_sync_value(&entry.work, true);
        }
        self.sync.journal = Some(*journal);
        for work in std::mem::take(&mut self.sync.waiting).into_values() {
            if self.enqueue_sync(work.clone()) {
                self.apply_sync_value(&work, true);
            }
        }
        self.sync.summarize();
    }
}

impl State {
    pub fn pending(&self) -> usize {
        self.waiting.len() + self.journal.as_ref().map_or(0, |j| j.pending.len())
    }
    fn summarize(&mut self) {
        self.failures.clear();
        self.listens = 0;
        if let Some(journal) = &self.journal {
            for (key, entry) in &journal.pending {
                self.listens += usize::from(matches!(entry.work, Work::Listen { .. }));
                if self.failures.len() < 8
                    && matches!(
                        entry.status,
                        crate::navidrome::sync::Status::Failed(_)
                            | crate::navidrome::sync::Status::Waiting(_)
                            | crate::navidrome::sync::Status::Uncertain(_)
                    )
                {
                    self.failures.push(key.clone());
                }
            }
        }
    }
}
