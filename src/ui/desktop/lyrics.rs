use super::*;
use crate::{
    model::Loadable,
    navidrome::lyrics::{Outcome, Source},
};

pub(super) fn panel(v: &Paint<'_>, wide: bool) -> El {
    let mut body = col()
        .key("lyrics-panel")
        .h_full()
        .min_h(0.)
        .px(if wide { 16. } else { 24. })
        .pt(if wide {
            v.s.titlebar_inset() + 22.
        } else {
            22.
        })
        .pb(16.)
        .fill(color(if wide { v.p.panel } else { v.p.window }));
    let mut header = row().w_full().items_center().gap(8.).child(
        label(v.tr(mark("Lyrics")), 28., 700., v.p.text)
            .key("lyrics-heading")
            .grow(1.),
    );
    if v.s.lyrics_share.draft.is_none()
        && !v.s.lyrics_view.following.get()
        && matches!(&v.s.app.lyrics.data, Loadable::Loaded(Outcome::Found(lyrics)) if lyrics.timed)
    {
        header = header.child(
            v.button(&v.tr(mark("Follow")), false, |s, tx| s.follow_lyrics(tx))
                .key("lyrics-follow"),
        );
    }
    if v.s.lyrics_share.draft.is_none()
        && matches!(&v.s.app.lyrics.data, Loadable::Loaded(Outcome::Found(_)))
    {
        let palette = v.p;
        header = header.child(
            v.icon_button(
                &v.tr(mark("Create lyric image")),
                Icon::Image,
                18.,
                move |s, tx| {
                    s.select_lyrics(palette);
                    if let Some(index) = s.lyric_active() {
                        tx.scroll_center_in_instant(
                            "lyric-selection",
                            format!("share-line-{index}"),
                        );
                    }
                },
            )
            .key("lyrics-image"),
        );
    }
    header = header.child(
        v.icon_button(&v.tr(mark("Close lyrics")), Icon::X, 20., |s, tx| {
            s.toggle_lyrics(tx)
        })
        .key("lyrics-close"),
    );
    body = body.child(header).child(
        el().w_full()
            .h(1.)
            .fill(color(v.p.outline))
            .mt(12.)
            .mb(10.)
            .a11y_hidden(),
    );
    if v.s.lyrics_share.draft.is_some() {
        return body.child(lyrics_card::selection(v));
    }
    if v.s.app.transport.song().is_none() {
        return body.child(v.empty(
            Icon::Mic,
            &v.tr(mark("Nothing playing")),
            &v.tr(mark("Play a song to see its lyrics.")),
        ));
    }
    match &v.s.app.lyrics.data {
        Loadable::NotLoaded | Loadable::Loading => {
            body.child(v.spinner(24., v.p.secondary).mt(24.))
        }
        Loadable::Failed(error) => body
            .child(label(error, 14., 400., v.p.secondary).wrap().mt(20.))
            .child(
                v.button(&v.tr(mark("Try again")), false, |s, _| s.retry_lyrics())
                    .key("lyrics-retry")
                    .mt(12.),
            ),
        Loadable::Loaded(Outcome::Missing) => body.child(v.empty(
            Icon::Mic,
            &v.tr(mark("No lyrics")),
            &v.tr(mark("No lyrics found for this track.")),
        )),
        Loadable::Loaded(Outcome::Instrumental) => body.child(v.empty(
            Icon::Music,
            &v.tr(mark("Instrumental")),
            &v.tr(mark("No words for this track.")),
        )),
        Loadable::Loaded(Outcome::Ambiguous) => body.child(v.empty(
            Icon::Mic,
            &v.tr(mark("No matching recording")),
            &v.tr(mark(
                "The artist, title, album and duration must identify the same recording.",
            )),
        )),
        Loadable::Loaded(Outcome::Found(lyrics)) => {
            if lyrics.invalid_timing {
                body = body.child(
                    label(
                        v.tr(mark("Timing is unavailable. Showing plain lyrics.")),
                        12.5,
                        400.,
                        v.p.secondary,
                    )
                    .wrap()
                    .mt(10.),
                );
            }
            let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
            let count = lyrics.lines.len();
            let rows = col()
                .key("lyrics-lines")
                .w_full()
                .py(16.)
                .growth(rune::GrowthSpec::default())
                .virtual_rows(
                    count + 1,
                    42.,
                    move |_: &State, i| {
                        if i == count {
                            "lyrics-source".into()
                        } else {
                            format!("lyric-{i}").into()
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
                        if i == count { source(&v) } else { line(&v, i) }
                    },
                )
                .virtual_rows_version(v.s.app.lyrics.song.as_deref());
            let scroller = col()
                .key("lyrics-scroll")
                .grow(1.)
                .min_h(0.)
                .w_full()
                .focusable(true)
                .role("list")
                .label(v.tr(mark("Lyrics")))
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .on_wheel(|s: &mut State, _, _| {
                    s.pause_lyrics_follow();
                    false
                })
                .on_key(|s: &mut State, k, tx| {
                    let pages = match k.key {
                        rune::Key::PageUp => -1,
                        rune::Key::PageDown => 1,
                        _ => return false,
                    };
                    s.pause_lyrics_follow();
                    tx.scroll_page("lyrics-scroll", pages);
                    true
                })
                .on_scroll_offset(v.s.lyrics_scroll_observer())
                .child(rows);
            body.child(scroller)
        }
    }
}

fn source(v: &Paint<'_>) -> El {
    // Keep the footer in the virtual stack, so wrapped rows can grow without overlapping it.
    label(
        v.tr(match v.s.app.lyrics.source {
            Source::Server => mark("Lyrics from Navidrome"),
            Source::Lrclib => mark("Lyrics from LRCLIB"),
        }),
        11.,
        400.,
        v.p.dim,
    )
    .key("lyrics-source")
    .wrap()
    .pt(12.)
    .pb(12.)
}

fn line(v: &Paint<'_>, index: usize) -> El {
    let Loadable::Loaded(Outcome::Found(lyrics)) = &v.s.app.lyrics.data else {
        return el();
    };
    let Some(line) = lyrics.lines.get(index) else {
        return el();
    };
    let active = v.s.lyric_active() == Some(index);
    let ink = if active && line.words.is_empty() {
        v.p.accent
    } else {
        v.p.secondary
    };
    let spans: Vec<_> = if active {
        line.active_words((v.s.app.transport.position.max(0.) * 1000.) as u64)
            .map(|i| {
                let range = &line.words[i].range;
                rune::Span::new(range.start, range.end).color(color(v.p.accent))
            })
            .collect()
    } else {
        vec![]
    };
    let mut row = col().key(format!("lyric-{index}")).w_full().py(6.).child(
        label(&line.text, 19., if active { 700. } else { 400. }, ink)
            .spans(spans)
            .wrap(),
    );
    if let Some(at) = line.at_ms {
        row = row
            .role("button")
            .focusable(true)
            .label(line.text.clone())
            .on_press_tx(move |s: &mut State, tx| {
                s.app.apply(navidrome::Action::Seek(at as f64 / 1000.));
                s.follow_lyrics(tx);
            });
    }
    row
}
