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
            cached,
        } => {
            let bytes = match cached {
                Some(bytes) => Ok(bytes),
                None => client.cover(&id).await.map(Arc::from),
            };
            let result = match bytes {
                Ok(bytes) => tokio::task::spawn_blocking(move || decode_cover(bytes))
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

fn decode_cover(bytes: Arc<[u8]>) -> Result<Cover, String> {
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
        .map_err(|_| "Unsupported or oversized artwork.")?
        .thumbnail(300, 300);
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

    #[tokio::test]
    async fn cached_cover_restores_without_a_server_and_preserves_aspect_ratio() {
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
            session,
            id,
            result: Ok(cover),
        }) = execute(Request::Cover {
            session: 42,
            client,
            id: "cover:/🌍".into(),
            cached: Some(bytes.clone()),
        })
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
    }

    #[test]
    fn invalid_artwork_is_not_retained() {
        assert!(decode_cover(Arc::from(&b"not an image"[..])).is_err());
    }
}
