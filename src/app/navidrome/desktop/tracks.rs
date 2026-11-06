//! Row keys survive sorting and disambiguate repeated songs in the same list.
use crate::{
    model::{RowPick, RowSelection},
    navidrome::api::Song,
};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Tracks {
    rows: Vec<(String, String)>,
    pub selection: RowSelection,
    pub cursor: Option<usize>,
}

impl Tracks {
    pub fn key(&self, index: usize) -> &str {
        &self.rows[index].1
    }
    pub fn index(&self, key: &str) -> Option<usize> {
        self.rows.iter().position(|(_, candidate)| candidate == key)
    }
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.map(|i| self.key(i))
    }
    pub fn pick(&mut self, index: usize, pick: RowPick) {
        self.selection.pick(index, pick, self.rows.len());
        self.cursor = Some(index);
    }
    pub fn update(&mut self, songs: &[Song]) {
        if self
            .rows
            .iter()
            .map(|(id, _)| id)
            .eq(songs.iter().map(|s| &s.id))
        {
            return;
        }
        let mut occurrences = HashMap::<&str, usize>::new();
        let rows: Vec<_> = songs
            .iter()
            .map(|s| {
                let n = occurrences.entry(&s.id).or_default();
                let key = format!("song-{n}:{}", s.id);
                *n += 1;
                (s.id.clone(), key)
            })
            .collect();
        let positions: HashMap<_, _> = rows
            .iter()
            .enumerate()
            .map(|(i, (_, key))| (key.as_str(), i))
            .collect();
        let remap = |i: usize| positions.get(self.key(i)).copied();
        let selected = self
            .selection
            .rows
            .iter()
            .copied()
            .filter_map(remap)
            .collect();
        self.selection.anchor = self.selection.anchor.and_then(remap);
        self.cursor = self
            .cursor
            .and_then(|i| positions.get(self.key(i)).copied());
        self.selection.rows = selected;
        self.rows = rows;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_cursor_and_range_follow_occurrences_after_an_insertion() {
        let songs = |ids: &[&str]| {
            ids.iter()
                .map(|id| Song {
                    id: id.to_string(),
                    ..Default::default()
                })
                .collect::<Vec<_>>()
        };
        let mut t = Tracks::default();
        t.update(&songs(&["dup", "x", "dup"]));
        t.pick(1, RowPick::Only);
        t.pick(2, RowPick::Range);
        let key = t.key(2).to_owned();
        t.update(&songs(&["new", "dup", "x", "dup"]));
        assert_eq!(t.cursor(), Some(key.as_str()));
        assert_eq!(t.index(&key), Some(3));
        assert_eq!(t.selection.rows.into_iter().collect::<Vec<_>>(), vec![2, 3]);
    }
}
