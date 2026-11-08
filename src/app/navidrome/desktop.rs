//! The Rune window for the active Navidrome controller.
mod artwork;
pub(crate) mod artwork_viewer;
pub(crate) mod autoscroll;
pub(crate) mod inspector;
mod lyrics;
pub(crate) mod lyrics_share;
mod placement;
#[cfg(all(test, feature = "navidrome-demo"))]
mod regression;
mod tracks;
use super::{App, Host, presentation};
use crate::ui::desktop as views;
use crate::{
    model::{Action, Page},
    navidrome::{
        api::CoverSize,
        backend::{Cover, Request},
    },
    settings::ThemeChoice,
    theme::Palette,
};
use rune::{AppSpec, Live, TextEdit, Tx, Waker};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

pub struct Launch {
    pub dirs: crate::paths::AppDirs,
    pub receipt: Option<crate::updates::Receipt>,
    pub update_error: Option<String>,
    pub demo: bool,
    pub size: Option<[f32; 2]>,
    pub page: Option<String>,
    pub flags: Option<String>,
    pub snapshot: Option<PathBuf>,
}

struct Window {
    handle: rune::AppHandle<State>,
    wake: Waker,
    delay: Cell<Option<Duration>>,
    covers: RefCell<artwork::Covers>,
}

impl Host for Window {
    fn repaint_after(&self, delay: Duration) {
        self.delay
            .set(Some(self.delay.get().map_or(delay, |d| d.min(delay))));
    }
    fn insert_cover(&self, _: &App, key: String, cover: Cover) {
        self.covers.borrow_mut().insert(key, cover);
    }
    fn retry_cover(&self, key: &str) -> bool {
        self.covers
            .borrow_mut()
            .failed(key, std::time::Instant::now());
        true
    }
    fn show_window(&self) {
        self.handle.open()
    }
    fn close_window(&self) {
        self.handle.quit()
    }
    fn local_action(&self, app: &mut App, action: Action) {
        match action {
            Action::Quit | Action::CloseWindow => self.close_window(),
            Action::ShowWindow => self.show_window(),
            action => app.presentation.apply_local(action, self.wake.clone()),
        }
    }
}

pub(crate) struct ItemMenu {
    pub anchor: Option<String>,
    pub uri: String,
    /// Stable occurrence for a collection track. Other catalog items use their own actions.
    pub row: Option<String>,
    /// A context click supplies window points. The More button anchors by row key.
    pub at: Option<[f32; 2]>,
}

pub(crate) struct State {
    pub(crate) app: App,
    window: Window,
    placement: Option<placement::Placement>,
    native_window: std::rc::Rc<RefCell<Option<rune::WindowHandle>>>,
    pub(crate) server: TextEdit,
    pub(crate) user: TextEdit,
    pub(crate) password: TextEdit,
    pub(crate) query: TextEdit,
    pub(crate) filter: TextEdit,
    pub(crate) library_filter: TextEdit,
    pub(crate) playlist_name: TextEdit,
    pub(crate) album_years: [TextEdit; 2],
    pub(crate) album_year_error: bool,
    pub(crate) library_search: bool,
    pub(crate) home_expanded: [bool; 2],
    #[cfg(feature = "navidrome-demo")]
    demo_scroll_end: bool,
    pub(crate) images: rune::ImageCache,
    pub(crate) icons: RefCell<HashMap<(crate::theme::Icon, crate::color::Srgba, u32), rune::Image>>,
    pub(crate) logos: RefCell<HashMap<u32, rune::Pixels>>,
    pub(crate) table: tracks::Tracks,
    pub(crate) autoscroll: autoscroll::Autoscroll,
    history_tables: HashMap<Page, tracks::Tracks>,
    browse_table_revision: Option<(Page, u64, Option<crate::model::TableSort>)>,
    pub(crate) offsets: HashMap<Page, Live<f32>>,
    restore_scroll: Option<f32>,
    stats_song: Option<(Page, String)>,
    pub(crate) analyser: crate::vis::WideAnalyser,
    samples: Vec<f32>,
    next_visual_frame: Option<(crate::settings::PlayerBarVis, std::time::Instant)>,
    pub(crate) trace: Vec<f32>,
    pub(crate) plot_width: f32,
    pub(crate) volume_wheel_points: f32,
    pub(crate) seek_preview: Option<(u64, f64)>,
    pub(crate) portrait_fade: RefCell<views::artist::PortraitFade>,
    pub(crate) artist_views: HashMap<Page, views::artist::Options>,
    pub(crate) favorite_menu: Option<ItemMenu>,
    pub(crate) exclusions_page: usize,
    pub(crate) inspector: Option<inspector::Inspector>,
    pub(crate) artwork_viewer: artwork_viewer::Viewer,
    pub(crate) menu: Option<&'static str>,
    pub(crate) lyrics_view: lyrics::View,
    pub(crate) lyrics_share: lyrics_share::Share,
    pub(crate) queue_open: bool,
    pub(crate) queue_compact: Cell<bool>,
    pub(crate) queue_selected: Option<crate::navidrome::queue::Id>,
    pub(crate) queue_menu: Option<crate::navidrome::queue::Id>,
    pub(crate) queue_offset: Live<f32>,
}

impl App {
    pub(crate) fn page_songs(&self) -> Option<&[crate::navidrome::api::Song]> {
        use crate::navidrome::api::Collection;
        match self.presentation.page() {
            Page::Songs | Page::Genre(_) => {
                self.browse.current().map(super::browse::Listing::songs)
            }
            Page::Album(_) => self.album.as_ref().map(|a| a.song.as_slice()),
            Page::Download(_) => Some(&self.downloads.songs),
            Page::Playlist(_) => match self.presentation.navidrome_search.collection.get() {
                Some(Collection::Playlist { songs, .. }) => Some(songs),
                _ => None,
            },
            _ => None,
        }
    }
}
impl State {
    pub(crate) fn open_stats_song(&mut self, page: Page, id: String, tx: &mut Tx) {
        self.dispatch(Action::Open(page.clone()), tx);
        self.stats_song = Some((page, id));
        self.focus_stats_song(tx);
    }
    fn focus_stats_song(&mut self, tx: &mut Tx) {
        let Some((page, id)) = &self.stats_song else {
            return;
        };
        if self.app.presentation.page() != page {
            self.stats_song = None;
            return;
        }
        if self.page_loading() {
            return;
        }
        let index = self
            .app
            .page_songs()
            .and_then(|songs| self.table.order().position(|(i, _)| songs[i].id == *id));
        if let Some(index) = index {
            self.table.pick(index, crate::model::RowPick::Only);
            self.restore_scroll = None;
            tx.scroll_into_view(self.table.key(index));
        }
        self.stats_song = None;
    }
    pub(crate) fn set_streaming_quality(&mut self, quality: crate::navidrome::quality::Quality) {
        self.app.set_streaming_quality(quality);
    }
    pub(crate) fn set_scrobbling(&mut self, enabled: bool) {
        self.app
            .scrobble_action(super::scrobble::Action::Enabled(enabled));
    }
    pub(crate) fn open_item_menu(&mut self, menu: ItemMenu) {
        self.app.prepare_rating(&menu.uri);
        self.favorite_menu = Some(menu);
    }
    pub(crate) fn toggle_autoplay_exclusion(&mut self, uri: &str) {
        self.app.toggle_autoplay_exclusion(uri);
        self.favorite_menu = None;
        self.menu = None;
        self.queue_menu = None;
    }
    pub(crate) fn include_in_autoplay(
        &mut self,
        kind: crate::navidrome::api::FavoriteKind,
        id: &str,
    ) {
        self.app.set_autoplay_exclusion(kind, id, "", false);
    }
    pub(crate) fn open_queue_menu(&mut self, id: u64) {
        if let Some(song) = self
            .app
            .transport
            .queue
            .upcoming()
            .iter()
            .find(|entry| entry.id == id)
            .and_then(|entry| self.app.transport.queue.song(entry))
        {
            let uri = crate::navidrome::ratings::Kind::Song.uri(&song.id);
            self.app.prepare_rating(&uri);
        }
        self.queue_selected = Some(id);
        self.queue_menu = Some(id);
    }
    pub(crate) fn apply_album_years(&mut self, tx: &mut Tx) {
        if let Some(order) = crate::navidrome::albums::Order::years(
            &self.album_years[0].text,
            &self.album_years[1].text,
        ) {
            self.album_year_error = false;
            self.dispatch(Action::Open(Page::AlbumList(order)), tx);
        } else {
            self.album_year_error = true;
        }
    }
    pub(crate) fn toggle_queue(&mut self, tx: &mut Tx) {
        self.queue_open = !self.queue_open;
        if self.queue_open {
            self.app.enable_lyrics(false);
        }
        self.queue_menu = None;
        self.menu = None;
        self.favorite_menu = None;
        if self.queue_open {
            tx.scroll_to_y("queue-scroll", self.queue_offset.get());
        } else if let Some(offset) = self.offsets.get(self.app.presentation.page()) {
            tx.scroll_to_y("page", offset.get());
        }
        tx.instant();
    }

    pub(crate) fn create_playlist(&mut self, tx: &mut Tx) {
        if !self.app.can_create_playlist() || self.playlist_name.text.trim().is_empty() {
            return;
        }
        let name = self.playlist_name.text.trim().to_owned();
        self.dispatch(
            crate::model::Action::EditPlaylists(super::playlists::Action::Create(name)),
            tx,
        );
        self.playlist_name.clear();
    }

    pub(crate) fn playlist_selection(&self, key: Option<&str>) -> Vec<usize> {
        if let Some(index) = key.and_then(|key| self.table.index(key)) {
            if !self.table.selection.rows.contains(&index) {
                return vec![self.table.source(index)];
            }
            self.table
                .selection
                .rows
                .iter()
                .map(|&i| self.table.source(i))
                .collect()
        } else {
            self.table.order().map(|(i, _)| i).collect()
        }
    }

    pub(crate) fn queue_selection(&mut self, key: Option<&str>, next: bool, tx: &mut Tx) {
        let Some(source) = self.app.page_songs() else {
            return;
        };
        let mut songs: Vec<_> = if key.is_some() {
            self.playlist_selection(key)
                .into_iter()
                .filter_map(|i| source.get(i).cloned())
                .collect()
        } else {
            source.to_vec()
        };
        if key.is_none() && matches!(self.app.presentation.page(), Page::Album(_)) {
            songs.sort_by_key(|s| (s.disc_number, s.track));
        }
        self.dispatch(
            Action::Queue(super::queue::Action::Songs { songs, next }),
            tx,
        );
    }

    pub(crate) fn add_to_playlists(&mut self, key: Option<&str>, tx: &mut Tx) {
        let Some(songs) = self.app.page_songs() else {
            return;
        };
        let selection = self
            .playlist_selection(key)
            .into_iter()
            .filter_map(|i| songs.get(i).cloned())
            .collect();
        self.playlist_name.clear();
        self.dispatch(
            crate::model::Action::EditPlaylists(super::playlists::Action::Open(selection)),
            tx,
        );
    }

    pub(crate) fn remove_playlist_selection(&mut self, key: &str, tx: &mut Tx) {
        let positions = self.playlist_selection(Some(key));
        self.dispatch(
            crate::model::Action::EditPlaylists(super::playlists::Action::Remove(positions)),
            tx,
        );
        self.table.selection = Default::default();
        self.sync_tracks();
    }

    fn new(
        launch: Launch,
        instance: Option<crate::single_instance::Guard>,
        handle: rune::AppHandle<Self>,
    ) -> Self {
        let notifier = handle.clone();
        let wake: Waker = Arc::new(move || notifier.wake());
        let window = Window {
            handle,
            delay: Cell::default(),
            covers: RefCell::default(),
            wake: wake.clone(),
        };
        let mut app = App::with_host(launch.dirs, instance, launch.demo, wake.clone());
        #[cfg(feature = "navidrome-demo")]
        if launch.demo {
            app.populate_demo_model(launch.page.as_deref(), launch.flags.as_deref(), &window);
        }
        let flags = launch
            .flags
            .as_deref()
            .unwrap_or("")
            .split(',')
            .collect::<Vec<_>>();
        let filter = if flags.contains(&"exclusions") {
            "Excluded from autoplay"
        } else if flags.contains(&"cache") {
            "Audio cache"
        } else if flags.contains(&"sync") {
            "Sync"
        } else if flags.contains(&"quality") {
            "Streaming quality"
        } else if flags.contains(&"appearance") {
            "Appearance"
        } else if flags.contains(&"about") {
            "About"
        } else {
            ""
        };
        let page = app.presentation.page().clone();
        if let Some(error) = launch.update_error {
            app.presentation.report_update_failure(error);
        }
        let mut state = Self {
            server: TextEdit::new(&app.login.server),
            user: TextEdit::new(&app.login.username),
            password: TextEdit::new("").secure(),
            query: TextEdit::new(&app.presentation.search.query),
            filter: TextEdit::new(filter),
            library_filter: TextEdit::new(""),
            playlist_name: TextEdit::new(""),
            album_years: if let Page::AlbumList(crate::navidrome::albums::Order::Year {
                from,
                to,
            }) = &page
            {
                [
                    TextEdit::new(from.to_string()),
                    TextEdit::new(to.to_string()),
                ]
            } else {
                [TextEdit::new("1990"), TextEdit::new("1999")]
            },
            album_year_error: false,
            library_search: false,
            home_expanded: [launch.demo && flags.contains(&"home-expanded"); 2],
            #[cfg(feature = "navidrome-demo")]
            demo_scroll_end: launch.demo
                && (flags.contains(&"home-artists")
                    || flags.contains(&"browse-end")
                    || flags.contains(&"inspector-output")),
            app,
            window,
            placement: None,
            native_window: Default::default(),
            images: rune::ImageCache::new(wake),
            icons: RefCell::default(),
            logos: RefCell::default(),
            table: Default::default(),
            autoscroll: Default::default(),
            history_tables: Default::default(),
            browse_table_revision: None,
            offsets: HashMap::from([(page, Live::new(0.))]),
            restore_scroll: None,
            stats_song: None,
            analyser: Default::default(),
            samples: Vec::new(),
            next_visual_frame: None,
            trace: Vec::new(),
            plot_width: 0.,
            volume_wheel_points: 0.,
            seek_preview: None,
            portrait_fade: Default::default(),
            artist_views: Default::default(),
            favorite_menu: None,
            exclusions_page: 0,
            inspector: None,
            artwork_viewer: Default::default(),
            menu: None,
            lyrics_view: Default::default(),
            lyrics_share: Default::default(),
            queue_open: launch.demo
                && flags
                    .iter()
                    .any(|flag| flag.starts_with("queue") || flag.starts_with("autoplay-")),
            queue_compact: Cell::new(false),
            queue_selected: None,
            queue_menu: None,
            queue_offset: Live::new(0.),
        };
        #[cfg(feature = "navidrome-demo")]
        if state.queue_open && flags.iter().any(|flag| flag.starts_with("queue")) {
            if flags.contains(&"queue-empty") {
                state.app.transport = super::Transport {
                    paused: true,
                    ..Default::default()
                };
            } else {
                let songs = state.app.albums[1].song.iter().take(2).cloned().collect();
                state
                    .app
                    .queue_action(super::queue::Action::Songs { songs, next: false });
                if flags.contains(&"queue-loading") {
                    state
                        .app
                        .transport
                        .queue
                        .reserve("loading".into(), "All Melody".into(), true);
                }
                if flags.contains(&"queue-menu") {
                    state.queue_menu = state.app.transport.queue.upcoming().front().map(|e| e.id);
                    state.queue_selected = state.queue_menu;
                }
                if flags.contains(&"queue-error") {
                    state
                        .app
                        .presentation
                        .toast_error("The album could not be loaded. Try adding it again.");
                }
            }
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo {
            state.populate_lyric_share_demo(&flags);
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo {
            state.app.demo_downloads(launch.page.as_deref(), &flags);
            state.app.configure_demo_exclusions(&flags);
        }
        state.sync_tracks();
        #[cfg(feature = "navidrome-demo")]
        if launch.demo {
            state.library_search = flags.iter().any(|flag| {
                matches!(
                    *flag,
                    "library-search" | "library-filtered" | "library-empty"
                )
            });
            if flags.contains(&"library-filtered") {
                state.library_filter.replace_text("Bonobo");
            } else if flags.contains(&"library-empty") {
                state.library_filter.replace_text("No matching album");
            }
            state.app.presentation.settings.sidebar_grid = flags.contains(&"library-grid");
            if flags.contains(&"library-sort") {
                state.menu = Some("sort");
            } else if flags.contains(&"library-options") {
                state.menu = Some("library-options");
            }
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo
            && flags.contains(&"exclusions-artist-menu")
            && let Page::Artist(id) = state.app.presentation.page()
            && let Some(id) = crate::app::navidrome::decode(id)
        {
            state.open_item_menu(ItemMenu {
                uri: crate::navidrome::api::FavoriteKind::Artist.uri(&id),
                row: None,
                anchor: None,
                at: Some([500., 300.]),
            });
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo && flags.contains(&"artwork") {
            state.demo_artwork(&flags);
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo && flags.contains(&"inspector") {
            state.demo_inspector(&flags);
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo && flags.contains(&"album-order") {
            state.menu = Some("album-order");
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo && flags.contains(&"album-year-error") {
            state.album_years[0].replace_text("2022");
            state.album_years[1].replace_text("2010");
            state.album_year_error = true;
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo
            && flags.contains(&"browse-menu")
            && state.app.presentation.page().is_browse_songs()
        {
            state.open_item_menu(ItemMenu {
                anchor: None,
                uri: crate::navidrome::api::FavoriteKind::Song.uri("browse-0"),
                row: Some(state.table.key(0).into()),
                at: None,
            });
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo
            && flags.contains(&"rating-album")
            && let Some(a) = &state.app.album
        {
            let uri = crate::navidrome::ratings::Kind::Album.uri(&a.id);
            state.app.prepare_rating(&uri);
            if flags.contains(&"rated") || flags.contains(&"rating-error") {
                state.app.demo_rating(&uri, flags.contains(&"rating-error"));
            }
            state.menu = Some("album");
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo
            && flags.contains(&"rating-song")
            && let Some(song) = state.app.page_songs().and_then(|s| s.first())
        {
            let uri = crate::navidrome::ratings::Kind::Song.uri(&song.id);
            state.app.prepare_rating(&uri);
            if flags.contains(&"rated") || flags.contains(&"rating-error") {
                state.app.demo_rating(&uri, flags.contains(&"rating-error"));
            }
            state.open_item_menu(ItemMenu {
                uri,
                row: Some(state.table.key(0).into()),
                at: None,
                anchor: None,
            });
        }
        #[cfg(feature = "navidrome-demo")]
        if launch.demo {
            use crate::navidrome::quality::{Capabilities, Quality};
            if flags.contains(&"quality-mp3") || flags.contains(&"quality-no-seek") {
                state.app.preferences.streaming_quality = Quality::Mp3Standard;
                state.app.quality.active = Quality::Mp3Standard;
                state.app.transport.seekable = true;
            }
            if flags.contains(&"quality-no-seek") {
                state.app.quality.capabilities = Some(Ok(Capabilities::default()));
                state.app.transport.seekable = false;
            }
            if flags.contains(&"quality-loading") {
                state.app.quality.capabilities = None;
            }
            if flags.contains(&"quality-error") {
                state.app.quality.capabilities = Some(Err(
                    "The server did not respond. Original playback is still available.".into(),
                ));
            }
            state.app.demo_sync(&flags);
            if flags.contains(&"cache") {
                state.app.preferences.audio_cache.enabled = flags.contains(&"cache-on");
                state.app.audio_cache.bytes = if flags.contains(&"cache-on") {
                    384 * 1024 * 1024
                } else {
                    0
                };
            }
            if flags.contains(&"cache-menu") {
                state.menu = Some("audio-cache-limit");
            }
            if flags.contains(&"cache-clearing") {
                state.app.audio_cache.retiring = state.app.audio_cache.bytes;
            }
            if flags.contains(&"cache-error") {
                state.app.audio_cache.error = Some("Audio could not be cached. Streaming continues. Free disk space or check folder permissions, then retry.".into());
            }
            if flags.contains(&"quality-menu") {
                state.menu = Some("streaming-quality");
            }
            if flags.contains(&"scrobbling-off") {
                state.set_scrobbling(false);
            }
            if flags.contains(&"scrobble-error") || flags.contains(&"scrobble-uncertain") {
                state
                    .app
                    .demo_scrobble_error(flags.contains(&"scrobble-uncertain"));
            }
        }
        state
    }

    pub(crate) fn spectrum_deadline(&self) -> Option<std::time::Instant> {
        self.next_visual_frame
            .filter(|(mode, _)| *mode == crate::settings::PlayerBarVis::Spectrum)
            .map(|(_, t)| t)
    }
    pub(crate) fn titlebar_inset(&self) -> f32 {
        #[cfg(target_os = "macos")]
        if !self
            .native_window
            .borrow()
            .as_ref()
            .is_some_and(placement::fullscreen)
        {
            return 28.;
        }
        0.
    }
    pub(crate) fn resized(&mut self, width: f32, height: f32) {
        let Some(placement) = &mut self.placement else {
            return;
        };
        placement.resize([width, height], self.app.presentation.settings.zoom);
    }
    fn save_placement(&mut self) {
        let Some(placement) = &mut self.placement else {
            return;
        };
        if let Some(handle) = self.native_window.borrow().as_ref() {
            placement.observe(handle);
        }
        if let Some((path, json)) = placement.update() {
            self.app.backend.send(Request::Save { path, json });
        }
    }
    pub(crate) fn sync_tracks(&mut self) {
        self.app.sync_download_page();
        let page = self.app.presentation.page();
        let revision = (page.is_browse_songs() || matches!(page, Page::Download(_))).then(|| {
            (
                page.clone(),
                if matches!(page, Page::Download(_)) {
                    self.app.downloads.content_revision
                } else {
                    self.app.browse.current().map_or(0, |list| list.revision)
                },
                self.collection_sort(),
            )
        });
        // Position updates do not change a paged catalogue. Avoid scanning every
        // loaded song on each playback tick.
        if revision.is_some() && revision == self.browse_table_revision {
            return;
        }
        if let Some(songs) = self.app.page_songs() {
            self.table.update(songs, self.collection_sort());
        }
        self.browse_table_revision = revision;
        self.history_tables
            .retain(|page, _| !page.is_browse() || self.app.browse.cached(page));
    }
    pub(crate) fn collection_sort(&self) -> Option<crate::model::TableSort> {
        let page = self.app.presentation.page();
        if matches!(page, Page::Album(_) | Page::Playlist(_) | Page::Download(_))
            || page.is_browse_songs()
        {
            self.app.presentation.table_sorts.get(page).copied()
        } else {
            None
        }
    }
    pub(crate) fn sort_collection(&mut self, column: crate::model::SortColumn) {
        let page = self.app.presentation.page().clone();
        if let Some(sort) = crate::model::TableSort::next(self.collection_sort(), column) {
            self.app.presentation.table_sorts.insert(page, sort);
        } else {
            self.app.presentation.table_sorts.remove(&page);
        }
        self.sync_tracks();
    }
    pub(crate) fn play_collection_row(&mut self, index: usize) {
        let page = self.app.presentation.page().clone();
        if matches!(page, Page::Playlist(_) | Page::Download(_)) || page.is_browse_songs() {
            let Some(source) = self.app.page_songs().filter(|songs| index < songs.len()) else {
                return;
            };
            let (songs, keys) = self
                .table
                .order()
                .map(|(i, key)| (source[i].clone(), key.to_owned()))
                .unzip();
            self.app.play_search_songs(songs, index);
            if let Page::Playlist(id) = page {
                self.app.playing_playlist = Some(id);
                self.app.remember_home_playlist();
            } else {
                self.app.browse.playing = Some(page);
            }
            self.app.transport.collection_rows = keys;
            return;
        }
        if self.collection_sort().is_none() {
            self.app.apply(super::Action::Play(index));
            return;
        }
        let Some(mut album) = self.app.album.clone() else {
            return;
        };
        let original = std::mem::take(&mut album.song);
        let (songs, keys) = self
            .table
            .order()
            .map(|(i, key)| (original[i].clone(), key.to_owned()))
            .unzip();
        self.app.play_album_songs(Some(album), songs, index);
        self.app.transport.collection_rows = keys;
    }
    pub(crate) fn playing_row(&self) -> Option<String> {
        let t = &self.app.transport;
        let i = t.queue.context_index()?;
        if let Some(key) = t.collection_rows.get(i) {
            return Some(key.clone());
        }
        let song = t.song()?;
        let occurrence = t.queue.context()[..i]
            .iter()
            .filter(|s| s.id == song.id)
            .count();
        Some(format!("song-{occurrence}:{}", song.id))
    }
    pub(crate) fn dispatch(&mut self, action: Action, tx: &mut Tx) {
        self.autoscroll.cancel();
        let previous = self.app.presentation.page().clone();
        if self.queue_compact.get()
            && matches!(
                &action,
                Action::Open(_)
                    | Action::Back
                    | Action::Forward
                    | Action::Search(_)
                    | Action::FocusSearch
                    | Action::NavidromeSearch(_)
            )
        {
            self.queue_open = false;
            self.app.enable_lyrics(false);
        }
        let session = self.app.session;
        self.app.ui_action(action, &self.window);
        self.menu = None;
        self.favorite_menu = None;
        self.queue_menu = None;
        if session != self.app.session {
            self.inspector = None;
            self.close_artwork();
            self.queue_open = false;
            self.lyrics_view = Default::default();
            self.cancel_lyric_selection();
            self.queue_selected = None;
            self.queue_offset.set(0.);
            self.window.covers.replace(Default::default());
            self.offsets.clear();
            self.history_tables.clear();
            self.browse_table_revision = None;
            self.artist_views.clear();
            self.table = Default::default();
            self.library_filter.clear();
            self.playlist_name.clear();
            self.library_search = false;
            self.home_expanded = [false; 2];
            self.password.clear();
            self.query.clear();
        }
        let page = self.app.presentation.page().clone();
        if previous != page {
            self.album_year_error = false;
            if let Page::AlbumList(crate::navidrome::albums::Order::Year { from, to }) = &page {
                self.album_years[0].replace_text(from.to_string());
                self.album_years[1].replace_text(to.to_string());
            }
            self.browse_table_revision = None;
            self.query.focused = page == Page::Search;
            if page != Page::Search {
                tx.blur();
            }
            let next = self.history_tables.remove(&page).unwrap_or_default();
            let old = std::mem::replace(&mut self.table, next);
            if session == self.app.session {
                self.history_tables.insert(previous, old);
            }
            self.history_tables
                .retain(|p, _| self.app.presentation.history.contains(p));
            self.sync_tracks();
            let offset = self
                .offsets
                .entry(page)
                .or_insert_with(|| Live::new(0.))
                .get();
            self.restore_scroll = (offset > 0. && self.page_loading()).then_some(offset);
            tx.scroll_to_y("page", offset);
        } else {
            self.sync_tracks();
        }
        tx.instant();
    }

    fn page_loading(&self) -> bool {
        let app = &self.app;
        match app.presentation.page() {
            page if page.is_browse() => app
                .browse
                .current()
                .is_some_and(|list| list.loading && !list.loaded),
            Page::Albums => app.list_loading,
            Page::Album(_) => app.album_loading,
            Page::Playlist(_) | Page::Artist(_) => matches!(
                app.presentation.navidrome_search.collection,
                crate::model::Loadable::Loading
            ),
            Page::FavoriteGroup(_) | Page::LikedSongs => {
                app.presentation.navidrome_favorites.loading
            }
            _ => false,
        }
    }

    pub(crate) fn palette(&self, system_dark: bool) -> Palette {
        let s = &self.app.presentation.settings;
        s.cached_palette().unwrap_or_else(|| {
            if matches!(s.theme, ThemeChoice::Dark)
                || (s.theme == ThemeChoice::System && system_dark)
            {
                Palette::dark()
            } else {
                Palette::light()
            }
        })
    }

    pub(crate) fn cover_tint(&self, uri: &str, palette: Palette) -> Option<crate::color::Srgba> {
        self.window
            .covers
            .borrow()
            .color(uri)
            .map(|rgb| palette.tint_from_art(rgb))
            .or_else(|| self.app.presentation.accents.get(uri).copied())
    }
    pub(crate) fn playing_tint(&self, palette: Palette) -> Option<crate::color::Srgba> {
        if !self.app.presentation.settings.accent_from_art {
            return None;
        }
        self.cover_tint(&self.app.cover_uri(self.app.playing_cover()?), palette)
    }
    pub(crate) fn cover_image(&self, id: &str, size: CoverSize) -> (String, Option<rune::Pixels>) {
        let uri = presentation::artwork_uri(self.app.session, id, size);
        let covers = self.window.covers.borrow();
        (
            self.artwork_visibility_key(&covers.view_key(&uri)),
            covers.get(&uri),
        )
    }

    /// Restored cards can become visible before credentials finish loading.
    /// Re-arm visibility when authenticated requests become possible.
    pub(crate) fn artwork_visibility_key(&self, key: &str) -> String {
        format!("{key}/{}/{}", self.app.session, self.app.client.is_some())
    }

    pub(crate) fn request_home_artist_cover(&mut self, id: &str) {
        self.app.request_home_artist_cover(id);
    }

    pub(crate) fn request_cover(&mut self, id: &str, size: CoverSize) {
        let key = presentation::artwork_uri(self.app.session, id, size);
        let Some(cached) = self.window.covers.borrow_mut().visible(&key) else {
            return;
        };
        if let Some(client) = &self.app.client
            && self.app.requested_covers.insert(key)
        {
            self.app.backend.send(Request::Cover {
                session: self.app.session,
                client: client.clone(),
                size,
                id: id.to_owned(),
                cached,
            });
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.save_placement();
        self.app.checkpoint_session(true);
        self.app.save();
    }
}

fn poll(s: &mut State, _time: f64, tx: &mut Tx) -> bool {
    s.autoscroll.poll(tx);
    let restoring = s.app.restoring_login;
    let mut changed = s.app.poll(&s.window);
    changed |= s.poll_artwork_save();
    #[cfg(feature = "navidrome-demo")]
    if s.demo_scroll_end && _time >= 0.1 {
        s.demo_scroll_end = false;
        tx.scroll_to_bottom_instant(if s.inspector.is_some() {
            "track-details-scroll"
        } else {
            "page"
        });
        changed = true;
    }
    if changed {
        s.sync_tracks();
        s.focus_stats_song(tx);
        if !s.page_loading()
            && let Some(y) = s.restore_scroll.take()
        {
            tx.scroll_to_y("page", y);
        }
    }
    // Lyric highlighting does not change browsing rows or their sort order.
    changed |= s.poll_lyrics_view(tx);
    changed |= s.poll_lyric_share();
    let (retry, deadline) = s
        .window
        .covers
        .borrow_mut()
        .poll_retries(std::time::Instant::now());
    changed |= retry;
    if let Some(delay) = deadline {
        tx.wake_after(delay);
    }
    if restoring && !s.app.restoring_login {
        s.server.replace_text(&s.app.login.server);
        s.user.replace_text(&s.app.login.username);
        if !s.app.connected {
            s.password.replace_text(&s.app.login.password);
        }
    }
    if s.app.presentation.custom_themes.needs_reload() {
        s.window.local_action(&mut s.app, Action::ReloadThemes);
    }
    if s.app.presentation.custom_themes.poll() {
        if s.app.presentation.refresh_theme_cache() {
            s.app.mark_dirty();
        }
        changed = true;
    }
    let toasts = &mut s.app.presentation.toasts;
    let old = toasts.len();
    toasts.retain(|t| t.created.elapsed() < crate::app::TOAST_LIFETIME);
    changed |= toasts.len() != old;
    for toast in toasts {
        let age = toast.created.elapsed();
        let fade_out = Duration::from_millis(2800);
        if age < Duration::from_millis(150) || age >= fade_out {
            tx.wake_after(Duration::from_millis(16));
            changed = true;
        } else {
            tx.wake_after(fade_out - age);
        }
    }
    // Align widgets with a completed credential restore; sensitive entry belongs
    // only to the login form and is cleared on leaving it.
    if s.query.focused && s.app.connected && s.app.presentation.page() != &Page::Search {
        s.dispatch(Action::Open(Page::Search), tx);
        changed = true;
    }
    if s.app.connected && !s.password.text.is_empty() {
        s.password.clear();
    }
    if s.app.dirty.is_some() {
        tx.wake_after(Duration::from_millis(500));
    }
    if let Some(delay) = s.app.checkpoint_delay() {
        tx.wake_after(delay);
    }
    if let Some(delay) = s.window.delay.take() {
        tx.wake_after(delay);
    }
    if s.window.handle.is_visible() && s.plot_width > 0. {
        let sounding = !s.app.transport.paused && s.app.transport.song().is_some();
        let mode = s.app.presentation.settings.player_bar_vis;
        use crate::{settings::PlayerBarVis, vis};
        let now = std::time::Instant::now();
        if let Some((previous, deadline)) = s.next_visual_frame
            && previous == mode
            && now < deadline
        {
            tx.wake_after(deadline - now);
            return changed;
        }
        s.next_visual_frame = None;
        match mode {
            PlayerBarVis::Spectrum if sounding || !s.analyser.settled() => {
                s.samples.clear();
                if sounding {
                    s.app
                        .tap
                        .window_into(vis::WIDE_SAMPLES, vis::LAG, &mut s.samples);
                }
                s.analyser.step(&s.samples, now);
                s.next_visual_frame = Some((mode, now + crate::ui::player_bar::VIS_FRAME));
                tx.wake_after(crate::ui::player_bar::VIS_FRAME);
            }
            PlayerBarVis::Waveform if sounding => {
                let points = (s.plot_width / 8.).clamp(75., 320.) as usize;
                s.app
                    .tap
                    .window_into(points * vis::SCOPE_STEP, vis::LAG, &mut s.samples);
                s.trace = vis::scope_line(&s.samples, points);
                s.next_visual_frame = Some((mode, now + crate::ui::player_bar::VIS_FRAME));
                tx.wake_after(crate::ui::player_bar::VIS_FRAME);
                changed = true;
            }
            _ if !s.trace.is_empty() => {
                s.trace.clear();
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

pub fn run(mut launch: Launch, prepare: impl FnOnce(&crate::paths::AppDirs)) -> Result<(), String> {
    let mut receipt = launch.receipt.take();
    let mut args = Vec::new();
    if let Some(path) = &launch.snapshot {
        args.extend([
            "--snap".to_owned(),
            path.to_string_lossy().into_owned(),
            "--at".into(),
            "1".into(),
            "--scale".into(),
            "2".into(),
        ]);
    }
    let placement = (!launch.demo)
        .then(|| {
            directories::ProjectDirs::from("", "", "navifast")
                .map(|dirs| dirs.data_dir().to_path_buf())
                .and_then(|dir| placement::Placement::load(dir.join("app.ron")))
        })
        .flatten();
    let restored = placement.as_ref().and_then(|p| {
        p.size(
            crate::settings::Settings::load(&launch.dirs.settings_file())
                .zoom
                .clamp(0.5, 2.5),
        )
    });
    let position = placement.as_ref().and_then(placement::Placement::position);
    let window_mode = placement
        .as_ref()
        .map(placement::Placement::mode)
        .unwrap_or_default();
    let demo_bounds = launch.size.map(|[w, h]| (w, h));
    let [w, h] = launch.size.or(restored).unwrap_or([1240., 800.]);
    let spec = AppSpec {
        poll: Some(poll),
        on_key: Some(views::keys),
        fonts: &[crate::theme::INTER],
        min_size: Some((
            crate::window::MAIN_MIN_SIZE[0],
            crate::window::MAIN_MIN_SIZE[1],
        )),
        ..AppSpec::new(
            "Navifast",
            (w, h),
            || unreachable!("state is supplied by the launcher"),
            views::view,
        )
    };
    #[cfg(target_os = "linux")]
    {
        static DESKTOP_ENTRY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        rune::set_app_id(DESKTOP_ENTRY.get_or_init(crate::media_controls::desktop_entry));
    }
    let app = rune::App::new(spec)
        .compositor(views::spectrum::Spectrum::default())
        .chrome(rune::Chrome {
            drag_regions: true,
            ..rune::Chrome::DEFAULT
        });
    let native_window = std::rc::Rc::new(RefCell::new(None));
    let observe = native_window.clone();
    let app = app
        .window_options(rune::WindowOptions {
            position,
            min_size: demo_bounds,
            max_size: demo_bounds,
            ..Default::default()
        })
        .on_window(move |handle| {
            window_mode.restore(handle);
            *observe.borrow_mut() = Some(handle.clone());
        })
        .on_ready(move |_| {
            if let Some(receipt) = receipt.take() {
                std::thread::spawn(move || {
                    if let Err(error) = receipt.acknowledge() {
                        log::error!("unable to acknowledge the update: {error}");
                    }
                });
            }
        })
        .on_close(|s, _| s.save_placement())
        .on_focus(|s, on, _| {
            if !on {
                s.autoscroll.cancel();
                s.save_placement();
            }
        });
    let handle = app.handle();
    let notify = handle.clone();
    let instance = if launch.demo {
        None
    } else {
        match crate::single_instance::acquire_with_wake(std::sync::Arc::new(move || notify.wake()))
        {
            crate::single_instance::Outcome::Only(guard) => Some(guard),
            crate::single_instance::Outcome::Surfaced => return Ok(()),
        }
    };
    prepare(&launch.dirs);
    app.args(args)
        .init(move || {
            let mut state = State::new(launch, instance, handle);
            state.native_window = native_window;
            state.placement = placement;
            state
        })
        .try_run()
}

#[cfg(all(test, feature = "navidrome-demo"))]
mod tests {
    use super::*;
    use rune::{Env, Key, KeyEvent, Mods, Runtime};

    #[test]
    fn stats_song_opens_its_album_and_selects_the_loaded_track_without_playing() {
        for width in [760., 1280.] {
            let mut rt = runtime("stats", "restored");
            rt.set_viewport((width, 800.), 2.);
            rt.frame(0.);
            rt.settle_all();
            assert!(rt.accessibility_action("stats-0-trk1", rune::accesskit::Action::Click, None));
            rt.frame(0.);
            assert_eq!(
                rt.state.app.presentation.page(),
                &Page::Album(super::super::encode("alb1"))
            );
            rt.update(|s, tx| {
                s.app.album = Some(crate::demo::album(1));
                s.app.album_loading = false;
                s.sync_tracks();
                s.focus_stats_song(tx);
            });
            assert_eq!(rt.state.table.cursor, Some(1));
            assert!(rt.state.app.transport.paused);
            assert!(rt.state.app.pending_play.is_none());
        }
    }

    pub(super) fn runtime(page: &str, flags: &str) -> Runtime<State> {
        let root =
            std::env::temp_dir().join(format!("navifast-rune-{:016x}", rand::random::<u64>()));
        let launch = Launch {
            receipt: None,
            update_error: None,
            dirs: crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            demo: true,
            size: Some([1280., 800.]),
            page: Some(page.into()),
            flags: Some(flags.into()),
            snapshot: None,
        };
        let spec = AppSpec::new("Navifast", (1280., 800.), || unreachable!(), views::view);
        let app = rune::App::new(spec);
        let state = State::new(launch, None, app.handle());
        let mut rt = Runtime::new(state, views::view, (1280., 800.), Env::new(true, 2.));
        rt.frame(0.);
        rt.settle_all();
        rt
    }

    fn click_row(rt: &mut Runtime<State>, i: usize) {
        let key = rt.state.table.key(i).to_owned();
        let el = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_ref() == Some(&key))
            .unwrap();
        let [x, y, _, h] = el.rect;
        // Click the title line. The artist credit beneath it opens the artist.
        rt.pointer_down(x + 90., y + h / 3.);
        rt.pointer_up(x + 90., y + h / 3.);
        rt.frame(0.);
    }

    #[test]
    fn artist_photo_crops_to_the_banner_without_stretching() {
        use crate::navidrome::{api::Collection, backend::Cover};

        let mut rt = runtime("artist:art0", "details");
        for (width, height) in [(150, 210), (200, 80)] {
            rt.update(|s, _| {
                let Some(Collection::Artist { cover_art, .. }) =
                    s.app.presentation.navidrome_search.collection.get_mut()
                else {
                    panic!("artist fixture missing")
                };
                *cover_art = Some("portrait".into());
                let uri = presentation::artwork_uri(s.app.session, "portrait", CoverSize::Portrait);
                s.window.covers.borrow_mut().insert(
                    uri,
                    Cover {
                        image: image::RgbaImage::new(width, height),
                        preview: image::RgbaImage::new(1, 1),
                        bytes: Arc::from([]),
                        color: None,
                    },
                );
            });
            for viewport in [760., 1673.] {
                rt.set_viewport((viewport, 800.), 2.);
                rt.frame(0.);
                rt.settle_all();
                let image = rt
                    .instances
                    .iter()
                    .find(|i| {
                        i.params[1] == rune::render::KIND_IMAGE
                            && i.rect[3] >= 432.
                            && ((i.uv[2] - width as f32).abs() < 0.01
                                || (i.uv[3] - height as f32).abs() < 0.01)
                    })
                    .expect("the loaded portrait is drawn");
                let aspect = image.rect[2] / image.rect[3];
                assert!((image.uv[2] / image.uv[3] - aspect).abs() < 0.01);
            }
        }
    }

    #[test]
    fn long_artist_name_uses_the_second_line_in_a_narrow_window() {
        use crate::navidrome::api::Collection;

        let mut rt = runtime("artist:art0", "details");
        let name = "AnnenMayKantereit";
        rt.update(|s, _| {
            let Some(Collection::Artist { name: artist, .. }) =
                s.app.presentation.navidrome_search.collection.get_mut()
            else {
                panic!("artist fixture missing")
            };
            *artist = name.into();
        });
        // At 800 pt Inter's tighter display cut would incorrectly fit on one line.
        for width in [760., 800.] {
            rt.set_viewport((width, 800.), 2.);
            rt.frame(0.);
            rt.settle_all();
            let title = rt
                .elements()
                .into_iter()
                .find(|e| e.text.as_deref() == Some(name))
                .unwrap();
            assert!(
                title.rect[3] >= 90.,
                "title should wrap at {width} points: {title:?}"
            );
        }
    }

    #[test]
    fn search_editor_fits_the_reference_toolbar_at_both_widths() {
        let mut rt = runtime("albums", "");
        for (viewport, width) in [(1280., 440.), (760., 200.)] {
            rt.set_viewport((viewport, 800.), 2.);
            rt.frame(0.);
            rt.settle_all();
            let elements = rt.elements();
            let field = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("search-field"))
                .unwrap();
            let editor = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("query"))
                .unwrap();
            let field_y = (crate::theme::TOP_BAR_HEIGHT + rt.state.titlebar_inset() - 34.) / 2.;
            assert_eq!(field.rect, [362., field_y, width, 34.]);
            assert_eq!(editor.rect, [396., field_y + 8., width - 64., 18.]);
            assert_eq!(editor.label.as_deref(), Some("What do you want to play?"));
        }
    }

    #[test]
    fn all_search_editors_and_their_placeholders_share_the_pill_center() {
        let mut rt = runtime("settings", "");
        rt.update(|s, _| s.library_search = true);
        rt.frame(0.);
        rt.settle_all();
        let elements = rt.elements();
        let rect = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
                .rect
        };
        for (outer, inner) in [
            ("search-field", "query"),
            ("library-search-field", "library-filter"),
            ("settings-search-field", "filter"),
        ] {
            let [_, y, _, h] = rect(outer);
            let [_, line, _, line_height] = rect(inner);
            assert_eq!((h, line_height), (34., 18.));
            assert_eq!(line + line_height / 2., y + h / 2.);
        }
    }
    #[test]
    fn recent_search_reopens_and_can_be_removed_independently() {
        let mut rt = runtime("search", "recent");
        let click = rune::accesskit::Action::Click;
        assert!(rt.accessibility_action("recent-query-Bonobo", click, None));
        rt.frame(0.);
        assert_eq!(rt.state.query.text, "Bonobo");
        assert_eq!(rt.state.app.presentation.search.committed, "Bonobo");
        assert!(rt.accessibility_action("clear-query", click, None));
        rt.frame(0.);
        assert!(rt.state.query.focused);
        assert!(rt.state.query.text.is_empty());
        assert!(rt.accessibility_action("forget-query-Bonobo", click, None));
        rt.frame(0.);
        assert!(rt.state.query.text.is_empty());
        assert_eq!(
            rt.state.app.presentation.navidrome_search.recent,
            ["Promises"]
        );
    }

    #[test]
    fn album_keyboard_plays_the_selected_occurrence_after_scrolling() {
        let mut rt = runtime("album:alb0", "");
        // Make the two duplicate entries reachable only after the header scrolls out.
        rt.update(|s, _| {
            let album = s.app.album.as_mut().unwrap();
            let track = album.song[0].clone();
            album.song = vec![track; 100];
            s.sync_tracks();
        });
        rt.update(|s, tx| tx.scroll_into_view(s.table.key(76)));
        rt.frame(0.);
        rt.settle_all();
        click_row(&mut rt, 76);
        assert_eq!(rt.state.table.cursor, Some(76));
        for key in [Key::Down, Key::Enter] {
            rt.key(KeyEvent {
                key,
                mods: Mods::default(),
            });
            rt.frame(0.);
        }
        assert_eq!(rt.state.table.cursor, Some(77));
        assert_eq!(rt.state.app.transport.queue.context_index(), Some(77));
        let old = rt.state.offsets[rt.state.app.presentation.page()].get();
        assert!(old > 1000.);
        rt.update(|s, tx| s.dispatch(Action::Open(Page::Settings), tx));
        rt.update(|s, tx| s.dispatch(Action::Back, tx));
        rt.frame(0.);
        assert_eq!(rt.state.table.cursor, Some(77));
    }

    #[test]
    fn playlist_play_control_starts_that_occurrence_and_the_empty_view_has_no_tracks() {
        let mut rt = runtime("playlist:list0", "");
        rt.update(|s, _| {
            if let crate::model::Loadable::Loaded(crate::navidrome::api::Collection::Playlist {
                songs,
                ..
            }) = &mut s.app.presentation.navidrome_search.collection
            {
                songs[1] = songs[0].clone();
            }
            s.sync_tracks();
        });
        rt.frame(0.);
        let key = format!("number-{}", rt.state.table.key(1));
        assert!(rt.accessibility_action(&key, rune::accesskit::Action::Click, None));
        rt.frame(0.);
        assert_eq!(rt.state.app.transport.queue.context_index(), Some(1));
        assert!(rt.state.app.playing_playlist.is_some());
        let empty = runtime("playlist:list0", "empty");
        assert!(
            empty
                .accessibility_json()
                .to_string()
                .contains("Nothing here yet.")
        );
    }

    #[test]
    fn album_number_and_more_menu_play_the_named_occurrence() {
        let mut rt = runtime("album:alb0", "");
        let target = rt.state.table.key(2).to_owned();
        assert!(rt.accessibility_action(
            &format!("number-{target}"),
            rune::accesskit::Action::Click,
            None
        ));
        rt.frame(0.);
        assert_eq!(rt.state.app.transport.queue.context_index(), Some(2));
        let key = rt.state.table.key(1).to_owned();
        assert!(rt.accessibility_action(
            &format!("more-{key}"),
            rune::accesskit::Action::Click,
            None
        ));
        rt.frame(0.);
        assert!(rt.accessibility_action("play-menu-row", rune::accesskit::Action::Click, None));
        rt.frame(0.);
        assert_eq!(rt.state.app.transport.queue.context_index(), Some(1));
        assert!(rt.state.favorite_menu.is_none());
    }

    #[test]
    fn albums_prefetch_at_the_end_and_wait_for_retry_after_an_error() {
        let mut rt = runtime("albums", "");
        rt.update(|s, _| {
            let sample = s.app.albums[0].clone();
            s.app.albums = (0..120)
                .map(|i| {
                    let mut a = sample.clone();
                    a.id = format!("fixture-{i}");
                    a
                })
                .collect();
            s.app.has_more = true;
            s.app.demo = false;
        });
        rt.frame(0.);
        for _ in 0..8 {
            rt.frame(1. / 60.);
        }
        assert!(!rt.state.app.list_loading);
        rt.update(|_, tx| tx.scroll_to_y("page", 10_000.));
        rt.frame(0.);
        for _ in 0..8 {
            rt.frame(1. / 60.);
        }
        assert!(rt.state.app.list_loading);
        rt.update(|s, tx| {
            s.app.list_loading = false;
            s.app.error = Some("fixture failure".into());
            tx.scroll_to_y("page", 0.);
        });
        rt.frame(0.);
        rt.update(|_, tx| tx.scroll_to_y("page", 10_000.));
        rt.frame(0.);
        for _ in 0..8 {
            rt.frame(1. / 60.);
        }
        assert!(!rt.state.app.list_loading);
        let [x, y, w, h] = rt
            .elements()
            .into_iter()
            .find(|e| e.role == "button" && e.label.as_deref() == Some("Retry"))
            .expect("failed pagination offers a retry")
            .rect;
        let request = rt.state.app.list_request;
        rt.pointer_down(x + w / 2., y + h / 2.);
        rt.pointer_up(x + w / 2., y + h / 2.);
        rt.frame(0.);
        assert_eq!(rt.state.app.list_request, request + 1);
        assert!(rt.state.app.list_loading);
    }

    #[test]
    fn playing_art_expands_into_the_sidebar_and_collapses_again() {
        let mut rt = runtime("albums", "");
        for (key, expanded) in [("expand-art", true), ("collapse-art", false)] {
            assert!(rt.accessibility_action(key, rune::accesskit::Action::Click, None));
            rt.frame(0.);
            assert_eq!(rt.state.app.presentation.settings.art_expanded, expanded);
            assert_eq!(rt.state.app.presentation.page(), &Page::Albums);
        }
        rt.update(|s, _| s.app.transport = Default::default());
        rt.frame(0.);
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("Nothing playing")
        );
        assert!(
            !rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("expand-art"))
        );
    }

    #[test]
    fn volume_wheel_accumulates_trackpad_motion_and_steps_once_per_mouse_detent() {
        let mut rt = runtime("albums", "");
        rt.update(|s, _| s.app.preferences.volume = 0.5);
        rt.frame(0.);
        let [x, y, w, h] = rt
            .elements()
            .into_iter()
            .find(|e| e.role == "slider" && e.label.as_deref() == Some("Volume (%)"))
            .unwrap()
            .rect;
        for (delta, precise, expected) in [(20., true, 0.5), (30., true, 0.55), (-120., false, 0.5)]
        {
            rt.wheel_event(
                rune::WheelEvent::new(0., delta)
                    .at(x + w / 2., y + h / 2.)
                    .precise(precise),
            );
            rt.frame(0.);
            assert!((rt.state.app.preferences.volume - expected).abs() < 0.001);
        }
    }

    #[test]
    fn library_card_play_starts_the_album_from_its_cover() {
        let mut rt = runtime("albums", "");
        let kind = crate::navidrome::api::FavoriteKind::Album;
        let key = format!("card-play-{}", kind.uri("alb1"));
        let [x, y, w, h] = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_ref() == Some(&key))
            .unwrap()
            .rect;
        rt.pointer_move(x + w / 2., y + h / 2.);
        rt.frame(0.);
        rt.pointer_down(x + w / 2., y + h / 2.);
        rt.pointer_up(x + w / 2., y + h / 2.);
        rt.frame(0.);
        assert_eq!(rt.state.app.pending_play, Some(("alb1".to_owned(), 0)));
        assert_eq!(rt.state.app.presentation.page(), &Page::Albums);
    }

    #[test]
    fn sorting_preserves_the_selected_occurrence_and_plays_in_display_order() {
        let mut rt = runtime("album:alb0", "");
        rt.update(|s, _| {
            let album = s.app.album.as_mut().unwrap();
            album.song.truncate(3);
            album.song[2] = album.song[0].clone();
            s.sync_tracks();
            s.table.pick(2, crate::model::RowPick::Only);
        });
        rt.frame(0.);
        let key = rt.state.table.key(2).to_owned();
        assert!(rt.accessibility_action("sort-Index", rune::accesskit::Action::Click, None));
        rt.frame(0.);
        assert_eq!(rt.state.table.cursor, Some(0));
        assert_eq!(rt.state.table.key(0), key);
        rt.update(|s, _| s.app.transport.paused = true);
        rt.frame(0.);
        assert!(rt.accessibility_action("album-play", rune::accesskit::Action::Click, None));
        rt.frame(0.);
        assert_eq!(rt.state.playing_row(), Some(key.clone()));
        rt.update(|s, _| s.app.apply(super::super::Action::Next));
        assert_eq!(
            rt.state.playing_row().as_deref(),
            Some(rt.state.table.key(1))
        );
        assert!(rt.accessibility_action("sort-Index", rune::accesskit::Action::Click, None));
        rt.frame(0.);
        assert!(rt.state.collection_sort().is_none());
        assert_eq!(rt.state.table.cursor, Some(2));
        for (key, ascending) in [
            ("sort-Title", Some(true)),
            ("sort-Title", Some(false)),
            ("sort-Title", None),
            ("sort-Duration", Some(true)),
        ] {
            assert!(rt.accessibility_action(key, rune::accesskit::Action::Click, None));
            rt.frame(0.);
            assert_eq!(rt.state.collection_sort().map(|s| s.ascending), ascending);
        }
    }

    #[test]
    fn favorites_shelves_and_full_groups_mount_in_the_page_viewport() {
        for page in [
            "favorites",
            "favorite-songs",
            "favorite-albums",
            "favorite-artists",
        ] {
            let rt = runtime(page, "many-favorites");
            assert!(
                rt.accessibility_json().to_string().contains("Refresh"),
                "{page}"
            );
        }
    }

    #[test]
    fn a_failed_favorite_still_offers_retry_on_another_page() {
        let mut rt = runtime("favorites", "write-error");
        rt.update(|s, tx| s.dispatch(Action::Open(Page::Settings), tx));
        rt.frame(0.);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("Could not update Favorites for"));
        assert!(tree.contains("Retry"));
    }

    #[test]
    fn opening_search_replaces_the_virtual_album_library() {
        let mut rt = runtime("albums", "");
        rt.update(|s, tx| s.dispatch(Action::Open(Page::Search), tx));
        rt.frame(0.);
        rt.settle_all();
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("What do you want to play?")
        );
        rt.update(|s, tx| s.dispatch(Action::Back, tx));
        rt.frame(0.);
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("album-library-rows"))
        );
    }

    #[test]
    fn account_menu_receives_clicks_over_the_album_grid() {
        let mut rt = runtime("albums", "");
        assert!(rt.accessibility_action("account", rune::accesskit::Action::Click, None));
        rt.frame(0.);
        rt.settle_all();
        let [x, y, w, h] = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("account-shortcuts"))
            .unwrap()
            .rect;
        rt.pointer_down(x + w / 2., y + h / 2.);
        rt.pointer_up(x + w / 2., y + h / 2.);
        rt.frame(0.);
        assert!(matches!(
            rt.state.app.presentation.dialog,
            Some(crate::model::Dialog::Shortcuts)
        ));
        assert_eq!(rt.state.app.presentation.page(), &Page::Albums);
    }

    #[test]
    fn a_filtered_equalizer_exposes_controls_and_updates_the_playback_filter() {
        let mut rt = runtime("settings", "");
        rt.update(|s, _| s.filter.replace_text("Equalizer"));
        rt.frame(0.);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("Pre"));
        assert!(!tree.contains("Open themes folder"));
        assert!(rt.accessibility_action("eq-1", rune::accesskit::Action::Increment, None));
        rt.frame(0.);
        assert_eq!(rt.state.app.presentation.settings.eq_bands_db[0], 0.1);
        assert_eq!(rt.state.app.eq.lock().unwrap().bands_db[0], 0.1);
    }

    #[test]
    fn transport_accessibility_press_toggles_real_controller_state() {
        let mut rt = runtime("album:alb0", "");
        let previous = rt.state.app.transport.paused;
        assert!(rt.accessibility_action("toggle", rune::accesskit::Action::Click, None));
        rt.frame(0.);
        assert_ne!(rt.state.app.transport.paused, previous);
    }

    #[test]
    fn login_exposes_a_named_password_field_and_connect_action() {
        let rt = runtime("albums", "signed-out");
        let tree = rt.accessibility_json();
        fn named<'a>(tree: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
            if tree["name"] == name {
                return Some(tree);
            }
            tree["children"]
                .as_array()?
                .iter()
                .find_map(|node| named(node, name))
        }
        assert!(named(&tree, "Password").is_some());
        let button = named(&tree, "Connect").unwrap();
        assert_eq!(button["role"], "button");
        assert!(
            button["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a == "click")
        );
    }
}
