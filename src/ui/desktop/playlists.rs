//! Playlist editing uses the same modal, buttons and list surfaces as settings.
use super::*;
use crate::app::navidrome::playlists::Action as Edit;
use crate::model::Loadable;

fn dialog(v: &Paint<'_>, title: &str, contents: El, actions: El) -> El {
    detail_dialog(
        v,
        title,
        contents,
        actions,
        |s| s.app.presentation.dialog = None,
        None,
    )
}

pub(super) fn editor(v: &Paint<'_>) -> El {
    let state = &v.s.app.presentation.navidrome_playlists;
    let title = v.tr(if state.songs.is_empty() {
        mark("Create playlist")
    } else {
        mark("Add to playlists")
    });
    let input = text_input(&v.s.playlist_name, TextStyle::new(14., 400.))
        .on_edit(|s: &mut State| &mut s.playlist_name)
        .on_submit(|s: &mut State, tx| s.create_playlist(tx))
        .key("playlist-name")
        .label(v.tr(mark("Playlist name")))
        .single_line()
        .w_full()
        .h(34.)
        .px(10.)
        .radius(5.)
        .fill(color(v.p.surface))
        .color(color(v.p.text))
        .caret(color(v.p.accent), color(v.p.accent.gamma_multiply(0.28)));
    let mut contents = col()
        .w_full()
        .gap(12.)
        .child(label(v.tr(mark("Playlist name")), 13., 500., v.p.secondary))
        .child(
            row()
                .w_full()
                .gap(8.)
                .items_center()
                .child(input.grow(1.).min_w(0.))
                .child(
                    v.button(&v.tr(mark("Create")), false, |s, tx| s.create_playlist(tx))
                        .disabled(
                            v.s.playlist_name.text.trim().is_empty()
                                || !v.s.app.can_create_playlist(),
                        )
                        .key("playlist-create"),
                ),
        );
    if let Some(progress) = progress(v, None) {
        contents = contents.child(progress);
    }
    if !state.songs.is_empty() {
        contents = contents.child(label(
            crate::i18n::ngettext(
                v.s.app.presentation.locale,
                "{count} song",
                "{count} songs",
                state.songs.len() as u32,
            )
            .replace("{count}", &state.songs.len().to_string()),
            13.,
            400.,
            v.p.secondary,
        ));
        match &state.destinations {
            Loadable::Loaded(destinations) => {
                let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
                let rows = col()
                    .w_full()
                    .growth(rune::GrowthSpec::default())
                    .virtual_rows(
                        destinations.len(),
                        40.,
                        |s: &State, i| {
                            format!(
                                "destination-{}",
                                s.app
                                    .presentation
                                    .navidrome_playlists
                                    .destinations
                                    .get()
                                    .unwrap()[i]
                                    .id
                            )
                            .into()
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
                            let state = &s.app.presentation.navidrome_playlists;
                            let destination = &state.destinations.get().unwrap()[i];
                            let id = destination.id.clone();
                            let selected = state.selected.contains(&id);
                            let writable = s.app.playlist_destination_writable(destination);
                            let busy = state
                                .jobs
                                .iter()
                                .any(|j| !j.dismissed && j.task.edit.id() == Some(id.as_str()));
                            let mut item = row()
                                .w_full()
                                .h(40.)
                                .px(8.)
                                .gap(10.)
                                .items_center()
                                .radius(5.)
                                .hover(color(p.surface_hover))
                                .role("checkbox")
                                .focusable(true)
                                .checked(selected)
                                .label(&destination.name)
                                .child(
                                    el().size(18., 18.)
                                        .radius(4.)
                                        .border(1., color(p.outline))
                                        .center()
                                        .child(if selected {
                                            v.icon(Icon::Check, 14., p.accent)
                                        } else {
                                            el()
                                        }),
                                )
                                .child(
                                    label(&destination.name, 14., 400., p.text)
                                        .grow(1.)
                                        .truncate(),
                                );
                            if !writable {
                                item = item.child(label(
                                    v.tr(mark("Read-only")),
                                    12.,
                                    400.,
                                    p.secondary,
                                ));
                            }
                            item.disabled(!writable || busy).on_press_tx(
                                move |s: &mut State, tx| {
                                    s.dispatch(Action::EditPlaylists(Edit::Toggle(id.clone())), tx)
                                },
                            )
                        },
                    );
                contents = contents.child(rows);
            }
            Loadable::Failed(error) => {
                contents = contents
                    .child(label(error, 13., 400., v.p.danger).wrap())
                    .child(v.button(&v.tr(mark("Retry")), false, |s, tx| {
                        s.dispatch(Action::EditPlaylists(Edit::LoadDestinations), tx)
                    }));
            }
            _ => contents = contents.child(v.loading()),
        }
    }
    let mut actions = row()
        .gap(8.)
        .child(v.button(&v.tr(mark("Done")), false, |s, tx| {
            s.dispatch(Action::CloseDialog, tx)
        }));
    if !state.songs.is_empty() {
        actions = actions.child(
            v.button(&v.tr(mark("Add")), true, |s, tx| {
                s.dispatch(Action::EditPlaylists(Edit::Add), tx)
            })
            .disabled(state.selected.is_empty())
            .key("playlist-add"),
        );
    }
    dialog(v, &title, contents, actions)
}

pub(super) fn confirm_delete(v: &Paint<'_>) -> El {
    let name =
        v.s.app
            .presentation
            .navidrome_playlists
            .deleting
            .as_ref()
            .map_or("", |p| p.name.as_str());
    let contents = col()
        .gap(12.)
        .child(label(name, 18., 600., v.p.text).wrap())
        .child(
            label(
                v.tr(mark(
                    "Delete this playlist from the server? Its songs stay in your library.",
                )),
                14.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
    let actions = row()
        .gap(8.)
        .child(v.button(&v.tr(mark("Cancel")), false, |s, tx| {
            s.dispatch(Action::CloseDialog, tx)
        }))
        .child(
            v.button(&v.tr(mark("Delete playlist")), true, |s, tx| {
                s.dispatch(Action::EditPlaylists(Edit::Delete), tx)
            })
            .key("playlist-delete"),
        );
    dialog(v, &v.tr(mark("Delete playlist")), contents, actions)
}

pub(super) fn progress(v: &Paint<'_>, id: Option<&str>) -> Option<El> {
    let state = &v.s.app.presentation.navidrome_playlists;
    if !state
        .jobs
        .iter()
        .any(|j| !j.dismissed && id.is_none_or(|id| j.task.edit.id() == Some(id)))
    {
        return None;
    }
    let mut contents = col().w_full().gap(12.);
    for (i, job) in state
        .jobs
        .iter()
        .enumerate()
        .filter(|(_, j)| !j.dismissed && id.is_none_or(|id| j.task.edit.id() == Some(id)))
    {
        let status = v.tr(if job.pending {
            mark("Saving…")
        } else if job.error.is_some() {
            mark("Needs attention")
        } else {
            mark("Saved")
        });
        let mut line = col().gap(4.).child(
            row()
                .gap(8.)
                .items_center()
                .child(label(&job.name, 14., 500., v.p.text).grow(1.).truncate())
                .child(label(status, 12., 400., v.p.secondary)),
        );
        if let Some(error) = &job.error {
            let remove = matches!(
                job.task.edit,
                crate::navidrome::playlists::Edit::Remove { .. }
            ) && !job.task.sent;
            line = line.child(label(error, 13., 400., v.p.danger).wrap());
            if remove {
                let page = Page::Playlist(navidrome::encode(job.task.edit.id().unwrap()));
                line =
                    line.child(
                        v.button(&v.tr(mark("Refresh playlist")), false, move |s, tx| {
                            s.dispatch(Action::EditPlaylists(Edit::Dismiss(i)), tx);
                            s.dispatch(Action::Reload(page.clone()), tx);
                        }),
                    );
            } else {
                line = line.child(
                    v.button(&v.tr(mark("Retry")), false, move |s, tx| {
                        s.dispatch(Action::EditPlaylists(Edit::Retry(i)), tx)
                    })
                    .key(format!("playlist-retry-{i}")),
                );
                if !job.task.sent {
                    line = line.child(v.button(&v.tr(mark("Dismiss")), false, move |s, tx| {
                        s.dispatch(Action::EditPlaylists(Edit::Dismiss(i)), tx)
                    }));
                }
            }
        }
        contents = contents.child(line);
    }
    Some(contents)
}
