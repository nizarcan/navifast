//! Explicit account-scoped downloads. Only selected music is retained, never a catalogue.
use super::api::Song;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[cfg(test)]
#[path = "tests.rs"]
mod integration_tests;
pub(crate) mod storage;
mod transfer;
pub mod worker;
const MAX_TRACKS: usize = 10_000;
const MAX_SELECTIONS: usize = 1_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Song,
    Album,
    Playlist,
}
impl Kind {
    pub fn key(self, id: &str) -> String {
        let prefix = match self {
            Self::Song => "song",
            Self::Album => "album",
            Self::Playlist => "playlist",
        };
        format!("{prefix}:{}", track_stem(id))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    #[default]
    Queued,
    Downloading {
        bytes: u64,
        total: Option<u64>,
    },
    Ready {
        bytes: u64,
        codec: String,
    },
    Failed {
        message: String,
    },
    Cancelled,
}
impl Status {
    pub fn ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
    pub fn bytes(&self) -> u64 {
        match self {
            Self::Ready { bytes, .. } | Self::Downloading { bytes, .. } => *bytes,
            _ => 0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub song: Song,
    // Progress never rewrites the large selection manifest. Small completion records
    // live beside their files; the worker restores them before advertising readiness.
    #[serde(skip)]
    pub status: Status,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selection {
    pub kind: Kind,
    pub id: String,
    pub title: String,
    pub cover: Option<String>,
    pub songs: Vec<String>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub error: Option<String>,
}
impl Selection {
    pub fn key(&self) -> String {
        self.kind.key(&self.id)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Library {
    version: u32,
    pub scope: String,
    pub selections: BTreeMap<String, Selection>,
    pub tracks: BTreeMap<String, Track>,
    #[serde(skip)]
    active: BTreeSet<String>,
}
impl Library {
    pub fn new(scope: String) -> Self {
        Self {
            version: 1,
            scope,
            selections: BTreeMap::new(),
            tracks: BTreeMap::new(),
            active: BTreeSet::new(),
        }
    }
    pub fn valid(&self, scope: &str) -> bool {
        self.version == 1
            && self.scope == scope
            && self.tracks.len() <= MAX_TRACKS
            && self.selections.len() <= MAX_SELECTIONS
            && self
                .tracks
                .iter()
                .all(|(id, track)| id == &track.song.id && !id.is_empty() && id.len() <= 16_384)
            && self.selections.iter().all(|(key, s)| {
                key == &s.key()
                    && !s.id.is_empty()
                    && s.id.len() <= 16_384
                    && s.songs.len() <= MAX_TRACKS
                    && s.songs.iter().all(|id| self.tracks.contains_key(id))
            })
    }
    pub fn keep(&mut self, selection: Selection, songs: Option<Vec<Song>>) -> Result<(), String> {
        let key = selection.key();
        if self.selections.contains_key(&key) {
            return Ok(());
        }
        if self.selections.len() >= MAX_SELECTIONS {
            return Err("Keep fewer than 1,000 downloaded selections.".into());
        }
        if selection.id.is_empty() || selection.id.len() > 16_384 {
            return Err("This item has an invalid identifier.".into());
        }
        self.selections.insert(key.clone(), selection);
        if let Some(songs) = songs {
            if let Err(error) = self.resolve(&key, songs) {
                self.selections.remove(&key);
                return Err(error);
            }
        } else if let Some(selection) = self.selections.get_mut(&key) {
            selection.pending = true;
        }
        Ok(())
    }
    pub fn resolve(&mut self, key: &str, songs: Vec<Song>) -> Result<(), String> {
        if !self.selections.contains_key(key) {
            return Ok(());
        }
        let added: BTreeSet<_> = songs
            .iter()
            .map(|s| &s.id)
            .filter(|id| !self.tracks.contains_key(*id))
            .collect();
        if songs.len() > MAX_TRACKS || self.tracks.len() + added.len() > MAX_TRACKS {
            return Err("Keep fewer than 10,000 downloaded songs.".into());
        }
        if songs.iter().any(|s| s.id.is_empty() || s.id.len() > 16_384) {
            return Err("This collection contains an invalid song identifier.".into());
        }
        let selection = self.selections.get_mut(key).unwrap();
        selection.songs = songs.iter().map(|s| s.id.clone()).collect();
        selection.pending = false;
        selection.error = None;
        for song in songs {
            self.tracks.entry(song.id.clone()).or_insert(Track {
                song,
                status: Status::Queued,
            });
        }
        self.reindex();
        Ok(())
    }
    pub(crate) fn reindex(&mut self) {
        self.active = self
            .selections
            .values()
            .filter(|s| !s.paused)
            .flat_map(|s| s.songs.iter().cloned())
            .collect();
    }
    pub fn wanted(&self, id: &str) -> bool {
        self.active.contains(id)
    }
    pub fn pause(&mut self, key: &str) {
        if let Some(s) = self.selections.get_mut(key) {
            s.paused = true;
        }
        self.reindex();
        let cancelled: Vec<_> = self
            .tracks
            .iter()
            .filter(|(id, t)| !t.status.ready() && !self.wanted(id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in cancelled {
            self.tracks.get_mut(&id).unwrap().status = Status::Cancelled;
        }
    }
    pub fn retry(&mut self, key: &str) -> Vec<String> {
        let Some(s) = self.selections.get_mut(key) else {
            return Vec::new();
        };
        s.paused = false;
        s.error = None;
        let mut retried = Vec::new();
        for id in &s.songs {
            if let Some(track) = self.tracks.get_mut(id)
                && matches!(track.status, Status::Failed { .. } | Status::Cancelled)
            {
                track.status = Status::Queued;
                retried.push(id.clone());
            }
        }
        self.reindex();
        retried
    }
    pub fn remove(&mut self, key: &str) -> Vec<String> {
        self.selections.remove(key);
        let retained: BTreeSet<_> = self
            .selections
            .values()
            .flat_map(|s| s.songs.iter())
            .collect();
        let removed: Vec<_> = self
            .tracks
            .keys()
            .filter(|id| !retained.contains(id))
            .cloned()
            .collect();
        for id in &removed {
            self.tracks.remove(id);
        }
        self.reindex();
        removed
    }
    pub fn next_song(&self) -> Option<&Track> {
        self.tracks
            .iter()
            .find(|(id, t)| matches!(t.status, Status::Queued) && self.wanted(id))
            .map(|(_, t)| t)
    }
    pub fn ready_songs(&self, key: &str) -> Vec<Song> {
        self.selections
            .get(key)
            .into_iter()
            .flat_map(|s| &s.songs)
            .filter_map(|id| self.tracks.get(id))
            .filter(|t| t.status.ready())
            .map(|t| t.song.clone())
            .collect()
    }
    pub fn bytes(&self) -> u64 {
        self.tracks.values().map(|t| t.status.bytes()).sum()
    }
}

pub(crate) fn track_stem(id: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(id.as_bytes()))
}
pub(crate) fn audio_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("{}.audio", track_stem(id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn song(id: &str) -> Song {
        Song {
            id: id.into(),
            title: id.into(),
            suffix: "wav".into(),
            duration: 40.,
            ..Default::default()
        }
    }
    pub(super) fn selection(id: &str) -> Selection {
        Selection {
            kind: Kind::Playlist,
            id: id.into(),
            title: id.into(),
            cover: None,
            songs: Vec::new(),
            paused: false,
            pending: false,
            error: None,
        }
    }
    #[test]
    fn overlapping_downloads_store_once_and_preserve_repeated_play_order() {
        let mut l = Library::new("scope".into());
        l.keep(
            selection("one"),
            Some(vec![song("a"), song("a"), song("b")]),
        )
        .unwrap();
        l.keep(selection("two"), Some(vec![song("a"), song("c")]))
            .unwrap();
        assert_eq!(l.tracks.len(), 3);
        for t in l.tracks.values_mut() {
            t.status = Status::Ready {
                bytes: 100,
                codec: "pcm".into(),
            };
        }
        assert_eq!(l.bytes(), 300);
        let one = Kind::Playlist.key("one");
        assert_eq!(
            l.ready_songs(&one)
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "a", "b"]
        );
        assert_eq!(l.remove(&one), ["b"]);
        assert!(l.tracks["a"].status.ready());
        assert_eq!(l.bytes(), 200);
    }
    #[test]
    fn cancel_does_not_cancel_another_selection_and_retry_keeps_completed_files() {
        let mut l = Library::new("scope".into());
        l.keep(selection("one"), Some(vec![song("a"), song("b")]))
            .unwrap();
        l.keep(selection("two"), Some(vec![song("a"), song("c")]))
            .unwrap();
        l.tracks.get_mut("b").unwrap().status = Status::Ready {
            bytes: 100,
            codec: "pcm".into(),
        };
        let one = Kind::Playlist.key("one");
        let two = Kind::Playlist.key("two");
        l.pause(&one);
        assert!(l.wanted("a"));
        assert!(l.tracks["b"].status.ready());
        l.pause(&two);
        assert!(!l.wanted("a"));
        assert_eq!(l.tracks["a"].status, Status::Cancelled);
        l.retry(&one);
        assert!(l.wanted("a"));
        assert!(l.tracks["b"].status.ready());
        assert_eq!(l.tracks["c"].status, Status::Cancelled);
    }
    #[test]
    fn malformed_accounts_and_unknown_versions_never_load_as_a_different_library() {
        let mut l = Library::new("one".into());
        l.keep(selection("p:/λ"), Some(vec![song("s:/λ")])).unwrap();
        assert!(l.valid("one"));
        assert!(!l.valid("two"));
        l.version = 2;
        assert!(!l.valid("one"));
        assert_eq!(
            audio_path(Path::new("downloads"), "../../etc/passwd").parent(),
            Some(Path::new("downloads"))
        );
    }
}
