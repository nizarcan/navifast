//! Available offline reuses the collection cards, track table and native controls.
use super::*;
use crate::navidrome::downloads::{Selection, Status};

fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1}\u{a0}GB", value as f64 / (1024. * 1024. * 1024.))
    } else {
        format!("{:.1}\u{a0}MB", value as f64 / (1024. * 1024.))
    }
}
fn summary(v: &Paint<'_>, key: &str) -> String {
    let Some(s) = v.s.app.downloads.summaries.get(key) else {
        return String::new();
    };
    let count = v
        .tr(mark("{ready} of {total} songs"))
        .replace("{ready}", &s.ready.to_string())
        .replace("{total}", &s.total.to_string());
    let mut result = format!("{count} · {}", bytes(s.bytes));
    if s.failed > 0 {
        result.push_str(&format!(" · {}", v.tr(mark("Download failed"))));
    }
    result
}
fn selection<'a>(v: &'a Paint<'_>, key: &str) -> Option<&'a Selection> {
    v.s.app.downloads.library.as_ref()?.selections.get(key)
}
fn controls(v: &Paint<'_>, key: &str) -> El {
    let Some(selection) = selection(v, key) else {
        return el();
    };
    let stats = v.s.app.downloads.summaries.get(key);
    let pending = selection.pending || stats.is_some_and(|s| s.active > 0);
    let failed =
        selection.error.is_some() || stats.is_some_and(|s| s.failed > 0 || s.ready < s.total);
    let mut actions = row().gap(8.).items_center().flex_wrap();
    if pending && !selection.paused {
        let key = key.to_owned();
        actions = actions.child(
            v.button(&v.tr(mark("Cancel")), false, move |s, _| {
                s.app.pause_download(key.clone())
            })
            .key("download-cancel"),
        );
    } else if selection.paused || failed {
        let key = key.to_owned();
        actions = actions.child(
            v.button(&v.tr(mark("Retry")), false, move |s, _| {
                s.app.retry_download(key.clone())
            })
            .disabled(v.s.app.offline())
            .key("download-retry"),
        );
    }
    let key = key.to_owned();
    actions.child(
        v.button(&v.tr(mark("Remove")), false, move |s, _| {
            s.app.remove_download(key.clone());
            s.sync_tracks();
        })
        .key("download-remove"),
    )
}
pub(super) fn heading(v: &Paint<'_>) -> El {
    let mut header = row()
        .w_full()
        .items_center()
        .gap(12.)
        .child(label(v.tr(mark("Downloads")), 28., 700., v.p.text).grow(1.));
    if v.s.app.offline() {
        header = header.child(
            v.button(&v.tr(mark("Reconnect")), false, |s, _| s.app.reconnect())
                .key("downloads-reconnect"),
        );
    }
    let used = v.s.app.downloads.bytes;
    let mut block = col().w_full().gap(8.).mb(20.).child(header).child(
        label(
            format!(
                "{} · {}",
                bytes(used),
                v.tr(mark("Original quality · Stored on this device"))
            ),
            13.,
            400.,
            v.p.secondary,
        )
        .wrap(),
    );
    if v.s.app.offline() {
        block = block.child(label(v.tr(mark("Working offline")), 13., 600., v.p.accent));
    }
    if let Some(error) = &v.s.app.downloads.error {
        block = block.child(
            row()
                .gap(12.)
                .items_center()
                .child(label(error, 13., 400., v.p.danger).grow(1.).wrap())
                .child(v.button(&v.tr(mark("Retry")), false, |s, _| {
                    s.app.retry_download_storage()
                })),
        );
    }
    if v.s.app.downloads.loading {
        block = block.child(v.loading());
    }
    block
}
pub(super) fn page(v: &Paint<'_>) -> El {
    let count = v.s.app.downloads.order.len();
    let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
    col()
        .key("download-list")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            count + 1,
            94.,
            |s: &State, i| {
                if i == 0 {
                    "downloads-heading".into()
                } else {
                    s.app.downloads.order[i - 1].clone().into()
                }
            },
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
                    let mut head = heading(&v);
                    if count == 0 && !s.app.downloads.loading && s.app.downloads.error.is_none() {
                        head = head.child(
                        label(
                            v.tr(mark(
                                "No downloads yet. Use Download in a song, album or playlist menu.",
                            )),
                            14.,
                            400.,
                            p.secondary,
                        )
                        .wrap(),
                    );
                    }
                    return head;
                }
                let key = &s.app.downloads.order[i - 1];
                let Some(selection) = selection(&v, key) else {
                    return el();
                };
                let page = Page::Download(key.clone());
                let status = if let Some(error) = &selection.error {
                    error.clone()
                } else if selection.pending {
                    v.tr(mark("Reading collection…"))
                } else if selection.paused {
                    format!("{} · {}", summary(&v, key), v.tr(mark("Cancelled")))
                } else {
                    summary(&v, key)
                };
                row()
                    .w_full()
                    .min_h(94.)
                    .gap(14.)
                    .items_center()
                    .child(
                        row()
                            .key(format!("download-open-{key}"))
                            .grow(1.)
                            .min_w(0.)
                            .gap(14.)
                            .items_center()
                            .role("button")
                            .label(&selection.title)
                            .focusable(true)
                            .hover(color(p.surface_hover))
                            .radius(6.)
                            .py(10.)
                            .child(v.cover(selection.cover.as_deref(), CoverSize::Sleeve, 48., 6.))
                            .child(
                                col()
                                    .grow(1.)
                                    .min_w(0.)
                                    .gap(4.)
                                    .child(label(&selection.title, 14., 600., p.text).truncate())
                                    .child(label(status, 12.5, 400., p.secondary).wrap()),
                            )
                            .on_press_tx(move |s: &mut State, tx| {
                                s.dispatch(Action::Open(page.clone()), tx)
                            }),
                    )
                    .child(controls(&v, key))
            },
        )
}
pub(super) fn collection_heading(v: &Paint<'_>) -> El {
    let Page::Download(key) = v.s.app.presentation.page() else {
        return el();
    };
    let Some(selection) = selection(v, key) else {
        return label(
            v.tr(mark("This download was removed.")),
            14.,
            400.,
            v.p.secondary,
        );
    };
    let active = v.s.app.browse.playing.as_ref() == Some(v.s.app.presentation.page())
        && !v.s.app.transport.paused;
    let byline = label(summary(v, key), 13.5, 400., v.p.secondary).wrap();
    let actions = library::collection_actions(
        v,
        "download-play",
        active,
        v.s.app.downloads.songs.is_empty(),
    )
    .child(controls(v, key));
    let mut header = library::collection_header(
        v,
        &v.tr(mark("Downloaded music")),
        &selection.title,
        selection.cover.as_deref(),
        byline,
        actions,
        false,
    );
    if selection.pending && !selection.paused && selection.error.is_none() {
        header = header.child(v.loading());
    }
    if let Some(error) = selection
        .error
        .as_ref()
        .or(v.s.app.downloads.error.as_ref())
    {
        header = header.child(label(error, 13., 400., v.p.danger).wrap());
    }
    header
}
pub(super) fn shelf_heading(v: &Paint<'_>) -> El {
    row()
        .w_full()
        .min_h(44.)
        .gap(12.)
        .items_center()
        .child(label(v.tr(mark("Available offline")), 20., 700., v.p.text).grow(1.))
        .child(
            v.button(&v.tr(mark("Manage")), false, |s, tx| {
                s.dispatch(Action::Open(Page::Downloads), tx)
            })
            .fill(Color::TRANSPARENT)
            .key("home-downloads-manage"),
        )
}
pub(super) fn shelf(v: &Paint<'_>, columns: usize) -> El {
    if let Some(error) = &v.s.app.downloads.error {
        return label(error, 13., 400., v.p.danger).wrap();
    }
    if v.s.app.downloads.loading {
        return v.loading();
    }
    let mut band = row().w_full().h(240.).gap(7.);
    for key in v.s.app.downloads.order.iter().take(columns) {
        let Some(selection) = selection(v, key) else {
            continue;
        };
        band = band.child(library::collection_card(
            v,
            format!("offline-{key}"),
            &selection.title,
            selection.cover.as_deref(),
            label(summary(v, key), 12.5, 400., v.p.secondary).wrap(),
            el(),
            Page::Download(key.clone()),
        ));
    }
    band
}
pub(super) fn availability(v: &Paint<'_>, id: &str) -> El {
    let Some(status) = v.s.app.download_status(id) else {
        return el();
    };
    let (name, icon, ink) = match status {
        Status::Ready { .. } => (
            v.tr(mark("Available offline")),
            Icon::CircleCheck,
            v.p.accent,
        ),
        Status::Failed { message } => (message.clone(), Icon::CircleAlert, v.p.danger),
        Status::Downloading { bytes, total } => (
            format!(
                "{} / {}",
                self::bytes(*bytes),
                total.map_or_else(|| "?".into(), self::bytes)
            ),
            Icon::Clock,
            v.p.secondary,
        ),
        Status::Queued => (
            v.tr(mark("Queued for download")),
            Icon::Clock,
            v.p.secondary,
        ),
        Status::Cancelled => (v.tr(mark("Download cancelled")), Icon::Clock, v.p.secondary),
    };
    el().size(24., 24.)
        .center()
        .role("status")
        .label(&name)
        .help(&name)
        .child(v.icon(icon, 14., ink))
}
pub(super) fn song_control(v: &Paint<'_>, id: &str) -> El {
    let Some(status) = v.s.app.download_status(id) else {
        return el();
    };
    if status.ready() {
        return el();
    }
    let retry = matches!(status, Status::Failed { .. } | Status::Cancelled);
    let disabled = retry
        && (v.s.app.offline()
            || !v
                .s
                .app
                .downloads
                .library
                .as_ref()
                .is_some_and(|l| l.wanted(id)));
    let id = id.to_owned();
    v.icon_button(
        &v.tr(if retry {
            mark("Retry download")
        } else {
            mark("Cancel download")
        }),
        if retry { Icon::Refresh } else { Icon::X },
        14.,
        move |s, _| s.app.download_song_control(id.clone(), retry),
    )
    .disabled(disabled)
}
pub(super) fn menu_item(v: &Paint<'_>, uri: &str) -> El {
    if !uri.starts_with("navidrome:track:") && !uri.starts_with("navidrome:album:") {
        return el();
    }
    let uri = uri.to_owned();
    v.menu_item(&v.tr(mark("Download")), None, move |s, _| {
        s.app.download_item(&uri);
        s.favorite_menu = None;
        s.menu = None;
        s.queue_menu = None;
    })
    .disabled(v.s.app.offline() || v.s.app.downloads.library.is_none())
    .key("download-item")
}
