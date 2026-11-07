//! Small, account-scoped metadata snapshots for the local Home history.
use super::App;
use crate::model::{Loadable, Page};
use crate::navidrome::api::{Album, ArtistCredits, Collection};
use serde::{Deserialize, Serialize};

const LIMIT: usize = 24;
const FIELD_LIMIT: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Album,
    Playlist,
    Artist,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: Kind,
    pub id: String,
    pub name: String,
    pub by: String,
    pub cover: Option<String>,
}

impl Entry {
    pub fn page(&self) -> Page {
        let id = super::encode(&self.id);
        match self.kind {
            Kind::Album => Page::Album(id),
            Kind::Playlist => Page::Playlist(id),
            Kind::Artist => Page::Artist(id),
        }
    }

    fn valid(&self) -> bool {
        !self.id.is_empty()
            && !self.name.trim().is_empty()
            && [&self.id, &self.name, &self.by]
                .into_iter()
                .chain(self.cover.as_ref())
                .all(|text| text.len() <= FIELD_LIMIT)
    }

    fn new(kind: Kind, id: &str, name: &str, by: &str, cover: Option<&str>) -> Option<Self> {
        if id.is_empty()
            || name.trim().is_empty()
            || [id, name, by]
                .into_iter()
                .chain(cover)
                .any(|s| s.len() > FIELD_LIMIT)
        {
            return None;
        }
        Some(Self {
            kind,
            id: id.into(),
            name: name.into(),
            by: by.into(),
            cover: cover.map(str::to_owned),
        })
    }

    fn album(album: &Album) -> Option<Self> {
        Self::new(
            Kind::Album,
            &album.id,
            &album.name,
            &album.artist,
            album.cover_art.as_deref(),
        )
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub collections: Vec<Entry>,
    pub artists: Vec<Entry>,
}

impl History {
    pub fn sanitize(&mut self) {
        for (entries, artists) in [(&mut self.collections, false), (&mut self.artists, true)] {
            let mut seen = std::collections::HashSet::new();
            entries.retain(|entry| {
                seen.len() < LIMIT
                    && entry.valid()
                    && (entry.kind == Kind::Artist) == artists
                    && seen.insert((entry.kind as u8, entry.id.clone()))
            });
            entries.truncate(LIMIT);
            entries.shrink_to_fit();
        }
    }

    fn push(&mut self, mut entry: Entry) -> bool {
        if !entry.valid() {
            return false;
        }
        let entries = if entry.kind == Kind::Artist {
            &mut self.artists
        } else {
            &mut self.collections
        };
        // Sparse playback credits must not erase a previously resolved portrait.
        if entry.cover.is_none() {
            entry.cover = entries
                .iter()
                .find(|old| old.kind == entry.kind && old.id == entry.id)
                .and_then(|old| old.cover.clone());
        }
        if entries.first() == Some(&entry) {
            return false;
        }
        entries.retain(|old| old.kind != entry.kind || old.id != entry.id);
        entries.insert(0, entry);
        entries.truncate(LIMIT);
        true
    }
}

impl App {
    pub(super) fn request_home_artist_cover(&mut self, id: &str) {
        let artists = &self.preferences.home_history.artists;
        if !artists
            .iter()
            .any(|entry| entry.id == id && entry.cover.is_none())
        {
            return;
        }
        let Some(client) = &self.client else { return };
        // History is capped at 24 artists. Retain no session-long catalogue cache.
        self.home_artist_covers
            .retain(|id| artists.iter().any(|entry| entry.id == *id));
        if self.home_artist_covers.insert(id.to_owned()) {
            self.backend
                .send(crate::navidrome::backend::Request::ArtistCover {
                    session: self.session,
                    client: client.clone(),
                    id: id.to_owned(),
                });
        }
    }

    pub(super) fn home_artist_cover(
        &mut self,
        session: u64,
        id: &str,
        result: Result<Option<String>, String>,
    ) {
        if session != self.session || !self.home_artist_covers.contains(id) {
            return;
        }
        match result {
            Ok(Some(cover)) if !cover.trim().is_empty() && cover.len() <= FIELD_LIMIT => {
                if let Some(entry) = self
                    .preferences
                    .home_history
                    .artists
                    .iter_mut()
                    .find(|entry| entry.id == id && entry.cover.is_none())
                {
                    entry.cover = Some(cover);
                    self.mark_dirty();
                }
            }
            // A later visit can retry a failed lookup. Missing artwork stays a
            // placeholder without sending the same request on every frame.
            Err(_) => {
                self.home_artist_covers.remove(id);
            }
            _ => {}
        }
    }

    pub(super) fn remember_home_album(&mut self, album: &Album) {
        if let Some(entry) = Entry::album(album)
            && self.preferences.home_history.push(entry)
        {
            self.mark_dirty();
        }
    }

    /// Use already loaded metadata to migrate older album-only history. No extra requests.
    pub(super) fn migrate_home_history(&mut self) {
        if !self.preferences.home_history.collections.is_empty() {
            return;
        }
        let entries: Vec<_> = self
            .presentation
            .recent_contexts
            .iter()
            .filter_map(|uri| {
                let id = super::decode(uri.strip_prefix("navidrome:album:")?)?;
                self.albums
                    .iter()
                    .find(|album| album.id == id)
                    .and_then(Entry::album)
            })
            .take(LIMIT)
            .collect();
        if !entries.is_empty() {
            self.preferences.home_history.collections = entries;
            self.preferences.home_history.sanitize();
            self.mark_dirty();
        }
    }

    pub(super) fn remember_home_playlist(&mut self) {
        let (
            Some(id),
            Loadable::Loaded(Collection::Playlist {
                name, cover_art, ..
            }),
        ) = (
            self.playing_playlist.as_deref().and_then(super::decode),
            &self.presentation.navidrome_search.collection,
        )
        else {
            return;
        };
        if let Some(entry) = Entry::new(Kind::Playlist, &id, name, "", cover_art.as_deref())
            && self.preferences.home_history.push(entry)
        {
            self.mark_dirty();
        }
    }

    /// Called once on preparing a song, never by the frame or position timer.
    pub(super) fn remember_home_artists(&mut self) {
        let Some(song) = self.transport.song() else {
            return;
        };
        let (name, credits) = if song.credits.has_ids() {
            (&song.artist, &song.credits)
        } else if let Some(album) = self
            .playing_album
            .as_ref()
            .filter(|_| self.transport.queue.context_index().is_some())
        {
            (&album.artist, &album.credits)
        } else {
            return;
        };
        let entries = artist_entries(name, credits);
        if self.preferences.home_history.artists.starts_with(&entries) {
            return;
        }
        let mut changed = false;
        for entry in entries.into_iter().rev() {
            changed |= self.preferences.home_history.push(entry);
        }
        if changed {
            self.mark_dirty();
        }
    }
}

fn artist_entries(name: &str, credits: &ArtistCredits) -> Vec<Entry> {
    let mut entries: Vec<_> = credits
        .artists
        .iter()
        .filter_map(|artist| {
            Entry::new(
                Kind::Artist,
                artist.id.as_deref()?,
                &artist.name,
                "",
                artist.cover_art.as_deref(),
            )
        })
        .take(LIMIT)
        .collect();
    if entries.is_empty()
        && let Some(id) = &credits.artist_id
    {
        entries.extend(Entry::new(Kind::Artist, id, name, "", None));
    }
    entries
}

#[cfg(feature = "navidrome-demo")]
impl App {
    pub(super) fn populate_demo_home(&mut self, flags: &[&str]) {
        self.presentation.history = vec![Page::Home];
        self.populate_demo_playlists(&[]);
        let mut history = History::default();
        if !flags.contains(&"first-visit")
            && !flags.contains(&"empty")
            && !flags.contains(&"loading")
        {
            for album in self.albums.iter().take(4).rev() {
                if let Some(entry) = Entry::album(album) {
                    history.push(entry);
                }
                history.push(Entry {
                    kind: Kind::Artist,
                    id: format!("artist-{}", album.id),
                    name: album.artist.clone(),
                    by: String::new(),
                    cover: album.cover_art.clone(),
                });
            }
            for item in self
                .presentation
                .navidrome_playlists
                .list
                .get()
                .into_iter()
                .flatten()
                .take(2)
            {
                history.push(Entry {
                    kind: Kind::Playlist,
                    id: item.id.clone(),
                    name: item.name.clone(),
                    by: String::new(),
                    cover: item.cover_art.clone(),
                });
            }
        }
        self.preferences.home_history = history;
        if flags.contains(&"empty") || flags.contains(&"loading") {
            self.populate_demo_playlists(flags);
        } else if flags.contains(&"error") {
            self.presentation.navidrome_playlists.error =
                Some("The server did not respond in time.".into());
        }
        if flags.contains(&"no-art") {
            for entry in self
                .preferences
                .home_history
                .collections
                .iter_mut()
                .chain(&mut self.preferences.home_history.artists)
            {
                entry.cover = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::tests::with_app;
    use crate::navidrome::api::ArtistCredit;

    #[test]
    fn history_is_bounded_deduplicated_and_kind_specific() {
        let mut history = History::default();
        for id in 0..100 {
            history.push(Entry {
                kind: Kind::Album,
                id: id.to_string(),
                name: format!("Album {id}"),
                by: String::new(),
                cover: None,
            });
        }
        assert_eq!(history.collections.len(), LIMIT);
        let entry = history.collections[4].clone();
        assert!(history.push(entry.clone()));
        assert!(!history.push(entry.clone()));
        assert_eq!(history.collections[0], entry);
        assert!(history.push(Entry {
            kind: Kind::Playlist,
            ..entry.clone()
        }));
        assert!(!history.push(Entry {
            name: "x".repeat(FIELD_LIMIT + 1),
            ..entry
        }));
        assert_eq!(history.collections[1].kind, Kind::Album);
        let mut restored: History =
            serde_json::from_str(&serde_json::to_string(&history).unwrap()).unwrap();
        restored.artists = restored.collections.clone();
        restored.sanitize();
        assert!(restored.artists.is_empty());
        assert_eq!(restored.collections, history.collections);
    }

    #[test]
    fn playback_records_real_credits_and_account_changes_clear_history() {
        with_app(|app, _| {
            let album = app.album.as_mut().unwrap();
            album.song[0].credits.artists = vec![ArtistCredit {
                id: Some("artist:/ع".into()),
                name: "فنان".into(),
                cover_art: Some("portrait".into()),
            }];
            app.apply(super::super::Action::Play(0));
            assert_eq!(
                app.preferences.home_history.collections[0].name,
                "Original album"
            );
            let artist = &app.preferences.home_history.artists[0];
            assert_eq!(
                artist.page(),
                Page::Artist(super::super::encode("artist:/ع"))
            );
            assert_eq!(artist.cover.as_deref(), Some("portrait"));
            app.remember_account("https://another.example", "listener");
            assert!(app.preferences.home_history.collections.is_empty());
            assert!(app.preferences.home_history.artists.is_empty());
        });
    }

    #[test]
    fn resolved_portraits_survive_sparse_credits_and_restarts() {
        with_app(|app, _| {
            let entry = Entry::new(Kind::Artist, "a", "Artist", "", None).unwrap();
            app.preferences.home_history.push(entry.clone());
            app.home_artist_covers.insert(entry.id.clone());
            app.home_artist_cover(app.session + 1, "a", Ok(Some("wrong-account".into())));
            assert!(app.preferences.home_history.artists[0].cover.is_none());
            app.home_artist_cover(app.session, "a", Ok(Some("portrait".into())));
            assert!(!app.preferences.home_history.push(entry.clone()));
            // Late metadata must not replace a newer cover or change history order.
            app.preferences
                .home_history
                .push(Entry::new(Kind::Artist, "b", "Other", "", None).unwrap());
            app.home_artist_cover(app.session, "a", Ok(Some("stale".into())));
            let json = serde_json::to_string(&app.preferences.home_history).unwrap();
            let mut restored: History = serde_json::from_str(&json).unwrap();
            restored.sanitize();
            assert_eq!(restored.artists[0].id, "b");
            assert_eq!(restored.artists[1].cover.as_deref(), Some("portrait"));
            restored.push(entry);
            assert_eq!(restored.artists[0].cover.as_deref(), Some("portrait"));
            app.preferences.home_history.artists.clear();
            app.home_artist_cover(app.session, "a", Ok(Some("late".into())));
            assert!(app.preferences.home_history.artists.is_empty());
        });
    }

    #[test]
    fn failed_portraits_retry_on_a_later_visit_but_absent_artwork_does_not() {
        with_app(|app, _| {
            app.preferences
                .home_history
                .push(Entry::new(Kind::Artist, "a", "Artist", "", None).unwrap());
            app.request_home_artist_cover("a");
            assert!(
                app.home_artist_covers.is_empty(),
                "no request before sign-in"
            );
            app.home_artist_covers.insert("a".into());
            app.home_artist_cover(app.session, "a", Ok(None));
            assert!(app.home_artist_covers.contains("a"));
            app.home_artist_cover(app.session, "a", Err("temporarily unavailable".into()));
            assert!(!app.home_artist_covers.contains("a"));
            app.home_artist_covers.insert("a".into());
            app.home_artist_cover(app.session, "a", Ok(Some("x".repeat(FIELD_LIMIT + 1))));
            assert!(app.preferences.home_history.artists[0].cover.is_none());
        });
    }
}
