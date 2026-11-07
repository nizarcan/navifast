//! Library and genre paging, built from the established table and sleeve cards.
use super::*;
use crate::app::navidrome::browse::Action as Browse;
use std::borrow::Cow;

pub(super) fn heading(v: &Paint<'_>) -> El {
    let page = v.s.app.presentation.page();
    let title = match page {
        Page::Genre(name) | Page::GenreAlbums(name) => name.clone(),
        Page::Genres => v.tr(mark("Genres")),
        _ => v.tr(mark("Songs")),
    };
    let mut block = col().w_full().gap(12.).pb(20.).child(
        row()
            .w_full()
            .items_center()
            .gap(12.)
            .child(label(title, 28., 700., v.p.text).wrap().min_w(0.).grow(1.))
            .child(
                v.icon_button(&v.tr(mark("Refresh")), Icon::Refresh, 16., |s, tx| {
                    s.dispatch(Action::Browse(Browse::Refresh), tx);
                })
                .key("browse-refresh"),
            ),
    );
    if let Page::Genre(name) | Page::GenreAlbums(name) = page {
        let mut tabs = row().gap(8.);
        for (title, target) in [
            (mark("Songs"), Page::Genre(name.clone())),
            (mark("Albums"), Page::GenreAlbums(name.clone())),
        ] {
            tabs = tabs.child(
                v.button(&v.tr(title), page == &target, move |s, tx| {
                    s.dispatch(Action::Open(target.clone()), tx);
                })
                .key(format!("browse-tab-{title}")),
            );
        }
        block = block.child(tabs);
    }
    block
}

pub(super) fn status(v: &Paint<'_>) -> El {
    let Some(list) = v.s.app.browse.current() else {
        return v.loading();
    };
    if let Some(error) = &list.error {
        return library::retry(v, error, Action::Browse(Browse::Retry)).py(16.);
    }
    if list.loading {
        return v.loading();
    }
    let message = if list.items.is_empty() {
        mark("No results")
    } else if !list.more {
        mark("End of list")
    } else {
        return v
            .button(&v.tr(mark("Load more")), false, |s, tx| {
                s.dispatch(Action::Browse(Browse::More), tx);
            })
            .key("browse-more")
            .my(16.);
    };
    label(v.tr(message), 14., 400., v.p.secondary)
        .wrap()
        .py(20.)
}

pub(super) fn load_ahead(v: &Paint<'_>, list: El, rows: usize, height: f32) -> El {
    if !v
        .s
        .app
        .browse
        .current()
        .is_some_and(|list| list.more && !list.loading && list.error.is_none())
    {
        return list;
    }
    let page = v.s.app.presentation.page().clone();
    list.on_visible_rows(move |s: &mut State, visible| {
        if s.app.presentation.page() == &page
            && rows.saturating_sub(visible.end) as f32 * height < crate::ui::widgets::LOAD_AHEAD
            && s.app
                .browse
                .current()
                .is_some_and(|list| list.more && !list.loading && list.error.is_none())
        {
            s.app.browse_action(Browse::More);
            s.sync_tracks();
            true
        } else {
            false
        }
    })
}

pub(super) fn page(v: &Paint<'_>) -> El {
    if v.s.app.presentation.page().is_browse_songs() {
        return library::songs(v);
    }
    let genres = v.s.app.presentation.page() == &Page::Genres;
    let columns = if genres {
        1
    } else {
        crate::ui::widgets::card_columns(v.content_width())
    };
    let count = v.s.app.browse.current().map_or(0, |list| list.items.len());
    let bands = count.div_ceil(columns);
    let height = if genres { 64. } else { 240. };
    let (p, scale, zoom, width, height_window) = (v.p, v.scale, v.zoom, v.width, v.height);
    let rows = col()
        .key("browse-rows")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            bands + 2,
            height,
            move |_: &State, i| Cow::Owned(format!("browse-band-{i}")),
            move |s: &State, i| {
                let v = Paint {
                    s,
                    p,
                    scale,
                    zoom,
                    width,
                    height: height_window,
                };
                if i == 0 {
                    return heading(&v);
                }
                if i == bands + 1 {
                    return status(&v);
                }
                let Some(list) = s.app.browse.current() else {
                    return el();
                };
                if genres {
                    let genre = &list.genres()[i - 1];
                    let page = Page::Genre(genre.value.clone());
                    return row()
                        .key(format!("genre-{}", genre.value))
                        .w_full()
                        .min_h(height)
                        .px(12.)
                        .gap(12.)
                        .items_center()
                        .radius(6.)
                        .hover(color(p.surface_hover))
                        .role("button")
                        .label(&genre.value)
                        .focusable(true)
                        .child(
                            col()
                                .min_w(0.)
                                .grow(1.)
                                .child(label(&genre.value, 15., 600., p.text).wrap())
                                .child(label(
                                    s.app.presentation.locale.song_count(genre.song_count),
                                    12.5,
                                    400.,
                                    p.secondary,
                                )),
                        )
                        .child(v.icon(Icon::ChevronRight, 16., p.secondary))
                        .on_press_tx(move |s: &mut State, tx| {
                            s.dispatch(Action::Open(page.clone()), tx)
                        });
                }
                let mut band = row().h(height).gap(7.);
                for album in list.albums().iter().skip((i - 1) * columns).take(columns) {
                    band = band.child(library::album_card(&v, album));
                }
                band
            },
        );
    let version = (
        v.s.app.presentation.page(),
        v.s.app.browse.current().map_or(0, |list| list.revision),
    );
    load_ahead(v, rows.virtual_rows_version(version), bands + 2, height)
}
