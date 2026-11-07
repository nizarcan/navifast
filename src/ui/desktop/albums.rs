//! Album discovery controls reuse the existing pills, menus and sleeve grid.
use super::*;
use crate::navidrome::albums::Order;
use crate::settings::{LibraryShelf, LibrarySort};

fn selected(v: &Paint<'_>) -> Order {
    if let Page::AlbumList(order) = v.s.app.presentation.page() {
        *order
    } else if v.s.app.newest {
        Order::Newest
    } else {
        Order::Name
    }
}
fn name(order: Order) -> &'static str {
    match order {
        Order::Newest => mark("Recently added"),
        Order::Name => mark("Name"),
        Order::Frequent => mark("Frequently played"),
        Order::Recent => mark("Recently played on server"),
        Order::Random => mark("Random selection"),
        Order::Year { .. } => mark("Year range"),
    }
}

pub(super) fn heading(v: &Paint<'_>) -> El {
    let order = selected(v);
    let title = if let Order::Year { from, to } = order {
        format!("{} · {from}-{to}", v.tr(name(order)))
    } else {
        v.tr(name(order))
    };
    let page = v.s.app.presentation.page().clone();
    let mut block = col().w_full().gap(10.).pb(18.).child(
        row()
            .w_full()
            .gap(8.)
            .items_center()
            .flex_wrap()
            .child(label(v.tr(mark("Albums")), 28., 700., v.p.text).grow(1.))
            .child(
                row()
                    .key("album-order")
                    .h(32.)
                    .px(12.)
                    .gap(6.)
                    .radius(16.)
                    .items_center()
                    .fill(color(v.p.surface))
                    .hover(color(v.p.surface_hover))
                    .role("button")
                    .label(&title)
                    .focusable(true)
                    .child(v.icon(Icon::ChevronDown, 14., v.p.secondary))
                    .child(label(title, 13., 600., v.p.text))
                    .on_press_tx(|s: &mut State, _| {
                        s.menu = if s.menu == Some("album-order") {
                            None
                        } else {
                            Some("album-order")
                        }
                    }),
            )
            .child(
                v.icon_button(&v.tr(mark("Refresh")), Icon::Refresh, 16., move |s, tx| {
                    s.dispatch(Action::Reload(page.clone()), tx)
                })
                .key("album-discovery-refresh"),
            ),
    );
    if matches!(order, Order::Recent | Order::Frequent) {
        block = block.child(
            label(
                v.tr(mark("Listening history reported by your server.")),
                13.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
    } else if order == Order::Random {
        block = block.child(
            label(
                v.tr(mark("Refresh for another selection.")),
                13.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
    }
    if matches!(order, Order::Year { .. }) {
        let mut fields = row().w_full().items_end().gap(10.).flex_wrap();
        for i in 0..2 {
            let label_text = v.tr(if i == 0 {
                mark("From year")
            } else {
                mark("To year")
            });
            fields = fields.child(
                col()
                    .gap(4.)
                    .child(label(&label_text, 12.5, 500., v.p.secondary))
                    .child(
                        text_input(&v.s.album_years[i], TextStyle::new(14., 400.))
                            .on_edit(move |s: &mut State| &mut s.album_years[i])
                            .on_submit(|s: &mut State, tx| s.apply_album_years(tx))
                            .key(if i == 0 {
                                "album-from-year"
                            } else {
                                "album-to-year"
                            })
                            .label(label_text)
                            .single_line()
                            .w(96.)
                            .h(34.)
                            .px(10.)
                            .radius(5.)
                            .fill(color(v.p.surface))
                            .color(color(v.p.text))
                            .caret(color(v.p.accent), color(v.p.accent.gamma_multiply(0.28))),
                    ),
            );
        }
        fields = fields.child(
            v.button(&v.tr(mark("Apply")), false, |s, tx| s.apply_album_years(tx))
                .key("album-years-apply"),
        );
        block = block.child(fields);
        if v.s.album_year_error {
            block = block.child(
                label(
                    v.tr(mark(
                        "Enter years from 1 to 9999, with the first year no later than the last.",
                    )),
                    13.,
                    400.,
                    v.p.danger,
                )
                .wrap(),
            );
        }
    }
    block
}

pub(super) fn menu(v: &Paint<'_>) -> El {
    let selected = selected(v);
    let mut menu = v.menu("album-order").w(266.).anchored_fit(
        "album-order",
        rune::Side::Bottom,
        rune::Align::Start,
        4.,
    );
    for order in [
        Order::Newest,
        Order::Name,
        Order::Frequent,
        Order::Recent,
        Order::Random,
        Order::Year {
            from: 1990,
            to: 1999,
        },
    ] {
        let active = order == selected
            || matches!((order, selected), (Order::Year { .. }, Order::Year { .. }));
        menu = menu.child(v.menu_item(
            &v.tr(name(order)),
            active.then_some(Icon::Check),
            move |s, tx| {
                if matches!(order, Order::Newest | Order::Name) {
                    s.dispatch(
                        Action::SetLibrarySort {
                            shelf: LibraryShelf::Albums,
                            sort: if order == Order::Newest {
                                LibrarySort::RecentlyAdded
                            } else {
                                LibrarySort::Name
                            },
                        },
                        tx,
                    );
                    s.dispatch(Action::Open(Page::Albums), tx);
                } else {
                    let order = if matches!(order, Order::Year { .. }) {
                        Order::years(&s.album_years[0].text, &s.album_years[1].text)
                            .unwrap_or(order)
                    } else {
                        order
                    };
                    s.dispatch(Action::Open(Page::AlbumList(order)), tx);
                }
            },
        ));
    }
    menu
}
