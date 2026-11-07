use super::*;
use crate::app::navidrome::shares::Action as ShareAction;

pub(super) fn menu(v: &Paint<'_>, uri: &str) -> El {
    if !uri.starts_with("navidrome:track:") && !uri.starts_with("navidrome:album:") {
        return el();
    }
    let uri = uri.to_owned();
    v.menu_item(&v.tr(mark("Share music…")), None, move |s, tx| {
        s.favorite_menu = None;
        s.menu = None;
        s.dispatch(Action::Share(ShareAction::Open(Some(uri.clone()))), tx);
    })
    .key("share-music")
}

pub(super) fn dialog(v: &Paint<'_>) -> El {
    let state = &v.s.app.shares;
    let mut contents = col().w_full().gap(12.).child(
        label(
            v.tr(mark(
                "Anyone with a public link can listen until it expires or you revoke it.",
            )),
            13.,
            400.,
            v.p.secondary,
        )
        .wrap(),
    );
    if let Some(target) = &state.target {
        contents = contents.child(label(&target.name, 16., 600., v.p.text).wrap());
        let mut choices = row().gap(8.).flex_wrap();
        for (days, title) in [
            (1, mark("1 day")),
            (7, mark("7 days")),
            (30, mark("30 days")),
        ] {
            choices = choices.child(
                v.button(&v.tr(title), state.days == days, move |s, tx| {
                    s.dispatch(Action::Share(ShareAction::Expiry(days)), tx);
                })
                .disabled(state.busy())
                .key(format!("share-expiry-{days}")),
            );
        }
        contents = contents
            .child(label(v.tr(mark("Expires after")), 13., 500., v.p.secondary))
            .child(choices)
            .child(
                v.button(
                    &v.tr(if state.creating {
                        mark("Creating link…")
                    } else {
                        mark("Create link")
                    }),
                    true,
                    |s, tx| {
                        s.dispatch(Action::Share(ShareAction::Create), tx);
                    },
                )
                .disabled(state.busy() || state.allowed != Some(true) || state.uncertain_creation)
                .key("share-create"),
            );
    }
    if state.uncertain_creation {
        contents = contents.child(
            label(
                v.tr(mark(
                    "The new link could not be confirmed. Refresh Music links before trying again.",
                )),
                13.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
    }
    if state.allowed == Some(false) {
        contents = contents.child(
            label(
                v.tr(mark(
                    "Sharing is disabled on this server or unavailable for your account.",
                )),
                13.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
    }
    if state.busy() {
        contents = contents.child(v.loading());
    }
    if let Some(error) = &state.error {
        contents = contents.child(label(error, 13., 400., v.p.secondary).wrap());
    }
    if state.failed_revoke.is_some() {
        contents = contents.child(
            v.button(&v.tr(mark("Retry revocation")), false, |s, tx| {
                s.dispatch(Action::Share(ShareAction::RetryRevoke), tx);
            })
            .key("share-retry-revoke")
            .disabled(state.busy()),
        );
    }
    if state.can_dismiss_revoke() {
        contents = contents.child(
            v.button(&v.tr(mark("Dismiss")), false, |s, tx| {
                s.dispatch(Action::Share(ShareAction::DismissRevoke), tx);
            })
            .disabled(state.busy()),
        );
    }
    contents = contents.child(
        row()
            .w_full()
            .items_center()
            .gap(12.)
            .child(label(v.tr(mark("Music links")), 16., 600., v.p.text).grow(1.))
            .child(
                v.icon_button(&v.tr(mark("Refresh")), Icon::Refresh, 16., |s, tx| {
                    s.dispatch(Action::Share(ShareAction::Refresh), tx);
                })
                .key("shares-refresh")
                .disabled(state.busy()),
            ),
    );
    if state.list.is_empty() && !state.busy() && state.allowed == Some(true) {
        contents = contents.child(label(
            v.tr(mark("No music links")),
            13.,
            400.,
            v.p.secondary,
        ));
    }
    let now = jiff::Timestamp::now();
    // The list is bounded by the response limit and only visible rows are built.
    let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
    contents = contents.child(
        col()
            .key("shares-list")
            .w_full()
            .growth(rune::GrowthSpec::default())
            .virtual_rows(
                state.list.len(),
                142.,
                |s: &State, i| format!("share-row-{}", s.app.shares.list[i].id).into(),
                move |s: &State, i| {
                    let v = Paint {
                        s,
                        p,
                        scale,
                        zoom,
                        width,
                        height,
                    };
                    let share = &s.app.shares.list[i];
                    let expiration = if !share.expiration_known {
                        v.tr(mark("Expiration unavailable"))
                    } else if let Some(expires) = share.expires {
                        format!(
                            "{} · {}",
                            v.tr(if expires <= now {
                                mark("Expired")
                            } else {
                                mark("Expires")
                            }),
                            expires.strftime("%Y-%m-%d %H:%M UTC")
                        )
                    } else {
                        v.tr(mark("No expiration"))
                    };
                    let copy_id = share.id.clone();
                    let revoke_id = share.id.clone();
                    col()
                        .key(format!("share-row-{}", share.id))
                        .w_full()
                        .min_h(142.)
                        .gap(6.)
                        .py(10.)
                        .child(label(
                            if share.name.is_empty() {
                                &share.id
                            } else {
                                &share.name
                            },
                            14.,
                            600.,
                            p.text,
                        ))
                        .child(label(expiration, 12.5, 400., p.secondary))
                        .child(
                            label(
                                share
                                    .url
                                    .clone()
                                    .unwrap_or_else(|| v.tr(mark("Public link unavailable"))),
                                12.,
                                400.,
                                p.secondary,
                            )
                            .wrap(),
                        )
                        .child(
                            row()
                                .gap(8.)
                                .child(
                                    v.button(
                                        &v.tr(if s.app.shares.copied.as_ref() == Some(&share.id) {
                                            mark("Copied")
                                        } else {
                                            mark("Copy link")
                                        }),
                                        false,
                                        move |s, tx| s.copy_music_link(&copy_id, tx),
                                    )
                                    .key(format!("share-copy-{}", share.id))
                                    .disabled(share.url.is_none()),
                                )
                                .child(
                                    v.button(&v.tr(mark("Revoke")), false, move |s, tx| {
                                        s.dispatch(
                                            Action::Share(ShareAction::Revoke(revoke_id.clone())),
                                            tx,
                                        )
                                    })
                                    .key(format!("share-revoke-{}", share.id))
                                    .disabled(
                                        s.app.shares.busy()
                                            || s.app.shares.allowed != Some(true)
                                            || s.app.shares.failed_revoke.is_some(),
                                    ),
                                ),
                        )
                },
            )
            .virtual_rows_version(state.revision),
    );
    playlists::dialog(
        v,
        &v.tr(mark("Music links")),
        contents,
        v.button(&v.tr(mark("Close")), false, |s, tx| {
            s.dispatch(Action::CloseDialog, tx)
        })
        .key("shares-close"),
    )
}
