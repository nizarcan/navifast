//! Cancellable API requests on a dedicated runtime.

use std::sync::{Arc, mpsc};

use super::api::{
    Album, ArtistInfo, Catalogue, Client, Collection, CollectionKind, CoverSize, FavoriteKind,
    NamedItem, RequestError, SearchPart,
};
use crate::credentials::{Grant, Lease, NavidromePassword, Slot, Store};

pub enum Request {
    Lyrics {
        session: u64,
        request: u64,
        client: Client,
        song: Box<super::api::Song>,
    },
    CancelLyrics,
    RestoreSession {
        session: u64,
        token: u64,
        path: std::path::PathBuf,
        client: Client,
    },
    CancelSessionRestore,
    SaveSession {
        path: std::path::PathBuf,
        snapshot: Option<Box<super::session::Session>>,
    },
    QueueAlbum {
        session: u64,
        token: u64,
        client: Client,
        id: String,
    },
    CancelQueueAlbum(u64),
    ClearQueueAlbums,
    PlaylistDestinations {
        session: u64,
        client: Client,
    },
    EditPlaylist {
        session: u64,
        job: usize,
        client: Client,
        task: super::playlists::Task,
    },
    Restore {
        session: u64,
    },
    Forget {
        session: u64,
    },
    Login {
        session: u64,
        server: String,
        username: String,
        password: String,
        proxy: crate::settings::ProxyConfig,
    },
    Albums {
        session: u64,
        request: u64,
        client: Client,
        offset: usize,
        newest: bool,
    },
    Album {
        session: u64,
        request: u64,
        client: Client,
        id: String,
    },
    Favorites {
        session: u64,
        request: u64,
        client: Client,
    },
    SetFavorite {
        session: u64,
        client: Client,
        kind: FavoriteKind,
        id: String,
        starred: bool,
    },
    Search {
        session: u64,
        request: u64,
        client: Client,
        query: String,
        part: SearchPart,
    },
    CancelSearch,
    PlaylistLibrary {
        session: u64,
        request: u64,
        client: Client,
    },
    CancelPlaylistLibrary,
    Collection {
        session: u64,
        request: u64,
        client: Client,
        kind: CollectionKind,
        id: String,
    },
    Appearances {
        session: u64,
        request: u64,
        client: Client,
        id: String,
        name: String,
        discography: std::collections::HashSet<String>,
    },
    ArtistInfo {
        session: u64,
        request: u64,
        client: Client,
        id: String,
    },
    CancelCollection,
    Cover {
        size: CoverSize,
        session: u64,
        client: Client,
        id: String,
        cached: Option<Arc<[u8]>>,
    },
    Save {
        path: std::path::PathBuf,
        json: String,
    },
    OpenThemesFolder(std::path::PathBuf),
    Cancel,
    Shutdown,
}

pub enum Event {
    Lyrics {
        session: u64,
        request: u64,
        song: String,
        result: Result<super::lyrics::Outcome, String>,
    },
    SessionRestored {
        session: u64,
        token: u64,
        result: Result<Option<Box<super::session::Restored>>, String>,
    },
    SessionSaveFailed,
    QueueAlbum {
        session: u64,
        token: u64,
        result: Result<Album, String>,
    },
    PlaylistDestinations {
        session: u64,
        result: Result<Vec<super::api::Playlist>, String>,
    },
    PlaylistEdited {
        session: u64,
        job: usize,
        task: Box<super::playlists::Task>,
        result: Result<super::playlists::Outcome, String>,
    },
    PlaylistLibrary {
        session: u64,
        request: u64,
        result: Result<Vec<NamedItem>, String>,
    },
    Restored {
        session: u64,
        result: Result<Option<NavidromePassword>, String>,
    },
    CredentialError {
        session: u64,
        message: String,
    },
    Login {
        session: u64,
        result: Result<Client, String>,
    },
    Albums {
        session: u64,
        request: u64,
        offset: usize,
        result: Result<Vec<Album>, String>,
    },
    Album {
        session: u64,
        request: u64,
        result: Result<Album, String>,
    },
    Favorites {
        session: u64,
        request: u64,
        result: Result<Catalogue, String>,
    },
    FavoriteWritten {
        session: u64,
        kind: FavoriteKind,
        id: String,
        starred: bool,
        result: Result<(), RequestError>,
    },
    Catalogue {
        session: u64,
        request: u64,
        result: Result<Catalogue, String>,
    },
    Playlists {
        session: u64,
        request: u64,
        result: Result<Vec<NamedItem>, String>,
    },
    Collection {
        session: u64,
        request: u64,
        result: Result<Collection, String>,
    },
    Appearances {
        session: u64,
        request: u64,
        result: Result<Vec<Album>, String>,
    },
    ArtistInfo {
        session: u64,
        request: u64,
        result: Result<ArtistInfo, String>,
    },
    Cover {
        size: CoverSize,
        session: u64,
        id: String,
        result: Result<Cover, String>,
    },
    SaveFailed,
}

/// Decoded straight-alpha sRGB, plus the source used to restore an evicted full image.
pub struct Cover {
    pub image: image::RgbaImage,
    pub preview: image::RgbaImage,
    pub bytes: Arc<[u8]>,
    pub color: Option<[u8; 3]>,
}

pub struct Backend {
    send: tokio::sync::mpsc::UnboundedSender<Request>,
    pub events: mpsc::Receiver<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Backend {
    pub fn spawn(wake: Arc<dyn Fn() + Send + Sync>, dirs: crate::paths::AppDirs) -> Self {
        Self::with_store(wake, move || {
            #[cfg(not(test))]
            let credentials = Store::new(dirs);
            #[cfg(test)]
            let credentials = Store::in_memory(dirs);
            credentials
        })
    }

    fn with_store(
        wake: Arc<dyn Fn() + Send + Sync>,
        create_store: impl FnOnce() -> Store + Send + 'static,
    ) -> Self {
        let (send, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let (events, receive) = mpsc::channel();
        let thread = std::thread::Builder::new().name("navifast-library".into()).spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("create Navidrome runtime");
            runtime.block_on(async move {
                let credentials = create_store();
                let permits = Arc::new(tokio::sync::Semaphore::new(4));
                let mut tasks = tokio::task::JoinSet::new();
                let mut collections = tokio::task::JoinSet::new();
                let mut searches = tokio::task::JoinSet::new();
                let mut playlists = tokio::task::JoinSet::new();
                let mut deletions = tokio::task::JoinSet::new();
                let mut lyrics = tokio::task::JoinSet::new();
                let mut restores = tokio::task::JoinSet::new();
                let mut queue_reads = tokio::task::JoinSet::new();
                let mut queue_handles: std::collections::HashMap<u64, tokio::task::AbortHandle> = std::collections::HashMap::new();
                loop {
                    tokio::select! {
                        request = requests.recv() => {
                            let Some(request) = request else { break; };
                            match request {
                                Request::Shutdown => break,
                                Request::Cancel => {
                                    credentials.invalidate(Slot::Navidrome);
                                    lyrics.abort_all();
                                    restores.abort_all();
                                    collections.abort_all();
                                    tasks.abort_all();
                                    searches.abort_all();
                                    playlists.abort_all();
                                    queue_reads.abort_all();
                                    queue_handles.clear();
                                }
                                Request::CancelLyrics => lyrics.abort_all(),
                                Request::CancelSessionRestore => restores.abort_all(),
                                Request::RestoreSession { session, token, path, client } => {
                                    restores.abort_all();
                                    let events = events.clone();
                                    let wake = wake.clone();
                                    let permits = permits.clone();
                                    restores.spawn(async move {
                                        let result = super::session::restore(path, client, permits).await.map(|s| s.map(Box::new));
                                        let _ = events.send(Event::SessionRestored { session, token, result });
                                        wake();
                                    });
                                }
                                Request::SaveSession { path, snapshot } => {
                                    let result = tokio::task::spawn_blocking(move || match snapshot {
                                        Some(snapshot) => snapshot.write(&path),
                                        None => match std::fs::remove_file(&path) {
                                            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                                            _ => Ok(()),
                                        },
                                    }).await;
                                    if !matches!(result, Ok(Ok(()))) { let _ = events.send(Event::SessionSaveFailed); wake(); }
                                }
                                Request::ClearQueueAlbums => {
                                    queue_reads.abort_all();
                                    queue_handles.clear();
                                }
                                Request::CancelQueueAlbum(token) => {
                                    if let Some(handle) = queue_handles.remove(&token) { handle.abort(); }
                                }
                                Request::QueueAlbum { session, token, client, id } => {
                                    let events = events.clone();
                                    let wake = wake.clone();
                                    let permits = permits.clone();
                                    let handle = queue_reads.spawn(async move {
                                        if let Ok(_permit) = permits.acquire().await {
                                            let result = client.album(&id).await;
                                            let _ = events.send(Event::QueueAlbum { session, token, result });
                                            wake();
                                        }
                                        token
                                    });
                                    queue_handles.insert(token, handle);
                                }
                                Request::CancelCollection => collections.abort_all(),
                                Request::CancelSearch => searches.abort_all(),
                                Request::CancelPlaylistLibrary => playlists.abort_all(),
                                Request::Forget { session } => {
                                    // Persist revocation off the UI thread before queuing deletion.
                                    // A locked keyring must not restore a signed-out account.
                                    let revoked = credentials.revoke(Slot::Navidrome);
                                    let deletion = credentials.lease(Slot::Navidrome).delete();
                                    let events = events.clone();
                                    let wake = wake.clone();
                                    deletions.spawn(async move {
                                        let deleted = deletion.await;
                                        if revoked.is_err() || deleted.is_err() {
                                            let _ = events.send(Event::CredentialError {
                                                session,
                                                message: if revoked.is_err() {
                                                    "The saved sign-in could not be forgotten. Check the application state directory permissions and unlock the system credential store before closing Navifast."
                                                } else {
                                                    "Automatic sign-in is disabled, but the saved password could not be removed. Unlock the system credential store and sign out again to remove it."
                                                }.into(),
                                            });
                                            wake();
                                        }
                                    });
                                }
                                Request::OpenThemesFolder(path) => {
                                    tokio::task::spawn_blocking(move || {
                                        if std::fs::create_dir_all(&path).is_ok() { let _ = open::that(path); }
                                    });
                                }
                                Request::Save { path, json } => {
                                    // Serialize writes in request order, outside the UI thread.
                                    let result = tokio::task::spawn_blocking(move || {
                                        let parent = path.parent().ok_or_else(|| std::io::Error::other("No settings directory"))?;
                                        std::fs::create_dir_all(parent)?;
                                        let temporary = path.with_extension("json.tmp");
                                        std::fs::write(&temporary, json)?;
                                        crate::util::replace_file(&temporary, &path)
                                    }).await;
                                    if !matches!(result, Ok(Ok(()))) { let _=events.send(Event::SaveFailed); wake(); }
                                }
                                request => {
                                    let events = events.clone();
                                    let wake = wake.clone();
                                    let permits = permits.clone();
                                    let lease = credentials.lease(Slot::Navidrome);
                                    let group = match request {
                                        Request::Lyrics { .. } => &mut lyrics,
                                        Request::Search { .. } => &mut searches,
                                        Request::PlaylistLibrary { .. } => &mut playlists,
                                        Request::Collection { .. } | Request::ArtistInfo { .. } | Request::Appearances { .. } => &mut collections,
                                        _ => &mut tasks,
                                    };
                                    group.spawn(async move {
                                        let Ok(_permit) = permits.acquire().await else { return; };
                                        let event = execute(request, &lease).await;
                                        // Queue persistence before showing success, without making
                                        // the library wait for an unlocked system credential store.
                                        let saving = match &event {
                                            Some(Event::Login { session, result: Ok(client) }) => {
                                                Some((*session, lease.save(Grant::Navidrome(client.saved_login()))))
                                            }
                                            _ => None,
                                        };
                                        if let Some(event) = event { let _ = events.send(event); wake(); }
                                        if let Some((session, saving)) = saving
                                            && let Err(error) = saving.await
                                            && lease.current()
                                        {
                                            let _ = events.send(Event::CredentialError { session, message: error.to_string() });
                                            wake();
                                        }
                                    });
                                }
                            }
                        }
                        result = queue_reads.join_next_with_id(), if !queue_reads.is_empty() => {
                            if let Some(Ok((task, token))) = result
                                && queue_handles.get(&token).is_some_and(|handle| handle.id() == task)
                            { queue_handles.remove(&token); }
                        }
                        _ = lyrics.join_next(), if !lyrics.is_empty() => {}
                        _ = restores.join_next(), if !restores.is_empty() => {}
                        _ = collections.join_next(), if !collections.is_empty() => {}
                        _ = searches.join_next(), if !searches.is_empty() => {}
                        _ = playlists.join_next(), if !playlists.is_empty() => {}
                        _ = tasks.join_next(), if !tasks.is_empty() => {}
                        _ = deletions.join_next(), if !deletions.is_empty() => {}
                    }
                }
                tasks.abort_all();
            });
            runtime.shutdown_timeout(std::time::Duration::from_secs(1));
        }).expect("start library worker");
        Self {
            send,
            events: receive,
            thread: Some(thread),
        }
    }

    pub fn send(&self, request: Request) {
        let _ = self.send.send(request);
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.send(Request::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn execute(request: Request, lease: &Lease) -> Option<Event> {
    Some(match request {
        Request::Lyrics {
            session,
            request,
            client,
            song,
        } => {
            let result = super::lyrics::fetch(&client, &song).await;
            Event::Lyrics {
                session,
                request,
                song: song.id,
                result,
            }
        }
        Request::Restore { session } => Event::Restored {
            session,
            result: lease
                .load()
                .await
                .map_err(|error| error.to_string())
                .and_then(|loaded| match loaded.grant {
                    Some(Grant::Navidrome(login)) => Ok(Some(login)),
                    None => Ok(None),
                    _ => Err("The stored sign-in is invalid. Sign in again.".into()),
                }),
        },
        Request::Login {
            session,
            server,
            username,
            password,
            proxy,
        } => {
            let result = async {
                let client = Client::new(&server, &username, password, &proxy)?;
                client.ping().await?;
                Ok(client)
            }
            .await;
            Event::Login { session, result }
        }
        Request::Albums {
            session,
            request,
            client,
            offset,
            newest,
        } => Event::Albums {
            session,
            request,
            offset,
            result: client.albums(offset, newest).await,
        },
        Request::Album {
            session,
            request,
            client,
            id,
        } => Event::Album {
            session,
            request,
            result: client.album(&id).await,
        },
        Request::Favorites {
            session,
            request,
            client,
        } => Event::Favorites {
            session,
            request,
            result: client.favorites().await,
        },
        Request::SetFavorite {
            session,
            client,
            kind,
            id,
            starred,
        } => Event::FavoriteWritten {
            session,
            kind,
            result: client.set_favorite(kind, &id, starred).await,
            id,
            starred,
        },
        Request::PlaylistDestinations { session, client } => Event::PlaylistDestinations {
            session,
            result: client.playlist_destinations().await,
        },
        Request::EditPlaylist {
            session,
            job,
            client,
            mut task,
        } => {
            let result = task.run(&client).await;
            Event::PlaylistEdited {
                session,
                job,
                task: Box::new(task),
                result,
            }
        }
        Request::PlaylistLibrary {
            session,
            request,
            client,
        } => Event::PlaylistLibrary {
            session,
            request,
            result: client.playlists().await,
        },
        Request::Search {
            session,
            request,
            client,
            query,
            part,
        } => match part {
            SearchPart::Catalogue => Event::Catalogue {
                session,
                request,
                result: client.search(&query).await,
            },
            SearchPart::Playlists => Event::Playlists {
                session,
                request,
                result: client.matching_playlists(&query).await,
            },
        },
        Request::Collection {
            session,
            request,
            client,
            kind,
            id,
        } => Event::Collection {
            session,
            request,
            result: client.collection(kind, &id).await,
        },
        Request::Appearances {
            session,
            request,
            client,
            id,
            name,
            discography,
        } => Event::Appearances {
            session,
            request,
            result: client.appearances(&id, &name, discography).await,
        },
        Request::ArtistInfo {
            session,
            request,
            client,
            id,
        } => Event::ArtistInfo {
            session,
            request,
            result: client.artist_info(&id).await,
        },
        Request::Cover {
            size,
            session,
            client,
            id,
            cached,
        } => {
            let bytes = match cached {
                Some(bytes) => Ok(bytes),
                None => client.cover(&id, size).await.map(Arc::from),
            };
            let result = match bytes {
                Ok(bytes) => tokio::task::spawn_blocking(move || decode_cover(bytes, size))
                    .await
                    .unwrap_or_else(|_| Err("Artwork could not be decoded.".into())),
                Err(error) => Err(error),
            };
            Event::Cover {
                size,
                session,
                id,
                result,
            }
        }
        Request::CancelLyrics
        | Request::RestoreSession { .. }
        | Request::CancelSessionRestore
        | Request::SaveSession { .. }
        | Request::Save { .. }
        | Request::Forget { .. }
        | Request::OpenThemesFolder(_)
        | Request::CancelCollection
        | Request::CancelSearch
        | Request::CancelPlaylistLibrary
        | Request::Cancel
        | Request::QueueAlbum { .. }
        | Request::CancelQueueAlbum(_)
        | Request::ClearQueueAlbums
        | Request::Shutdown => return None,
    })
}

fn decode_cover(bytes: Arc<[u8]>, size: CoverSize) -> Result<Cover, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(&*bytes))
        .with_guessed_format()
        .map_err(|_| "Invalid artwork.")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| "Unsupported or oversized artwork.")?;
    let edge = size.pixels();
    let image = if image.width().max(image.height()) > edge {
        image.thumbnail(edge, edge)
    } else {
        image
    };
    let preview = image.thumbnail(64, 64).into_rgba8();
    let image = image.into_rgba8();
    Ok(Cover {
        image,
        preview,
        color: crate::images::accent_color(&bytes),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: std::path::PathBuf,
        store: Store,
    }

    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("navifast-login-{:016x}", rand::random::<u64>()));
            let dirs = crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            };
            dirs.ensure().unwrap();
            Self {
                root,
                store: Store::in_memory(dirs),
            }
        }

        fn backend(&self) -> Backend {
            let store = self.store.clone();
            Backend::with_store(Arc::new(|| {}), move || store)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn receive(backend: &Backend) -> Event {
        backend
            .events
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
    }

    #[test]
    fn queued_album_reads_survive_navigation_but_clear_cancels_only_old_additions() {
        let fixture = Fixture::new();
        let (started, starts) = mpsc::channel();
        let (release, releases) = mpsc::channel();
        let server = crate::navidrome::test_server::Server::spawn(move |request| {
            assert!(request.starts_with("GET /rest/getAlbum.view?"));
            started.send(()).unwrap();
            releases
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            crate::navidrome::test_server::response("200 OK", "", br#"{"subsonic-response":{"status":"ok","album":{"id":"album","name":"Album","song":[{"id":"song","title":"Song"}]}}}"#)
        });
        let client = Client::new(
            &server.url,
            "listener",
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        let backend = fixture.backend();
        let request = |token| {
            backend.send(Request::QueueAlbum {
                session: 1,
                token,
                client: client.clone(),
                id: "album".into(),
            })
        };
        let started = || {
            starts
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
        };
        request(1);
        started();
        backend.send(Request::CancelCollection);
        release.send(()).unwrap();
        assert!(matches!(
            receive(&backend),
            Event::QueueAlbum {
                token: 1,
                result: Ok(_),
                ..
            }
        ));
        request(2);
        started();
        backend.send(Request::ClearQueueAlbums);
        // A completed independent request is a barrier after Clear was processed.
        backend.send(Request::Restore { session: 9 });
        assert!(matches!(
            receive(&backend),
            Event::Restored { session: 9, .. }
        ));
        release.send(()).unwrap();
        request(3);
        started();
        release.send(()).unwrap();
        assert!(matches!(
            receive(&backend),
            Event::QueueAlbum {
                token: 3,
                result: Ok(_),
                ..
            }
        ));
        assert!(backend.events.try_recv().is_err());
    }

    #[test]
    fn successful_login_is_restored_after_restart_and_forgotten_on_signout() {
        let fixture = Fixture::new();
        let server = crate::navidrome::test_server::Server::spawn(|request| {
            assert!(request.starts_with("GET /music/rest/ping.view?"));
            assert!(!request.contains("dummy-password"));
            crate::navidrome::test_server::response(
                "200 OK",
                "",
                br#"{"subsonic-response":{"status":"ok"}}"#,
            )
        });
        let backend = fixture.backend();
        backend.send(Request::Login {
            session: 1,
            server: format!("{}/music", server.url),
            username: " listener ".into(),
            password: "dummy-password".into(),
            proxy: crate::settings::ProxyConfig::Off,
        });
        assert!(matches!(
            receive(&backend),
            Event::Login {
                session: 1,
                result: Ok(_)
            }
        ));
        // Stopping the API runtime must not cancel the queued credential save.
        drop(backend);
        let backend = fixture.backend();
        backend.send(Request::Restore { session: 2 });
        let Event::Restored {
            session: 2,
            result: Ok(Some(login)),
        } = receive(&backend)
        else {
            panic!("the verified login should survive restart");
        };
        assert_eq!(login.server, format!("{}/music/", server.url));
        assert_eq!(login.username, "listener");
        assert_eq!(login.password, "dummy-password");
        backend.send(Request::Cancel);
        backend.send(Request::Forget { session: 3 });
        drop(backend);
        let backend = fixture.backend();
        backend.send(Request::Restore { session: 4 });
        assert!(matches!(
            receive(&backend),
            Event::Restored {
                session: 4,
                result: Ok(None)
            }
        ));
    }

    #[test]
    fn failed_login_does_not_replace_the_saved_account() {
        let fixture = Fixture::new();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let original = Grant::Navidrome(NavidromePassword {
            server: "https://music.example.com/".into(),
            username: "listener".into(),
            password: "dummy-original-password".into(),
        });
        runtime
            .block_on(fixture.store.lease(Slot::Navidrome).save(original.clone()))
            .unwrap();
        let server = crate::navidrome::test_server::Server::spawn(|_| {
            crate::navidrome::test_server::response("200 OK", "", br#"{"subsonic-response":{"status":"failed","error":{"code":40,"message":"private server text"}}}"#)
        });
        let backend = fixture.backend();
        backend.send(Request::Login {
            session: 1,
            server: server.url.clone(),
            username: "other-account".into(),
            password: "dummy-wrong-password".into(),
            proxy: crate::settings::ProxyConfig::Off,
        });
        let Event::Login {
            session: 1,
            result: Err(message),
        } = receive(&backend)
        else {
            panic!("invalid credentials must fail");
        };
        assert!(!message.contains("private server text"));
        assert!(
            runtime
                .block_on(fixture.store.lease(Slot::Navidrome).load())
                .unwrap()
                .grant
                == Some(original)
        );
    }

    #[tokio::test]
    async fn cached_cover_restores_without_a_server_and_preserves_aspect_ratio() {
        let fixture = Fixture::new();
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            600,
            300,
            image::Rgb([120, 40, 20]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
        let bytes: Arc<[u8]> = Arc::from(encoded.into_inner());
        // Nothing is listening here. A repeated getCoverArt request would fail.
        let client = Client::new(
            "http://127.0.0.1:1",
            "listener",
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        let Some(Event::Cover {
            size: CoverSize::Sleeve,
            session,
            id,
            result: Ok(cover),
        }) = execute(
            Request::Cover {
                size: CoverSize::Sleeve,
                session: 42,
                client,
                id: "cover:/🌍".into(),
                cached: Some(bytes.clone()),
            },
            &fixture.store.lease(Slot::Navidrome),
        )
        .await
        else {
            panic!("cached artwork should decode without contacting the server");
        };
        assert_eq!(session, 42);
        assert_eq!(id, "cover:/🌍");
        assert_eq!(cover.image.dimensions(), (300, 150));
        assert_eq!(cover.preview.dimensions(), (64, 32));
        assert!(Arc::ptr_eq(&bytes, &cover.bytes));
        assert!(cover.color.is_some());
        let portrait = decode_cover(bytes, CoverSize::Portrait).unwrap();
        assert_eq!(portrait.image.dimensions(), (600, 300));
        assert_eq!(portrait.preview.dimensions(), (64, 32));
    }

    #[test]
    fn decoded_art_preserves_straight_alpha_for_the_renderer() {
        let rgba = image::Rgba([220, 70, 10, 64]);
        let image = image::RgbaImage::from_pixel(2, 2, rgba);
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let cover = decode_cover(png.into_inner().into(), CoverSize::Sleeve).unwrap();
        assert_eq!(cover.image.get_pixel(0, 0), &rgba);
    }

    #[test]
    fn worker_wakes_the_host_after_enqueuing_its_event() {
        let fixture = Fixture::new();
        let store = fixture.store.clone();
        let (send, wakes) = mpsc::channel();
        let backend = Backend::with_store(
            Arc::new(move || {
                let _ = send.send(());
            }),
            move || store,
        );
        backend.send(Request::Restore { session: 42 });
        wakes
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(matches!(
            backend.events.try_recv(),
            Ok(Event::Restored {
                session: 42,
                result: Ok(None)
            })
        ));
    }

    #[test]
    fn invalid_artwork_is_not_retained() {
        assert!(decode_cover(Arc::from(&b"not an image"[..]), CoverSize::Sleeve).is_err());
    }
}
