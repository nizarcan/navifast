//! Local queue order. IDs identify occurrences, never catalogue songs.

use std::{collections::VecDeque, sync::Arc};

use super::api::Song;

pub type Id = u64;

#[derive(Clone, Debug)]
pub enum Source {
    Context(usize),
    Manual(Arc<Song>),
    /// Reserves a whole album's position while its songs load on the worker.
    Album {
        id: String,
        name: String,
    },
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: Id,
    pub source: Source,
}

impl Entry {
    pub fn manual(&self) -> bool {
        !matches!(self.source, Source::Context(_))
    }

    pub fn pending(&self) -> bool {
        matches!(self.source, Source::Album { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advance {
    Ready,
    Pending,
    Empty,
}

#[derive(Default)]
pub struct Queue {
    context: Vec<Song>,
    current: Option<Entry>,
    upcoming: VecDeque<Entry>,
    history: VecDeque<Entry>,
    serial: Id,
}

impl Queue {
    pub fn context(&self) -> &[Song] {
        &self.context
    }

    pub fn current(&self) -> Option<&Entry> {
        self.current.as_ref()
    }

    pub fn context_index(&self) -> Option<usize> {
        match &self.current.as_ref()?.source {
            Source::Context(index) => Some(*index),
            _ => None,
        }
    }

    pub fn song<'a>(&'a self, entry: &'a Entry) -> Option<&'a Song> {
        match &entry.source {
            Source::Context(index) => self.context.get(*index),
            Source::Manual(song) => Some(song),
            Source::Album { .. } => None,
        }
    }

    pub fn manual_len(&self) -> usize {
        self.upcoming.partition_point(Entry::manual)
    }

    pub fn upcoming(&self) -> &VecDeque<Entry> {
        &self.upcoming
    }

    fn entry(&mut self, source: Source) -> Entry {
        self.serial = self.serial.wrapping_add(1);
        Entry {
            id: self.serial,
            source,
        }
    }

    /// A new collection keeps explicit additions. It never restores consumed ones.
    pub fn start(&mut self, songs: Vec<Song>, index: usize) -> bool {
        if index >= songs.len() {
            return false;
        }
        self.upcoming.retain(Entry::manual);
        self.history.clear();
        self.context = songs;
        self.current = Some(self.entry(Source::Context(index)));
        for i in index + 1..self.context.len() {
            let entry = self.entry(Source::Context(i));
            self.upcoming.push_back(entry);
        }
        true
    }

    fn insert(&mut self, entries: impl IntoIterator<Item = Entry>, next: bool) {
        let at = if next { 0 } else { self.manual_len() };
        // Rotate the shorter end into place, append once, then restore the
        // prefix. Adding near the head never copies the full collection tail.
        let before = self.upcoming.len();
        self.upcoming.rotate_left(at);
        self.upcoming.extend(entries);
        let added = self.upcoming.len() - before;
        self.upcoming.rotate_right(at + added);
    }

    pub fn add(&mut self, songs: Vec<Song>, next: bool) {
        let entries: Vec<_> = songs
            .into_iter()
            .map(|s| self.entry(Source::Manual(Arc::new(s))))
            .collect();
        self.insert(entries, next);
    }

    pub fn reserve(&mut self, id: String, name: String, next: bool) -> Id {
        let entry = self.entry(Source::Album { id, name });
        let token = entry.id;
        self.insert([entry], next);
        token
    }

    /// Late replies for removed, cleared or skipped placeholders do nothing.
    /// The first song inherits the placeholder's ID, retaining selection.
    pub fn resolve(&mut self, token: Id, songs: Vec<Song>) -> bool {
        let Some(at) = self
            .upcoming
            .iter()
            .position(|e| e.id == token && e.pending())
        else {
            return false;
        };
        let added = songs.len();
        self.upcoming.rotate_left(at);
        self.upcoming.pop_front();
        self.upcoming.reserve(added);
        for (i, song) in songs.into_iter().enumerate() {
            let source = Source::Manual(Arc::new(song));
            let entry = if i == 0 {
                Entry { id: token, source }
            } else {
                self.entry(source)
            };
            self.upcoming.push_back(entry);
        }
        self.upcoming.rotate_right(at + added);
        true
    }

    fn remember(&mut self, entry: Entry) {
        self.history.push_back(entry);
        // A continuously fed manual queue must not retain every song ever played.
        if self.history.len() > 128 {
            self.history.pop_front();
        }
    }

    pub fn advance(&mut self) -> Advance {
        match self.upcoming.front() {
            None => return Advance::Empty,
            Some(entry) if entry.pending() => return Advance::Pending,
            _ => {}
        }
        if let Some(current) = self.current.take() {
            self.remember(current);
        }
        self.current = self.upcoming.pop_front();
        Advance::Ready
    }

    pub fn previous(&mut self) -> bool {
        let previous = self.history.pop_back().or_else(|| {
            let index = self.context_index()?.checked_sub(1)?;
            Some(self.entry(Source::Context(index)))
        });
        let Some(previous) = previous else {
            return false;
        };
        if let Some(current) = self.current.replace(previous) {
            let next = current.manual();
            self.insert([current], next);
        }
        true
    }

    pub fn play(&mut self, id: Id) -> Option<Advance> {
        let at = self.upcoming.iter().position(|e| e.id == id)?;
        self.upcoming.drain(..at);
        Some(self.advance())
    }

    pub fn remove(&mut self, id: Id) -> bool {
        let Some(at) = self.upcoming.iter().position(|e| e.id == id) else {
            return false;
        };
        self.upcoming.remove(at);
        true
    }

    /// Manual additions remain before the collection. Moves stay within their section.
    pub fn move_by(&mut self, id: Id, delta: isize) -> bool {
        let Some(at) = self.upcoming.iter().position(|e| e.id == id) else {
            return false;
        };
        let Some(to) = at
            .checked_add_signed(delta)
            .filter(|&to| to < self.upcoming.len())
        else {
            return false;
        };
        if self.upcoming[at].manual() != self.upcoming[to].manual() {
            return false;
        }
        let entry = self.upcoming.remove(at).expect("located occurrence");
        self.upcoming.insert(to, entry);
        true
    }

    pub fn clear_manual(&mut self) {
        self.upcoming.retain(|e| !e.manual());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter()
            .map(|id| Song {
                id: (*id).into(),
                ..Default::default()
            })
            .collect()
    }
    fn ids(q: &Queue) -> Vec<&str> {
        q.upcoming
            .iter()
            .map(|e| q.song(e).map_or("loading", |s| s.id.as_str()))
            .collect()
    }

    #[test]
    fn duplicates_and_manual_order_are_occurrences() {
        let mut q = Queue::default();
        q.start(songs(&["a", "a", "b"]), 0);
        q.add(songs(&["x", "x"]), false);
        let first = q.upcoming[0].id;
        let second = q.upcoming[1].id;
        q.add(songs(&["y", "z"]), true);
        assert_eq!(ids(&q), ["y", "z", "x", "x", "a", "b"]);
        assert_ne!(first, second);
        assert!(q.move_by(second, -1));
        assert!(q.remove(first));
        assert_eq!(q.play(second), Some(Advance::Ready));
        assert_eq!(q.current().unwrap().id, second);
        assert_eq!(ids(&q), ["a", "b"]);
        assert_eq!(q.advance(), Advance::Ready);
        assert_eq!(q.context_index(), Some(1));
    }

    #[test]
    fn async_albums_commit_atomically_in_reserved_order() {
        let mut q = Queue::default();
        q.start(songs(&["a", "b"]), 0);
        let album = q.reserve("album".into(), "Album".into(), false);
        q.add(songs(&["later"]), false);
        let later = q.upcoming[1].id;
        assert_eq!(ids(&q), ["loading", "later", "b"]);
        assert_eq!(q.advance(), Advance::Pending);
        assert!(q.resolve(album, songs(&["one", "one", "two"])));
        assert_eq!(ids(&q), ["one", "one", "two", "later", "b"]);
        assert_eq!(q.upcoming[0].id, album);
        assert_eq!(q.upcoming[3].id, later);
        q.advance();
        q.clear_manual();
        assert_eq!(q.song(q.current().unwrap()).unwrap().id, "one");
        assert_eq!(ids(&q), ["b"]);
    }

    #[test]
    fn clear_and_failed_or_skipped_fetches_cannot_restore_entries() {
        let mut q = Queue::default();
        q.start(songs(&["a", "b"]), 0);
        let cleared = q.reserve("album".into(), "Album".into(), false);
        q.clear_manual();
        assert!(!q.resolve(cleared, songs(&["late"])));
        let failed = q.reserve("album".into(), "Album".into(), false);
        q.add(songs(&["later"]), false);
        assert!(q.resolve(failed, vec![]));
        assert_eq!(ids(&q), ["later", "b"]);
        let skipped = q.reserve("album".into(), "Album".into(), true);
        let last = q.upcoming.back().unwrap().id;
        q.play(last);
        assert!(!q.resolve(skipped, songs(&["late"])));
        assert_eq!(q.context_index(), Some(1));
    }

    #[test]
    fn collection_changes_preserve_additions_and_previous_restores_consumed_head() {
        let mut q = Queue::default();
        q.start(songs(&["old", "tail"]), 0);
        q.add(songs(&["manual"]), false);
        let manual = q.upcoming[0].id;
        q.start(songs(&["new", "end"]), 0);
        assert_eq!(q.upcoming[0].id, manual);
        q.advance();
        q.previous();
        assert_eq!(q.context_index(), Some(0));
        assert_eq!(q.upcoming[0].id, manual);
        assert!(!q.move_by(manual, 1));
        q.clear_manual();
        assert_eq!(ids(&q), ["end"]);
    }
}
