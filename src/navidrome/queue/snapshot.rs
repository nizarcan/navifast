//! Version-independent queue data containing IDs, never authenticated URLs.
use super::{Entry, Queue, Source, repeat::Repeat};
use crate::navidrome::api::Song;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    context: Vec<String>,
    manual: Vec<String>,
    current: Option<SavedEntry>,
    upcoming: Vec<SavedEntry>,
    history: Vec<SavedEntry>,
    serial: u64,
    shuffle: bool,
    repeat: bool,
    preview: usize,
    standalone: Vec<SavedEntry>,
    removed: HashSet<usize>,
}

#[derive(Serialize, Deserialize)]
struct SavedEntry {
    id: u64,
    source: SavedSource,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SavedSource {
    Context(usize),
    Manual(usize),
    Automatic(usize),
    Album(String),
}

impl Queue {
    pub fn snapshot(&self) -> Snapshot {
        let mut manual = Vec::new();
        let mut indices = HashMap::new();
        let mut save = |e: &Entry| SavedEntry {
            id: e.id,
            source: match &e.source {
                Source::Context(i) => SavedSource::Context(*i),
                Source::Manual(song) | Source::Automatic(song) => {
                    let index = *indices.entry(Arc::as_ptr(song)).or_insert_with(|| {
                        manual.push(song.id.clone());
                        manual.len() - 1
                    });
                    if e.automatic() {
                        SavedSource::Automatic(index)
                    } else {
                        SavedSource::Manual(index)
                    }
                }
                Source::Album { id, .. } => SavedSource::Album(id.clone()),
            },
        };
        let current = self.current.as_ref().map(&mut save);
        let upcoming = self.upcoming.iter().map(&mut save).collect();
        let history = self.history.iter().map(&mut save).collect();
        let standalone = self.repeat.standalone.iter().map(&mut save).collect();
        Snapshot {
            context: self.context.iter().map(|s| s.id.clone()).collect(),
            manual,
            current,
            upcoming,
            history,
            serial: self.serial,
            shuffle: self.shuffle,
            repeat: self.repeat.enabled,
            preview: self.repeat.preview,
            standalone,
            removed: self.repeat.removed.clone(),
        }
    }
}

impl Snapshot {
    pub fn context_len(&self) -> usize {
        self.context.len()
    }

    pub fn repeats(&self) -> bool {
        self.repeat
    }

    pub fn current_id(&self) -> Option<u64> {
        self.current.as_ref().map(|e| e.id)
    }

    pub fn song_ids(&self) -> HashSet<String> {
        self.context.iter().chain(&self.manual).cloned().collect()
    }

    pub fn valid(&self) -> bool {
        const LIMIT: usize = 100_000;
        let id_ok = |id: &String| !id.is_empty() && id.len() <= 16_384;
        let entry_ok = |e: &SavedEntry| {
            e.id > 0
                && e.id <= self.serial
                && match &e.source {
                    SavedSource::Context(i) => *i < self.context.len(),
                    SavedSource::Manual(i) | SavedSource::Automatic(i) => *i < self.manual.len(),
                    SavedSource::Album(id) => id_ok(id),
                }
        };
        let mut ids = HashSet::new();
        let entries_ok = self
            .current
            .iter()
            .chain(&self.upcoming)
            .chain(&self.history)
            .all(|e| entry_ok(e) && ids.insert(e.id));
        let actual = self.upcoming.len().saturating_sub(self.preview);
        let mut section = 0;
        let order_ok = self.upcoming[..actual].iter().all(|e| {
            let next = match e.source {
                SavedSource::Manual(_) | SavedSource::Album(_) => 0,
                SavedSource::Context(_) => 1,
                SavedSource::Automatic(_) => 2,
            };
            let valid = next >= section;
            section = next;
            valid
        });
        self.context.len() <= LIMIT
            && self.manual.len() <= LIMIT
            && self.upcoming.len() <= LIMIT
            && self.history.len() <= 128
            && self.standalone.len() <= LIMIT
            && self.serial < u64::MAX - LIMIT as u64
            && self.preview <= self.upcoming.len()
            && (self.repeat || self.preview == 0)
            && (self.preview == 0
                || !self
                    .upcoming
                    .iter()
                    .any(|e| matches!(e.source, SavedSource::Automatic(_))))
            && (self.context.is_empty() || self.standalone.is_empty())
            && self.context.iter().chain(&self.manual).all(id_ok)
            && self.removed.iter().all(|i| *i < self.context.len())
            && entries_ok
            && order_ok
            && self.standalone.iter().all(entry_ok)
            && self
                .current
                .as_ref()
                .is_none_or(|e| !matches!(e.source, SavedSource::Album(_)))
            && (self.context.is_empty()
                || self.upcoming[actual..]
                    .iter()
                    .all(|e| matches!(e.source, SavedSource::Context(_))))
    }

    /// Drop unavailable songs, remapping collection indices without changing order.
    /// The returned indices keep sorted-table occurrence keys aligned.
    pub fn restore(self, songs: &HashMap<String, Song>) -> (Queue, Vec<usize>) {
        let mut context = Vec::new();
        let mut kept = Vec::new();
        let mut remap = HashMap::new();
        for (i, id) in self.context.iter().enumerate() {
            if let Some(song) = songs.get(id) {
                remap.insert(i, context.len());
                context.push(song.clone());
                kept.push(i);
            }
        }
        let manual: Vec<_> = self
            .manual
            .iter()
            .map(|id| songs.get(id).cloned().map(Arc::new))
            .collect();
        let restore = |e: SavedEntry| {
            Some(Entry {
                id: e.id,
                source: match e.source {
                    SavedSource::Context(i) => Source::Context(*remap.get(&i)?),
                    SavedSource::Manual(i) => Source::Manual(manual.get(i)?.clone()?),
                    SavedSource::Automatic(i) => Source::Automatic(manual.get(i)?.clone()?),
                    SavedSource::Album(id) => Source::Album {
                        id,
                        name: String::new(),
                    },
                },
            })
        };
        let boundary = self.upcoming.len() - self.preview;
        let mut preview = 0;
        let upcoming = self
            .upcoming
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let entry = restore(e)?;
                if i >= boundary {
                    preview += 1;
                }
                Some(entry)
            })
            .collect();
        let mut queue = Queue {
            context,
            current: self.current.and_then(restore),
            upcoming,
            history: self.history.into_iter().filter_map(restore).collect(),
            serial: self.serial,
            shuffle: self.shuffle,
            repeat: Repeat {
                enabled: self.repeat,
                preview,
                standalone: self.standalone.into_iter().filter_map(restore).collect(),
                removed: self
                    .removed
                    .into_iter()
                    .filter_map(|i| remap.get(&i).copied())
                    .collect(),
            },
        };
        // A missing playing occurrence leaves the next valid one ready, paused.
        if queue.current.is_none() {
            queue.advance();
        }
        (queue, kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoplay_origin_and_manual_precedence_survive_a_checkpoint() {
        let mut q = Queue::default();
        q.start(songs(&["chosen"]), 0);
        q.add_automatic(songs(&["random"]).remove(0));
        q.add(songs(&["manual"]), false);
        let mut restored = roundtrip(&q);
        assert_eq!(restored.manual_len(), 1);
        assert_eq!(restored.automatic_len(), 1);
        assert!(restored.upcoming.back().unwrap().automatic());
        restored.advance();
        assert_eq!(restored.automatic_len(), 1);
        restored.clear_automatic();
        assert!(restored.upcoming.is_empty());
    }
    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter()
            .map(|id| Song {
                id: (*id).into(),
                duration: 60.0,
                ..Default::default()
            })
            .collect()
    }
    fn roundtrip(q: &Queue) -> Queue {
        let saved: Snapshot =
            serde_json::from_slice(&serde_json::to_vec(&q.snapshot()).unwrap()).unwrap();
        assert!(saved.valid());
        let songs = q
            .context
            .iter()
            .chain(
                q.current
                    .iter()
                    .chain(&q.upcoming)
                    .filter_map(|e| q.song(e)),
            )
            .map(|s| (s.id.clone(), s.clone()))
            .collect();
        saved.restore(&songs).0
    }
    #[test]
    fn shuffled_duplicate_occurrence_manual_prefix_and_repeat_preview_survive() {
        let mut q = Queue::default();
        q.start(songs(&["a", "dup", "dup", "z"]), 2);
        q.set_shuffle(true);
        q.add(songs(&["manual", "manual"]), false);
        q.set_repeat_all(true);
        let restored = roundtrip(&q);
        assert_eq!(restored.current().unwrap().id, q.current().unwrap().id);
        assert_eq!(restored.context_index(), Some(2));
        assert_eq!(
            restored.upcoming.iter().map(|e| e.id).collect::<Vec<_>>(),
            q.upcoming.iter().map(|e| e.id).collect::<Vec<_>>()
        );
        assert!(restored.shuffled());
        while q.repeat.preview == 0 {
            q.advance();
        }
        let mut restored = roundtrip(&q);
        assert_eq!(restored.repeat.preview, q.repeat.preview);
        restored.set_repeat_all(false);
        assert!(restored.upcoming.is_empty());
    }
    #[test]
    fn standalone_repeated_duplicates_keep_distinct_identity_after_restart() {
        let mut q = Queue::default();
        q.add(songs(&["dup", "dup"]), false);
        q.advance();
        q.set_repeat_all(true);
        q.advance();
        let mut restored = roundtrip(&q);
        let id = restored.upcoming[0].id;
        restored.remove(id);
        restored.advance();
        assert_eq!(
            restored.upcoming.len(),
            1,
            "only the removed duplicate stays excluded"
        );
        assert_eq!(restored.repeat.standalone.len(), 1);
    }
    #[test]
    fn changed_library_keeps_occurrence_order_and_remaps_removed_indices() {
        let mut q = Queue::default();
        q.start(songs(&["gone", "dup", "dup", "last"]), 2);
        q.set_repeat_all(true);
        let map = songs(&["dup", "last"])
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
        let (mut restored, kept) = q.snapshot().restore(&map);
        assert_eq!(kept, [1, 2, 3]);
        assert_eq!(restored.context_index(), Some(1));
        assert_eq!(restored.current().unwrap().id, q.current().unwrap().id);
        restored.advance();
        assert_eq!(restored.upcoming.len(), 3);
        let mut invalid = q.snapshot();
        invalid.preview = usize::MAX;
        assert!(!invalid.valid());
        invalid = q.snapshot();
        invalid.upcoming[0].id = invalid.current.as_ref().unwrap().id;
        assert!(!invalid.valid());
    }
}
