//! One serialized sync job. Disk persistence precedes every network mutation.
use super::{Entry, Journal, Status, Work, network, storage};
use crate::navidrome::api::{Catalogue, Client};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};

pub enum Action {
    Open {
        root: PathBuf,
        client: Client,
        scrobbling: bool,
    },
    Enqueue(Work),
    Favorites(Catalogue),
    Retry(Option<String>),
    Dismiss(String),
    Scrobbling(bool),
    Close,
}
pub struct Command {
    pub session: u64,
    pub revision: u64,
    pub action: Action,
}
pub struct Update {
    pub journal: Option<Box<Journal>>,
    pub error: Option<String>,
}
type Notify = Arc<dyn Fn(u64, u64, Update) + Send + Sync>;
struct Job {
    key: String,
    serial: u64,
    task: JoinHandle<Result<(), crate::navidrome::api::RequestError>>,
}
struct Worker {
    session: u64,
    revision: u64,
    client: Client,
    path: PathBuf,
    journal: Journal,
    error: Option<String>,
    scrobbling: bool,
    due: BTreeMap<String, Instant>,
    job: Option<Job>,
    notify: Notify,
}
async fn disk<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| "The sync storage worker stopped.".to_owned())?
}
pub(crate) fn spawn(notify: Notify) -> (mpsc::UnboundedSender<Command>, JoinHandle<()>) {
    let (send, mut receive) = mpsc::unbounded_channel::<Command>();
    let task = tokio::spawn(async move {
        let mut active: Option<Worker> = None;
        loop {
            let deadline = active.as_ref().and_then(|w| w.deadline());
            tokio::select! {
                biased;
                command = receive.recv() => {
                    let Some(command) = command else { break };
                    match command.action {
                        Action::Open { root, client, scrobbling } => {
                            if let Some(old) = &mut active { old.close().await; }
                            active = None;
                            let session = command.session;
                            let revision = command.revision;
                            let scope = client.scope.clone();
                            let loaded = disk(move || {
                                let path = storage::path(&root, &scope)?;
                                Ok((storage::load(&path, &scope)?, path))
                            }).await;
                            match loaded {
                                Ok((journal, path)) => {
                                    let mut worker = Worker { session, revision, client, path, journal, scrobbling, due: BTreeMap::new(), job: None, error: None, notify: notify.clone() };
                                    worker.save().await;
                                    worker.emit();
                                    active = Some(worker);
                                }
                                Err(error) => notify(session, revision, Update { journal: None, error: Some(error) }),
                            }
                        }
                        Action::Close => {
                            if let Some(old) = &mut active { old.close().await; }
                            active = None;
                        }
                        action => if let Some(worker) = &mut active && worker.session == command.session {
                            worker.revision = command.revision;
                            worker.action(action).await;
                        },
                    }
                }
                result = async {
                    match active.as_mut().and_then(|w| w.job.as_mut()) {
                        Some(job) => (&mut job.task).await,
                        None => std::future::pending().await,
                    }
                } => if let Some(worker) = &mut active {
                    let job = worker.job.take().unwrap();
                    let result = result.unwrap_or_else(|_| Err(crate::navidrome::api::RequestError::Unconfirmed("Sync stopped before the server confirmed this change.".into())));
                    if worker.journal.finish(&job.key, job.serial, result) {
                        let attempt = worker.journal.pending[&job.key].attempts;
                        worker.due.insert(job.key, Instant::now() + Duration::from_secs(1 << attempt));
                    }
                    worker.save().await;
                    worker.emit();
                },
                _ = async { match deadline { Some(at) => tokio::time::sleep_until(at).await, None => std::future::pending().await } } => {}
            }
            // Drain queued user intent before choosing a network operation.
            if !receive.is_closed()
                && receive.is_empty()
                && let Some(worker) = &mut active
            {
                worker.start().await;
            }
        }
        if let Some(worker) = &mut active {
            worker.close().await;
        }
    });
    (send, task)
}
impl Worker {
    fn emit(&self) {
        (self.notify)(
            self.session,
            self.revision,
            Update {
                journal: Some(Box::new(self.journal.clone())),
                error: self.error.clone(),
            },
        );
    }
    async fn save(&mut self) -> bool {
        let path = self.path.clone();
        let journal = self.journal.clone();
        self.error = disk(move || storage::save(&path, &journal)).await.err();
        self.error.is_none()
    }
    async fn close(&mut self) {
        if let Some(job) = self.job.take() {
            job.task.abort();
            let _ = job.task.await;
        }
        self.journal.recover();
        self.save().await;
    }
    fn eligible(&self, entry: &Entry) -> bool {
        entry.status == Status::Pending
            && (self.scrobbling || !matches!(entry.work, Work::Listen { .. }))
    }
    fn deadline(&self) -> Option<Instant> {
        if self.client.is_offline() || self.job.is_some() || self.error.is_some() {
            return None;
        }
        self.journal
            .pending
            .iter()
            .filter(|(_, e)| self.eligible(e))
            .map(|(key, _)| self.due.get(key).copied().unwrap_or_else(Instant::now))
            .min()
    }
    async fn start(&mut self) {
        if self.client.is_offline() || self.job.is_some() || self.error.is_some() {
            return;
        }
        let next = self
            .journal
            .pending
            .iter()
            .filter(|(key, e)| {
                self.eligible(e) && self.due.get(*key).is_none_or(|at| *at <= Instant::now())
            })
            .min_by_key(|(_, e)| e.serial)
            .map(|(key, e)| (key.clone(), e.clone()));
        let Some((key, entry)) = next else { return };
        self.due.remove(&key);
        self.journal.pending.get_mut(&key).unwrap().status = Status::Sending;
        if !self.save().await {
            self.journal.pending.get_mut(&key).unwrap().status = Status::Pending;
            self.emit();
            return;
        }
        self.emit();
        let client = self.client.clone();
        self.job = Some(Job {
            key,
            serial: entry.serial,
            task: tokio::spawn(async move { network::execute(&client, &entry.work).await }),
        });
    }
    async fn action(&mut self, action: Action) {
        match action {
            Action::Enqueue(work) => {
                self.due.remove(&work.key());
                if let Err(error) = self.journal.enqueue(work) {
                    self.error = Some(error);
                    self.emit();
                    return;
                }
            }
            Action::Favorites(mut list) => {
                list.song.truncate(super::MAX_ENTRIES);
                list.album
                    .truncate(super::MAX_ENTRIES.saturating_sub(list.song.len()));
                list.artist.truncate(
                    super::MAX_ENTRIES.saturating_sub(list.song.len() + list.album.len()),
                );
                for album in &mut list.album {
                    album.song.clear();
                }
                self.journal.observe_favorites(list);
            }
            Action::Retry(key) => {
                self.journal.retry(key.as_deref());
                if let Some(key) = key {
                    self.due.remove(&key);
                } else {
                    self.due.clear();
                }
            }
            Action::Dismiss(key) => {
                if self.journal.pending.get(&key).is_some_and(|e| {
                    matches!(
                        e.status,
                        Status::Failed(_) | Status::Waiting(_) | Status::Uncertain(_)
                    )
                }) {
                    self.journal.pending.remove(&key);
                    self.due.remove(&key);
                }
            }
            Action::Scrobbling(enabled) => {
                self.scrobbling = enabled;
                if !enabled
                    && self.job.as_ref().is_some_and(|j| {
                        self.journal
                            .pending
                            .get(&j.key)
                            .is_some_and(|e| matches!(e.work, Work::Listen { .. }))
                    })
                {
                    let job = self.job.take().unwrap();
                    job.task.abort();
                    let _ = job.task.await;
                    self.journal.recover();
                }
            }
            Action::Open { .. } | Action::Close => unreachable!(),
        }
        self.save().await;
        self.emit();
    }
}
