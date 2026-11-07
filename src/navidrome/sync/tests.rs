use super::*;
use crate::{
    navidrome::{
        api::{Client, Song},
        test_server::{Server, response},
    },
    settings::ProxyConfig,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("navifast-sync-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn favorite(starred: bool) -> Work {
    Work::Favorite {
        item: Box::new(Item::Song(Song {
            id: "song".into(),
            title: "Song".into(),
            ..Default::default()
        })),
        starred,
    }
}
fn rating(value: u8) -> Work {
    Work::Rating {
        kind: Kind::Song,
        id: "song".into(),
        name: "Song".into(),
        value,
    }
}
fn listen(occurrence: u64) -> Work {
    Work::Listen {
        key: format!("boot:{occurrence}"),
        report: Report {
            id: "song".into(),
            title: "Song".into(),
            time: 123_000 + occurrence,
            occurrence,
            submission: true,
        },
    }
}
fn json(body: serde_json::Value) -> Vec<u8> {
    response(
        "200 OK",
        "",
        serde_json::json!({"subsonic-response": body})
            .to_string()
            .as_bytes(),
    )
}

#[test]
fn restart_coalesces_edits_preserves_each_listen_and_scopes_the_journal() {
    let f = Fixture::new();
    let scope = "a".repeat(64);
    let path = storage::path(&f.0, &scope).unwrap();
    let mut journal = Journal::new(scope.clone());
    for work in [
        favorite(true),
        favorite(false),
        rating(5),
        rating(2),
        listen(1),
        listen(2),
        listen(1),
    ] {
        journal.enqueue(work).unwrap();
    }
    storage::save(&path, &journal).unwrap();
    let loaded = storage::load(&path, &scope).unwrap();
    assert_eq!(loaded.pending.len(), 4);
    assert!(matches!(
        loaded.pending[&favorite(false).key()].work,
        Work::Favorite { starred: false, .. }
    ));
    assert!(matches!(
        loaded.pending[&rating(2).key()].work,
        Work::Rating { value: 2, .. }
    ));
    for occurrence in [1, 2] {
        let Work::Listen { report, .. } = &loaded.pending[&listen(occurrence).key()].work else {
            panic!()
        };
        assert_eq!(report.time, 123_000 + occurrence);
    }
    assert!(storage::load(&path, &"b".repeat(64)).is_err());
    assert!(storage::path(&f.0, "../../elsewhere").is_err());
}
#[test]
fn interrupted_listens_stay_uncertain_and_old_acknowledgements_cannot_retire_new_intent() {
    let mut journal = Journal::new("account".into());
    journal.enqueue(listen(1)).unwrap();
    journal.pending.get_mut(&listen(1).key()).unwrap().status = Status::Sending;
    journal.recover();
    journal.retry(None);
    assert!(matches!(
        journal.pending[&listen(1).key()].status,
        Status::Uncertain(_)
    ));
    journal.enqueue(favorite(true)).unwrap();
    let old = journal.pending[&favorite(true).key()].serial;
    journal.enqueue(favorite(false)).unwrap();
    journal.finish(&favorite(true).key(), old, Ok(()));
    assert_eq!(journal.pending.len(), 2);
    assert!(matches!(
        journal.pending[&favorite(false).key()].work,
        Work::Favorite { starred: false, .. }
    ));
}
#[test]
fn failed_atomic_save_and_damaged_files_preserve_previous_intent() {
    let f = Fixture::new();
    let path = f.0.join("state.json");
    let mut journal = Journal::new("scope".into());
    journal.enqueue(favorite(true)).unwrap();
    storage::save(&path, &journal).unwrap();
    let previous = std::fs::read(&path).unwrap();
    // The bounded writer uses the same abort-before-rename path as ENOSPC.
    assert!(crate::util::write_json_atomic(&path, &journal, 1).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), previous);
    std::fs::write(&path, b"{").unwrap();
    assert!(storage::load(&path, "scope").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{");
}

#[test]
fn lagging_favorites_cannot_undo_a_completed_durable_checkpoint() {
    let mut journal = Journal::new("scope".into());
    let work = favorite(true);
    journal.enqueue(work.clone()).unwrap();
    let key = work.key();
    journal.finish(&key, journal.pending[&key].serial, Ok(()));
    journal.observe_favorites(Catalogue::default());
    assert_eq!(journal.favorites.song.len(), 1);
    journal.observe_favorites(journal.favorites.clone());
    journal.observe_favorites(Catalogue::default());
    assert!(
        journal.favorites.song.is_empty(),
        "later edits on another client are visible"
    );
}

#[tokio::test]
#[ignore = "requires the disposable Navidrome fixture"]
async fn local_navidrome_sync_confirms_favorites_ratings_and_distinct_listens() {
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
    for starred in [true, false] {
        network::execute(
            &client,
            &Work::Favorite {
                item: Box::new(Item::Song(song.clone())),
                starred,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            client
                .favorites()
                .await
                .unwrap()
                .song
                .iter()
                .any(|s| s.id == song.id),
            starred
        );
    }
    for value in [4, 0] {
        network::execute(
            &client,
            &Work::Rating {
                kind: Kind::Song,
                id: song.id.clone(),
                name: song.title.clone(),
                value,
            },
        )
        .await
        .unwrap();
        assert_eq!(client.rating(Kind::Song, &song.id).await.unwrap(), value);
    }
    let f = Fixture::new();
    let (tx, task, mut events) = actor();
    send(
        &tx,
        1,
        worker::Action::Open {
            root: f.0.clone(),
            client: client.clone(),
            scrobbling: true,
        },
    );
    until(&mut events, 1, |j| j.pending.is_empty()).await;
    for occurrence in [1, 2] {
        let report = Report {
            id: song.id.clone(),
            title: song.title.clone(),
            occurrence,
            time: now() * 1000 - occurrence * 60_000,
            submission: true,
        };
        send(
            &tx,
            1,
            worker::Action::Enqueue(Work::Listen {
                key: format!("fixture:{occurrence}"),
                report,
            }),
        );
    }
    until(&mut events, 1, |j| {
        j.pending.is_empty() && j.last_sync.is_some()
    })
    .await;
    drop(tx);
    task.await.unwrap();
}
#[tokio::test]
async fn lost_write_acknowledgements_reconcile_without_a_second_write() {
    for work in [favorite(true), rating(4)] {
        let count = Arc::new(AtomicUsize::new(0));
        let writes = count.clone();
        let mut saved = false;
        let server = Server::spawn(move |request| {
            if request.contains("/star.view?") || request.contains("/setRating.view?") {
                writes.fetch_add(1, Ordering::SeqCst);
                saved = true;
                return Vec::new(); // Applied, then connection closed without its acknowledgement.
            }
            if request.contains("/getStarred2.view?") {
                json(
                    serde_json::json!({"status":"ok", "starred2":{"song":if saved { vec![serde_json::json!({"id":"song","title":"Song"})] } else { vec![] }}}),
                )
            } else {
                json(
                    serde_json::json!({"status":"ok","song":{"id":"song","userRating":if saved { 4 } else { 0 }}}),
                )
            }
        });
        let client =
            Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
        network::execute(&client, &work).await.unwrap();
        network::execute(&client, &work).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn an_applied_listen_with_lost_ack_is_never_blindly_retried() {
    let count = Arc::new(AtomicUsize::new(0));
    let submitted = count.clone();
    let server = Server::spawn(move |request| {
        assert!(request.contains("time=123001"));
        submitted.fetch_add(1, Ordering::SeqCst);
        Vec::new()
    });
    let client = Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
    let work = listen(1);
    let mut journal = Journal::new(client.scope.clone());
    journal.enqueue(work.clone()).unwrap();
    let serial = journal.pending[&work.key()].serial;
    let result = network::execute(&client, &work).await;
    assert!(matches!(result, Err(RequestError::Unconfirmed(_))));
    journal.finish(&work.key(), serial, result);
    journal.retry(None);
    assert!(matches!(
        journal.pending[&work.key()].status,
        Status::Uncertain(_)
    ));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn deleted_items_fail_individually_and_a_definite_connection_failure_is_retryable() {
    let server = Server::spawn(|_| {
        json(serde_json::json!({"status":"failed","error":{"code":70,"message":"Not found"}}))
    });
    let client = Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
    let mut journal = Journal::new(client.scope.clone());
    journal.enqueue(rating(4)).unwrap();
    journal.enqueue(listen(2)).unwrap();
    let key = rating(4).key();
    journal.finish(
        &key,
        journal.pending[&key].serial,
        network::execute(&client, &rating(4)).await,
    );
    assert!(matches!(journal.pending[&key].status, Status::Failed(_)));
    assert_eq!(journal.pending[&listen(2).key()].status, Status::Pending);
    let offline = Client::offline(&server.url, "listener", &ProxyConfig::Off).unwrap();
    assert!(matches!(
        network::execute(&offline, &listen(2)).await,
        Err(RequestError::Unsent(_))
    ));
}

type Updates = tokio::sync::mpsc::UnboundedReceiver<(u64, worker::Update)>;
fn actor() -> (
    tokio::sync::mpsc::UnboundedSender<worker::Command>,
    tokio::task::JoinHandle<()>,
    Updates,
) {
    let (events, receive) = tokio::sync::mpsc::unbounded_channel();
    let (send, task) = worker::spawn(Arc::new(move |session, _, update| {
        let _ = events.send((session, update));
    }));
    (send, task, receive)
}
async fn until(
    events: &mut Updates,
    session: u64,
    predicate: impl Fn(&Journal) -> bool,
) -> Journal {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let (current, update) = events.recv().await.unwrap();
            assert!(update.error.is_none(), "{:?}", update.error);
            if current == session
                && let Some(journal) = update.journal
                && predicate(&journal)
            {
                return *journal;
            }
        }
    })
    .await
    .expect("sync update")
}
fn send(
    tx: &tokio::sync::mpsc::UnboundedSender<worker::Command>,
    session: u64,
    action: worker::Action,
) {
    tx.send(worker::Command {
        session,
        revision: 1,
        action,
    })
    .unwrap();
}

#[tokio::test]
async fn worker_restarts_offline_edits_and_replays_both_listens_only_to_the_original_account() {
    let f = Fixture::new();
    let plays = Arc::new(std::sync::Mutex::new(Vec::new()));
    let submitted = plays.clone();
    let mut saved_rating = 0;
    let server = Server::spawn(move |request| {
        let url = reqwest::Url::parse(&format!(
            "http://localhost{}",
            request.split_whitespace().nth(1).unwrap()
        ))
        .unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(params["u"], "original");
        if url.path().contains("scrobble") {
            submitted.lock().unwrap().push(params["time"].to_string());
        }
        if url.path().contains("setRating") {
            saved_rating = params["rating"].parse::<u8>().unwrap();
        }
        json(serde_json::json!({"status":"ok","song":{"id":"song","userRating":saved_rating}}))
    });
    let offline = Client::offline(&server.url, "original", &ProxyConfig::Off).unwrap();
    let (tx, task, mut events) = actor();
    send(
        &tx,
        1,
        worker::Action::Open {
            root: f.0.clone(),
            client: offline,
            scrobbling: true,
        },
    );
    until(&mut events, 1, |j| j.pending.is_empty()).await;
    for work in [rating(1), rating(5), listen(1), listen(2)] {
        send(&tx, 1, worker::Action::Enqueue(work));
    }
    until(&mut events, 1, |j| j.pending.len() == 3).await;
    drop(tx);
    task.await.unwrap();
    assert!(plays.lock().unwrap().is_empty());
    let (tx, task, mut events) = actor();
    let other = Client::new(&server.url, "other", "dummy".into(), &ProxyConfig::Off).unwrap();
    send(
        &tx,
        2,
        worker::Action::Open {
            root: f.0.clone(),
            client: other,
            scrobbling: true,
        },
    );
    until(&mut events, 2, |j| j.pending.is_empty()).await;
    let original = Client::new(&server.url, "original", "dummy".into(), &ProxyConfig::Off).unwrap();
    send(
        &tx,
        3,
        worker::Action::Open {
            root: f.0.clone(),
            client: original.clone(),
            scrobbling: false,
        },
    );
    // Disabling scrobbling holds listens while unrelated ratings still sync.
    until(&mut events, 3, |j| {
        j.pending.len() == 2 && j.ratings.len() == 1
    })
    .await;
    assert!(plays.lock().unwrap().is_empty());
    send(&tx, 3, worker::Action::Scrobbling(true));
    let done = until(&mut events, 3, |j| {
        j.pending.is_empty() && j.last_sync.is_some()
    })
    .await;
    assert!(matches!(
        done.ratings[&rating(5).key()],
        Work::Rating { value: 5, .. }
    ));
    drop(tx);
    task.await.unwrap();
    assert_eq!(*plays.lock().unwrap(), ["123001", "123002"]);
    let loaded = storage::load(
        &storage::path(&f.0, &original.scope).unwrap(),
        &original.scope,
    )
    .unwrap();
    assert!(loaded.pending.is_empty());
}

#[tokio::test]
async fn worker_continues_after_permanent_failure_and_keeps_uncertain_listens_through_restart() {
    let f = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let server = Server::spawn(move |request| {
        if request.contains("scrobble") {
            seen.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        } else {
            json(serde_json::json!({"status":"failed","error":{"code":70,"message":"Not found"}}))
        }
    });
    let client = Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
    let (tx, task, mut events) = actor();
    send(
        &tx,
        1,
        worker::Action::Open {
            root: f.0.clone(),
            client: client.clone(),
            scrobbling: true,
        },
    );
    until(&mut events, 1, |j| j.pending.is_empty()).await;
    send(&tx, 1, worker::Action::Enqueue(rating(5)));
    send(&tx, 1, worker::Action::Enqueue(listen(1)));
    until(&mut events, 1, |j| {
        j.pending.len() == 2
            && j.pending
                .values()
                .all(|e| matches!(e.status, Status::Failed(_) | Status::Uncertain(_)))
    })
    .await;
    drop(tx);
    task.await.unwrap();
    let (tx, task, mut events) = actor();
    send(
        &tx,
        2,
        worker::Action::Open {
            root: f.0.clone(),
            client,
            scrobbling: true,
        },
    );
    until(&mut events, 2, |j| j.pending.len() == 2).await;
    send(&tx, 2, worker::Action::Retry(None));
    until(&mut events, 2, |j| {
        matches!(j.pending[&rating(5).key()].status, Status::Failed(_))
    })
    .await;
    drop(tx);
    task.await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_storage_failure_blocks_network_until_a_successful_retry() {
    let f = Fixture::new();
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let server = Server::spawn(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        json(serde_json::json!({"status":"ok"}))
    });
    let client = Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
    let path = storage::path(&f.0, &client.scope).unwrap();
    let (tx, task, mut events) = actor();
    send(
        &tx,
        1,
        worker::Action::Open {
            root: f.0.clone(),
            client,
            scrobbling: true,
        },
    );
    until(&mut events, 1, |j| j.pending.is_empty()).await;
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap(); // Atomic replacement cannot overwrite a directory.
    send(&tx, 1, worker::Action::Enqueue(listen(1)));
    let (_, update) = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(update.error.is_some());
    assert_eq!(update.journal.unwrap().pending.len(), 1);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    std::fs::remove_dir(&path).unwrap();
    send(&tx, 1, worker::Action::Retry(None));
    until(&mut events, 1, |j| {
        j.pending.is_empty() && j.last_sync.is_some()
    })
    .await;
    drop(tx);
    task.await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
