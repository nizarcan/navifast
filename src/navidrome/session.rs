//! Local listening-session persistence. Disk and validation work run on workers.
use super::{
    api::{Album, Client, RequestError},
    queue::{Queue, Source, snapshot::Snapshot},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{BufWriter, Read, Write},
    path::Path,
    sync::Arc,
};

const VERSION: u32 = 1;
const MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Session {
    version: u32,
    scope: String,
    pub queue: Snapshot,
    pub position: f64,
    pub repeat: String,
    pub album: Option<String>,
    pub playlist: Option<String>,
    #[serde(default)]
    pub rows: Vec<String>,
}

pub struct Restored {
    pub queue: Queue,
    pub position: f64,
    pub repeat: crate::player::RepeatMode,
    pub album: Option<Album>,
    pub playlist: Option<String>,
    pub rows: Vec<String>,
}

impl Session {
    pub fn new(scope: String, queue: Snapshot) -> Self {
        Self {
            version: VERSION,
            scope,
            queue,
            position: 0.0,
            repeat: "off".into(),
            album: None,
            playlist: None,
            rows: Vec::new(),
        }
    }

    pub fn read(path: &Path, scope: &str) -> Option<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > MAX_BYTES {
            return None;
        }
        let session: Self = serde_json::from_slice(&bytes).ok()?;
        (session.scope == scope && session.valid()).then_some(session)
    }

    fn valid(&self) -> bool {
        self.version == VERSION
            && self.queue.valid()
            && self.position.is_finite()
            && (self.rows.is_empty() || self.rows.len() == self.queue.context_len())
            && self.rows.iter().all(|row| row.len() <= 16_384)
            && self.queue.repeats() == (self.repeat == "context")
            && matches!(self.repeat.as_str(), "off" | "context" | "track")
    }

    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if !self.valid() {
            return Err(std::io::Error::other(
                "Invalid or oversized listening session",
            ));
        }
        atomic_write(path, |file| {
            // Buffer JSON fragments and bound the result to what the reader accepts.
            let mut writer = Limited {
                inner: BufWriter::new(file),
                remaining: MAX_BYTES as usize,
            };
            serde_json::to_writer_pretty(&mut writer, self).map_err(std::io::Error::other)?;
            writer.flush()
        })
    }
}

struct Limited<W> {
    inner: W,
    remaining: usize,
}
impl<W: Write> Write for Limited<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other("Listening session is too large"));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Flush before replacement. An interrupted or failed write leaves the old file readable.
fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("No state directory"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    let result = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        write(&mut file)?;
        file.sync_all()?;
        crate::util::replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub async fn restore(
    path: std::path::PathBuf,
    client: Client,
    permits: Arc<tokio::sync::Semaphore>,
) -> Result<Option<Restored>, String> {
    let scope = client.scope.clone();
    let Some(saved) = tokio::task::spawn_blocking(move || Session::read(&path, &scope))
        .await
        .map_err(|_| "The saved queue could not be read.")?
    else {
        return Ok(None);
    };
    let mut ids = saved.queue.song_ids().into_iter();
    let mut tasks = tokio::task::JoinSet::new();
    let mut songs = HashMap::new();
    // At most four requests/futures, sharing the library's request budget.
    loop {
        while tasks.len() < 4 {
            let Some(id) = ids.next() else {
                break;
            };
            let client = client.clone();
            let permits = permits.clone();
            tasks.spawn(async move {
                let _permit = permits
                    .acquire()
                    .await
                    .map_err(|_| "Queue restoration canceled.")?;
                client.song(&id).await
            });
        }
        let Some(result) = tasks.join_next().await else {
            break;
        };
        if let Some(song) = result.map_err(|_| "Queue restoration canceled.")?? {
            songs.insert(song.id.clone(), song);
        }
    }
    let original = saved.queue.current_id();
    let (mut queue, kept) = saved.queue.restore(&songs);
    let pending: Vec<_> = queue
        .upcoming()
        .iter()
        .filter_map(|e| match &e.source {
            Source::Album { id, .. } => Some((e.id, id.clone())),
            _ => None,
        })
        .collect();
    for (token, id) in pending {
        let _permit = permits
            .acquire()
            .await
            .map_err(|_| "Queue restoration canceled.")?;
        let songs = match client.checked_request("getAlbum", &[("id", id)]).await {
            Ok(body) => {
                let mut album: Album = serde_json::from_value(body["album"].clone())
                    .map_err(|_| "The server returned an invalid album.")?;
                album.song.sort_by_key(|s| (s.disc_number, s.track));
                for song in &mut album.song {
                    if song.cover_art.is_none() {
                        song.cover_art.clone_from(&album.cover_art);
                    }
                }
                album.song
            }
            Err(RequestError::Missing) => Vec::new(),
            Err(error) => return Err(error.to_string()),
        };
        queue.resolve(token, songs);
    }
    if queue.current().is_none() {
        queue.advance();
    }
    let position = if queue.current().map(|e| e.id) == original {
        saved.position.max(0.0).min(
            queue
                .current()
                .and_then(|e| queue.song(e))
                .map_or(0.0, |s| s.duration),
        )
    } else {
        0.0
    };
    let album = saved.album.map(|id| {
        let song = queue.context().first().cloned().unwrap_or_default();
        Album {
            id,
            name: song.album,
            artist: song.artist,
            cover_art: song.cover_art,
            credits: song.credits,
            song_count: queue.context().len() as u32,
            ..Default::default()
        }
    });
    Ok(Some(Restored {
        queue,
        position,
        repeat: crate::player::RepeatMode::from_api(&saved.repeat),
        album,
        playlist: saved.playlist,
        rows: kept
            .into_iter()
            .filter_map(|i| saved.rows.get(i).cloned())
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::{
        api::Song,
        test_server::{Server, response},
    };
    use std::{io::Write, sync::Mutex};
    fn path() -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("navifast-queue-{}", rand::random::<u64>()))
            .join("session.json")
    }
    fn song(id: &str) -> Song {
        Song {
            id: id.into(),
            duration: 90.0,
            ..Default::default()
        }
    }
    fn client(server: &Server, user: &str) -> Client {
        Client::new(
            &server.url,
            user,
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap()
    }
    #[test]
    fn interrupted_write_keeps_last_checkpoint_and_corrupt_or_old_files_are_ignored() {
        let path = path();
        let mut q = Queue::default();
        q.start(vec![song("opaque")], 0);
        let saved = Session::new("account".into(), q.snapshot());
        saved.write(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(
            atomic_write(&path, |f| {
                f.write_all(b"partial")?;
                Err(std::io::Error::other("interrupted"))
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(Session::read(&path, "another-account").is_none());
        // A crash leaves a temporary file but never replaces the last checkpoint.
        std::fs::write(path.with_extension("json.tmp"), b"interrupted").unwrap();
        assert!(Session::read(&path, "account").is_some());
        for bytes in [b"{".as_slice(), b"{}", b"{\"version\":0}"] {
            std::fs::write(&path, bytes).unwrap();
            assert!(Session::read(&path, "account").is_none());
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[tokio::test]
    async fn restart_validates_each_unique_id_and_clamps_position_without_losing_duplicates() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let server = Server::spawn(move |request| {
            let url = reqwest::Url::parse(&format!(
                "http://localhost{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            assert_eq!(url.path(), "/rest/getSong.view");
            let id = url
                .query_pairs()
                .find(|(k, _)| k == "id")
                .unwrap()
                .1
                .into_owned();
            seen.lock().unwrap().push(id.clone());
            let body = if id == "gone" {
                serde_json::json!({"status":"failed","error":{"code":70}})
            } else {
                serde_json::json!({"status":"ok","song":{"id":id,"duration":30,"title":"Fresh title"}})
            };
            response(
                "200 OK",
                "",
                serde_json::to_string(&serde_json::json!({"subsonic-response":body}))
                    .unwrap()
                    .as_bytes(),
            )
        });
        let client = client(&server, "a");
        let path = path();
        let mut queue = Queue::default();
        queue.start(
            vec![song("gone"), song("dup"), song("dup"), song("last")],
            2,
        );
        queue.add(vec![song("manual"), song("manual")], false);
        queue.set_shuffle(true);
        queue.set_repeat_all(true);
        let playing = queue.current().unwrap().id;
        let expected: Vec<_> = queue.upcoming().iter().map(|e| e.id).collect();
        let mut saved = Session::new(client.scope.clone(), queue.snapshot());
        saved.position = 80.0;
        saved.repeat = "context".into();
        saved.rows = vec!["gone".into(), "dup0".into(), "dup1".into(), "last".into()];
        saved.write(&path).unwrap();
        let restored = restore(
            path.clone(),
            client,
            Arc::new(tokio::sync::Semaphore::new(4)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(restored.queue.current().unwrap().id, playing);
        assert_eq!(
            restored
                .queue
                .upcoming()
                .iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(restored.position, 30.0);
        assert_eq!(restored.rows, ["dup0", "dup1", "last"]);
        assert_eq!(restored.repeat, crate::player::RepeatMode::Context);
        let mut calls = calls.lock().unwrap().clone();
        calls.sort();
        assert_eq!(calls, ["dup", "gone", "last", "manual"]);
        let file = std::fs::read_to_string(&path).unwrap();
        for secret in ["dummy", "Fresh title", "http://", "stream.view"] {
            assert!(!file.contains(secret));
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[tokio::test]
    async fn changed_account_sends_no_requests_and_network_failure_keeps_the_checkpoint() {
        let server = Server::spawn(|_| response("503 Unavailable", "", b""));
        let a = client(&server, "a");
        let b = client(&server, "b");
        let path = path();
        let mut queue = Queue::default();
        queue.start(vec![song("song")], 0);
        Session::new(a.scope.clone(), queue.snapshot())
            .write(&path)
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(
            restore(path.clone(), b, Arc::new(tokio::sync::Semaphore::new(4)))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            restore(path.clone(), a, Arc::new(tokio::sync::Semaphore::new(4)))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[cfg(test)]
mod missing_tests {
    use super::*;
    use crate::navidrome::{
        api::Song,
        test_server::{Server, response},
    };
    #[tokio::test]
    async fn missing_playing_song_falls_forward_and_pending_album_keeps_its_slot() {
        let server = Server::spawn(|request| {
            let url = reqwest::Url::parse(&format!(
                "http://localhost{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            let body = match url.path() {
                "/rest/getSong.view" => serde_json::json!({"status":"failed","error":{"code":70}}),
                "/rest/getAlbum.view" => {
                    serde_json::json!({"status":"ok","album":{"id":"album","song":[{"id":"second","track":2,"duration":30},{"id":"first","track":1,"duration":30}]}})
                }
                _ => panic!("unexpected endpoint"),
            };
            response(
                "200 OK",
                "",
                serde_json::to_string(&serde_json::json!({"subsonic-response":body}))
                    .unwrap()
                    .as_bytes(),
            )
        });
        let client = Client::new(
            &server.url,
            "listener",
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        let mut queue = Queue::default();
        queue.start(
            vec![Song {
                id: "missing".into(),
                duration: 90.0,
                ..Default::default()
            }],
            0,
        );
        let pending = queue.reserve("album".into(), "album".into(), false);
        let mut saved = Session::new(client.scope.clone(), queue.snapshot());
        saved.position = 44.0;
        let root = std::env::temp_dir().join(format!("navifast-pending-{}", rand::random::<u64>()));
        let path = root.join("session.json");
        saved.write(&path).unwrap();
        let restored = restore(path, client, Arc::new(tokio::sync::Semaphore::new(4)))
            .await
            .unwrap()
            .unwrap();
        let playing = restored.queue.current().unwrap();
        assert_eq!(playing.id, pending);
        assert_eq!(restored.queue.song(playing).unwrap().id, "first");
        assert_eq!(
            restored
                .queue
                .song(&restored.queue.upcoming()[0])
                .unwrap()
                .id,
            "second"
        );
        assert_eq!(restored.position, 0.0);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    use crate::navidrome::backend::{Backend, Request};
    #[test]
    fn orderly_shutdown_drains_checkpoint_and_signout_removal_in_order() {
        let root =
            std::env::temp_dir().join(format!("navifast-checkpoint-{}", rand::random::<u64>()));
        let dirs = crate::paths::AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let path = dirs.state.join("navidrome-session.json");
        let saved = || Session::new("account".into(), Queue::default().snapshot());
        let worker = Backend::spawn(Arc::new(|| {}), dirs.clone());
        worker.send(Request::SaveSession {
            path: path.clone(),
            snapshot: Some(Box::new(saved())),
        });
        drop(worker);
        assert!(Session::read(&path, "account").is_some());
        let worker = Backend::spawn(Arc::new(|| {}), dirs);
        worker.send(Request::SaveSession {
            path: path.clone(),
            snapshot: Some(Box::new(saved())),
        });
        worker.send(Request::SaveSession {
            path: path.clone(),
            snapshot: None,
        });
        drop(worker);
        assert!(!path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
