use super::*;
use crate::app::navidrome::queue::Action as QueueAction;
use crate::navidrome::queue::{Entry, Source};

const ROW_HEIGHT: f32 = 60.;

pub(super) fn panel(v: &Paint<'_>, wide: bool) -> El {
    let queue = &v.s.app.transport.queue;
    let mut body = col()
        .key("queue-panel")
        .h_full()
        .min_h(0.)
        .px(if wide { 16. } else { 24. })
        .pt(if wide {
            v.s.titlebar_inset() + 22.
        } else {
            22.
        })
        .pb(16.)
        .fill(color(if wide { v.p.panel } else { v.p.window }))
        .child(
            row()
                .w_full()
                .items_center()
                .child(
                    label(
                        v.tr(mark("Queue")),
                        if wide { 22. } else { 28. },
                        700.,
                        v.p.text,
                    )
                    .grow(1.),
                )
                .child(
                    v.icon_button(&v.tr(mark("Close queue")), Icon::X, 20., |s, tx| {
                        s.toggle_queue(tx)
                    })
                    .key("queue-close"),
                ),
        );
    body = body.child(
        row()
            .w_full()
            .gap(12.)
            .items_center()
            .mt(14.)
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(3.)
                    .child(label(v.tr(mark("Autoplay")), 13., 600., v.p.text))
                    .child(
                        label(
                            v.tr(mark("Continue with random library songs")),
                            12.,
                            400.,
                            v.p.secondary,
                        )
                        .wrap(),
                    ),
            )
            .child(
                preferences::switch(
                    v,
                    &v.tr(mark("Autoplay")),
                    v.s.app.preferences.autoplay,
                    |s| {
                        s.app.set_autoplay(!s.app.preferences.autoplay);
                    },
                )
                .key("autoplay-toggle"),
            ),
    );
    if v.s.app.preferences.autoplay {
        if v.s.app.autoplay.loading() {
            let mut status = row().w_full().items_center().gap(8.).mt(8.).child(
                label(
                    v.tr(mark("Finding a library song…")),
                    12.,
                    400.,
                    v.p.secondary,
                )
                .wrap()
                .grow(1.)
                .min_w(0.),
            );
            if queue.manual_len() == 0 {
                status = status.child(clear_button(v, "queue-clear-autoplay"));
            }
            body = body.child(status);
        } else if let Some(error) = &v.s.app.autoplay.error {
            body = body.child(label(error, 12., 400., v.p.secondary).wrap().mt(8.));
        }
    }
    if let Some(current) = queue.current() {
        body = body
            .child(heading(v, mark("Now playing")))
            .child(track(v, current, true));
    }
    if queue.upcoming().is_empty() {
        return body.child(v.empty(
            Icon::ListVideo,
            &v.tr(mark("Nothing here yet.")),
            &v.tr(mark("Add songs or albums to the queue.")),
        ));
    }
    let manual = queue.manual_len();
    let automatic = queue.automatic_len();
    let count = queue.upcoming().len()
        + usize::from(manual > 0)
        + usize::from(manual + automatic < queue.upcoming().len())
        + usize::from(automatic > 0);
    let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
    let mut list = col()
        .key("queue-tracks")
        .w_full()
        .growth(rune::GrowthSpec::default());
    if let Some(id) = v.s.queue_selected {
        list = list.active_descendant(format!("queue-{id}"));
    }
    list = list.virtual_rows(
        count,
        ROW_HEIGHT,
        |s: &State, row| match item(s, row) {
            Item::Manual => "queue-manual-heading".into(),
            Item::Context => "queue-context-heading".into(),
            Item::Automatic => "queue-automatic-heading".into(),
            Item::Track(entry) => format!("queue-{}", entry.id).into(),
        },
        move |s: &State, row| {
            let v = Paint {
                s,
                p,
                scale,
                zoom,
                width,
                height,
            };
            match item(s, row) {
                Item::Manual => row_clear(&v),
                Item::Context => heading(&v, mark("Next up")),
                Item::Automatic => {
                    if s.app.transport.queue.manual_len() == 0 {
                        clear_heading(&v, mark("Autoplay"), "queue-clear-automatic")
                    } else {
                        heading(&v, mark("Autoplay"))
                    }
                }
                Item::Track(entry) => track(&v, entry, false),
            }
        },
    );
    let offset = v.s.queue_offset.clone();
    let scroller = col()
        .key("queue-scroll")
        .grow(1.)
        .min_h(0.)
        .w_full()
        .role("listbox")
        .label(v.tr(mark("Queue")))
        .focusable(true)
        .on_key(keys)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .on_scroll_offset(move |pos| {
            offset.set(pos.offset[1]);
        })
        .child(list);
    body.child(scroller)
}

enum Item<'a> {
    Manual,
    Context,
    Automatic,
    Track(&'a Entry),
}

fn item(s: &State, mut row: usize) -> Item<'_> {
    let queue = &s.app.transport.queue;
    let manual = queue.manual_len();
    let automatic = queue.automatic_len();
    let collection = queue.upcoming().len() - manual - automatic;
    let mut offset = 0;
    for (count, heading) in [
        (manual, Item::Manual),
        (collection, Item::Context),
        (automatic, Item::Automatic),
    ] {
        if count == 0 {
            continue;
        }
        if row == 0 {
            return heading;
        }
        row -= 1;
        if row < count {
            return Item::Track(&queue.upcoming()[offset + row]);
        }
        row -= count;
        offset += count;
    }
    unreachable!("virtual row count matches the three queue sections")
}

fn heading(v: &Paint<'_>, title: &'static str) -> El {
    row()
        .w_full()
        .h(40.)
        .items_center()
        .child(label(v.tr(title), 14., 600., v.p.text))
}

fn row_clear(v: &Paint<'_>) -> El {
    clear_heading(v, mark("Playing next"), "queue-clear")
}

fn clear_heading(v: &Paint<'_>, title: &'static str, key: &'static str) -> El {
    heading(v, title)
        .child(el().grow(1.))
        .child(clear_button(v, key))
}

fn clear_button(v: &Paint<'_>, key: &'static str) -> El {
    v.button(&v.tr(mark("Clear")), false, |s, tx| {
        s.dispatch(Action::Queue(QueueAction::Clear), tx)
    })
    .key(key)
    .h(28.)
    .px(12.)
}

fn track(v: &Paint<'_>, entry: &Entry, current: bool) -> El {
    let queue = &v.s.app.transport.queue;
    let id = entry.id;
    let (title, artist, cover) = if let Some(song) = queue.song(entry) {
        (
            song.title.as_str(),
            song.artist.as_str(),
            song.cover_art.as_deref(),
        )
    } else if let Source::Album { name, .. } = &entry.source {
        (name.as_str(), "", None)
    } else {
        return el();
    };
    let ink = if current { v.p.accent } else { v.p.text };
    let mut art = el()
        .size(42., 42.)
        .child(v.cover(cover, CoverSize::Sleeve, 42., 6.));
    if entry.pending() {
        art = art.child(
            el().absolute(0., 0.)
                .size(42., 42.)
                .center()
                .child(v.spinner(20., v.p.accent)),
        );
    } else if !current {
        art = art
            .child(
                el().absolute(0., 0.)
                    .size(42., 42.)
                    .center()
                    .fill(color(v.p.window.gamma_multiply(0.7)))
                    .child(v.icon(Icon::PlayFilled, 20., v.p.text))
                    .group_opacity(0., 1.),
            )
            .role("button")
            .focusable(true)
            .label(v.tr(mark("Play")))
            .key(format!("queue-play-{id}"))
            .on_press_tx(move |s: &mut State, tx| {
                s.dispatch(Action::Queue(QueueAction::Play(id)), tx)
            });
    }
    let selected = !current && v.s.queue_selected == Some(id);
    let mut track = row()
        .key(format!("queue-{id}"))
        .w_full()
        .h(ROW_HEIGHT)
        .px(4.)
        .gap(10.)
        .items_center()
        .radius(6.)
        .hover_group()
        .fill(if selected {
            color(v.p.secondary.gamma_multiply(0.2))
        } else {
            Color::TRANSPARENT
        })
        .child(art)
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .gap(3.)
                .child(label(title, 14., 550., ink).truncate())
                .child(
                    label(
                        if entry.pending() {
                            v.tr(mark("Loading…"))
                        } else {
                            artist.to_owned()
                        },
                        12.5,
                        400.,
                        v.p.secondary,
                    )
                    .truncate(),
                ),
        );
    if !current {
        track = track
            .role("option")
            .focus_selected(selected)
            .label(format!("{title} {artist}"))
            .hover(color(v.p.surface_hover))
            .child(
                v.icon_button(&v.tr(mark("More")), Icon::Ellipsis, 18., move |s, _| {
                    s.open_queue_menu(id);
                })
                .key(format!("queue-more-{id}"))
                .group_opacity(if v.s.queue_menu == Some(id) { 1. } else { 0. }, 1.),
            )
            .focusable(true)
            .on_key(keys)
            .on_press(move |s: &mut State| {
                s.queue_selected = Some(id);
            })
            .on_double_press_tx(move |s: &mut State, tx| {
                s.dispatch(Action::Queue(QueueAction::Play(id)), tx)
            })
            .on_secondary_press(move |s: &mut State, _, _| {
                s.open_queue_menu(id);
            });
    }
    track
}

pub(super) fn menu(v: &Paint<'_>) -> Option<El> {
    let id = v.s.queue_menu?;
    let entries = v.s.app.transport.queue.upcoming();
    let at = entries.iter().position(|e| e.id == id)?;
    let anchor = format!("queue-more-{id}");
    let mut menu = col()
        .w(208.)
        .padding(6.)
        .gap(2.)
        .radius(8.)
        .key("queue-menu")
        .role("menu")
        .fill(color(v.p.overlay))
        .border(1., color(v.p.outline))
        .anchored_fit(&anchor, rune::Side::Bottom, rune::Align::End, 4.)
        .focus_return(anchor)
        .modal(true, |s: &mut State| s.queue_menu = None)
        .child(
            v.menu_item(&v.tr(mark("Play")), Some(Icon::Play), move |s, tx| {
                s.dispatch(Action::Queue(QueueAction::Play(id)), tx)
            })
            .disabled(entries[at].pending())
            .key("queue-play-selected"),
        );
    if let Some(song) = v.s.app.transport.queue.song(&entries[at]) {
        let uri = crate::navidrome::ratings::Kind::Song.uri(&song.id);
        menu = menu.child(
            v.menu_item(
                &v.tr(mark("Track details")),
                Some(Icon::Info),
                move |s, _| s.inspect_song(&uri, Some(format!("queue-more-{id}"))),
            )
            .key("inspect-queue-track"),
        );
        menu = menu.child(ratings::menu(
            v,
            &crate::navidrome::ratings::Kind::Song.uri(&song.id),
        ));
    }
    for (delta, name, icon) in [
        (-1, mark("Move up"), Icon::ChevronUp),
        (1, mark("Move down"), Icon::ChevronDown),
    ] {
        let available = v.s.app.transport.queue.can_move(id, delta);
        menu = menu.child(
            v.menu_item(&v.tr(name), Some(icon), move |s, tx| {
                s.dispatch(Action::Queue(QueueAction::Move { id, delta }), tx)
            })
            .disabled(!available)
            .opacity(if available { 1. } else { 0.4 })
            .key(format!("queue-move-{delta}")),
        );
    }
    Some(
        menu.child(
            v.menu_item(
                &v.tr(mark("Remove from queue")),
                Some(Icon::X),
                move |s, tx| s.dispatch(Action::Queue(QueueAction::Remove(id)), tx),
            )
            .key("queue-remove-selected"),
        ),
    )
}

pub(super) fn keys(s: &mut State, event: &rune::KeyEvent, tx: &mut Tx) -> bool {
    let entries = s.app.transport.queue.upcoming();
    if entries.is_empty() {
        return false;
    }
    let at = s
        .queue_selected
        .and_then(|id| entries.iter().position(|e| e.id == id));
    let action = match event.key {
        rune::Key::Enter => at.map(|i| QueueAction::Play(entries[i].id)),
        rune::Key::Delete | rune::Key::Backspace => at.map(|i| QueueAction::Remove(entries[i].id)),
        rune::Key::Up | rune::Key::Down if event.mods.alt => at.map(|i| QueueAction::Move {
            id: entries[i].id,
            delta: if event.key == rune::Key::Up { -1 } else { 1 },
        }),
        rune::Key::Up | rune::Key::Down | rune::Key::Home | rune::Key::End => {
            let i = match event.key {
                rune::Key::Home => 0,
                rune::Key::End => entries.len() - 1,
                rune::Key::Up => at.unwrap_or(0).saturating_sub(1),
                _ => at.map_or(0, |i| (i + 1).min(entries.len() - 1)),
            };
            let id = entries[i].id;
            s.queue_selected = Some(id);
            tx.scroll_into_view(format!("queue-{id}"));
            return true;
        }
        _ => return false,
    };
    if let Some(action) = action {
        s.dispatch(Action::Queue(action), tx);
    }
    true
}
