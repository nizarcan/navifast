//! Row keys survive sorting and disambiguate repeated songs in the same list.
use crate::{
    model::{RowPick, RowSelection, SortColumn, TableSort},
    navidrome::api::Song,
};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Tracks {
    // Input metadata stays in server order. Rows carry their original index and
    // occurrence key, even when two entries share the same song ID.
    inputs: Vec<(String, String, u32)>,
    rows: Vec<(usize, String)>,
    sort: Option<TableSort>,
    pub selection: RowSelection,
    pub cursor: Option<usize>,
}

impl Tracks {
    pub fn source(&self, index: usize) -> usize {
        self.rows[index].0
    }
    pub fn order(&self) -> impl Iterator<Item = (usize, &str)> {
        self.rows.iter().map(|(i, key)| (*i, key.as_str()))
    }
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
    pub fn update(&mut self, songs: &[Song], sort: Option<TableSort>) {
        let duration = |s: &Song| (s.duration * 1000.) as u32;
        if self.sort == sort
            && self
                .inputs
                .iter()
                .map(|(id, title, ms)| (id, title, *ms))
                .eq(songs.iter().map(|s| (&s.id, &s.title, duration(s))))
        {
            return;
        }
        self.sort = sort;
        self.inputs = songs
            .iter()
            .map(|s| (s.id.clone(), s.title.clone(), duration(s)))
            .collect();
        let mut occurrences = HashMap::<&str, usize>::new();
        let mut rows: Vec<_> = songs
            .iter()
            .enumerate()
            .map(|(index, s)| {
                let n = occurrences.entry(&s.id).or_default();
                let key = format!("song-{n}:{}", s.id);
                *n += 1;
                (index, key)
            })
            .collect();
        if let Some(sort) = sort {
            match sort.column {
                SortColumn::Title => {
                    if sort.ascending {
                        rows.sort_by_cached_key(|(i, _)| songs[*i].title.to_lowercase());
                    } else {
                        rows.sort_by_cached_key(|(i, _)| {
                            std::cmp::Reverse(songs[*i].title.to_lowercase())
                        });
                    }
                }
                SortColumn::Duration => rows.sort_by(|(a, _), (b, _)| {
                    let order = duration(&songs[*a]).cmp(&duration(&songs[*b]));
                    if sort.ascending {
                        order
                    } else {
                        order.reverse()
                    }
                }),
                SortColumn::Index if !sort.ascending => rows.reverse(),
                _ => {}
            }
        }
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
        t.update(&songs(&["dup", "x", "dup"]), None);
        t.pick(1, RowPick::Only);
        t.pick(2, RowPick::Range);
        let key = t.key(2).to_owned();
        t.update(&songs(&["new", "dup", "x", "dup"]), None);
        assert_eq!(t.cursor(), Some(key.as_str()));
        assert_eq!(t.index(&key), Some(3));
        assert_eq!(t.selection.rows.into_iter().collect::<Vec<_>>(), vec![2, 3]);
    }
}
