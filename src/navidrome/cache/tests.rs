use super::*;
use crate::{
    navidrome::{
        api::Client,
        stream::HttpSource,
        test_server::{Server, response},
    },
    settings::ProxyConfig,
};
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[test]
#[ignore = "requires the disposable local Navidrome fixture"]
fn local_navidrome_original_and_converted_streams_replay_complete_cached_bytes() {
    use crate::navidrome::stream::forward::ForwardSource;
    let server = std::env::var("NAVIFAST_TEST_SERVER").unwrap();
    assert!(server.starts_with("http://127.0.0.1:"));
    let client = Client::new(
        &server,
        &std::env::var("NAVIFAST_TEST_USER").unwrap(),
        std::env::var("NAVIFAST_TEST_PASSWORD").unwrap(),
        &ProxyConfig::Off,
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let song = runtime.block_on(async {
        let albums = client.albums(0, true).await.unwrap();
        client.album(&albums[0].id).await.unwrap().song[0].clone()
    });
    let f = Fixture::new();
    for quality in [Quality::Original, Quality::Mp3Standard] {
        let plan = runtime
            .block_on(client.streaming_plan(&song.id, quality, 0.))
            .unwrap();
        let key = key(&client.scope, &song, quality, plan.ranged, 0.);
        let mut expected = Vec::new();
        for replay in [false, true] {
            let lease = f.cache.open(key.clone(), !plan.ranged).unwrap();
            if replay {
                assert!(lease.complete_length().is_some());
            }
            // Replay must work even with an unreachable audio endpoint.
            let url = if replay {
                reqwest::Url::parse("http://127.0.0.1:1/unreachable").unwrap()
            } else {
                plan.url.clone()
            };
            let http = Client::stream_client(&ProxyConfig::Off).unwrap();
            let generation = Arc::new(AtomicU64::new(1));
            let mut source: Box<dyn Read> = if plan.ranged {
                Box::new(
                    HttpSource::open_cached(http, url, generation, 1, None, Some(lease)).unwrap(),
                )
            } else {
                Box::new(
                    ForwardSource::open_cached(http, url, generation, 1, None, Some(lease))
                        .unwrap(),
                )
            };
            let mut bytes = Vec::new();
            source.read_to_end(&mut bytes).unwrap();
            assert!(!bytes.is_empty());
            if replay {
                assert_eq!(bytes, expected);
            } else {
                expected = bytes;
            }
        }
    }
}

struct Fixture {
    root: PathBuf,
    cache: Cache,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("navifast-cache-{:016x}", rand::random::<u64>()));
        let cache = Cache::default();
        cache.configure(
            root.clone(),
            Config {
                enabled: true,
                limit_mib: 128,
            },
            false,
        );
        Self { root, cache }
    }
    fn open(&self, key: &str) -> Lease {
        self.cache.open(key.repeat(64), false).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn account_format_quality_and_source_offsets_have_distinct_secret_free_keys() {
    let song = Song {
        id: "shared/id".into(),
        suffix: "flac".into(),
        size: Some(400),
        ..Default::default()
    };
    let mut keys = std::collections::HashSet::new();
    for scope in ["account A", "account B"] {
        for quality in Quality::ALL {
            for (ranged, offset) in [(true, 0.0), (false, 0.0), (false, 12.0)] {
                let key = key(scope, &song, quality, ranged, offset);
                assert!(storage::hash(&key));
                assert!(keys.insert(key));
            }
        }
    }
    assert_eq!(
        key("A", &song, Quality::Original, true, 0.),
        key("A", &song, Quality::Original, true, 30.)
    );
    assert!(!Config::default().enabled);
}
#[test]
fn holes_remain_partial_and_restart_only_restores_verified_ranges() {
    let f = Fixture::new();
    let lease = f.open("a");
    lease.store(0, b"abc", Some(9));
    lease.store(6, b"ghi", Some(9));
    assert_eq!(lease.complete_length(), None);
    assert_eq!(lease.read(3, 3), None);
    drop(lease);
    let restarted = Cache::default();
    restarted.configure(
        f.root.clone(),
        Config {
            enabled: true,
            limit_mib: 128,
        },
        false,
    );
    let lease = restarted.open("a".repeat(64), false).unwrap();
    assert_eq!(lease.read(1, 8).unwrap(), b"bc");
    assert_eq!(lease.read(3, 3), None);
    lease.store(3, b"def", Some(9));
    assert_eq!(lease.complete_length(), Some(9));
    // Same-length corruption must also be rejected, then fetched from the network.
    let state = restarted.0.lock().unwrap();
    let path = state.entries.values().next().unwrap().path.clone();
    drop(state);
    std::fs::write(storage::block_path(&path, 3), b"bad").unwrap();
    assert_eq!(lease.read(3, 3), None);
    assert_eq!(lease.complete_length(), None);
}
#[test]
fn clear_and_disabling_retire_active_files_without_breaking_reads_or_reusing_old_writes() {
    let f = Fixture::new();
    let active = f.open("a");
    active.store(0, b"playing", Some(7));
    f.cache.configure(
        f.root.clone(),
        Config {
            enabled: true,
            limit_mib: 128,
        },
        true,
    );
    assert_eq!(active.read(0, 7).unwrap(), b"playing");
    active.store(7, b"ignored", Some(14));
    assert_eq!(active.length(), Some(7));
    let fresh = f.open("a");
    assert_eq!(fresh.read(0, 7), None);
    fresh.store(0, b"new", Some(3));
    drop(active);
    assert_eq!(f.cache.0.lock().unwrap().bytes, 3);
    f.cache.configure(f.root.clone(), Config::default(), false);
    assert!(f.cache.open("a".repeat(64), false).is_none());
    assert_eq!(fresh.read(0, 3).unwrap(), b"new");
    drop(fresh);
    assert_eq!(f.cache.0.lock().unwrap().bytes, 0);
}
#[test]
fn lru_eviction_respects_active_leases_and_lowering_limits_reclaims_after_release() {
    let f = Fixture::new();
    let bytes = vec![1; BLOCK];
    for key in ["a", "b", "c"] {
        let lease = f.open(key);
        for block in 0..2 {
            lease.store(block * BLOCK as u64, &bytes, Some(2 * BLOCK as u64));
        }
    }
    let active = f.open("a");
    {
        let mut state = f.cache.0.lock().unwrap();
        // Small deterministic budget exercises the same eviction path as the settings limit.
        state.config.limit_mib = 1;
        for entry in state.entries.values_mut() {
            entry.record.touched = if entry.record.key.starts_with('b') {
                1
            } else {
                2
            };
        }
        state.evict(0);
        assert!(!state.current.contains_key(&"b".repeat(64)));
        assert!(state.current.contains_key(&"a".repeat(64)));
        state.config.limit_mib = 0;
        state.evict(0);
        assert_eq!(state.bytes, 2 * BLOCK as u64);
    }
    assert_eq!(active.read(0, BLOCK).unwrap(), bytes);
    drop(active);
    assert_eq!(f.cache.0.lock().unwrap().bytes, 0);
}
fn range_server(bytes: Vec<u8>, count: Arc<AtomicUsize>) -> Server {
    Server::spawn(move |request| {
        count.fetch_add(1, Ordering::SeqCst);
        let value = request
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("range:"))
            .unwrap()
            .split_once("bytes=")
            .unwrap()
            .1;
        let (start, end) = value.trim().split_once('-').unwrap();
        let start = start.parse::<usize>().unwrap();
        let end = end.parse::<usize>().unwrap().min(bytes.len() - 1);
        response(
            "206 Partial Content",
            &format!("Content-Range: bytes {start}-{end}/{}\r\n", bytes.len()),
            &bytes[start..=end],
        )
    })
}
#[test]
fn replay_and_seek_use_fetched_ranges_without_another_audio_request() {
    let f = Fixture::new();
    let count = Arc::new(AtomicUsize::new(0));
    let bytes: Vec<_> = (0..BLOCK * 3 + 45).map(|i| (i % 251) as u8).collect();
    let server = range_server(bytes.clone(), count.clone());
    for pass in 0..2 {
        let mut source = HttpSource::open_cached(
            Client::stream_client(&ProxyConfig::Off).unwrap(),
            reqwest::Url::parse(&server.url).unwrap(),
            Arc::new(AtomicU64::new(1)),
            1,
            None,
            Some(f.open("a")),
        )
        .unwrap();
        let mut read = Vec::new();
        source.read_to_end(&mut read).unwrap();
        assert_eq!(read, bytes);
        source.seek(SeekFrom::Start(19)).unwrap();
        let mut read = [0; 22];
        source.read_exact(&mut read).unwrap();
        assert_eq!(&read, &bytes[19..41]);
        if pass == 0 {
            assert_eq!(count.load(Ordering::SeqCst), 4);
        }
    }
    assert_eq!(count.load(Ordering::SeqCst), 4);
}

#[test]
fn converted_bodies_replay_only_after_eof_and_offsets_do_not_share_partial_data() {
    use crate::navidrome::stream::forward::ForwardSource;
    let f = Fixture::new();
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let data = vec![7; BLOCK + 43];
    let body = data.clone();
    let server = Server::spawn(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        response("200 OK", "", &body)
    });
    let open = |cache| {
        ForwardSource::open_cached(
            Client::stream_client(&ProxyConfig::Off).unwrap(),
            reqwest::Url::parse(&server.url).unwrap(),
            Arc::new(AtomicU64::new(1)),
            1,
            None,
            cache,
        )
        .unwrap()
    };
    {
        let mut source = open(f.cache.open("a".repeat(64), true));
        let mut prefix = vec![0; BLOCK];
        source.read_exact(&mut prefix).unwrap();
        assert_eq!(prefix, data[..BLOCK]);
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    // The previous partial response must not masquerade as a complete offline file.
    for pass in 0..2 {
        let mut source = open(f.cache.open("a".repeat(64), true));
        let mut read = Vec::new();
        source.read_to_end(&mut read).unwrap();
        assert_eq!(read, data);
        if pass == 0 {
            assert_eq!(count.load(Ordering::SeqCst), 2);
        }
    }
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "the complete replay sends no audio request"
    );
    let mut other = open(f.cache.open("b".repeat(64), true));
    let mut read = Vec::new();
    other.read_to_end(&mut read).unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[test]
fn clearing_an_active_stream_preserves_later_playback_and_seek() {
    let f = Fixture::new();
    let count = Arc::new(AtomicUsize::new(0));
    let data: Vec<_> = (0..BLOCK * 2 + 39).map(|i| (i % 251) as u8).collect();
    let server = range_server(data.clone(), count);
    let mut source = HttpSource::open_cached(
        Client::stream_client(&ProxyConfig::Off).unwrap(),
        reqwest::Url::parse(&server.url).unwrap(),
        Arc::new(AtomicU64::new(1)),
        1,
        None,
        Some(f.open("a")),
    )
    .unwrap();
    let mut prefix = [0; 29];
    source.read_exact(&mut prefix).unwrap();
    f.cache.configure(
        f.root.clone(),
        Config {
            enabled: true,
            limit_mib: 128,
        },
        true,
    );
    let mut tail = Vec::new();
    source.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, data[29..]);
    source.seek(SeekFrom::Start(11)).unwrap();
    source.read_exact(&mut prefix).unwrap();
    assert_eq!(&prefix, &data[11..40]);
    // Readable audio belongs to the active lease, and new opens cannot adopt it.
    assert!(f.open("a").read(0, 10).is_none());
}

#[test]
fn cache_write_failure_leaves_streaming_and_other_entries_usable() {
    let f = Fixture::new();
    let kept = f.open("b");
    kept.store(0, b"kept", Some(4));
    let cache = f.open("a");
    let path = {
        let state = f.cache.0.lock().unwrap();
        state
            .entries
            .values()
            .find(|e| e.record.key == "a".repeat(64))
            .unwrap()
            .path
            .clone()
    };
    std::fs::create_dir(storage::block_path(&path, 0)).unwrap(); // Atomic replacement failure.
    let data = vec![9; BLOCK / 2];
    let server = range_server(data.clone(), Arc::new(AtomicUsize::new(0)));
    let mut source = HttpSource::open_cached(
        Client::stream_client(&ProxyConfig::Off).unwrap(),
        reqwest::Url::parse(&server.url).unwrap(),
        Arc::new(AtomicU64::new(1)),
        1,
        None,
        Some(cache),
    )
    .unwrap();
    let mut read = Vec::new();
    source.read_to_end(&mut read).unwrap();
    assert_eq!(read, data);
    assert_eq!(kept.read(0, 4).unwrap(), b"kept");
    assert!(f.cache.0.lock().unwrap().error.is_some());
}

#[test]
fn expiration_and_clear_preserve_unmanaged_files_and_explicit_downloads() {
    let f = Fixture::new();
    let lease = f.open("a");
    lease.store(0, b"old", Some(3));
    {
        let mut state = f.cache.0.lock().unwrap();
        state.entries.values_mut().next().unwrap().record.created = 1;
    }
    drop(lease);
    std::fs::create_dir(f.root.join("downloads")).unwrap();
    std::fs::write(f.root.join("downloads/explicit.audio"), b"keep").unwrap();
    std::fs::write(f.root.join("personal.txt"), b"keep").unwrap();
    let restarted = Cache::default();
    restarted.configure(
        f.root.clone(),
        Config {
            enabled: true,
            limit_mib: 128,
        },
        false,
    );
    assert_eq!(restarted.0.lock().unwrap().bytes, 0);
    restarted.configure(f.root.clone(), Config::default(), true);
    assert_eq!(
        std::fs::read(f.root.join("downloads/explicit.audio")).unwrap(),
        b"keep"
    );
    assert_eq!(std::fs::read(f.root.join("personal.txt")).unwrap(), b"keep");
}

#[test]
fn disk_full_during_atomic_publication_keeps_the_previous_block() {
    use std::io::Write;
    let f = Fixture::new();
    std::fs::create_dir_all(&f.root).unwrap();
    storage::write(&f.root, 0, b"old").unwrap();
    let error = storage::write_with(&f.root, 0, |file| {
        file.write_all(b"part")?;
        Err(std::io::Error::from(std::io::ErrorKind::StorageFull))
    })
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::StorageFull);
    assert_eq!(
        std::fs::read(storage::block_path(&f.root, 0)).unwrap(),
        b"old"
    );
    assert_eq!(std::fs::read_dir(&f.root).unwrap().count(), 1);
}

#[test]
fn default_preferences_keep_streaming_memory_only_and_survive_old_settings() {
    let preferences: crate::app::navidrome::Preferences = serde_json::from_str("{}").unwrap();
    assert!(!preferences.audio_cache.enabled);
    assert_eq!(preferences.audio_cache.limit_mib, 1024);
    let saved = serde_json::to_string(&preferences).unwrap();
    let restored: crate::app::navidrome::Preferences = serde_json::from_str(&saved).unwrap();
    assert_eq!(preferences.audio_cache, restored.audio_cache);
}
