//! Quiet metrics and ranked destinations, using library typography and rows.
use super::*;
use crate::navidrome::stats::Entry;

pub(super) fn page(v: &Paint<'_>) -> El {
    let stats = &v.s.app.stats;
    let mut body = col()
        .w_full()
        .gap(16.)
        .child(
            row()
                .w_full()
                .gap(12.)
                .items_center()
                .child(
                    label(v.tr(mark("Listening stats")), 28., 700., v.p.text)
                        .wrap()
                        .min_w(0.)
                        .grow(1.),
                )
                .child(
                    v.icon_button(&v.tr(mark("Refresh")), Icon::Refresh, 16., |s, tx| {
                        s.dispatch(Action::Reload(Page::Stats), tx);
                    })
                    .key("stats-refresh"),
                ),
        )
        .child(
            label(
                v.tr(mark(
                    "Your Navidrome account’s library play counts, including other clients.",
                )),
                14.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
    if stats.loading {
        body = body.child(v.loading());
    }
    if let Some(error) = &stats.error {
        body = body.child(library::retry(v, error, Action::Reload(Page::Stats)));
    }
    let Some(summary) = &stats.summary else {
        return body;
    };
    body = body.child(
        label(
            format!(
                "{}: {}",
                v.tr(mark("Last completed refresh")),
                summary.refreshed
            ),
            12.5,
            400.,
            v.p.secondary,
        )
        .wrap(),
    );
    if summary.unavailable() {
        return body.child(v.empty(
            Icon::Clock,
            &v.tr(mark("Listening history unavailable")),
            &v.tr(mark("This server did not provide song play counts.")),
        ));
    }
    if summary.partial() {
        body = body.child(label(v.tr(mark("Partial summary: some play counts, durations or library destinations are missing.")), 13., 400., v.p.secondary).wrap());
    }
    let metrics = row()
        .w_full()
        .gap(32.)
        .flex_wrap()
        .child(metric(
            v,
            mark("Total plays"),
            &summary.total_plays.to_string(),
        ))
        .child(metric(
            v,
            mark("Estimated listening time"),
            &v.tr(mark("{hours} h {minutes} min"))
                .replace(
                    "{hours}",
                    &((summary.estimated_seconds / 3600.).floor() as u128).to_string(),
                )
                .replace(
                    "{minutes}",
                    &(((summary.estimated_seconds / 60.).floor() % 60.) as u8).to_string(),
                ),
        ));
    body = body.child(metrics);
    if summary.total_plays == 0 {
        if summary.counts_known < summary.songs_seen {
            return body;
        }
        return body.child(v.empty(
            Icon::Music,
            &v.tr(mark("No listening history yet")),
            &v.tr(mark(
                "Play music, then refresh to see your account’s listening summary.",
            )),
        ));
    }
    body = body.child(label(v.tr(mark("Artist counts include every credited artist. Listening time uses song duration × plays.")), 12.5, 400., v.p.secondary).wrap());
    let mut lists = if v.content_width() >= 780. {
        row()
    } else {
        col()
    }
    .w_full()
    .gap(24.);
    for (kind, title, entries) in [
        (0, mark("Top songs"), &summary.songs),
        (1, mark("Top albums"), &summary.albums),
        (2, mark("Top artists"), &summary.artists),
    ] {
        let mut section = col()
            .min_w(0.)
            .grow(1.)
            .gap(4.)
            .child(label(v.tr(title), 20., 700., v.p.text).pb(8.));
        for (index, entry) in entries.iter().enumerate() {
            section = section.child(ranked(v, kind, index, entry));
        }
        lists = lists.child(section);
    }
    body.child(lists)
}

fn metric(v: &Paint<'_>, name: &'static str, value: &str) -> El {
    col()
        .gap(4.)
        .child(label(v.tr(name), 13., 400., v.p.secondary))
        .child(label(value, 28., 700., v.p.text))
}

fn ranked(v: &Paint<'_>, kind: usize, index: usize, entry: &Entry) -> El {
    let target = match kind {
        0 => entry
            .album_id
            .as_ref()
            .map(|id| Page::Album(navidrome::encode(id))),
        1 => Some(Page::Album(navidrome::encode(&entry.id))),
        _ => Some(Page::Artist(navidrome::encode(&entry.id))),
    };
    let mut item = row()
        .key(format!("stats-{kind}-{}", entry.id))
        .w_full()
        .h(58.)
        .px(8.)
        .gap(10.)
        .items_center()
        .radius(6.)
        .child(label((index + 1).to_string(), 12.5, 400., v.p.secondary).w(20.))
        .child(
            col()
                .min_w(0.)
                .grow(1.)
                .child(label(&entry.name, 14., 600., v.p.text))
                .child(label(&entry.byline, 12.5, 400., v.p.secondary)),
        )
        .child(label(entry.plays.to_string(), 14., 500., v.p.secondary));
    if let Some(target) = target {
        let id = entry.id.clone();
        item = item
            .hover(color(v.p.surface_hover))
            .role("button")
            .label(format!("{}, {}", entry.name, entry.plays))
            .focusable(true)
            .on_press_tx(move |s: &mut State, tx| {
                if kind == 0 {
                    s.open_stats_song(target.clone(), id.clone(), tx);
                } else {
                    s.dispatch(Action::Open(target.clone()), tx);
                }
            });
    }
    item
}
