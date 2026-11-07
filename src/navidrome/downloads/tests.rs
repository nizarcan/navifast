use super::{
    worker::{Action, Command, Update},
    *,
};
use crate::{
    navidrome::{
        api::Client,
        test_server::{Server, response},
    },
    settings::ProxyConfig,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::mpsc;
const AUDIO: &[u8] = include_bytes!("../../../tests/fixtures/gapless/first.flac");
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("navifast-offline-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        Self { root }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn track() -> Song {
    Song {
        id: "track:/λ".into(),
        title: "Fixture".into(),
        suffix: "flac".into(),
        size: Some(AUDIO.len() as u64),
        duration: 0.25,
        ..Default::default()
    }
}
fn selection(id: &str) -> Selection {
    Selection {
        kind: Kind::Playlist,
        id: id.into(),
        title: id.into(),
        cover: None,
        songs: vec![],
        paused: false,
        pending: false,
        error: None,
    }
}
async fn until(
    receive: &mut mpsc::UnboundedReceiver<(u64, u64, Update)>,
    predicate: impl Fn(&Update) -> bool,
) -> Update {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let (_, _, update) = receive.recv().await.unwrap();
            if predicate(&update) {
                return update;
            }
        }
    })
    .await
    .expect("download update")
}
fn command(send: &mpsc::UnboundedSender<Command>, revision: u64, action: Action) {
    send.send(Command {
        session: 7,
        revision,
        action,
    })
    .unwrap();
}
type Harness = (
    worker::Shared,
    mpsc::UnboundedSender<Command>,
    tokio::task::JoinHandle<()>,
    mpsc::UnboundedReceiver<(u64, u64, Update)>,
);
fn setup() -> Harness {
    let available = worker::Shared::default();
    let (out, receive) = mpsc::unbounded_channel();
    let (send, task) = worker::spawn(
        available.clone(),
        Arc::new(move |s, r, u| {
            let _ = out.send((s, r, u));
        }),
    );
    (available, send, task, receive)
}
#[tokio::test]
async fn downloads_share_one_verified_file_and_remain_playable_after_server_disappears() {
    let fixture = Fixture::new();
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let server = Server::spawn(move |request| {
        if request.starts_with("GET /rest/getUser.view") {
            response("200 OK","",br#"{"subsonic-response":{"status":"ok","user":{"username":"listener","downloadRole":true}}}"#)
        } else {
            assert!(request.starts_with("GET /rest/download.view"));
            count.fetch_add(1, Ordering::SeqCst);
            response("200 OK", "Content-Type: audio/flac\r\n", AUDIO)
        }
    });
    let client = Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
    let scope = client.scope.clone();
    let (available, send, task, mut receive) = setup();
    command(
        &send,
        1,
        Action::Open {
            root: fixture.root.clone(),
            client: client.clone(),
            proxy: ProxyConfig::Off,
            online: true,
        },
    );
    until(&mut receive, |u| matches!(u, Update::Library(_))).await;
    command(
        &send,
        2,
        Action::Keep {
            selection: selection("one"),
            songs: Some(vec![track(), track()]),
        },
    );
    command(
        &send,
        3,
        Action::Keep {
            selection: selection("two"),
            songs: Some(vec![track()]),
        },
    );
    until(&mut receive, |u| {
        matches!(
            u,
            Update::Track {
                status: Status::Ready { .. },
                ..
            }
        )
    })
    .await;
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    let path = worker::local(&available, &scope, &track().id).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), AUDIO);
    command(&send, 4, Action::Remove(Kind::Playlist.key("one")));
    until(
        &mut receive,
        |u| matches!(u,Update::Library(l) if l.selections.len()==1),
    )
    .await;
    assert!(path.exists());
    drop(server);
    assert_eq!(
        crate::navidrome::player::inspect_download(&path, "flac")
            .unwrap()
            .codec,
        "flac"
    );
    command(&send, 5, Action::Close);
    drop(send);
    task.await.unwrap();
    let (available, send, task, mut receive) = setup();
    command(
        &send,
        1,
        Action::Open {
            root: fixture.root.clone(),
            client,
            proxy: ProxyConfig::Off,
            online: false,
        },
    );
    let Update::Library(l) = until(&mut receive, |u| matches!(u, Update::Library(_))).await else {
        unreachable!()
    };
    assert_eq!(l.bytes(), AUDIO.len() as u64);
    assert!(worker::local(&available, &scope, &track().id).is_some());
    assert_eq!(l.ready_songs(&Kind::Playlist.key("two")).len(), 1);
    command(&send, 2, Action::Remove(Kind::Playlist.key("two")));
    until(
        &mut receive,
        |u| matches!(u,Update::Library(l) if l.selections.is_empty()),
    )
    .await;
    assert!(!path.exists());
    assert!(worker::local(&available, &scope, &track().id).is_none());
    drop(send);
    task.await.unwrap();
}
#[tokio::test]
async fn server_denial_and_write_failure_retry_without_losing_completed_music() {
    let fixture = Fixture::new();
    let allowed = Arc::new(AtomicBool::new(false));
    let permit = allowed.clone();
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    let server = Server::spawn(move |request| {
        if request.starts_with("GET /rest/getUser.view") {
            response("200 OK","",format!(r#"{{"subsonic-response":{{"status":"ok","user":{{"username":"listener","downloadRole":{}}}}}}}"#,permit.load(Ordering::SeqCst)).as_bytes())
        } else {
            count.fetch_add(1, Ordering::SeqCst);
            response("200 OK", "Content-Type: audio/flac\r\n", AUDIO)
        }
    });
    let client = Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
    let root = storage::account_root(&fixture.root, &client.scope).unwrap();
    let (available, send, task, mut receive) = setup();
    command(
        &send,
        1,
        Action::Open {
            root: fixture.root.clone(),
            client: client.clone(),
            proxy: ProxyConfig::Off,
            online: true,
        },
    );
    until(&mut receive, |u| matches!(u, Update::Library(_))).await;
    command(
        &send,
        2,
        Action::Keep {
            selection: selection("one"),
            songs: Some(vec![track()]),
        },
    );
    until(&mut receive, |u| {
        matches!(
            u,
            Update::Track {
                status: Status::Failed { .. },
                ..
            }
        )
    })
    .await;
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    assert!(worker::local(&available, &client.scope, &track().id).is_none());
    allowed.store(true, Ordering::SeqCst);
    // A deterministic unwritable target exercises the same visible storage failure path.
    std::fs::create_dir(storage::partial_path(&root, &track().id)).unwrap();
    command(&send, 3, Action::Retry(Kind::Playlist.key("one")));
    until(&mut receive, |u| {
        matches!(
            u,
            Update::Track {
                status: Status::Failed { .. },
                ..
            }
        )
    })
    .await;
    std::fs::remove_dir(storage::partial_path(&root, &track().id)).unwrap();
    command(&send, 4, Action::Retry(Kind::Playlist.key("one")));
    until(&mut receive, |u| {
        matches!(
            u,
            Update::Track {
                status: Status::Ready { .. },
                ..
            }
        )
    })
    .await;
    assert!(worker::local(&available, &client.scope, &track().id).is_some());
    drop(send);
    task.await.unwrap();
}
#[tokio::test]
async fn cancelled_transfer_never_publishes_and_retry_is_not_stuck_downloading() {
    let fixture = Fixture::new();
    let requested = Arc::new(AtomicBool::new(false));
    let started = requested.clone();
    let server = Server::spawn(move |request| {
        if request.starts_with("GET /rest/getUser.view") {
            response("200 OK","",br#"{"subsonic-response":{"status":"ok","user":{"username":"listener","downloadRole":"true"}}}"#)
        } else {
            started.store(true, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(200));
            response("200 OK", "Content-Type: audio/flac\r\n", AUDIO)
        }
    });
    let client = Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
    let scope = client.scope.clone();
    let (available, send, task, mut receive) = setup();
    command(
        &send,
        1,
        Action::Open {
            root: fixture.root.clone(),
            client,
            proxy: ProxyConfig::Off,
            online: true,
        },
    );
    until(&mut receive, |u| matches!(u, Update::Library(_))).await;
    command(
        &send,
        2,
        Action::Keep {
            selection: selection("one"),
            songs: Some(vec![track()]),
        },
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !requested.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    command(&send, 3, Action::Pause(Kind::Playlist.key("one")));
    until(&mut receive,|u|matches!(u,Update::Library(l) if matches!(l.tracks[&track().id].status,Status::Cancelled))).await;
    assert!(worker::local(&available, &scope, &track().id).is_none());
    command(&send, 4, Action::Retry(Kind::Playlist.key("one")));
    until(&mut receive, |u| {
        matches!(
            u,
            Update::Track {
                status: Status::Ready { .. },
                ..
            }
        )
    })
    .await;
    drop(send);
    task.await.unwrap();
}

#[tokio::test]
async fn missing_source_invalid_audio_and_incomplete_transfers_never_publish() {
    let fixture = Fixture::new();
    for mode in 0..5 {
        let server = Server::spawn(move |request| {
            if request.starts_with("GET /rest/getUser.view") {
                return response("200 OK", "", br#"{"subsonic-response":{"status":"ok","user":{"username":"listener","downloadRole":true}}}"#);
            }
            match mode {
                0 => response("404 Not Found", "", b"missing"),
                1 => response("200 OK", "Content-Type: text/xml\r\n", b"<error/>"),
                2 => response(
                    "200 OK",
                    "Content-Type: audio/flac\r\n",
                    &vec![0; AUDIO.len()],
                ),
                3 => {
                    let mut result = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        AUDIO.len()
                    )
                    .into_bytes();
                    result.extend_from_slice(&AUDIO[..AUDIO.len() / 2]);
                    result
                }
                _ => {
                    let mut result = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", AUDIO.len()).into_bytes();
                    result.extend_from_slice(AUDIO);
                    result.extend_from_slice(b"\r\n0\r\n\r\n");
                    result
                }
            }
        });
        let client =
            Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
        let (_cancel, cancellation) = tokio::sync::watch::channel(false);
        let (progress, _) = mpsc::channel(8);
        let result = transfer::download(
            client,
            reqwest::Client::new(),
            track(),
            fixture.root.clone(),
            cancellation,
            progress,
            1,
        )
        .await;
        if mode == 4 {
            assert_eq!(
                result.unwrap().bytes,
                AUDIO.len() as u64,
                "chunked original uses server song size"
            );
            assert!(storage::partial_path(&fixture.root, &track().id).exists());
        } else {
            assert!(result.is_err(), "invalid source {mode}");
            assert!(!storage::partial_path(&fixture.root, &track().id).exists());
        }
        assert!(!audio_path(&fixture.root, &track().id).exists());
    }
}

#[tokio::test]
async fn interrupted_transfer_restarts_queued_but_explicit_song_cancel_stays_cancelled() {
    let fixture = Fixture::new();
    let server = Server::spawn(|request| {
        if request.starts_with("GET /rest/getUser.view") {
            response("200 OK", "", br#"{"subsonic-response":{"status":"ok","user":{"username":"listener","downloadRole":true}}}"#)
        } else {
            std::thread::sleep(std::time::Duration::from_millis(200));
            response("200 OK", "", AUDIO)
        }
    });
    let client = Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
    let root = storage::account_root(&fixture.root, &client.scope).unwrap();
    let (_, send, task, mut receive) = setup();
    command(
        &send,
        1,
        Action::Open {
            root: fixture.root.clone(),
            client: client.clone(),
            proxy: ProxyConfig::Off,
            online: true,
        },
    );
    until(&mut receive, |u| matches!(u, Update::Library(_))).await;
    command(
        &send,
        2,
        Action::Keep {
            selection: selection("one"),
            songs: Some(vec![track()]),
        },
    );
    until(&mut receive, |u| {
        matches!(
            u,
            Update::Track {
                status: Status::Downloading { .. },
                ..
            }
        )
    })
    .await;
    drop(send);
    task.await.unwrap();
    assert_eq!(
        storage::load(&root, &client.scope).unwrap().tracks[&track().id].status,
        Status::Queued
    );
    assert!(!storage::partial_path(&root, &track().id).exists());
    let (_, send, task, mut receive) = setup();
    command(
        &send,
        1,
        Action::Open {
            root: fixture.root.clone(),
            client: client.clone(),
            proxy: ProxyConfig::Off,
            online: false,
        },
    );
    until(&mut receive, |u| matches!(u, Update::Library(_))).await;
    command(&send, 2, Action::PauseSong(track().id));
    until(
        &mut receive,
        |u| matches!(u,Update::Library(l) if l.tracks[&track().id].status==Status::Cancelled),
    )
    .await;
    assert_eq!(
        storage::load(&root, &client.scope).unwrap().tracks[&track().id].status,
        Status::Cancelled
    );
    command(&send, 3, Action::RetrySong(track().id));
    until(
        &mut receive,
        |u| matches!(u,Update::Library(l) if l.tracks[&track().id].status==Status::Queued),
    )
    .await;
    assert_eq!(
        storage::load(&root, &client.scope).unwrap().tracks[&track().id].status,
        Status::Queued
    );
    drop(send);
    task.await.unwrap();
}

#[tokio::test]
#[ignore = "requires the disposable Navidrome fixture"]
async fn local_navidrome_original_download_is_verified_and_restored_offline() {
    let fixture = Fixture::new();
    let client = Client::new(
        &std::env::var("NAVIFAST_TEST_SERVER").unwrap(),
        &std::env::var("NAVIFAST_TEST_USER").unwrap(),
        std::env::var("NAVIFAST_TEST_PASSWORD").unwrap(),
        &ProxyConfig::Off,
    )
    .unwrap();
    let album = client
        .album(&client.albums(0, true).await.unwrap()[0].id)
        .await
        .unwrap();
    let song = album.song[0].clone();
    assert!(client.download_allowed().await.unwrap());
    assert!(song.size.is_some_and(|size| size > 0));
    let root = storage::account_root(&fixture.root, &client.scope).unwrap();
    let mut library = Library::new(client.scope.clone());
    library
        .keep(selection("fixture"), Some(vec![song.clone()]))
        .unwrap();
    storage::persist(&root, &library).unwrap();
    let (_cancel, cancellation) = tokio::sync::watch::channel(false);
    let (progress, _) = mpsc::channel(8);
    let done = transfer::download(
        client.clone(),
        reqwest::Client::new(),
        song.clone(),
        root.clone(),
        cancellation,
        progress,
        1,
    )
    .await
    .unwrap();
    assert_eq!(Some(done.bytes), song.size);
    storage::publish(&root, &song.id, done.bytes, done.codec).unwrap();
    let restored = storage::load(&root, &client.scope).unwrap();
    assert!(restored.tracks[&song.id].status.ready());
    assert_eq!(
        crate::navidrome::player::inspect_download(&audio_path(&root, &song.id), &song.suffix)
            .unwrap()
            .sample_rate,
        44_100
    );
}
