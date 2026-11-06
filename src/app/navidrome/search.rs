//! Search state shared with the native view. Each result index belongs to one
//! group so the arrival of the other response cannot move the user's selection.
use super::{App, Duration, Instant};
use crate::model::{Loadable, Page};
use crate::navidrome::api::{Catalogue, Collection, CollectionKind, NamedItem, SearchPart};
use crate::navidrome::backend::{Event, Request};

const DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Song(usize),
    Album(usize),
    Artist(usize),
    Playlist(usize),
}

#[derive(Clone, Debug)]
pub enum Action {
    Edit(String),
    Move(bool),
    Submit,
    Activate(Hit),
    Retry(SearchPart),
    PlayEntry(usize),
}

#[derive(Default)]
pub struct State {
    pub catalogue: Loadable<Catalogue>,
    pub playlists: Loadable<Vec<NamedItem>>,
    pub selected: Option<Hit>,
    pub recent: Vec<String>,
    pub collection: Loadable<Collection>,
    collection_request: u64,
}

impl State {
    pub fn rows(&self) -> impl DoubleEndedIterator<Item = (Hit, &str, &str)> {
        let catalogue = self.catalogue.get();
        let songs = catalogue.map_or(&[][..], |c| c.song.as_slice());
        let albums = catalogue.map_or(&[][..], |c| c.album.as_slice());
        let artists = catalogue.map_or(&[][..], |c| c.artist.as_slice());
        let playlists = self.playlists.get().map(Vec::as_slice).unwrap_or_default();
        songs
            .iter()
            .enumerate()
            .map(|(i, s)| (Hit::Song(i), s.title.as_str(), s.artist.as_str()))
            .chain(
                albums
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (Hit::Album(i), a.name.as_str(), a.artist.as_str())),
            )
            .chain(
                artists
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (Hit::Artist(i), a.name.as_str(), "")),
            )
            .chain(
                playlists
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (Hit::Playlist(i), p.name.as_str(), "")),
            )
    }

    fn hits(&self) -> impl DoubleEndedIterator<Item = Hit> {
        self.rows().map(|(hit, _, _)| hit)
    }

    fn move_selection(&mut self, down: bool) {
        let next = if let Some(selected) = self.selected {
            if down {
                self.hits().skip_while(|hit| *hit != selected).nth(1)
            } else {
                self.hits().rev().skip_while(|hit| *hit != selected).nth(1)
            }
        } else if down {
            self.hits().next()
        } else {
            self.hits().next_back()
        };
        self.selected = next.or(self.selected);
    }
}

impl App {
    pub(super) fn edit_search(&mut self, query: String, ctx: &egui::Context) {
        let key = query.trim().to_owned();
        let search = &mut self.presentation.search;
        search.query = query;
        if search.committed == key {
            return;
        }
        self.backend.send(Request::CancelSearch);
        search.serial = search.serial.wrapping_add(1);
        search.committed = key;
        search.typed_at = (!search.committed.is_empty()).then(Instant::now);
        let state = &mut self.presentation.navidrome_search;
        state.selected = None;
        state.catalogue = Loadable::NotLoaded;
        state.playlists = Loadable::NotLoaded;
        ctx.request_repaint();
    }

    pub(super) fn poll_search(&mut self, ctx: &egui::Context) {
        if let Some(typed) = self.presentation.search.typed_at {
            if typed.elapsed() >= DEBOUNCE {
                self.start_search();
            } else {
                ctx.request_repaint_after(DEBOUNCE.saturating_sub(typed.elapsed()));
            }
        }
    }

    fn start_search(&mut self) {
        self.presentation.search.typed_at = None;
        self.request_search_part(SearchPart::Catalogue);
        self.request_search_part(SearchPart::Playlists);
    }

    fn request_search_part(&mut self, part: SearchPart) {
        let search = &self.presentation.search;
        if search.committed.is_empty() {
            return;
        }
        let state = &mut self.presentation.navidrome_search;
        match part {
            SearchPart::Catalogue => state.catalogue = Loadable::Loading,
            SearchPart::Playlists => state.playlists = Loadable::Loading,
        }
        if let Some(client) = &self.client {
            self.backend.send(Request::Search {
                session: self.session,
                request: search.serial,
                client: client.clone(),
                query: search.committed.clone(),
                part,
            });
        }
        #[cfg(feature = "demo")]
        if self.demo && self.client.is_none() {
            self.demo_search_part(part);
        }
    }

    pub(super) fn search_event(&mut self, event: Event) {
        let serial = self.presentation.search.serial;
        let state = &mut self.presentation.navidrome_search;
        match event {
            Event::Catalogue {
                session,
                request,
                result,
            } if session == self.session && request == serial => {
                state.catalogue = Loadable::from_result(result)
            }
            Event::Playlists {
                session,
                request,
                result,
            } if session == self.session && request == serial => {
                state.playlists = Loadable::from_result(result)
            }
            Event::Collection {
                session,
                request,
                result,
            } if session == self.session && request == state.collection_request => {
                state.collection = Loadable::from_result(result)
            }
            _ => {}
        }
    }

    pub(super) fn search_action(&mut self, action: Action, ctx: &egui::Context) {
        ctx.request_repaint();
        match action {
            Action::Edit(query) => self.edit_search(query, ctx),
            Action::Move(down) => self.presentation.navidrome_search.move_selection(down),
            Action::Submit => {
                if let Some(hit) = self.presentation.navidrome_search.selected {
                    self.search_action(Action::Activate(hit), ctx);
                } else {
                    self.submit_query();
                }
            }
            Action::Retry(part) => self.request_search_part(part),
            Action::Activate(hit) => {
                self.submit_query();
                let state = &self.presentation.navidrome_search;
                let catalogue = state.catalogue.get();
                let page = match hit {
                    Hit::Song(i) => {
                        if let Some(c) = catalogue
                            && let Some(song) = c.song.get(i)
                        {
                            self.play_search_songs(vec![song.clone()], 0);
                        }
                        return;
                    }
                    Hit::Album(i) => catalogue
                        .and_then(|c| c.album.get(i))
                        .map(|a| Page::Album(super::presentation::encode(&a.id))),
                    Hit::Artist(i) => catalogue
                        .and_then(|c| c.artist.get(i))
                        .map(|a| Page::Artist(super::presentation::encode(&a.id))),
                    Hit::Playlist(i) => state
                        .playlists
                        .get()
                        .and_then(|p| p.get(i))
                        .map(|p| Page::Playlist(super::presentation::encode(&p.id))),
                };
                if let Some(page) = page {
                    self.ui_action(crate::model::Action::Open(page), ctx);
                }
            }
            Action::PlayEntry(index) => {
                if let Some(Collection::Playlist { songs, .. }) =
                    self.presentation.navidrome_search.collection.get()
                    && index < songs.len()
                {
                    self.play_search_songs(songs.clone(), index);
                }
            }
        }
    }

    fn play_search_songs(&mut self, songs: Vec<crate::navidrome::api::Song>, index: usize) {
        self.playing_album = None;
        self.transport.songs = songs;
        self.load(index, 0.0, false);
    }

    pub(super) fn submit_query(&mut self) {
        let query = self.presentation.search.committed.clone();
        if query.is_empty() {
            return;
        }
        let recent = &mut self.presentation.navidrome_search.recent;
        let folded = query.to_lowercase();
        recent.retain(|entry| entry.to_lowercase() != folded);
        recent.insert(0, query);
        recent.truncate(10);
        self.save_recent_searches();
        if self.presentation.search.typed_at.is_some() {
            self.start_search();
        }
    }

    pub(super) fn save_recent_searches(&mut self) {
        if let Some(client) = &self.client {
            self.preferences.search_history.insert(
                client.scope.clone(),
                self.presentation.navidrome_search.recent.clone(),
            );
            self.mark_dirty();
        }
    }

    pub(super) fn fetch_collection(&mut self, page: &Page) {
        let (kind, id) = match page {
            Page::Artist(id) => (CollectionKind::Artist, id),
            Page::Playlist(id) => (CollectionKind::Playlist, id),
            _ => return,
        };
        let Some(id) = super::presentation::decode(id) else {
            return;
        };
        let state = &mut self.presentation.navidrome_search;
        state.collection_request = state.collection_request.wrapping_add(1);
        state.collection = Loadable::Loading;
        #[cfg(feature = "demo")]
        if self.demo && self.client.is_none() {
            self.demo_collection(kind, &id);
            return;
        }
        if let Some(client) = &self.client {
            self.backend.send(Request::Collection {
                session: self.session,
                request: state.collection_request,
                client: client.clone(),
                kind,
                id,
            });
        }
    }
}

#[cfg(feature = "demo")]
impl App {
    fn demo_search_part(&mut self, part: SearchPart) {
        let query = self.presentation.search.committed.to_lowercase();
        let matches = |value: &str| value.to_lowercase().contains(&query);
        let state = &mut self.presentation.navidrome_search;
        match part {
            SearchPart::Catalogue => {
                let albums: Vec<_> = self
                    .albums
                    .iter()
                    .filter(|a| matches(&a.name) || matches(&a.artist))
                    .cloned()
                    .collect();
                let mut artists = Vec::new();
                for album in &self.albums {
                    if matches(&album.artist)
                        && !artists.iter().any(|a: &NamedItem| a.id == album.artist)
                    {
                        artists.push(NamedItem {
                            id: album.artist.clone(),
                            name: album.artist.clone(),
                        });
                    }
                }
                let songs = self
                    .albums
                    .first()
                    .into_iter()
                    .flat_map(|a| &a.song)
                    .filter(|s| matches(&s.title) || matches(&s.artist) || matches(&s.album))
                    .cloned()
                    .collect();
                state.catalogue = Loadable::Loaded(Catalogue {
                    song: songs,
                    artist: artists,
                    album: albums,
                });
            }
            SearchPart::Playlists => {
                let name = "Bonobo favorites";
                state.playlists = Loadable::Loaded(
                    matches(name)
                        .then(|| NamedItem {
                            id: "mix".into(),
                            name: name.into(),
                        })
                        .into_iter()
                        .collect(),
                );
            }
        }
    }

    fn demo_collection(&mut self, kind: CollectionKind, id: &str) {
        let collection = match kind {
            CollectionKind::Artist => Collection::Artist {
                name: id.into(),
                albums: self
                    .albums
                    .iter()
                    .filter(|a| a.artist == id)
                    .cloned()
                    .collect(),
            },
            CollectionKind::Playlist => Collection::Playlist {
                name: "Bonobo favorites".into(),
                songs: self
                    .albums
                    .first()
                    .into_iter()
                    .flat_map(|a| &a.song)
                    .filter(|s| s.artist == "Bonobo")
                    .cloned()
                    .collect(),
            },
        };
        self.presentation.navidrome_search.collection = Loadable::Loaded(collection);
    }

    pub(super) fn populate_demo_search(&mut self, flags: &[&str], ctx: &egui::Context) {
        self.presentation.history = vec![Page::Search];
        if flags.contains(&"recent") {
            self.presentation.navidrome_search.recent = vec!["Bonobo".into(), "Promises".into()];
            return;
        }
        self.edit_search(
            if flags.contains(&"empty") {
                "Distant shores"
            } else {
                "Bonobo"
            }
            .into(),
            ctx,
        );
        self.start_search();
        let state = &mut self.presentation.navidrome_search;
        if flags.contains(&"loading") {
            state.catalogue = Loadable::Loading;
            state.playlists = Loadable::Loading;
        } else if flags.contains(&"error") {
            state.playlists = Loadable::Failed("The server did not respond in time.".into());
        } else {
            let first = state.hits().next();
            state.selected = first;
        }
    }
}

#[cfg(test)]
mod tests;
