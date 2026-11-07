//! Quick return Home, using the same card components and virtual rows as Albums.
use super::*;
use crate::app::navidrome::home::Kind;
use crate::model::Loadable;
use std::borrow::Cow;

pub(super) fn page(v: &Paint<'_>) -> El {
    let app = &v.s.app;
    let columns = crate::ui::widgets::card_columns(v.content_width());
    let playlists = app
        .presentation
        .navidrome_playlists
        .list
        .get()
        .map_or(0, Vec::len);
    let playlist_rows = if v.s.home_expanded[0] {
        playlists.div_ceil(columns)
    } else {
        usize::from(playlists > 0)
    };
    let history = &app.preferences.home_history;
    let fallback = history.collections.is_empty();
    let count = if fallback {
        app.albums.len().min(columns)
    } else {
        history.artists.len()
    };
    let artist_rows = if v.s.home_expanded[1] {
        count.div_ceil(columns)
    } else {
        usize::from(count > 0)
    };
    let rows = 2 + playlist_rows + 1 + artist_rows;
    let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
    col()
        .key("home-rows")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            rows,
            240.,
            move |_: &State, i| Cow::Owned(format!("home-row-{i}")),
            move |s: &State, i| {
                let v = Paint {
                    s,
                    p,
                    scale,
                    zoom,
                    width,
                    height,
                };
                if i == 0 {
                    return intro(&v);
                }
                if i == 1 {
                    return playlist_heading(&v, playlists, columns);
                }
                let app = &s.app;
                let history = &app.preferences.home_history;
                if i < 2 + playlist_rows {
                    let mut band = row().w_full().h(240.).gap(7.);
                    for item in app
                        .presentation
                        .navidrome_playlists
                        .list
                        .get()
                        .into_iter()
                        .flatten()
                        .skip((i - 2) * columns)
                        .take(columns)
                    {
                        band = band.child(library::playlist_card(&v, item));
                    }
                    return band;
                }
                if i == 2 + playlist_rows {
                    let mut head = section_heading(
                        &v,
                        if fallback {
                            mark("From your library")
                        } else {
                            mark("Recently played artists")
                        },
                    );
                    if fallback {
                        head = head.child(
                            v.button(&v.tr(mark("See all")), false, |s, tx| {
                                s.dispatch(Action::Open(Page::Albums), tx)
                            })
                            .fill(Color::TRANSPARENT),
                        );
                        let mut block = col().w_full().child(head);
                        if let Some(error) = &app.error {
                            block =
                                block.child(library::retry(&v, error, Action::Reload(Page::Home)));
                        } else if app.list_loading {
                            block = block.child(v.loading());
                        } else if app.albums.is_empty() {
                            block = block.child(
                                label(
                                    v.tr(mark("Albums in your Navidrome library appear here.")),
                                    14.,
                                    400.,
                                    p.secondary,
                                )
                                .wrap()
                                .py(16.),
                            );
                        }
                        return block;
                    }
                    if history.artists.len() > columns {
                        head = head.child(expand(&v, 1));
                    }
                    let mut block = col().w_full().child(head);
                    if history.artists.is_empty() {
                        block = block.child(
                            label(
                                v.tr(mark("Artists you listen to appear here.")),
                                14.,
                                400.,
                                p.secondary,
                            )
                            .wrap()
                            .py(16.),
                        );
                    }
                    return block;
                }
                let start = (i - 3 - playlist_rows) * columns;
                let mut band = row().w_full().h(240.).gap(7.);
                if fallback {
                    for album in app.albums.iter().skip(start).take(columns) {
                        band = band.child(library::album_card(&v, album));
                    }
                } else {
                    for artist in history.artists.iter().skip(start).take(columns) {
                        band = band.child(library::collection_card(
                            &v,
                            format!("home-artist-{}", artist.id),
                            &artist.name,
                            artist.cover.as_deref(),
                            label(v.tr(mark("Artist")), 12.5, 400., p.secondary),
                            el(),
                            artist.page(),
                        ));
                    }
                }
                band
            },
        )
}

fn section_heading(v: &Paint<'_>, title: &'static str) -> El {
    row()
        .w_full()
        .min_h(44.)
        .items_center()
        .gap(12.)
        .child(label(v.tr(title), 20., 700., v.p.text).wrap().grow(1.))
}

fn expand(v: &Paint<'_>, section: usize) -> El {
    v.button(
        &v.tr(if v.s.home_expanded[section] {
            mark("Less")
        } else {
            mark("More")
        }),
        false,
        move |s, _| s.home_expanded[section] = !s.home_expanded[section],
    )
    .key(format!("home-expand-{section}"))
    .fill(Color::TRANSPARENT)
}

fn playlist_heading(v: &Paint<'_>, count: usize, columns: usize) -> El {
    let state = &v.s.app.presentation.navidrome_playlists;
    let mut head = section_heading(v, mark("Your playlists"));
    if count > columns {
        head = head.child(expand(v, 0));
    }
    head = head.child(
        v.icon_button(&v.tr(mark("Refresh")), Icon::Refresh, 16., |s, tx| {
            s.dispatch(Action::Reload(Page::Playlists), tx)
        })
        .key("home-refresh-playlists"),
    );
    let mut block = col().w_full().child(head);
    if let Some(error) = &state.error {
        block = block.child(library::retry(v, error, Action::Reload(Page::Playlists)));
    } else if state.refreshing || matches!(state.list, Loadable::Loading | Loadable::NotLoaded) {
        block = block.child(v.loading());
    } else if count == 0 {
        block = block.child(
            label(
                v.tr(mark("Playlists from your server appear here.")),
                14.,
                400.,
                v.p.secondary,
            )
            .wrap()
            .py(16.),
        );
    }
    block
}

fn intro(v: &Paint<'_>) -> El {
    let entries = &v.s.app.preferences.home_history.collections;
    let mut block = col().w_full().pb(24.).child(
        label(
            v.tr(if entries.is_empty() {
                mark("Welcome to your library")
            } else {
                mark("Welcome back")
            }),
            28.,
            700.,
            v.p.text,
        )
        .wrap(),
    );
    block = block.child(
        label(
            v.tr(if entries.is_empty() {
                mark("Start with an album or one of your playlists.")
            } else {
                mark("Your music, close at hand.")
            }),
            14.,
            400.,
            v.p.secondary,
        )
        .wrap()
        .mt(7.)
        .mb(24.),
    );
    if entries.is_empty() {
        return block.child(
            label(
                v.tr(mark("Your recent listening will appear here as you play.")),
                14.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
    }
    block = block.child(section_heading(v, mark("Recently played")));
    let columns = if v.content_width() >= 720. { 3 } else { 2 };
    let tile_width = (v.content_width() - (columns - 1) as f32 * 10.) / columns as f32;
    for chunk in entries[..entries.len().min(6)].chunks(columns) {
        let mut band = row().w_full().gap(10.).mt(10.);
        for entry in chunk {
            let page = entry.page();
            let by = if entry.kind == Kind::Playlist {
                v.tr(mark("Playlist"))
            } else {
                entry.by.clone()
            };
            band = band.child(
                row()
                    .key(format!("home-recent-{}-{}", entry.kind as u8, entry.id))
                    .w(tile_width)
                    .h(72.)
                    .padding(8.)
                    .gap(12.)
                    .items_center()
                    .radius(6.)
                    .fill(color(v.p.surface))
                    .hover(color(v.p.surface_hover))
                    .role("button")
                    .label(format!("{}, {}", entry.name, by))
                    .focusable(true)
                    .child(v.cover(
                        entry.cover.as_deref(),
                        CoverSize::Sleeve,
                        if tile_width < 220. { 42. } else { 56. },
                        6.,
                    ))
                    .child(
                        col()
                            .min_w(0.)
                            .grow(1.)
                            .gap(3.)
                            .child(label(&entry.name, 14., 600., v.p.text).truncate())
                            .child(label(by, 12.5, 400., v.p.secondary).truncate()),
                    )
                    .on_press_tx(move |s: &mut State, tx| {
                        s.dispatch(Action::Open(page.clone()), tx)
                    }),
            );
        }
        block = block.child(band);
    }
    block
}
