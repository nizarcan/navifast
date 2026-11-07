//! Ratings stay in the existing item menus; the star remains Favorites.
use super::*;
use crate::app::navidrome::ratings::Action as Rating;

pub(super) fn menu(v: &Paint<'_>, uri: &str) -> El {
    let Some(value) = v.s.app.ratings.value(uri) else {
        return el();
    };
    let mut choices = row().gap(4.).role("group").label(v.tr(mark("Your rating")));
    for rating in 1..=5 {
        let uri = uri.to_owned();
        let selected = value == rating;
        choices = choices.child(
            el().size(30., 30.)
                .key(format!("rating-{rating}"))
                .center()
                .radius(5.)
                .fill(color(if selected { v.p.accent } else { v.p.surface }))
                .hover(color(if selected {
                    v.p.accent_hover
                } else {
                    v.p.surface_hover
                }))
                .role("button")
                .focusable(true)
                .checked(selected)
                .label(
                    v.tr(mark("Rate {rating} out of 5"))
                        .replace("{rating}", &rating.to_string()),
                )
                .child(label(
                    rating.to_string(),
                    13.,
                    600.,
                    if selected { v.p.on_accent } else { v.p.text },
                ))
                .on_press_tx(move |s: &mut State, tx| {
                    s.dispatch(
                        Action::Rating(Rating::Set {
                            uri: uri.clone(),
                            value: rating,
                        }),
                        tx,
                    )
                }),
        );
    }
    let uri = uri.to_owned();
    col()
        .w_full()
        .pt(6.)
        .gap(6.)
        .child(
            label(
                v.tr(if value == 0 {
                    mark("Your rating · Not rated")
                } else {
                    mark("Your rating")
                }),
                12.,
                500.,
                v.p.secondary,
            )
            .px(8.),
        )
        .child(choices.px(8.))
        .child(
            v.menu_item(&v.tr(mark("Clear rating")), None, move |s, tx| {
                s.dispatch(
                    Action::Rating(Rating::Set {
                        uri: uri.clone(),
                        value: 0,
                    }),
                    tx,
                )
            })
            .key("rating-clear")
            .disabled(value == 0)
            .opacity(if value == 0 { 0.4 } else { 1. }),
        )
}

pub(super) fn errors(v: &Paint<'_>) -> El {
    let mut errors = col()
        .w_full()
        .max_h(120.)
        .px(crate::ui::widgets::PAGE_PADDING)
        .py(4.)
        .gap(4.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    for (uri, failure) in &v.s.app.ratings.failures {
        let uri = uri.clone();
        let key = format!("rating-retry-{uri}");
        let message = v
            .tr(mark("Could not save the rating for {name}. {error}"))
            .replace("{name}", &failure.name)
            .replace("{error}", &failure.message);
        errors = errors.child(
            row()
                .w_full()
                .gap(8.)
                .items_center()
                .child(
                    v.button(&v.tr(mark("Retry")), false, move |s, tx| {
                        s.dispatch(Action::Rating(Rating::Retry(uri.clone())), tx)
                    })
                    .key(key),
                )
                .child(
                    label(message, 13., 400., v.p.text)
                        .min_w(0.)
                        .grow(1.)
                        .wrap(),
                ),
        );
    }
    errors
}
