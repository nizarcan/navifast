//! Cancellable API requests on a dedicated runtime.

use std::sync::{Arc, mpsc};

use super::api::{
    Album, ArtistInfo, Catalogue, Client, Collection, CollectionKind, CoverSize, NamedItem,
    SearchPart,
};
use crate::credentials::{Grant, Lease, NavidromePassword, Slot, Store};

pub enum Request {
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
    Search {
        session: u64,
        request: u64,
        client: Client,
        query: String,
        part: SearchPart,
    },
    CancelSearch,
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

pub struct Cover {
    pub image: egui::ColorImage,
    pub preview: egui::ColorImage,
    pub bytes: Arc<[u8]>,
    pub color: Option<[u8; 3]>,
}

pub struct Backend {
    send: tokio::sync::mpsc::UnboundedSender<Request>,
    pub events: mpsc::Receiver<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Backend {
    pub fn spawn(ctx: egui::Context, dirs: crate::paths::AppDirs) -> Self {
        Self::with_store(ctx, move || {
            #[cfg(not(test))]
            let credentials = Store::new(dirs);
            #[cfg(test)]
            let credentials = Store::in_memory(dirs);
            credentials
        })
    }

    fn with_store(
        ctx: egui::Context,
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
                let mut deletions = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        request = requests.recv() => {
                            let Some(request) = request else { break; };
                            match request {
                                Request::Shutdown => break,
                                Request::Cancel => {
                                    credentials.invalidate(Slot::Navidrome);
                                    collections.abort_all();
                                    tasks.abort_all();
                                    searches.abort_all();
                                }
                                Request::CancelCollection => collections.abort_all(),
                                Request::CancelSearch => searches.abort_all(),
                                Request::Forget { session } => {
                                    // Persist revocation off the UI thread before queuing deletion.
                                    // A locked keyring must not restore a signed-out account.
                                    let revoked = credentials.revoke(Slot::Navidrome);
                                    let deletion = credentials.lease(Slot::Navidrome).delete();
                                    let events = events.clone();
                                    let ctx = ctx.clone();
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
                                            ctx.request_repaint();
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
                                    if !matches!(result, Ok(Ok(()))) { let _=events.send(Event::SaveFailed); ctx.request_repaint(); }
                                }
                                request => {
                                    let events = events.clone();
                                    let ctx = ctx.clone();
                                    let permits = permits.clone();
                                    let lease = credentials.lease(Slot::Navidrome);
                                    let group = match request {
                                        Request::Search { .. } => &mut searches,
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
                                        if let Some(event) = event { let _ = events.send(event); ctx.request_repaint(); }
                                        if let Some((session, saving)) = saving
                                            && let Err(error) = saving.await
                                            && lease.current()
                                        {
                                            let _ = events.send(Event::CredentialError { session, message: error.to_string() });
                                            ctx.request_repaint();
                                        }
                                    });
                                }
                            }
                        }
                        _ = collections.join_next(), if !collections.is_empty() => {}
                        _ = searches.join_next(), if !searches.is_empty() => {}
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
        Request::Save { .. }
        | Request::Forget { .. }
        | Request::OpenThemesFolder(_)
        | Request::CancelCollection
        | Request::CancelSearch
        | Request::Cancel
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
        image: egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        preview: egui::ColorImage::from_rgba_unmultiplied(
            [preview.width() as usize, preview.height() as usize],
            preview.as_raw(),
        ),
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
            Backend::with_store(egui::Context::default(), move || store)
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
        assert_eq!(cover.image.size, [300, 150]);
        assert_eq!(cover.preview.size, [64, 32]);
        assert!(Arc::ptr_eq(&bytes, &cover.bytes));
        assert!(cover.color.is_some());
        let portrait = decode_cover(bytes, CoverSize::Portrait).unwrap();
        assert_eq!(portrait.image.size, [600, 300]);
        assert_eq!(portrait.preview.size, [64, 32]);
    }

    #[test]
    fn invalid_artwork_is_not_retained() {
        assert!(decode_cover(Arc::from(&b"not an image"[..]), CoverSize::Sleeve).is_err());
    }
}
