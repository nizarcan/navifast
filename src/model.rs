//! Native navigation, table sorting and control actions.
use crate::navidrome::api::FavoriteKind;
use std::{sync::Arc, time::Instant};
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    Downloads,
    Download(String),
    Stats,
    Search,
    LikedSongs,
    FavoriteGroup(FavoriteKind),
    Albums,
    AlbumList(crate::navidrome::albums::Order),
    Songs,
    Genres,
    Genre(String),
    GenreAlbums(String),
    Playlists,
    Playlist(String),
    Album(String),
    Artist(String),
    Settings,
}

impl Page {
    /// Discover groups existing routes without replacing their history or caches.
    pub fn discover_root(&self) -> Option<Self> {
        match self {
            Self::Albums | Self::AlbumList(_) => Some(Self::Albums),
            Self::Songs => Some(Self::Songs),
            Self::Genres | Self::Genre(_) | Self::GenreAlbums(_) => Some(Self::Genres),
            Self::Stats => Some(Self::Stats),
            _ => None,
        }
    }

    pub fn is_browse(&self) -> bool {
        matches!(
            self,
            Self::Songs | Self::Genres | Self::Genre(_) | Self::GenreAlbums(_) | Self::AlbumList(_)
        )
    }
    pub fn is_browse_songs(&self) -> bool {
        matches!(self, Self::Songs | Self::Genre(_))
    }
    pub fn is_favorites(&self) -> bool {
        matches!(self, Page::LikedSongs | Page::FavoriteGroup(_))
    }

    pub fn encode(&self) -> String {
        match self {
            Page::Home => "home".into(),
            Page::Downloads => "downloads".into(),
            Page::Download(key) => format!("download:{key}"),
            Page::Stats => "stats".into(),
            Page::Search => "search".into(),
            Page::LikedSongs => "liked".into(),
            Page::FavoriteGroup(FavoriteKind::Song) => "favorite-songs".into(),
            Page::FavoriteGroup(FavoriteKind::Album) => "favorite-albums".into(),
            Page::FavoriteGroup(FavoriteKind::Artist) => "favorite-artists".into(),
            Page::Albums => "albums".into(),
            Page::AlbumList(order) => format!("albums:{}", order.encode()),
            Page::Songs => "songs".into(),
            Page::Genres => "genres".into(),
            Page::Genre(genre) => format!("genre:{genre}"),
            Page::GenreAlbums(genre) => format!("genre-albums:{genre}"),
            Page::Playlists => "playlists".into(),
            Page::Playlist(id) => format!("playlist:{id}"),
            Page::Album(id) => format!("album:{id}"),
            Page::Artist(id) => format!("artist:{id}"),
            Page::Settings => "settings".into(),
        }
    }

    pub fn decode(text: &str) -> Option<Self> {
        Some(match text {
            "home" => Page::Home,
            "downloads" => Page::Downloads,
            "stats" => Page::Stats,
            "search" => Page::Search,
            "liked" => Page::LikedSongs,
            "favorite-songs" => Page::FavoriteGroup(FavoriteKind::Song),
            "favorite-albums" => Page::FavoriteGroup(FavoriteKind::Album),
            "favorite-artists" => Page::FavoriteGroup(FavoriteKind::Artist),
            "albums" => Page::Albums,
            "songs" => Page::Songs,
            "genres" => Page::Genres,
            "playlists" => Page::Playlists,
            "settings" => Page::Settings,
            other => {
                let (kind, id) = other.split_once(':')?;
                match kind {
                    "playlist" => Page::Playlist(id.into()),
                    "album" => Page::Album(id.into()),
                    "download" => Page::Download(id.into()),
                    "albums" => Page::AlbumList(crate::navidrome::albums::Order::decode(id)?),
                    "artist" => Page::Artist(id.into()),
                    "genre" => Page::Genre(id.into()),
                    "genre-albums" => Page::GenreAlbums(id.into()),
                    _ => return None,
                }
            }
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    NotLoaded,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Loadable::Loading)
    }

    pub fn needs_load(&self) -> bool {
        matches!(self, Loadable::NotLoaded | Loadable::Failed(_))
    }

    pub fn from_result<E: std::fmt::Display>(result: Result<T, E>) -> Self {
        match result {
            Ok(value) => Loadable::Loaded(value),
            Err(error) => Loadable::Failed(error.to_string()),
        }
    }

    /// Keeps an already loaded value when a refresh fails.
    pub fn refresh<E: std::fmt::Display>(&mut self, result: Result<T, E>) {
        if result.is_ok() || self.get().is_none() {
            *self = Self::from_result(result);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TableSort {
    pub column: SortColumn,
    pub ascending: bool,
}

impl TableSort {
    pub fn next(sort: Option<Self>, column: SortColumn) -> Option<Self> {
        // Ascending, descending, back to the list's own order.
        match sort {
            Some(sort) if sort.column == column && sort.ascending => Some(TableSort {
                column,
                ascending: false,
            }),
            Some(sort) if sort.column == column => None,
            // The # stands for the list's own order: from any other sort
            // it returns there rather than layering a sort of its own.
            Some(_) if column == SortColumn::Index => None,
            // Ascending by # is the list's own order, a click that would
            // change nothing; the first click on # reverses instead.
            _ => Some(TableSort {
                column,
                ascending: column != SortColumn::Index,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SortColumn {
    Title,
    Album,
    Added,
    Duration,
    AddedBy,
    /// The list's own order, for playing it reversed from the # heading.
    Index,
}

/// Playback and action context for a track row.
#[derive(Clone, Debug, PartialEq)]
pub enum RowContext {
    Queue,
    /// An album or playlist that can be played from an offset.
    Context {
        uri: String,
        /// The playlist id when the user owns it, enabling removal.
        editable_playlist: Option<(String, Option<String>)>,
    },
    /// A loose list of tracks, played as a queue of URIs.
    Uris(Arc<[String]>),
    /// A sorted or filtered context view that plays the displayed rows.
    View {
        uris: Arc<[String]>,
        context_uri: String,
        /// The playlist id when the user can edit it, enabling removal.
        /// Screen positions no longer match server positions, so moves
        /// stay disabled; removal is URI-based and safe.
        editable_playlist: Option<(String, Option<String>)>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub created: Instant,
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub committed: String,
    pub serial: u64,
    pub typed_at: Option<Instant>,
    pub focus_requested: bool,
}
#[derive(Clone, Debug)]
pub enum Dialog {
    Shortcuts,
    Playlists,
    DeletePlaylist,
}
#[derive(Clone, Debug)]
pub enum Action {
    Browse(crate::app::navidrome::browse::Action),
    Open(Page),
    OpenUri(String),
    Back,
    Forward,
    PlayContext {
        uri: String,
        offset_uri: Option<String>,
        offset_index: Option<u32>,
    },
    PlayUris {
        uris: Vec<String>,
        index: u32,
    },
    PlayFromRow {
        context: RowContext,
        uri: String,
        index: u32,
    },
    TogglePlay,
    Next,
    Previous,
    Seek(u32),
    SeekBy(i64),
    SetVolume(u8),
    PreviewVolume(u8),
    VolumeBy(i8),
    ToggleMute,
    ToggleSaved(String),
    Rating(crate::app::navidrome::ratings::Action),
    Scrobble(crate::app::navidrome::scrobble::Action),
    ShowDialog(Dialog),
    CloseDialog,
    OpenUrl(String),
    Search(String),
    ForgetSearch(String),
    PlayFavorite(String),
    RetryFavorite(String),
    NavidromeSearch(crate::app::navidrome::search::Action),
    EditPlaylists(crate::app::navidrome::playlists::Action),
    Queue(crate::app::navidrome::queue::Action),
    FocusSearch,
    LoadMore(Page),
    Reload(Page),
    SignOut,
    ToggleSidebar,
    SettingsChanged,
    SetTheme(crate::settings::ThemeChoice),
    SetLanguage(crate::settings::LanguageChoice),
    OpenThemesFolder,
    SetCustomTheme(String),
    ReloadThemes,
    SetLibrarySort {
        shelf: crate::settings::LibraryShelf,
        sort: crate::settings::LibrarySort,
    },
    SetLibraryGrid(bool),
    ArrangeLibrary {
        pinned: Vec<String>,
        /// A drag outside the pin block selects this local playlist order.
        playlist_order: Option<Vec<String>>,
    },
    ShowWindow,
    CyclePlayerBarVis,
    ToggleEq,
    SetEqBand(usize, f32),
    SetEqPreamp(f32),
    ApplyEqPreset(usize),
    CloseWindow,
    Quit,
}

#[derive(Clone, Debug, Default)]
pub struct RowSelection {
    pub rows: std::collections::BTreeSet<usize>,
    /// Row used as the anchor for shift-click ranges.
    pub anchor: Option<usize>,
}

impl RowSelection {
    pub(crate) fn pick(&mut self, row: usize, pick: RowPick, len: usize) {
        match pick {
            RowPick::Only => {
                // Clicking the sole selected row clears the self.
                let only_this = self.rows.len() == 1 && self.rows.contains(&row);
                self.rows.clear();
                if only_this {
                    self.anchor = None;
                } else {
                    self.rows.insert(row);
                    self.anchor = Some(row);
                }
            }
            RowPick::Toggle => {
                if !self.rows.remove(&row) {
                    self.rows.insert(row);
                }
                self.anchor = Some(row);
            }
            RowPick::Range => {
                // Without an anchor, shift-click selects only this row.
                let anchor = self.anchor.unwrap_or(row);
                let (from, to) = if anchor <= row {
                    (anchor, row)
                } else {
                    (row, anchor)
                };
                self.rows.clear();
                self.rows.extend((from..=to).filter(|row| *row < len));
                self.anchor = Some(anchor);
            }
        }
    }
}

/// Selection behavior for a row click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowPick {
    /// Select only this row.
    Only,
    /// Toggle this row.
    Toggle,
    /// Everything from the anchor to here.
    Range,
}
