//! Cancellable API requests on a dedicated runtime.

use std::sync::{Arc, mpsc};

use super::api::{Album, Client};

pub enum Request {
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
    Cover {
        session: u64,
        client: Client,
        id: String,
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
    Cover {
        session: u64,
        id: String,
        result: Result<(egui::ColorImage, Option<[u8; 3]>), String>,
    },
    SaveFailed,
}

pub struct Backend {
    send: tokio::sync::mpsc::UnboundedSender<Request>,
    pub events: mpsc::Receiver<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Backend {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (send, mut requests) = tokio::sync::mpsc::unbounded_channel();
        let (events, receive) = mpsc::channel();
        let thread = std::thread::Builder::new().name("navifast-library".into()).spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("create Navidrome runtime");
            runtime.block_on(async move {
                let permits = Arc::new(tokio::sync::Semaphore::new(4));
                let mut tasks = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        request = requests.recv() => {
                            let Some(request) = request else { break; };
                            match request {
                                Request::Shutdown => break,
                                Request::Cancel => tasks.abort_all(),
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
                                    tasks.spawn(async move {
                                        let Ok(_permit) = permits.acquire().await else { return; };
                                        let event = execute(request).await;
                                        if let Some(event) = event { let _ = events.send(event); ctx.request_repaint(); }
                                    });
                                }
                            }
                        }
                        _ = tasks.join_next(), if !tasks.is_empty() => {}
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

async fn execute(request: Request) -> Option<Event> {
    Some(match request {
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
        Request::Cover {
            session,
            client,
            id,
        } => {
            let result = match client.cover(&id).await {
                Ok(bytes) => tokio::task::spawn_blocking(move || {
                    decode_cover(&bytes).map(|image| (image, crate::images::accent_color(&bytes)))
                })
                .await
                .unwrap_or_else(|_| Err("Artwork could not be decoded.".into())),
                Err(error) => Err(error),
            };
            Event::Cover {
                session,
                id,
                result,
            }
        }
        Request::Save { .. }
        | Request::OpenThemesFolder(_)
        | Request::Cancel
        | Request::Shutdown => return None,
    })
}

fn decode_cover(bytes: &[u8]) -> Result<egui::ColorImage, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Invalid artwork.")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| "Unsupported or oversized artwork.")?
        .thumbnail(300, 300)
        .into_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    ))
}
