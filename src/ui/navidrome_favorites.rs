//! Favorite controls shared by Navidrome rows, cards and detail pages.
use super::{navidrome_search, widgets};
use crate::app::{
    App,
    navidrome::{encode, favorites::State},
};
use crate::i18n::{Locale, gettext};
use crate::model::{Action, Page};
use crate::navidrome::api::FavoriteKind;
use crate::theme::{self, Icon, Palette};

fn appearance(locale: Locale, saved: bool) -> (Icon, std::borrow::Cow<'static, str>) {
    if saved {
        (Icon::StarFilled, gettext(locale, "Remove from Favorites"))
    } else {
        (Icon::Star, gettext(locale, "Add to Favorites"))
    }
}

pub(super) fn toggle(
    ui: &mut egui::Ui,
    state: &State,
    style: (&Palette, Locale),
    uri: &str,
    size: f32,
) -> bool {
    let (palette, locale) = style;
    let saved = state.contains(uri);
    let (icon, label) = appearance(locale, saved == Some(true));
    ui.add_enabled_ui(saved.is_some(), |ui| {
        theme::icon_button(
            ui,
            icon,
            size,
            if saved == Some(true) {
                palette.accent
            } else {
                palette.secondary
            },
            palette.text,
            &label,
        )
    })
    .inner
    .clicked()
}

pub(super) fn menu(ui: &mut egui::Ui, state: &State, style: (&Palette, Locale), uri: &str) -> bool {
    let (palette, locale) = style;
    let saved = state.contains(uri);
    let (icon, label) = appearance(locale, saved == Some(true));
    widgets::menu_item_enabled(ui, palette, Some(icon), &label, saved.is_some())
}

pub(super) fn row(
    ui: &mut egui::Ui,
    state: &State,
    actions: &mut Vec<Action>,
    style: (&Palette, Locale),
    uri: Option<&str>,
    draw: impl FnOnce(&mut egui::Ui, &mut Vec<Action>) -> egui::Response,
) -> egui::Response {
    let Some(uri) = uri else {
        return draw(ui, actions);
    };
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 32.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            if toggle(ui, state, style, uri, 17.0) {
                actions.push(Action::ToggleSaved(uri.to_owned()));
            }
            draw(ui, actions)
        },
    )
    .inner
}

pub(super) fn write_errors(app: &mut App, ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(widgets::PAGE_PADDING as i8, 4))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("favorite-write-errors")
                .max_height(120.0)
                .show(ui, |ui| {
                    for (uri, failure) in &app.navidrome_favorites.failures {
                        ui.push_id(uri, |ui| {
                            let message = gettext(
                                app.locale,
                                "Could not update Favorites for {name}. {error}",
                            )
                            .replace("{error}", &failure.message)
                            .replace("{name}", failure.item.name());
                            ui.horizontal(|ui| {
                                if theme::soft_button(
                                    ui,
                                    &app.palette,
                                    Some(Icon::Refresh),
                                    &gettext(app.locale, "Retry"),
                                    false,
                                )
                                .clicked()
                                {
                                    app.actions.push(Action::RetryFavorite(uri.clone()));
                                }
                                let text = crate::bidi::layout(
                                    ui.painter(),
                                    &message,
                                    theme::regular(13.0),
                                    app.palette.text,
                                    ui.available_width().max(1.0),
                                    usize::MAX,
                                    None,
                                );
                                ui.add(egui::Label::new(text).wrap());
                            });
                        });
                    }
                });
        });
}

pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {
    let state = &app.navidrome_favorites;
    let palette = &app.palette;
    let locale = app.locale;
    ui.horizontal(|ui| {
        theme::text(
            ui,
            gettext(locale, "Favorites"),
            theme::bold(28.0),
            palette.text,
        );
        if ui
            .add_enabled_ui(!state.loading, |ui| {
                theme::icon_button(
                    ui,
                    Icon::Refresh,
                    16.0,
                    palette.secondary,
                    palette.text,
                    &gettext(locale, "Refresh"),
                )
            })
            .inner
            .clicked()
        {
            app.actions.push(Action::Reload(Page::LikedSongs));
        }
    });
    if state.loading {
        widgets::loading_row(ui, palette, locale);
    }
    if let Some(error) = &state.error
        && navidrome_search::retry(ui, palette, locale, error)
    {
        app.actions.push(Action::Reload(Page::LikedSongs));
    }
    if !state.ready {
        return;
    }
    for kind in [
        FavoriteKind::Song,
        FavoriteKind::Album,
        FavoriteKind::Artist,
    ] {
        let (section, empty, icon, count) = match kind {
            FavoriteKind::Song => (
                gettext(locale, "Songs"),
                gettext(locale, "Songs you favorite appear here."),
                Icon::Music,
                state.list.song.len(),
            ),
            FavoriteKind::Album => (
                gettext(locale, "Albums"),
                gettext(locale, "Albums you favorite appear here."),
                Icon::Disc,
                state.list.album.len(),
            ),
            FavoriteKind::Artist => (
                gettext(locale, "Artists"),
                gettext(locale, "Artists you favorite appear here."),
                Icon::User,
                state.list.artist.len(),
            ),
        };
        ui.add_space(14.0);
        theme::section_title(ui, palette, &section);
        if count == 0 {
            theme::subtle(ui, palette, &empty);
        }
        widgets::virtual_rows(ui, count, 32.0, |ui, index| {
            let (id, name, by) = match kind {
                FavoriteKind::Song => {
                    let s = &state.list.song[index];
                    (&s.id, &s.title, Some((s.artist.as_str(), &s.credits)))
                }
                FavoriteKind::Album => {
                    let a = &state.list.album[index];
                    (&a.id, &a.name, Some((a.artist.as_str(), &a.credits)))
                }
                FavoriteKind::Artist => {
                    let a = &state.list.artist[index];
                    (&a.id, &a.name, None)
                }
            };
            let uri = kind.uri(id);
            if ui
                .push_id(&uri, |ui| {
                    row(
                        ui,
                        state,
                        &mut app.actions,
                        (palette, locale),
                        Some(&uri),
                        |ui, actions| {
                            navidrome_search::row(ui, actions, palette, icon, name, by, false)
                        },
                    )
                })
                .inner
                .clicked()
            {
                app.actions.push(match kind {
                    FavoriteKind::Song => Action::PlayFavorite(id.clone()),
                    FavoriteKind::Album => Action::Open(Page::Album(encode(id))),
                    FavoriteKind::Artist => Action::Open(Page::Artist(encode(id))),
                });
            }
        });
    }
}
