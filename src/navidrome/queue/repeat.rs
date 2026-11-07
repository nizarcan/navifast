//! A future repeat cycle is visible before the current cycle ends.
use super::{Entry, Queue, Source};
use std::{collections::HashSet, sync::Arc};

#[derive(Default)]
pub(super) struct Repeat {
    pub enabled: bool,
    /// Tail entries belonging to a cycle that has not started yet.
    pub preview: usize,
    /// Only needed without an album/playlist context. Song data stays shared.
    pub standalone: Vec<Entry>,
    pub removed: HashSet<usize>,
}

// Each manual occurrence gets a distinct Arc, shared only with its later cycles.
fn same(a: &Source, b: &Source) -> bool {
    match (a, b) {
        (Source::Manual(a), Source::Manual(b)) => Arc::ptr_eq(a, b),
        (Source::Context(a), Source::Context(b)) => a == b,
        _ => false,
    }
}

impl Queue {
    pub fn set_repeat_all(&mut self, enabled: bool) {
        if self.repeat.enabled == enabled {
            return;
        }
        self.repeat.enabled = enabled;
        if enabled {
            self.clear_automatic();
            if self.context.is_empty() {
                if let Some(Entry { source, .. }) = &mut self.current
                    && let Source::Automatic(song) = source
                {
                    *source = Source::Manual(song.clone());
                }
                self.repeat.standalone =
                    self.current.iter().chain(&self.upcoming).cloned().collect();
            }
            self.ensure_repeat_preview();
        } else {
            self.discard_repeat_preview();
            self.repeat.standalone = Vec::new();
        }
    }

    pub(super) fn discard_repeat_preview(&mut self) {
        self.upcoming
            .truncate(self.upcoming.len() - self.repeat.preview);
        self.repeat.preview = 0;
    }

    pub(super) fn ensure_repeat_preview(&mut self) {
        if !self.repeat.enabled || self.repeat.preview > 0 || self.current.is_none() {
            return;
        }
        if self.context.is_empty() {
            if !self.upcoming.is_empty() {
                return;
            }
            self.upcoming.reserve(self.repeat.standalone.len());
            for i in 0..self.repeat.standalone.len() {
                let entry = self.entry(self.repeat.standalone[i].source.clone());
                self.upcoming.push_back(entry);
            }
            self.repeat.preview = self.upcoming.len();
        } else if self.upcoming.len() == self.manual_len() {
            let before = self.upcoming.len();
            self.upcoming
                .reserve(self.context.len() - self.repeat.removed.len());
            for index in 0..self.context.len() {
                if !self.repeat.removed.contains(&index) {
                    let entry = self.entry(Source::Context(index));
                    self.upcoming.push_back(entry);
                }
            }
            self.repeat.preview = self.upcoming.len() - before;
            if self.shuffle {
                self.order_collection();
            }
        }
    }

    pub(super) fn repeat_additions(&mut self, entries: &[Entry], next: bool) {
        if !self.repeat.enabled || !self.context.is_empty() {
            return;
        }
        // New explicit additions postpone the preview until this pass reaches
        // them. Rebuild it from the updated loop, including any loaded album.
        self.discard_repeat_preview();
        let at = if next {
            self.current
                .as_ref()
                .and_then(|current| {
                    self.repeat
                        .standalone
                        .iter()
                        .position(|e| same(&e.source, &current.source))
                })
                .map_or(0, |i| i + 1)
        } else {
            self.repeat.standalone.len()
        };
        self.repeat
            .standalone
            .splice(at..at, entries.iter().cloned());
    }

    pub(super) fn repeat_moved(&mut self, from: usize, to: usize) {
        if !self.repeat.enabled || !self.context.is_empty() {
            return;
        }
        let index = |entry: &Entry| {
            self.repeat
                .standalone
                .iter()
                .position(|e| e.id == entry.id || same(&e.source, &entry.source))
        };
        if let (Some(from), Some(to)) = (index(&self.upcoming[from]), index(&self.upcoming[to])) {
            let entry = self.repeat.standalone.remove(from);
            self.repeat.standalone.insert(to, entry);
        }
    }

    pub(super) fn repeat_resolved(&mut self, token: u64, at: usize, count: usize) {
        if let Some(i) = self
            .repeat
            .standalone
            .iter()
            .position(|e| e.id == token && e.pending())
        {
            self.repeat
                .standalone
                .splice(i..=i, self.upcoming.iter().skip(at).take(count).cloned());
        }
    }

    pub(super) fn repeat_removed(&mut self, entry: &Entry) {
        match &entry.source {
            Source::Context(index) => {
                self.repeat.removed.insert(*index);
            }
            Source::Manual(_) => self
                .repeat
                .standalone
                .retain(|e| !same(&e.source, &entry.source)),
            Source::Album { .. } => self.repeat.standalone.retain(|e| e.id != entry.id),
            Source::Automatic(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::api::Song;
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
    fn repeat_cycle_has_new_occurrences_and_off_removes_only_the_future_cycle() {
        let mut q = Queue::default();
        q.start(songs(&["a", "dup", "dup"]), 0);
        q.set_repeat_all(true);
        q.advance();
        q.advance();
        let current = q.current().unwrap().id;
        assert_eq!(ids(&q), ["a", "dup", "dup"]);
        assert!(q.upcoming.iter().all(|e| e.id != current));
        q.add(songs(&["manual"]), false);
        assert_eq!(ids(&q), ["manual", "a", "dup", "dup"]);
        q.set_repeat_all(false);
        assert_eq!(ids(&q), ["manual"]);
        assert_eq!(q.current().unwrap().id, current);
        q.set_repeat_all(true);
        q.advance();
        let head = q.upcoming[0].id;
        q.advance();
        assert_eq!(q.current().unwrap().id, head);
        q.set_repeat_all(false);
        assert_eq!(ids(&q), ["dup", "dup"], "the new cycle has already started");
    }

    #[test]
    fn standalone_manual_queue_repeats_duplicates_and_new_additions_in_order() {
        let mut q = Queue::default();
        q.add(songs(&["dup", "dup"]), false);
        q.set_repeat_all(true);
        q.advance();
        q.advance();
        assert_eq!(ids(&q), ["dup", "dup"]);
        assert_eq!(
            q.manual_len(),
            0,
            "the future pass is not a new explicit addition"
        );
        q.add(songs(&["later"]), false);
        assert_eq!(ids(&q), ["later"]);
        q.advance();
        assert_eq!(ids(&q), ["dup", "dup", "later"]);
        let second = q.upcoming[1].id;
        q.remove(second);
        assert_eq!(ids(&q), ["dup", "later"]);
        q.advance();
        q.advance();
        assert_eq!(
            ids(&q),
            ["dup", "later"],
            "removal also applies to future passes"
        );
        q.clear_manual();
        assert_eq!(
            ids(&q),
            ["later"],
            "Clear keeps only the playing occurrence in the standalone loop"
        );
        q.set_repeat_all(false);
        assert!(q.upcoming.is_empty());
    }

    #[test]
    fn canceled_standalone_album_never_enters_a_repeated_cycle() {
        let mut q = Queue::default();
        q.add(songs(&["playing"]), false);
        q.advance();
        q.set_repeat_all(true);
        let token = q.reserve("album".into(), "Album".into(), false);
        q.add(songs(&["after"]), false);
        q.resolve(token, songs(&["one", "two"]));
        q.advance();
        q.advance();
        q.advance();
        assert_eq!(ids(&q), ["playing", "one", "two", "after"]);
        let token = q.reserve("cancel".into(), "Cancel".into(), false);
        q.clear_manual();
        assert!(!q.resolve(token, songs(&["late"])));
        assert_eq!(ids(&q), ["after"]);
    }

    #[test]
    fn removing_the_only_repeat_occurrence_does_not_recreate_it() {
        let mut q = Queue::default();
        q.start(songs(&["only"]), 0);
        q.set_repeat_all(true);
        assert_eq!(ids(&q), ["only"]);
        let next = q.upcoming[0].id;
        q.remove(next);
        assert!(q.upcoming.is_empty());
        assert_eq!(q.context_index(), Some(0));
    }
}
