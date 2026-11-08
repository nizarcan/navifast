//! The virtual Library shelf shares one row layout for albums and playlists.
use super::*;
use crate::app::navidrome::home::Kind;
use crate::settings::{LibraryContent, LibrarySort};
use std::borrow::Cow;

pub(super) fn title(content: LibraryContent) -> &'static str {
    match content {
        LibraryContent::Albums => mark("Albums"),
        LibraryContent::Playlists => mark("Playlists"),
        LibraryContent::All => mark("All"),
    }
}

pub(super) fn selected_sort(app: &navidrome::App) -> LibrarySort {
    match crate::ui::sidebar::selected_sort(
        &app.presentation,
        app.presentation.settings.library_content.shelf(),
    ) {
        LibrarySort::Local | LibrarySort::Spotify => LibrarySort::Library,
        sort => sort,
    }
}

#[derive(Clone, Copy)]
enum Entry {
    Album(usize),
    Playlist(usize),
}

struct Item<'a> {
    kind: Kind,
    id: &'a str,
    name: &'a str,
    artist: Option<&'a str>,
    cover: Option<&'a str>,
}

impl Entry {
    fn item(self, app: &navidrome::App) -> Item<'_> {
        match self {
            Self::Album(index) => {
                let album = &app.albums[index];
                Item {
                    kind: Kind::Album,
                    id: &album.id,
                    name: &album.name,
                    artist: Some(&album.artist),
                    cover: album.cover_art.as_deref(),
                }
            }
            Self::Playlist(index) => {
                let playlist = &app.presentation.navidrome_playlists.list.get().unwrap()[index];
                Item {
                    kind: Kind::Playlist,
                    id: &playlist.id,
                    name: &playlist.name,
                    artist: None,
                    cover: playlist.cover_art.as_deref(),
                }
            }
        }
    }
}

impl Item<'_> {
    fn key(&self) -> String {
        let kind = if self.kind == Kind::Album {
            "album"
        } else {
            "playlist"
        };
        format!("{kind}-{}", self.id)
    }

    fn page(&self) -> Page {
        let id = navidrome::encode(self.id);
        if self.kind == Kind::Album {
            Page::Album(id)
        } else {
            Page::Playlist(id)
        }
    }
}

fn entries(app: &navidrome::App, filter: &str) -> Vec<Entry> {
    let content = app.presentation.settings.library_content;
    let album_count = if content == LibraryContent::Playlists {
        0
    } else {
        app.albums.len()
    };
    let playlist_count = if content == LibraryContent::Albums {
        0
    } else {
        app.presentation
            .navidrome_playlists
            .list
            .get()
            .map_or(0, Vec::len)
    };
    let albums = (0..album_count).map(Entry::Album);
    let playlists = (0..playlist_count).map(Entry::Playlist);
    let mut entries: Vec<_> = albums
        .chain(playlists)
        .filter(|entry| {
            let item = entry.item(app);
            filter.is_empty()
                || item.name.to_lowercase().contains(filter)
                || item
                    .artist
                    .is_some_and(|artist| artist.to_lowercase().contains(filter))
        })
        .collect();
    match selected_sort(app) {
        LibrarySort::Name => entries.sort_by_cached_key(|entry| {
            let item = entry.item(app);
            (item.name.to_lowercase(), item.key())
        }),
        LibrarySort::RecentlyPlayed => entries.sort_by_cached_key(|entry| {
            let item = entry.item(app);
            if content == LibraryContent::Albums {
                let uri = format!("navidrome:album:{}", navidrome::encode(item.id));
                app.presentation
                    .recent_contexts
                    .iter()
                    .position(|key| key == &uri)
            } else {
                app.preferences
                    .home_history
                    .collections
                    .iter()
                    .position(|old| old.kind == item.kind && old.id == item.id)
            }
            .unwrap_or(usize::MAX)
        }),
        _ => {}
    }
    entries
}

pub(super) fn list(v: &Paint<'_>, width: f32) -> El {
    let app = &v.s.app;
    let grid = app.presentation.settings.sidebar_grid;
    let compact = app.presentation.settings.sidebar_compact;
    let layout = crate::ui::sidebar::grid_layout(width - 24.);
    let columns = if grid { layout.columns } else { 1 };
    let height = if grid {
        layout.row_height
    } else if compact {
        34.
    } else {
        60.
    };
    let edge = (layout.card_width - 14.).max(1.);
    let (p, scale, zoom) = (v.p, v.scale, v.zoom);
    let order = std::rc::Rc::new(entries(app, &v.s.library_filter.text.to_lowercase()));
    let keys = order.clone();
    let mut list = col()
        .key("sidebar-albums")
        .grow(1.)
        .min_h(0.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    if app.presentation.settings.library_content != LibraryContent::Albums {
        let playlists = &app.presentation.navidrome_playlists;
        if let Some(error) = &playlists.error {
            list = list.child(
                library::retry(v, error, Action::Reload(Page::Playlists))
                    .key("shelf-playlists-error")
                    .px(8.),
            );
        } else if playlists.refreshing || playlists.list.is_loading() {
            list = list.child(
                row()
                    .px(8.)
                    .gap(4.)
                    .items_center()
                    .key("shelf-playlists-loading")
                    .child(label(v.tr(mark("Playlists")), 13., 400., p.secondary))
                    .child(v.loading()),
            );
        } else if playlists.list.get().is_some_and(Vec::is_empty)
            && app.presentation.settings.library_content == LibraryContent::Playlists
        {
            list = list.child(
                label(v.tr(mark("No playlists")), 13., 400., p.secondary)
                    .key("shelf-playlists-empty")
                    .padding(12.),
            );
        }
    }
    list.child(col().growth(rune::GrowthSpec::default()).virtual_rows(
        order.len().div_ceil(columns),
        height,
        move |s: &State, i| Cow::Owned(format!("shelf-{}", keys[i * columns].item(&s.app).key())),
        move |s: &State, r| {
            let v = Paint {
                s,
                p,
                scale,
                zoom,
                width,
                height: 0.,
            };
            let mut band = row().w_full().h(height);
            for entry in order.iter().skip(r * columns).take(columns) {
                let data = entry.item(&s.app);
                let page = data.page();
                let playing = if data.kind == Kind::Album {
                    s.app.playing_playlist.is_none()
                        && s.app
                            .playing_album
                            .as_ref()
                            .is_some_and(|a| a.id == data.id)
                } else {
                    s.app.playing_playlist.as_deref() == Some(navidrome::encode(data.id).as_str())
                };
                let name = label(
                    data.name,
                    14.,
                    550.,
                    if playing { p.accent } else { p.text },
                )
                .truncate();
                let by = label(
                    data.artist.map_or_else(
                        || v.tr(mark("Playlist")),
                        |artist| format!("{} • {artist}", v.tr(mark("Album"))),
                    ),
                    12.5,
                    400.,
                    p.secondary,
                )
                .truncate();
                let mut item = if grid {
                    col()
                        .w(layout.card_width)
                        .padding(6.)
                        .gap(3.)
                        .child(v.cover(data.cover, CoverSize::Sleeve, edge, 6.))
                        .child(name)
                        .child(by)
                } else {
                    let mut line = row().w_full().px(8.).items_center().gap(12.);
                    if !compact {
                        line = line.child(v.cover(data.cover, CoverSize::Sleeve, 44., 6.));
                    }
                    let mut titles = col().min_w(0.).grow(1.).gap(3.).child(name);
                    if !compact {
                        titles = titles.child(by);
                    }
                    line = line.child(titles);
                    if playing {
                        line = line.child(v.icon(Icon::Volume2, 16., p.accent));
                    }
                    line
                };
                if s.app.presentation.page() == &page {
                    item = item.fill(color(p.surface));
                }
                band = band.child(
                    item.key(format!("library-{}", data.key()))
                        .h_full()
                        .radius(6.)
                        .role("button")
                        .label(data.name)
                        .focusable(true)
                        .hover(color(p.surface_hover))
                        .on_press_tx(move |s: &mut State, tx| {
                            s.dispatch(Action::Open(page.clone()), tx)
                        }),
                );
            }
            band
        },
    ))
}
