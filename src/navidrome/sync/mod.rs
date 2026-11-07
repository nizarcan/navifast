//! Durable account intent. Desired values coalesce; listening occurrences never do.
use super::{
    api::{Catalogue, FavoriteKind, RequestError},
    ratings::Kind,
    scrobble::Report,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod item;
pub use item::Item;
mod network;
mod storage;
pub mod worker;

const MAX_ENTRIES: usize = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Work {
    Favorite {
        item: Box<Item>,
        starred: bool,
    },
    Rating {
        kind: Kind,
        id: String,
        name: String,
        value: u8,
    },
    Listen {
        key: String,
        report: Report,
    },
}
impl Work {
    pub fn key(&self) -> String {
        match self {
            Self::Favorite { item, .. } => {
                let (kind, id) = item.key();
                format!("favorite:{kind:?}:{id}")
            }
            Self::Rating { kind, id, .. } => format!("rating:{kind:?}:{id}"),
            Self::Listen { key, .. } => format!("listen:{key}"),
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Favorite { item, .. } => item.name(),
            Self::Rating { name, .. } => name,
            Self::Listen { report, .. } => &report.title,
        }
    }
    fn valid(&self) -> bool {
        match self {
            Self::Favorite { item, .. } => !item.key().1.is_empty(),
            Self::Rating { id, value, .. } => !id.is_empty() && *value <= 5,
            Self::Listen { key, report } => {
                !key.is_empty() && !report.id.is_empty() && report.submission
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    Pending,
    Sending,
    Failed(String),
    Waiting(String),
    Uncertain(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub work: Work,
    pub serial: u64,
    pub status: Status,
    pub attempts: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Journal {
    version: u32,
    pub scope: String,
    serial: u64,
    pub pending: BTreeMap<String, Entry>,
    pub favorites: Catalogue,
    #[serde(default)]
    favorite_guards: BTreeMap<String, Work>,
    pub ratings: BTreeMap<String, Work>,
    pub last_sync: Option<u64>,
}
impl Journal {
    pub fn new(scope: String) -> Self {
        Self {
            version: 1,
            scope,
            serial: 0,
            pending: BTreeMap::new(),
            favorites: Catalogue::default(),
            favorite_guards: BTreeMap::new(),
            ratings: BTreeMap::new(),
            last_sync: None,
        }
    }
    pub fn enqueue(&mut self, work: Work) -> Result<(), String> {
        if !work.valid() {
            return Err("This change is invalid.".into());
        }
        let key = work.key();
        // A repeated delivery of one completed occurrence is not another listen.
        if matches!(work, Work::Listen { .. }) && self.pending.contains_key(&key) {
            return Ok(());
        }
        if self.pending.len() >= MAX_ENTRIES && !self.pending.contains_key(&key) {
            return Err(
                "Too many changes are waiting. Sync or dismiss failed changes first.".into(),
            );
        }
        self.serial = self.serial.wrapping_add(1);
        self.pending.insert(
            key,
            Entry {
                work,
                serial: self.serial,
                status: Status::Pending,
                attempts: 0,
            },
        );
        Ok(())
    }
    fn recover(&mut self) {
        for entry in self.pending.values_mut() {
            if matches!(entry.status, Status::Waiting(_)) {
                entry.status = Status::Pending;
                entry.attempts = 0;
            }
            if entry.status == Status::Sending {
                entry.status = if matches!(entry.work, Work::Listen { .. }) {
                    Status::Uncertain("The app closed before this listen was confirmed. It will not be sent again.".into())
                } else {
                    Status::Pending
                };
            }
        }
    }
    fn finish(&mut self, key: &str, serial: u64, result: Result<(), RequestError>) -> bool {
        let Some(entry) = self.pending.get_mut(key).filter(|e| e.serial == serial) else {
            return false;
        };
        match result {
            Ok(()) => {
                match &entry.work {
                    Work::Favorite { item, starred } => {
                        self.favorites.show(item, *starred);
                        if self.favorite_guards.len() >= MAX_ENTRIES {
                            self.favorite_guards.pop_first();
                        }
                        self.favorite_guards.insert(key.into(), entry.work.clone());
                    }
                    work @ Work::Rating { .. } => {
                        if self.ratings.len() >= MAX_ENTRIES && !self.ratings.contains_key(key) {
                            self.ratings.pop_first();
                        }
                        self.ratings.insert(key.into(), work.clone());
                    }
                    Work::Listen { .. } => {}
                }
                self.pending.remove(key);
                self.last_sync = Some(now());
                false
            }
            Err(error) => {
                let retry = matches!(
                    error,
                    RequestError::Unsent(_) | RequestError::Unconfirmed(_)
                ) && !(matches!(error, RequestError::Unconfirmed(_))
                    && matches!(entry.work, Work::Listen { .. }));
                entry.attempts = entry.attempts.saturating_add(1);
                entry.status = if matches!(error, RequestError::Unconfirmed(_))
                    && matches!(entry.work, Work::Listen { .. })
                {
                    Status::Uncertain(error.to_string())
                } else if retry && entry.attempts < 4 {
                    Status::Pending
                } else if retry {
                    Status::Waiting(error.to_string())
                } else {
                    Status::Failed(error.to_string())
                };
                entry.status == Status::Pending
            }
        }
    }
    fn retry(&mut self, key: Option<&str>) {
        for (id, entry) in &mut self.pending {
            if key.is_none_or(|key| key == id)
                && !matches!(entry.status, Status::Sending | Status::Uncertain(_))
            {
                entry.status = Status::Pending;
                entry.attempts = 0;
            }
        }
    }
    fn observe_favorites(&mut self, mut list: Catalogue) {
        // Reads queued before a successful write cannot undo its durable checkpoint.
        self.favorite_guards.retain(|_, work| {
            let Work::Favorite { item, starred } = work else {
                return false;
            };
            if list.contains(item) == *starred {
                false
            } else {
                list.show(item, *starred);
                true
            }
        });
        self.favorites = list;
    }
}
impl Catalogue {
    pub(crate) fn contains(&self, item: &Item) -> bool {
        let (kind, id) = item.key();
        match kind {
            FavoriteKind::Song => self.song.iter().any(|s| s.id == id),
            FavoriteKind::Album => self.album.iter().any(|a| a.id == id),
            FavoriteKind::Artist => self.artist.iter().any(|a| a.id == id),
        }
    }
    pub(crate) fn show(&mut self, item: &Item, starred: bool) {
        if self.contains(item) == starred {
            return;
        }
        match item {
            Item::Song(s) if starred => self.song.insert(0, s.clone()),
            Item::Album(a) if starred => self.album.insert(0, a.clone()),
            Item::Artist(a) if starred => self.artist.insert(0, a.clone()),
            Item::Song(s) => self.song.retain(|row| row.id != s.id),
            Item::Album(a) => self.album.retain(|row| row.id != a.id),
            Item::Artist(a) => self.artist.retain(|row| row.id != a.id),
        }
    }
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
