//! A single download job shares the library runtime; disk work never runs on the UI.
use super::*;
use crate::{
    navidrome::api::{Client, Collection, CollectionKind},
    settings::ProxyConfig,
};
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, watch};

pub enum Action {
    Open {
        root: PathBuf,
        client: Client,
        proxy: ProxyConfig,
        online: bool,
    },
    Keep {
        selection: Selection,
        songs: Option<Vec<Song>>,
    },
    Pause(String),
    PauseSong(String),
    RetrySong(String),
    Retry(String),
    Remove(String),
    Online(bool),
    Close,
}
pub struct Command {
    pub session: u64,
    pub revision: u64,
    pub action: Action,
}
pub enum Update {
    Library(Box<Library>),
    Track { id: String, status: Status },
    Error(String),
}
#[derive(Default)]
pub(crate) struct Available {
    pub scope: String,
    pub root: PathBuf,
    pub songs: BTreeMap<String, Song>,
}
pub(crate) type Shared = Arc<RwLock<Available>>;
pub(crate) fn local(shared: &Shared, scope: &str, id: &str) -> Option<PathBuf> {
    let available = shared.read().unwrap_or_else(|p| p.into_inner());
    (available.scope == scope && available.songs.contains_key(id))
        .then(|| audio_path(&available.root, id))
}
type Notify = Arc<dyn Fn(u64, u64, Update) + Send + Sync>;
enum JobKind {
    Selection(String),
    Song(String),
}
enum Outcome {
    Selection(Vec<Song>),
    Song(transfer::Completed),
}
struct Job {
    serial: u64,
    kind: JobKind,
    cancel: watch::Sender<bool>,
    task: tokio::task::JoinHandle<Result<Outcome, String>>,
}
struct Worker {
    session: u64,
    revision: u64,
    root: PathBuf,
    client: Client,
    http: reqwest::Client,
    library: Library,
    online: bool,
    writable: bool,
    job: Option<Job>,
    available: Shared,
    notify: Notify,
    progress: mpsc::Sender<transfer::Progress>,
}

pub(crate) fn spawn(
    available: Shared,
    notify: Notify,
) -> (mpsc::UnboundedSender<Command>, tokio::task::JoinHandle<()>) {
    let (send, mut receive) = mpsc::unbounded_channel::<Command>();
    let (progress, mut changes) = mpsc::channel(8);
    let task = tokio::spawn(async move {
        let mut active: Option<Worker> = None;
        loop {
            tokio::select! {
                command = receive.recv() => {
                    let Some(command) = command else { break };
                    if matches!(command.action, Action::Open { .. } | Action::Close) {
                        if let Some(old) = &mut active { old.cancel().await; }
                        active = None;
                        *available.write().unwrap_or_else(|p| p.into_inner()) = Available::default();
                        if matches!(command.action, Action::Open { .. }) {
                            let (session, revision) = (command.session, command.revision);
                            match Worker::open(command, available.clone(), notify.clone(), progress.clone()).await {
                                Ok(worker) => {
                                    worker.refresh_available();
                                    worker.snapshot();
                                    active = Some(worker);
                                }
                                Err(error) => notify(session, revision, Update::Error(error)),
                            }
                        }
                    } else if let Some(worker) = &mut active && worker.session == command.session {
                        worker.revision = command.revision;
                        worker.action(command.action).await;
                    }
                }
                result = async {
                    match active.as_mut().and_then(|w| w.job.as_mut()) {
                        Some(job) => (&mut job.task).await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(worker) = &mut active {
                        let job = worker.job.take().unwrap();
                        let result = result.unwrap_or_else(|_| Err("The download worker stopped. Try again.".into()));
                        worker.finished(job.kind, result).await;
                    }
                }
                progress = changes.recv() => {
                    if let (Some(worker), Some((serial, id, bytes, total))) = (&mut active, progress)
                        && worker.job.as_ref().is_some_and(|j| j.serial == serial)
                        && matches!(worker.job.as_ref().map(|j| &j.kind), Some(JobKind::Song(current)) if current == &id)
                        && let Some(track) = worker.library.tracks.get_mut(&id)
                    {
                        track.status = Status::Downloading { bytes, total };
                        worker.emit(Update::Track { id, status: Status::Downloading { bytes, total } });
                    }
                }
            }
            if let Some(worker) = &mut active {
                worker.start();
            }
        }
        if let Some(worker) = &mut active {
            worker.cancel().await;
        }
    });
    (send, task)
}
async fn disk<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| "The download files could not be updated.".to_owned())?
}
impl Worker {
    async fn open(
        command: Command,
        available: Shared,
        notify: Notify,
        progress: mpsc::Sender<transfer::Progress>,
    ) -> Result<Self, String> {
        let Action::Open {
            root,
            client,
            proxy,
            online,
        } = command.action
        else {
            return Err("No download account selected.".into());
        };
        let root = storage::account_root(&root, &client.scope)?;
        let path = root.clone();
        let scope = client.scope.clone();
        let library = disk(move || storage::load(&path, &scope)).await?;
        let http = crate::http::client_builder(&proxy)?
            .connect_timeout(std::time::Duration::from_secs(8))
            .read_timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.without_url().to_string())?;
        Ok(Self {
            session: command.session,
            revision: command.revision,
            root,
            client,
            http,
            library,
            online,
            writable: true,
            job: None,
            available,
            notify,
            progress,
        })
    }

    fn emit(&self, update: Update) {
        (self.notify)(self.session, self.revision, update)
    }
    fn snapshot(&self) {
        self.emit(Update::Library(Box::new(self.library.clone())))
    }
    fn refresh_available(&self) {
        let mut available = self.available.write().unwrap_or_else(|p| p.into_inner());
        available.scope = self.library.scope.clone();
        available.root = self.root.clone();
        available.songs = self
            .library
            .tracks
            .iter()
            .filter(|(_, t)| t.status.ready())
            .map(|(id, t)| (id.clone(), t.song.clone()))
            .collect();
    }
    async fn save(&mut self) -> bool {
        let root = self.root.clone();
        let library = self.library.clone();
        match disk(move || storage::persist(&root, &library)).await {
            Ok(()) => {
                self.writable = true;
                true
            }
            Err(error) => {
                self.writable = false;
                self.emit(Update::Error(error));
                false
            }
        }
    }
    async fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.cancel.send(true);
            let _ = job.task.await;
            if let JobKind::Song(id) = job.kind
                && self.library.wanted(&id)
                && let Some(track) = self.library.tracks.get_mut(&id)
                && matches!(track.status, Status::Downloading { .. })
            {
                track.status = Status::Queued;
                self.emit(Update::Track {
                    id,
                    status: Status::Queued,
                });
            }
        }
    }
    async fn action(&mut self, action: Action) {
        let mut removed = Vec::new();
        let mut retried = Vec::new();
        match action {
            Action::Keep { selection, songs } => {
                if let Err(error) = self.library.keep(selection, songs) {
                    self.emit(Update::Error(error));
                    return;
                }
            }
            Action::Pause(key) => self.library.pause(&key),
            Action::PauseSong(id) => {
                if let Some(track) = self.library.tracks.get_mut(&id)
                    && !track.status.ready()
                {
                    track.status = Status::Cancelled;
                    let root = self.root.clone();
                    let saved = id.clone();
                    if let Err(error) =
                        disk(move || storage::record_failure(&root, &saved, &Status::Cancelled))
                            .await
                    {
                        self.emit(Update::Error(error));
                    }
                }
            }
            Action::RetrySong(id) => {
                if self.library.wanted(&id)
                    && let Some(track) = self.library.tracks.get_mut(&id)
                    && !track.status.ready()
                {
                    track.status = Status::Queued;
                    retried.push(id);
                }
            }
            Action::Retry(key) => retried = self.library.retry(&key),
            Action::Remove(key) => removed = self.library.remove(&key),
            Action::Online(online) => self.online = online,
            Action::Open { .. } | Action::Close => return,
        }
        let cancel = self.job.as_ref().is_some_and(|job| {
            !self.online
                || match &job.kind {
                    JobKind::Song(id) => {
                        !self.library.wanted(id)
                            || self
                                .library
                                .tracks
                                .get(id)
                                .is_some_and(|t| matches!(t.status, Status::Cancelled))
                    }
                    JobKind::Selection(key) => {
                        self.library.selections.get(key).is_none_or(|s| s.paused)
                    }
                }
        });
        if cancel {
            self.cancel().await;
        }
        if self.save().await {
            let root = self.root.clone();
            if let Err(error) = disk(move || {
                for id in removed {
                    storage::remove_track(&root, &id)?;
                }
                for id in retried {
                    storage::clear_record(&root, &id)?;
                }
                Ok(())
            })
            .await
            {
                self.emit(Update::Error(error));
            }
        }
        self.refresh_available();
        self.snapshot();
    }
    fn start(&mut self) {
        if !self.online || !self.writable || self.job.is_some() {
            return;
        }
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (cancel, mut cancelled) = watch::channel(false);
        if let Some((key, selection)) = self
            .library
            .selections
            .iter()
            .find(|(_, s)| s.pending && !s.paused && s.error.is_none())
        {
            let kind = selection.kind;
            let id = selection.id.clone();
            let client = self.client.clone();
            let key = key.clone();
            let task = tokio::spawn(async move {
                tokio::select! {
                    biased;
                    _=cancelled.changed()=>Err("Download cancelled.".into()),
                    result=resolve(&client,kind,&id)=>result.map(Outcome::Selection),
                }
            });
            self.job = Some(Job {
                serial,
                kind: JobKind::Selection(key),
                cancel,
                task,
            });
            return;
        }
        let Some(track) = self.library.next_song() else {
            return;
        };
        let song = track.song.clone();
        let id = song.id.clone();
        let client = self.client.clone();
        let http = self.http.clone();
        let root = self.root.clone();
        let progress = self.progress.clone();
        self.library.tracks.get_mut(&id).unwrap().status = Status::Downloading {
            bytes: 0,
            total: song.size,
        };
        self.emit(Update::Track {
            id: id.clone(),
            status: Status::Downloading {
                bytes: 0,
                total: song.size,
            },
        });
        let task = tokio::spawn(async move {
            transfer::download(client, http, song, root, cancelled, progress, serial)
                .await
                .map(Outcome::Song)
        });
        self.job = Some(Job {
            serial,
            kind: JobKind::Song(id),
            cancel,
            task,
        });
    }
    async fn finished(&mut self, kind: JobKind, result: Result<Outcome, String>) {
        match kind {
            JobKind::Selection(key) => {
                if !self.library.selections.contains_key(&key) {
                    return;
                }
                let result = match result {
                    Ok(Outcome::Selection(songs)) => self.library.resolve(&key, songs),
                    Err(error) => Err(error),
                    _ => Err("Could not read this collection.".into()),
                };
                if let Err(error) = result {
                    self.library.selections.get_mut(&key).unwrap().error = Some(error);
                }
                self.save().await;
                self.snapshot();
            }
            JobKind::Song(id) => {
                if !self.library.wanted(&id) {
                    return;
                }
                let root = self.root.clone();
                let track = id.clone();
                let result = match result {
                    Ok(Outcome::Song(done)) => {
                        disk(move || storage::publish(&root, &track, done.bytes, done.codec)).await
                    }
                    Err(error) => Err(error),
                    _ => Err("Could not complete the download.".into()),
                };
                let status = match result {
                    Ok(status) => status,
                    Err(message) => {
                        let status = Status::Failed { message };
                        let root = self.root.clone();
                        let track = id.clone();
                        let record = status.clone();
                        if let Err(error) =
                            disk(move || storage::record_failure(&root, &track, &record)).await
                        {
                            self.writable = false;
                            self.emit(Update::Error(error));
                        }
                        status
                    }
                };
                if let Some(track) = self.library.tracks.get_mut(&id) {
                    track.status = status.clone();
                }
                if status.ready() {
                    let song = self.library.tracks[&id].song.clone();
                    self.available
                        .write()
                        .unwrap_or_else(|p| p.into_inner())
                        .songs
                        .insert(id.clone(), song);
                }
                self.emit(Update::Track { id, status });
            }
        }
    }
}
async fn resolve(client: &Client, kind: Kind, id: &str) -> Result<Vec<Song>, String> {
    match kind {
        Kind::Song => client
            .song(id)
            .await?
            .map(|s| vec![s])
            .ok_or_else(|| "This song is no longer available.".into()),
        Kind::Album => Ok(client.album(id).await?.song),
        Kind::Playlist => match client.collection(CollectionKind::Playlist, id).await? {
            Collection::Playlist { songs, .. } => Ok(songs),
            _ => Err("This playlist is unavailable.".into()),
        },
    }
}
