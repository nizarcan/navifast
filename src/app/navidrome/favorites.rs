//! Account-local favorites. Every surface reads the same membership set.
//! Keep one write in flight per entity, even when the user changes their mind.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use super::{App, encode, presentation::decode};
use crate::model::Page;
use crate::navidrome::api::{
    Album, Catalogue, Collection, FavoriteKind, NamedItem, RequestError, Song,
};
use crate::navidrome::backend::{Event, Request};

impl FavoriteKind {
    pub fn uri(self, id: &str) -> String {
        let kind = match self {
            Self::Song => "track",
            Self::Album => "album",
            Self::Artist => "artist",
        };
        format!("navidrome:{kind}:{}", encode(id))
    }
}

#[derive(Clone, Debug)]
pub enum Item {
    Song(Song),
    Album(Album),
    Artist(NamedItem),
}

impl Item {
    fn key(&self) -> (FavoriteKind, &str) {
        match self {
            Self::Song(s) => (FavoriteKind::Song, &s.id),
            Self::Album(a) => (FavoriteKind::Album, &a.id),
            Self::Artist(a) => (FavoriteKind::Artist, &a.id),
        }
    }

    fn uri(&self) -> String {
        let (kind, id) = self.key();
        kind.uri(id)
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Song(s) => &s.title,
            Self::Album(a) => &a.name,
            Self::Artist(a) => &a.name,
        }
    }
}

impl Catalogue {
    fn favorite(&self, kind: FavoriteKind, id: &str) -> Option<Item> {
        match kind {
            FavoriteKind::Song => self
                .song
                .iter()
                .find(|s| s.id == id)
                .cloned()
                .map(Item::Song),
            FavoriteKind::Album => self
                .album
                .iter()
                .find(|a| a.id == id)
                .cloned()
                .map(Item::Album),
            FavoriteKind::Artist => self
                .artist
                .iter()
                .find(|a| a.id == id)
                .cloned()
                .map(Item::Artist),
        }
    }
}

struct Edit {
    item: Item,
    confirmed: Option<bool>,
    desired: bool,
    sending: Option<bool>,
    // Only a read started after completion can retire this edit.
    after_request: u64,
}

pub struct Failure {
    pub item: Item,
    pub message: String,
    desired: bool,
}

#[derive(Default)]
pub struct State {
    pub list: Catalogue,
    members: HashSet<String>,
    edits: HashMap<String, Edit>,
    pub failures: BTreeMap<String, Failure>,
    pub ready: bool,
    pub loading: bool,
    pub error: Option<String>,
    request: u64,
    verify_at: Option<Instant>,
    backoff: u32,
}

impl State {
    pub fn contains(&self, uri: &str) -> Option<bool> {
        self.ready.then(|| self.members.contains(uri))
    }

    fn show(&mut self, uri: &str, item: &Item, starred: bool) {
        if starred == self.members.contains(uri) {
            return;
        }
        if starred {
            self.members.insert(uri.to_owned());
            match item {
                Item::Song(s) => self.list.song.insert(0, s.clone()),
                Item::Album(a) => self.list.album.insert(0, a.clone()),
                Item::Artist(a) => self.list.artist.insert(0, a.clone()),
            }
        } else {
            self.members.remove(uri);
            match item {
                Item::Song(s) => self.list.song.retain(|row| row.id != s.id),
                Item::Album(a) => self.list.album.retain(|row| row.id != a.id),
                Item::Artist(a) => self.list.artist.retain(|row| row.id != a.id),
            }
        }
    }

    fn set(&mut self, item: Item, desired: bool) -> bool {
        let uri = item.uri();
        let confirmed = Some(self.members.contains(&uri));
        self.failures.remove(&uri);
        self.show(&uri, &item, desired);
        let edit = self.edits.entry(uri).or_insert(Edit {
            item,
            confirmed,
            desired,
            sending: None,
            after_request: self.request,
        });
        edit.desired = desired;
        if edit.sending.is_some() {
            return false;
        }
        edit.sending = Some(desired);
        true
    }

    fn schedule_verify(&mut self) {
        self.verify_at = Some(Instant::now() + Duration::from_millis(350 << self.backoff.min(6)));
    }

    fn written(
        &mut self,
        uri: &str,
        sent: bool,
        result: Result<(), RequestError>,
    ) -> Option<(Item, bool)> {
        let edit = self.edits.get_mut(uri)?;
        if edit.sending != Some(sent) {
            return None;
        }
        edit.sending = None;
        edit.after_request = self.request;
        match result {
            Ok(()) => edit.confirmed = Some(sent),
            Err(error) => {
                if matches!(error, RequestError::Unconfirmed(_)) {
                    edit.confirmed = None;
                }
                if sent == edit.desired {
                    self.failures.insert(
                        uri.into(),
                        Failure {
                            item: edit.item.clone(),
                            desired: sent,
                            message: error.to_string(),
                        },
                    );
                    edit.desired = edit.confirmed.unwrap_or(sent);
                }
            }
        }
        if Some(edit.desired) != edit.confirmed && !self.failures.contains_key(uri) {
            edit.sending = Some(edit.desired);
            return Some((edit.item.clone(), edit.desired));
        }
        let item = edit.item.clone();
        let desired = edit.desired;
        self.show(uri, &item, desired);
        self.backoff = 0;
        self.schedule_verify();
        None
    }

    fn received(&mut self, request: u64, result: Result<Catalogue, String>) {
        if request != self.request {
            return;
        }
        self.loading = false;
        let list = match result {
            Ok(list) => list,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.ready = true;
        self.error = None;
        self.members = list
            .song
            .iter()
            .map(|s| FavoriteKind::Song.uri(&s.id))
            .chain(list.album.iter().map(|a| FavoriteKind::Album.uri(&a.id)))
            .chain(list.artist.iter().map(|a| FavoriteKind::Artist.uri(&a.id)))
            .collect();
        self.list = list;
        self.failures.retain(|uri, failure| {
            self.edits.contains_key(uri) || self.members.contains(uri) != failure.desired
        });
        for (uri, edit) in std::mem::take(&mut self.edits) {
            let observed = self.members.contains(&uri);
            if edit.sending.is_none() && request > edit.after_request {
                // A rejected write may concern an item another client already
                // changed or deleted. The first subsequent read resolves it.
                if let Some(failure) = self.failures.get(&uri) {
                    if observed == failure.desired {
                        self.failures.remove(&uri);
                    }
                    continue;
                }
                if observed == edit.desired {
                    continue;
                }
            }
            self.show(&uri, &edit.item, edit.desired);
            self.edits.insert(uri, edit);
        }
        if self.edits.values().any(|edit| edit.sending.is_none()) {
            self.backoff = self.backoff.saturating_add(1);
            self.schedule_verify();
        }
    }
}

impl App {
    // Resolve shared table, sidebar and player URIs against native metadata at click time.
    fn favorite_item(&self, uri: &str) -> Option<Item> {
        let (kind, encoded) = uri.strip_prefix("navidrome:")?.split_once(':')?;
        let id = decode(encoded).filter(|id| !id.is_empty())?;
        let kind = match kind {
            "track" => FavoriteKind::Song,
            "album" => FavoriteKind::Album,
            "artist" => FavoriteKind::Artist,
            _ => return None,
        };
        let search = &self.presentation.navidrome_search;
        let favorites = &self.presentation.navidrome_favorites;
        if let Some(item) = favorites
            .edits
            .get(uri)
            .map(|edit| edit.item.clone())
            .or_else(|| favorites.list.favorite(kind, &id))
            .or_else(|| search.catalogue.get()?.favorite(kind, &id))
        {
            return Some(item);
        }
        let collection = search.collection.get();
        match kind {
            FavoriteKind::Song => {
                let playlist = match collection {
                    Some(Collection::Playlist { songs, .. }) => songs.as_slice(),
                    _ => &[],
                };
                self.transport
                    .songs
                    .iter()
                    .chain(playlist)
                    .chain(self.album.iter().flat_map(|album| &album.song))
                    .find(|s| s.id == id)
                    .cloned()
                    .map(Item::Song)
            }
            FavoriteKind::Album => {
                let discography = match collection {
                    Some(Collection::Artist { albums, .. }) => albums.as_slice(),
                    _ => &[],
                };
                let mut album = self
                    .album
                    .iter()
                    .chain(&self.albums)
                    .chain(discography)
                    .chain(search.appearances.get().into_iter().flatten())
                    .find(|a| a.id == id)?
                    .clone();
                album.song.clear();
                Some(Item::Album(album))
            }
            FavoriteKind::Artist => {
                let name = match collection {
                    Some(Collection::Artist { name, .. })
                        if self.presentation.page() == &Page::Artist(encoded.into()) =>
                    {
                        name
                    }
                    _ => {
                        &search
                            .artist_info
                            .get()?
                            .similar_artist
                            .iter()
                            .find(|a| a.id.as_deref() == Some(&id))?
                            .name
                    }
                };
                Some(Item::Artist(NamedItem {
                    id,
                    name: name.clone(),
                }))
            }
        }
    }

    pub(super) fn toggle_favorite(&mut self, uri: &str) {
        if let Some(saved) = self.presentation.navidrome_favorites.contains(uri)
            && let Some(item) = self.favorite_item(uri)
        {
            self.set_favorite(item, !saved);
        }
    }

    pub(super) fn retry_favorite(&mut self, uri: &str) {
        if let Some(failure) = self.presentation.navidrome_favorites.failures.get(uri) {
            self.set_favorite(failure.item.clone(), failure.desired);
        }
    }

    fn set_favorite(&mut self, item: Item, desired: bool) {
        if self
            .presentation
            .navidrome_favorites
            .set(item.clone(), desired)
        {
            self.send_favorite(&item, desired);
        }
    }

    fn send_favorite(&mut self, item: &Item, starred: bool) {
        let (kind, id) = item.key();
        if let Some(client) = &self.client {
            self.backend.send(Request::SetFavorite {
                session: self.session,
                client: client.clone(),
                kind,
                id: id.to_owned(),
                starred,
            });
        } else if self.demo {
            self.favorite_event(Event::FavoriteWritten {
                session: self.session,
                kind,
                id: id.to_owned(),
                starred,
                result: Ok(()),
            });
        }
    }

    pub(super) fn refresh_favorites(&mut self) {
        let state = &mut self.presentation.navidrome_favorites;
        if state.loading {
            return;
        }
        state.verify_at = None;
        if let Some(client) = &self.client {
            state.request += 1;
            state.loading = true;
            state.error = None;
            self.backend.send(Request::Favorites {
                session: self.session,
                request: state.request,
                client: client.clone(),
            });
        }
    }

    pub(super) fn poll_favorites(&mut self, ctx: &egui::Context) {
        let state = &self.presentation.navidrome_favorites;
        if !state.loading
            && let Some(at) = state.verify_at
        {
            if at <= Instant::now() {
                self.refresh_favorites();
            } else {
                ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
            }
        }
    }

    pub(super) fn favorite_event(&mut self, event: Event) {
        let state = &mut self.presentation.navidrome_favorites;
        match event {
            Event::Favorites {
                session,
                request,
                result,
            } if session == self.session => state.received(request, result),
            Event::FavoriteWritten {
                session,
                kind,
                id,
                starred,
                result,
            } if session == self.session => {
                if let Some((item, desired)) = state.written(&kind.uri(&id), starred, result) {
                    self.send_favorite(&item, desired);
                }
            }
            _ => {}
        }
    }
}

#[cfg(feature = "demo")]
impl App {
    pub(super) fn populate_demo_favorites(&mut self, page: Option<&str>, flags: &[&str]) {
        let mut list = Catalogue::default();
        let album = &self.albums[0];
        let state = &mut self.presentation.navidrome_favorites;
        if page == Some("favorites") {
            if flags.contains(&"loading") {
                state.loading = true;
                return;
            }
            if flags.contains(&"error") {
                state.error = Some("The server did not respond in time.".into());
                return;
            }
        }
        if page != Some("favorites") || !flags.contains(&"empty") {
            list.song.extend(album.song.iter().take(2).cloned());
            let mut sleeve = album.clone();
            sleeve.song.clear();
            list.album.push(sleeve);
            if let Some(id) = &album.credits.artist_id {
                list.artist.push(NamedItem {
                    id: id.clone(),
                    name: album.artist.clone(),
                });
            }
        }
        state.received(0, Ok(list));
        if flags.contains(&"write-error") {
            let item = Item::Song(album.song[2].clone());
            state.failures.insert(
                item.uri(),
                Failure {
                    item,
                    desired: true,
                    message: "Your account does not have permission for this operation.".into(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests;
