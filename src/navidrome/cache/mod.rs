//! Disposable streamed bytes. Explicit downloads never enter this store.
use super::{api::Song, quality::Quality};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

mod lease;
mod storage;
pub use lease::Lease;
const BLOCK: usize = 256 * 1024;
const MAX_ENTRIES: usize = 4096;
const RETENTION: u64 = 7 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub limit_mib: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            limit_mib: 1024,
        }
    }
}
impl Config {
    pub const LIMITS: [u32; 6] = [128, 512, 1024, 2048, 4096, 8192];
    pub fn normalized(self) -> Self {
        Self {
            limit_mib: self.limit_mib.clamp(128, 8192),
            ..self
        }
    }
    fn bytes(self) -> u64 {
        u64::from(self.limit_mib) * 1024 * 1024
    }
}
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub revision: u64,
    pub bytes: u64,
    pub retiring: u64,
    pub error: Option<String>,
}
pub fn key(scope: &str, song: &Song, quality: Quality, ranged: bool, offset: f64) -> String {
    // Length-prefix every field. No request URL, credential, or server token is retained.
    let mut hash = Sha256::new();
    for field in [scope, &song.id, &song.suffix] {
        hash.update((field.len() as u64).to_le_bytes());
        hash.update(field.as_bytes());
    }
    hash.update(song.size.unwrap_or(0).to_le_bytes());
    hash.update(song.duration.to_bits().to_le_bytes());
    hash.update(quality.kbps().unwrap_or(0).to_le_bytes());
    hash.update([u8::from(ranged)]);
    hash.update(if ranged { 0 } else { offset.to_bits() }.to_le_bytes());
    format!("{:x}", hash.finalize())
}
#[derive(Clone, Serialize, Deserialize)]
struct Block {
    bytes: u32,
    sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    version: u32,
    key: String,
    created: u64,
    touched: u64,
    length: Option<u64>,
    blocks: BTreeMap<u64, Block>,
}
impl Record {
    fn bytes(&self) -> u64 {
        self.blocks.values().map(|b| u64::from(b.bytes)).sum()
    }
    fn complete(&self) -> bool {
        let Some(length) = self.length.filter(|n| *n > 0) else {
            return false;
        };
        let mut end = 0;
        for (start, block) in &self.blocks {
            if *start > end {
                return false;
            }
            end = end.max(start.saturating_add(u64::from(block.bytes)));
            if end >= length {
                return true;
            }
        }
        false
    }
}
struct Entry {
    record: Record,
    path: PathBuf,
    pins: usize,
    retiring: bool,
    failed: bool,
    dirty: usize,
}
struct State {
    root: Option<PathBuf>,
    config: Config,
    entries: BTreeMap<u64, Entry>,
    current: BTreeMap<String, u64>,
    serial: u64,
    access: u64,
    revision: u64,
    bytes: u64,
    error: Option<String>,
    notify: Arc<dyn Fn(Summary) + Send + Sync>,
    reported: Instant,
}
#[derive(Clone)]
pub struct Cache(Arc<Mutex<State>>);
impl Default for Cache {
    fn default() -> Self {
        Self::new(Arc::new(|_| {}))
    }
}
impl Cache {
    pub(crate) fn new(notify: Arc<dyn Fn(Summary) + Send + Sync>) -> Self {
        Self(Arc::new(Mutex::new(State {
            root: None,
            config: Config::default(),
            entries: BTreeMap::new(),
            current: BTreeMap::new(),
            serial: 0,
            access: 0,
            revision: 0,
            bytes: 0,
            error: None,
            notify,
            reported: Instant::now(),
        })))
    }
    /// Called on a blocking worker, never by the UI or audio callback.
    pub(crate) fn configure(&self, root: PathBuf, config: Config, clear: bool) {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        state.revision = state.revision.wrapping_add(1);
        state.config = config.normalized();
        state.error = None;
        if state.root.is_none() {
            match storage::load(&root) {
                Ok(entries) => {
                    for (path, record) in entries {
                        state.serial += 1;
                        let serial = state.serial;
                        state.bytes += record.bytes();
                        state.access = state.access.max(record.touched);
                        let retiring = record.created.saturating_add(RETENTION) < now();
                        if !retiring
                            && let Some(old) = state.current.insert(record.key.clone(), serial)
                        {
                            state.entries.get_mut(&old).unwrap().retiring = true;
                        }
                        state.entries.insert(
                            serial,
                            Entry {
                                record,
                                path,
                                pins: 0,
                                retiring,
                                failed: false,
                                dirty: 0,
                            },
                        );
                    }
                }
                Err(error) => {
                    state.error = Some(error);
                    state.emit(true);
                    return;
                }
            }
            state.root = Some(root);
        }
        for entry in state.entries.values_mut() {
            entry.failed = false;
        }
        if clear || !state.config.enabled {
            state.current.clear();
            for entry in state.entries.values_mut() {
                entry.retiring = true;
            }
        }
        state.evict(0);
        state.emit(true);
    }
    pub(crate) fn open(&self, key: String, forward: bool) -> Option<Lease> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if !state.config.enabled || state.root.is_none() || !storage::hash(&key) {
            return None;
        }
        state.access = state.access.saturating_add(1).max(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        );
        let touched = state.access;
        if let Some(id) = state.current.get(&key).copied() {
            let entry = state.entries.get_mut(&id).unwrap();
            if entry.record.created.saturating_add(RETENTION) < now()
                || (forward && !entry.record.complete())
            {
                entry.retiring = true;
                state.current.remove(&key);
            } else {
                entry.pins += 1;
                entry.record.touched = touched;
                return Some(Lease::new(self.clone(), id));
            }
        }
        state.evict(0);
        if state.entries.len() >= MAX_ENTRIES {
            return None;
        }
        let path = state
            .root
            .as_ref()
            .unwrap()
            .join(format!("{key}-{:016x}", rand::random::<u64>()));
        if std::fs::create_dir_all(state.root.as_ref().unwrap())
            .and_then(|()| std::fs::create_dir(&path))
            .is_err()
        {
            state.error = Some("Audio cache is unavailable. Streaming continues without it. Check disk space and folder permissions.".into());
            state.emit(true);
            return None;
        }
        state.serial = state.serial.wrapping_add(1);
        let serial = state.serial;
        state.current.insert(key.clone(), serial);
        state.entries.insert(
            serial,
            Entry {
                record: Record {
                    version: 1,
                    key,
                    created: now(),
                    touched,
                    length: None,
                    blocks: BTreeMap::new(),
                },
                path,
                pins: 1,
                retiring: false,
                failed: false,
                dirty: 0,
            },
        );
        Some(Lease::new(self.clone(), serial))
    }
}
impl State {
    fn emit(&mut self, force: bool) {
        if !force && self.reported.elapsed() < Duration::from_millis(250) {
            return;
        }
        self.reported = Instant::now();
        (self.notify)(Summary {
            revision: self.revision,
            bytes: self.bytes,
            retiring: self
                .entries
                .values()
                .filter(|e| e.retiring)
                .map(|e| e.record.bytes())
                .sum(),
            error: self.error.clone(),
        });
    }
    fn evict(&mut self, required: u64) {
        if self.bytes.saturating_add(required) <= self.config.bytes()
            && self.entries.len() < MAX_ENTRIES
            && self
                .entries
                .values()
                .all(|entry| !entry.retiring || entry.pins > 0)
        {
            return;
        }
        let mut candidates: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| e.pins == 0)
            .map(|(id, e)| (*id, !e.retiring, e.record.touched))
            .collect();
        candidates.sort_by_key(|(_, live, touched)| (*live, *touched));
        for (id, _, _) in candidates {
            let entry = &self.entries[&id];
            if !entry.retiring
                && self.bytes.saturating_add(required) <= self.config.bytes()
                && self.entries.len() < MAX_ENTRIES
            {
                break;
            }
            let removed = std::fs::remove_dir_all(&entry.path).or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            });
            match removed {
                Ok(()) => {
                    let entry = self.entries.remove(&id).unwrap();
                    self.bytes = self.bytes.saturating_sub(entry.record.bytes());
                    if self.current.get(&entry.record.key) == Some(&id) { self.current.remove(&entry.record.key); }
                }
                Err(_) => self.error = Some("Some cached files could not be removed. Check folder permissions and try Clear again.".into()),
            }
        }
    }
}
fn now() -> u64 {
    super::sync::now()
}

#[cfg(test)]
mod tests;
