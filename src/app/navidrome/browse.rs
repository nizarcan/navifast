//! Demand paging with independent query caches and session/request guards.
use super::{App, Request};
use crate::{
    model::Page,
    navidrome::{
        api::{Album, PAGE_SIZE, Song},
        browse::{Genre, Items, Query},
    },
};
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Debug)]
pub enum Action {
    More,
    Retry,
    Refresh,
}

pub struct Listing {
    pub items: Items,
    pub loaded: bool,
    pub loading: bool,
    pub more: bool,
    pub error: Option<String>,
    pub revision: u64,
    offset: usize,
    pending: Option<usize>,
    retry_offset: Option<usize>,
    seen: HashSet<String>,
}

impl Listing {
    fn new(query: &Query) -> Self {
        Self {
            items: match query {
                Query::Songs | Query::GenreSongs(_) => Items::Songs(Vec::new()),
                Query::GenreAlbums(_) | Query::Albums(_) => Items::Albums(Vec::new()),
                Query::Genres => Items::Genres(Vec::new()),
            },
            loaded: false,
            loading: false,
            more: true,
            error: None,
            revision: 0,
            offset: 0,
            pending: None,
            retry_offset: None,
            seen: HashSet::new(),
        }
    }
    pub fn songs(&self) -> &[Song] {
        if let Items::Songs(items) = &self.items {
            items
        } else {
            &[]
        }
    }
    pub fn albums(&self) -> &[Album] {
        if let Items::Albums(items) = &self.items {
            items
        } else {
            &[]
        }
    }
    pub fn genres(&self) -> &[Genre] {
        if let Items::Genres(items) = &self.items {
            items
        } else {
            &[]
        }
    }
}

#[derive(Default)]
pub struct State {
    pub active: Option<Query>,
    pub playing: Option<Page>,
    cache: VecDeque<(Query, Listing)>,
    serial: u64,
}

impl State {
    pub fn query(page: &Page) -> Option<Query> {
        match page {
            Page::AlbumList(order) => Some(Query::Albums(*order)),
            Page::Songs => Some(Query::Songs),
            Page::Genres => Some(Query::Genres),
            Page::Genre(name) => Some(Query::GenreSongs(name.clone())),
            Page::GenreAlbums(name) => Some(Query::GenreAlbums(name.clone())),
            _ => None,
        }
    }
    pub fn current(&self) -> Option<&Listing> {
        let active = self.active.as_ref()?;
        self.cache
            .iter()
            .find(|(key, _)| key == active)
            .map(|(_, list)| list)
    }
    fn current_mut(&mut self) -> Option<&mut Listing> {
        let active = self.active.as_ref()?;
        self.cache
            .iter_mut()
            .find(|(key, _)| key == active)
            .map(|(_, list)| list)
    }
    pub fn cached(&self, page: &Page) -> bool {
        Self::query(page).is_some_and(|query| self.cache.iter().any(|(key, _)| *key == query))
    }
    fn trim(&mut self) {
        // Keep the current listing regardless of size. Inactive pages share a
        // 20k-item allowance and at most eight query slots for back navigation.
        // The random sample is at most one page and stays fixed until refresh.
        while !self.cache.is_empty()
            && (self.cache.len() > 8
                || self
                    .cache
                    .iter()
                    .filter(|(q, _)| Some(q) != self.active.as_ref())
                    .map(|(_, l)| l.items.len())
                    .sum::<usize>()
                    > 20_000)
        {
            let index = self
                .cache
                .iter()
                .position(|(q, _)| {
                    Some(q) != self.active.as_ref()
                        && *q != Query::Albums(crate::navidrome::albums::Order::Random)
                })
                .unwrap();
            self.cache.remove(index);
        }
    }
}

impl App {
    pub(super) fn sync_browse(&mut self) {
        let query = State::query(self.presentation.page());
        if query == self.browse.active {
            return;
        }
        self.backend.send(Request::CancelBrowse);
        self.browse.serial = self.browse.serial.wrapping_add(1);
        if let Some(list) = self.browse.current_mut() {
            list.loading = false;
            list.pending = None;
        }
        self.browse.active = query.clone();
        let Some(query) = query else {
            self.browse.trim();
            return;
        };
        if let Some(index) = self.browse.cache.iter().position(|(key, _)| *key == query) {
            let entry = self.browse.cache.remove(index).unwrap();
            self.browse.cache.push_back(entry);
        } else {
            self.browse
                .cache
                .push_back((query.clone(), Listing::new(&query)));
        }
        self.browse.trim();
        if self
            .browse
            .current()
            .is_some_and(|list| !list.loaded && list.error.is_none())
        {
            self.browse_action(Action::More);
        }
    }

    pub(crate) fn browse_action(&mut self, action: Action) {
        let Some(list) = self.browse.current_mut() else {
            return;
        };
        if list.loading {
            return;
        }
        if matches!(action, Action::More) && (!list.more || list.error.is_some()) {
            return;
        }
        let offset = match action {
            Action::Refresh => 0,
            Action::Retry => list.retry_offset.unwrap_or(list.offset),
            Action::More => list.offset,
        };
        list.loading = true;
        list.error = None;
        list.pending = Some(offset);
        self.browse.serial = self.browse.serial.wrapping_add(1);
        let query = self.browse.active.clone().unwrap();
        if let Some(client) = &self.client {
            self.backend.send(Request::Browse {
                session: self.session,
                request: self.browse.serial,
                client: client.clone(),
                query,
                offset,
            });
        }
        #[cfg(feature = "navidrome-demo")]
        if self.demo && self.client.is_none() {
            self.demo_browse_page(offset);
        }
    }

    pub(super) fn browse_result(
        &mut self,
        session: u64,
        request: u64,
        query: Query,
        offset: usize,
        result: Result<Items, String>,
    ) {
        if session != self.session
            || request != self.browse.serial
            || self.browse.active.as_ref() != Some(&query)
        {
            return;
        }
        let Some(list) = self
            .browse
            .current_mut()
            .filter(|l| l.pending == Some(offset))
        else {
            return;
        };
        list.loading = false;
        list.pending = None;
        let items = match result {
            Ok(items) => items,
            Err(error) => {
                list.error = Some(error);
                list.retry_offset = Some(offset);
                return;
            }
        };
        if std::mem::discriminant(&items) != std::mem::discriminant(&list.items) {
            list.error = Some("The server returned the wrong kind of library results.".into());
            list.retry_offset = Some(offset);
            return;
        }
        list.retry_offset = None;
        let length = items.len();
        if offset == 0 {
            list.seen.clear();
        }
        let previous = list.seen.len();
        macro_rules! append {
            ($target:expr,$items:expr,$key:ident) => {{
                if offset == 0 {
                    $target.clear();
                }
                $target.extend(
                    $items.into_iter().filter(|item| {
                        !item.$key.is_empty() && list.seen.insert(item.$key.clone())
                    }),
                );
            }};
        }
        match (&mut list.items, items) {
            (Items::Songs(target), Items::Songs(items)) => append!(target, items, id),
            (Items::Albums(target), Items::Albums(items)) => append!(target, items, id),
            (Items::Genres(target), Items::Genres(items)) => append!(target, items, value),
            _ => {
                list.error = Some("The server returned the wrong kind of library results.".into());
                return;
            }
        }
        list.loaded = true;
        list.revision = list.revision.wrapping_add(1);
        if offset > 0 && length > 0 && list.seen.len() == previous {
            list.error = Some("The server repeated a page. Refresh this list to try again.".into());
            list.retry_offset = Some(offset);
        } else {
            list.offset = offset.saturating_add(length);
            list.more = query != Query::Genres
                && query != Query::Albums(crate::navidrome::albums::Order::Random)
                && length == PAGE_SIZE;
        }
        self.browse.trim();
    }
}

#[cfg(feature = "navidrome-demo")]
impl App {
    pub(super) fn configure_demo_browse(&mut self, flags: &[&str]) {
        let album_list = matches!(self.browse.active, Some(Query::Albums(_)));
        if flags.contains(&"browse-end") && !flags.contains(&"error") {
            for _ in 0..2 {
                self.browse_action(Action::More);
            }
        }
        let Some(list) = self.browse.current_mut() else {
            return;
        };
        if flags.contains(&"empty") || flags.contains(&"loading") {
            list.items = match &list.items {
                Items::Songs(_) => Items::Songs(Vec::new()),
                Items::Albums(_) => Items::Albums(Vec::new()),
                Items::Genres(_) => Items::Genres(Vec::new()),
            };
            list.more = false;
            list.revision += 1;
        }
        if flags.contains(&"loading") {
            list.loading = true;
        }
        if flags.contains(&"error") {
            list.error = Some("The server did not respond in time. Try again.".into());
            list.retry_offset = Some(if album_list { 0 } else { list.offset });
        }
    }
    fn demo_browse_page(&mut self, offset: usize) {
        let query = self.browse.active.clone().unwrap();
        let items = match &query {
            Query::Albums(order) => {
                let mut albums: Vec<_> = self
                    .albums
                    .iter()
                    .filter(|album| match order {
                        crate::navidrome::albums::Order::Year { from, to } => {
                            (u32::from(*from)..=u32::from(*to)).contains(&album.year)
                        }
                        _ => true,
                    })
                    .cloned()
                    .collect();
                match order {
                    crate::navidrome::albums::Order::Random => {
                        use rand::{SeedableRng, seq::SliceRandom};
                        albums.shuffle(&mut rand::rngs::StdRng::seed_from_u64(self.browse.serial));
                    }
                    crate::navidrome::albums::Order::Recent
                    | crate::navidrome::albums::Order::Frequent => albums.reverse(),
                    _ => {}
                }
                Items::Albums(albums.into_iter().skip(offset).take(PAGE_SIZE).collect())
            }
            Query::Genres => Items::Genres(
                [
                    "Electronic",
                    "Jazz",
                    "R&B / موسيقى + 夜",
                    "Compilation only",
                    "Empty genre",
                ]
                .into_iter()
                .enumerate()
                .map(|(i, value)| Genre {
                    value: value.into(),
                    song_count: if i == 4 { 0 } else { 135 },
                    album_count: if i >= 3 { 0 } else { 12 },
                })
                .collect(),
            ),
            Query::GenreAlbums(name) => {
                Items::Albums(if name == "Compilation only" || name == "Empty genre" {
                    Vec::new()
                } else {
                    self.albums
                        .iter()
                        .skip(offset)
                        .take(PAGE_SIZE)
                        .cloned()
                        .collect()
                })
            }
            Query::Songs | Query::GenreSongs(_) => {
                let source = self
                    .albums
                    .first()
                    .map_or(&[][..], |album| album.song.as_slice());
                let total = if source.is_empty() || query == Query::GenreSongs("Empty genre".into())
                {
                    0
                } else {
                    135
                };
                Items::Songs(
                    (offset..(offset + PAGE_SIZE).min(total))
                        .map(|i| {
                            let mut song = source[i % source.len()].clone();
                            song.album_id = Some(self.albums[0].id.clone());
                            song.id = format!("browse-{i}");
                            song.title = format!("{} · {}", song.title, i + 1);
                            song
                        })
                        .collect(),
                )
            }
        };
        self.browse_result(self.session, self.browse.serial, query, offset, Ok(items));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::tests::with_app;

    fn open(app: &mut App, page: Page) {
        app.demo = false;
        app.presentation.open(page);
        app.sync_browse();
    }
    fn songs(start: usize, count: usize) -> Items {
        Items::Songs(
            (start..start + count)
                .map(|i| Song {
                    id: format!("s{i}"),
                    title: format!("Song {i}"),
                    ..Default::default()
                })
                .collect(),
        )
    }
    fn finish(app: &mut App, result: Result<Items, String>) {
        let offset = app.browse.current().unwrap().pending.unwrap();
        app.browse_result(
            app.session,
            app.browse.serial,
            app.browse.active.clone().unwrap(),
            offset,
            result,
        );
    }

    #[test]
    fn random_albums_stay_cached_and_only_refresh_replaces_the_selection() {
        use crate::navidrome::albums::Order;
        with_app(|app, _| {
            let page = Page::AlbumList(Order::Random);
            open(app, page.clone());
            let albums = || {
                Items::Albums(
                    (0..PAGE_SIZE)
                        .map(|i| Album {
                            id: format!("a{i}"),
                            ..Default::default()
                        })
                        .collect(),
                )
            };
            finish(app, Ok(albums()));
            assert!(!app.browse.current().unwrap().more);
            let serial = app.browse.serial;
            app.browse_action(Action::More);
            assert_eq!(app.browse.serial, serial);
            // Browsing enough other views to evict ordinary caches keeps this
            // small random sample for the full account session.
            for i in 0..12 {
                open(app, Page::Genre(format!("genre-{i}")));
                finish(app, Ok(songs(i, 1)));
            }
            open(app, page.clone());
            assert_eq!(app.browse.current().unwrap().albums()[0].id, "a0");
            assert!(!app.browse.current().unwrap().loading);
            app.browse_action(Action::Refresh);
            finish(app, Err("offline".into()));
            assert_eq!(app.browse.current().unwrap().albums().len(), PAGE_SIZE);
            app.browse_action(Action::Retry);
            assert_eq!(app.browse.current().unwrap().pending, Some(0));
            let stale = app.browse.serial;
            open(app, Page::Home);
            app.browse_result(
                app.session,
                stale,
                Query::Albums(Order::Random),
                0,
                Ok(Items::Albums(Vec::new())),
            );
            open(app, page);
            assert_eq!(app.browse.current().unwrap().albums().len(), PAGE_SIZE);
            app.browse_action(Action::Refresh);
            finish(
                app,
                Ok(Items::Albums(vec![Album {
                    id: "new".into(),
                    ..Default::default()
                }])),
            );
            assert_eq!(app.browse.current().unwrap().albums()[0].id, "new");
        });
    }

    #[test]
    fn pages_keep_order_and_loaded_rows_survive_failure_and_refresh_retry() {
        with_app(|app, _| {
            open(app, Page::Songs);
            finish(app, Ok(songs(0, PAGE_SIZE)));
            app.browse_action(Action::More);
            finish(app, Err("offline".into()));
            assert_eq!(app.browse.current().unwrap().songs().len(), PAGE_SIZE);
            app.browse_action(Action::More);
            assert!(!app.browse.current().unwrap().loading);
            app.browse_action(Action::Retry);
            assert_eq!(app.browse.current().unwrap().pending, Some(PAGE_SIZE));
            finish(app, Ok(songs(PAGE_SIZE - 1, 3)));
            let list = app.browse.current().unwrap();
            assert!(!list.more);
            assert_eq!(list.songs().len(), PAGE_SIZE + 2);
            for (i, song) in list.songs().iter().enumerate() {
                assert_eq!(song.id, format!("s{i}"));
            }
            app.browse_action(Action::Refresh);
            finish(app, Err("offline".into()));
            assert_eq!(app.browse.current().unwrap().songs().len(), PAGE_SIZE + 2);
            app.browse_action(Action::Retry);
            assert_eq!(app.browse.current().unwrap().pending, Some(0));
            finish(app, Ok(songs(100, 1)));
            assert_eq!(app.browse.current().unwrap().songs()[0].id, "s100");
        });
    }

    #[test]
    fn stale_genre_pages_sessions_and_cancelled_requests_cannot_mix() {
        with_app(|app, _| {
            let genre = "  R&B / موسيقى + 夜: live  ";
            let page = Page::Genre(genre.into());
            assert_eq!(Page::decode(&page.encode()), Some(page.clone()));
            open(app, page.clone());
            finish(app, Ok(songs(0, PAGE_SIZE)));
            app.browse_action(Action::More);
            let stale = app.browse.serial;
            open(app, Page::Genre("Jazz".into()));
            app.browse_result(
                app.session,
                stale,
                Query::GenreSongs(genre.into()),
                PAGE_SIZE,
                Ok(songs(60, 4)),
            );
            assert!(app.browse.current().unwrap().songs().is_empty());
            app.browse_result(
                app.session + 1,
                app.browse.serial,
                Query::GenreSongs("Jazz".into()),
                0,
                Ok(songs(500, 1)),
            );
            assert!(app.browse.current().unwrap().loading);
            finish(app, Ok(songs(200, 1)));
            open(app, page);
            assert_eq!(app.browse.current().unwrap().songs().len(), PAGE_SIZE);
            assert!(!app.browse.current().unwrap().loading);
            app.browse_action(Action::More);
            assert_eq!(app.browse.current().unwrap().pending, Some(PAGE_SIZE));
        });
    }

    #[test]
    fn repeated_pages_stop_paging_and_query_cache_is_bounded() {
        with_app(|app, _| {
            open(app, Page::Songs);
            finish(app, Ok(songs(0, PAGE_SIZE)));
            app.browse_action(Action::More);
            finish(app, Ok(songs(0, PAGE_SIZE)));
            assert!(app.browse.current().unwrap().error.is_some());
            app.browse_action(Action::More);
            assert!(!app.browse.current().unwrap().loading);
            for i in 0..10 {
                open(app, Page::Genre(format!("genre-{i}")));
                finish(app, Ok(songs(i, 1)));
            }
            assert_eq!(app.browse.cache.len(), 8);
            assert!(!app.browse.cached(&Page::Songs));
            assert!(app.browse.cached(&Page::Genre("genre-9".into())));
            app.browse.current_mut().unwrap().items = songs(0, 20_001);
            open(app, Page::Home);
            assert!(app.browse.cache.len() <= 1);
            assert!(
                app.browse
                    .cache
                    .iter()
                    .all(|(_, list)| list.items.len() <= 20_000)
            );
        });
    }
}
