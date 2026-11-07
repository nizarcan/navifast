//! Local, account-scoped Autoplay rules. IDs, not names, determine membership.
use crate::navidrome::api::{FavoriteKind, Song};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const MAX_ENTRIES: usize = 2000;
const MAX_ACCOUNTS: usize = 32;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Exclusions {
    songs: BTreeMap<String, String>,
    albums: BTreeMap<String, String>,
    artists: BTreeMap<String, String>,
}
impl Exclusions {
    fn map(&self, kind: FavoriteKind) -> &BTreeMap<String, String> {
        match kind {
            FavoriteKind::Song => &self.songs,
            FavoriteKind::Album => &self.albums,
            FavoriteKind::Artist => &self.artists,
        }
    }
    fn map_mut(&mut self, kind: FavoriteKind) -> &mut BTreeMap<String, String> {
        match kind {
            FavoriteKind::Song => &mut self.songs,
            FavoriteKind::Album => &mut self.albums,
            FavoriteKind::Artist => &mut self.artists,
        }
    }
    pub fn contains(&self, kind: FavoriteKind, id: &str) -> bool {
        self.map(kind).contains_key(id)
    }
    pub fn len(&self) -> usize {
        self.songs.len() + self.albums.len() + self.artists.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn entries(&self) -> impl Iterator<Item = (FavoriteKind, &str, &str)> {
        [
            FavoriteKind::Song,
            FavoriteKind::Album,
            FavoriteKind::Artist,
        ]
        .into_iter()
        .flat_map(|kind| {
            self.map(kind)
                .iter()
                .map(move |(id, name)| (kind, id.as_str(), name.as_str()))
        })
    }
    pub fn blocks(&self, song: &Song) -> bool {
        self.songs.contains_key(&song.id)
            || song
                .album_id
                .as_ref()
                .is_some_and(|id| self.albums.contains_key(id))
            || song
                .credits
                .ids()
                .chain(song.album_artists.iter().filter_map(|a| a.id.as_deref()))
                .any(|id| self.artists.contains_key(id))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Store(BTreeMap<String, Exclusions>);
impl Store {
    pub fn get(&self, scope: &str) -> Option<&Exclusions> {
        self.0.get(scope)
    }
    pub fn set(
        &mut self,
        scope: &str,
        kind: FavoriteKind,
        id: &str,
        name: &str,
        excluded: bool,
    ) -> Result<(), &'static str> {
        if !excluded {
            if let Some(rules) = self.0.get_mut(scope) {
                rules.map_mut(kind).remove(id);
                if rules.is_empty() {
                    self.0.remove(scope);
                }
            }
            return Ok(());
        }
        if !valid_scope(scope) || id.is_empty() || id.len() > 1024 {
            return Err(
                "This item cannot be excluded because its account or identifier is invalid.",
            );
        }
        if !self
            .0
            .get(scope)
            .is_some_and(|rules| rules.contains(kind, id))
            && (self.0.values().map(Exclusions::len).sum::<usize>() >= MAX_ENTRIES
                || (!self.0.contains_key(scope) && self.0.len() >= MAX_ACCOUNTS))
        {
            return Err(
                "The Autoplay exclusion list is full. Include an item in Settings before adding another.",
            );
        }
        let mut name = if name.trim().is_empty() { id } else { name }.to_owned();
        truncate(&mut name);
        self.0
            .entry(scope.to_owned())
            .or_default()
            .map_mut(kind)
            .insert(id.to_owned(), name);
        Ok(())
    }
    pub fn sanitize(&mut self) {
        let mut count = 0;
        let mut accounts = 0;
        self.0.retain(|scope, rules| {
            if !valid_scope(scope) || accounts >= MAX_ACCOUNTS {
                return false;
            }
            for kind in [
                FavoriteKind::Song,
                FavoriteKind::Album,
                FavoriteKind::Artist,
            ] {
                rules.map_mut(kind).retain(|id, name| {
                    if id.is_empty() || id.len() > 1024 || count >= MAX_ENTRIES {
                        return false;
                    }
                    count += 1;
                    if name.trim().is_empty() {
                        *name = id.clone();
                    }
                    truncate(name);
                    true
                });
            }
            accounts += usize::from(!rules.is_empty());
            !rules.is_empty()
        });
    }
}
fn valid_scope(scope: &str) -> bool {
    scope.len() == 64 && scope.bytes().all(|b| b.is_ascii_hexdigit())
}
fn truncate(value: &mut String) {
    let mut end = value.len().min(256);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::api::{ArtistCredit, ArtistCredits};
    #[test]
    fn rules_match_ids_across_credits_and_album_credits_without_matching_names() {
        let scope = "a".repeat(64);
        let mut store = Store::default();
        for (kind, id) in [
            (FavoriteKind::Song, "song"),
            (FavoriteKind::Album, "album"),
            (FavoriteKind::Artist, "artist"),
        ] {
            store.set(&scope, kind, id, "Shared name", true).unwrap();
        }
        let rules = store.get(&scope).unwrap();
        let base = Song {
            id: "other".into(),
            title: "Shared name".into(),
            artist: "Shared name".into(),
            ..Default::default()
        };
        assert!(!rules.blocks(&base));
        assert!(rules.blocks(&Song {
            id: "song".into(),
            ..base.clone()
        }));
        assert!(rules.blocks(&Song {
            album_id: Some("album".into()),
            ..base.clone()
        }));
        assert!(rules.blocks(&Song {
            credits: ArtistCredits {
                artist_id: Some("artist".into()),
                ..Default::default()
            },
            ..base.clone()
        }));
        let guest = ArtistCredit {
            id: Some("artist".into()),
            name: "Renamed artist".into(),
            ..Default::default()
        };
        assert!(rules.blocks(&Song {
            credits: ArtistCredits {
                artists: vec![
                    ArtistCredit {
                        id: Some("other".into()),
                        ..Default::default()
                    },
                    guest.clone()
                ],
                ..Default::default()
            },
            ..base.clone()
        }));
        assert!(rules.blocks(&Song {
            album_artists: vec![guest],
            ..base.clone()
        }));
        // Modern ID credits supersede legacy artistId, as elsewhere in the client.
        assert!(!rules.blocks(&Song {
            credits: ArtistCredits {
                artist_id: Some("artist".into()),
                artists: vec![ArtistCredit {
                    id: Some("other".into()),
                    ..Default::default()
                }]
            },
            ..base
        }));
    }
    #[test]
    fn persisted_rules_are_account_scoped_and_including_a_song_keeps_parent_rules() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let mut store = Store::default();
        store
            .set(&a, FavoriteKind::Song, "same/夜", "Night", true)
            .unwrap();
        store
            .set(&a, FavoriteKind::Album, "album", "Album", true)
            .unwrap();
        store
            .set(
                &b,
                FavoriteKind::Artist,
                "same/夜",
                "Different artist",
                true,
            )
            .unwrap();
        let mut restored: Store =
            serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        restored.sanitize();
        assert!(
            restored
                .get(&a)
                .unwrap()
                .contains(FavoriteKind::Song, "same/夜")
        );
        assert!(
            !restored
                .get(&b)
                .unwrap()
                .contains(FavoriteKind::Song, "same/夜")
        );
        restored
            .set(&a, FavoriteKind::Song, "same/夜", "", false)
            .unwrap();
        let song = Song {
            id: "same/夜".into(),
            album_id: Some("album".into()),
            ..Default::default()
        };
        assert!(restored.get(&a).unwrap().blocks(&song));
        assert!(!restored.get(&b).unwrap().blocks(&song));
        restored
            .set(&a, FavoriteKind::Album, "album", "", false)
            .unwrap();
        assert!(restored.get(&a).is_none());
        assert!(restored.get(&b).is_some());
    }
    #[test]
    fn storage_limits_preserve_existing_rules_and_truncate_labels_at_utf8_boundaries() {
        let scope = "a".repeat(64);
        let mut store = Store::default();
        for i in 0..MAX_ENTRIES {
            store
                .set(
                    &scope,
                    FavoriteKind::Song,
                    &i.to_string(),
                    &"夜".repeat(100),
                    true,
                )
                .unwrap();
        }
        assert!(
            store
                .set(&scope, FavoriteKind::Song, "extra", "Extra", true)
                .is_err()
        );
        assert_eq!(store.get(&scope).unwrap().len(), MAX_ENTRIES);
        assert!(
            store
                .get(&scope)
                .unwrap()
                .entries()
                .all(|(_, _, name)| name.len() <= 256)
        );
        store
            .set(&scope, FavoriteKind::Song, "0", "", false)
            .unwrap();
        store
            .set(&scope, FavoriteKind::Song, "extra", "Extra", true)
            .unwrap();
        assert!(
            store
                .set("invalid", FavoriteKind::Song, "x", "X", true)
                .is_err()
        );
        let old: crate::app::navidrome::Preferences = serde_json::from_str("{}").unwrap();
        assert!(old.autoplay_exclusions.get(&scope).is_none());
    }
}
