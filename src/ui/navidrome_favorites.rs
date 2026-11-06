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

pub(super) fn show(app: &mut App, ui: &mut egui::Ui, group: Option<FavoriteKind>) {
    let page = app.page().clone();
    let state = &app.navidrome_favorites;
    let palette = &app.palette;
    let locale = app.locale;
    ui.horizontal(|ui| {
        theme::text(
            ui,
            match group {
                None => gettext(locale, "Favorites"),
                Some(FavoriteKind::Song) => gettext(locale, "Favorite Songs"),
                Some(FavoriteKind::Album) => gettext(locale, "Favorite Albums"),
                Some(FavoriteKind::Artist) => gettext(locale, "Favorite Artists"),
            },
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
            app.actions.push(Action::Reload(page.clone()));
        }
    });
    if state.loading {
        widgets::loading_row(ui, palette, locale);
    }
    if let Some(error) = &state.error
        && navidrome_search::retry(ui, palette, locale, error)
    {
        app.actions.push(Action::Reload(page.clone()));
    }
    if !state.ready {
        return;
    }
    for kind in [
        FavoriteKind::Song,
        FavoriteKind::Album,
        FavoriteKind::Artist,
    ] {
        if group.is_some_and(|selected| selected != kind) {
            continue;
        }
        let (section, empty, count) = match kind {
            FavoriteKind::Song => (
                gettext(locale, "Songs"),
                gettext(locale, "Songs you favorite appear here."),
                app.navidrome_favorites.list.song.len(),
            ),
            FavoriteKind::Album => (
                gettext(locale, "Albums"),
                gettext(locale, "Albums you favorite appear here."),
                app.navidrome_favorites.list.album.len(),
            ),
            FavoriteKind::Artist => (
                gettext(locale, "Artists"),
                gettext(locale, "Artists you favorite appear here."),
                app.navidrome_favorites.list.artist.len(),
            ),
        };
        ui.add_space(14.0);
        ui.push_id(kind, |ui| {
            let count = if group.is_none() {
                ui.horizontal(|ui| {
                    theme::section_title(ui, &app.palette, &section);
                    if count > 0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::link(
                                ui,
                                gettext(locale, "See all"),
                                theme::semibold(13.0),
                                app.palette.secondary,
                            )
                            .clicked()
                            {
                                app.actions.push(Action::Open(Page::FavoriteGroup(kind)));
                            }
                        });
                    }
                });
                count.min(2 * widgets::card_columns(ui.available_width()))
            } else {
                count
            };
            if count == 0 {
                theme::subtle(ui, &app.palette, &empty);
                return;
            }
            let height = widgets::card_row_height(ui);
            widgets::virtual_wrapped_cards(ui, count, height, |ui, index| {
                if let Some(action) = favorite_card(ui, app, kind, index) {
                    app.actions.push(action);
                }
            });
        });
    }
}

fn favorite_card(ui: &mut egui::Ui, app: &App, kind: FavoriteKind, index: usize) -> Option<Action> {
    let list = &app.navidrome_favorites.list;
    let (id, name, cover, credits) = match kind {
        FavoriteKind::Song => {
            let s = &list.song[index];
            (&s.id, &s.title, &s.cover_art, Some((&s.artist, &s.credits)))
        }
        FavoriteKind::Album => {
            let a = &list.album[index];
            (&a.id, &a.name, &a.cover_art, Some((&a.artist, &a.credits)))
        }
        FavoriteKind::Artist => {
            let a = &list.artist[index];
            (&a.id, &a.name, &a.cover_art, None)
        }
    };
    let cover = app.navidrome_search.artwork_uri(cover.as_deref());
    let artist = kind == FavoriteKind::Artist;
    let subtitle = if artist {
        gettext(app.locale, "Artist")
    } else {
        "".into()
    };
    ui.push_id(id, |ui| {
        let card = widgets::card(ui, app, cover.as_deref(), name, &subtitle, artist, !artist);
        if ui.is_rect_visible(card.response.rect) {
            let rect = egui::Rect::from_center_size(
                card.image_rect.right_top() + egui::vec2(-17.0, 17.0),
                egui::Vec2::splat(26.0),
            );
            // The hover-only play button can change automatic IDs between press and release.
            let mut button = ui.new_child(
                egui::UiBuilder::new()
                    .id(card.response.id.with("star"))
                    .max_rect(rect)
                    .layout(egui::Layout::centered_and_justified(
                        egui::Direction::LeftToRight,
                    )),
            );
            if theme::circle_button(
                &mut button,
                Icon::StarFilled,
                26.0,
                app.palette.panel,
                app.palette.surface_hover,
                app.palette.accent,
                &gettext(app.locale, "Remove from Favorites"),
            )
            .clicked()
            {
                return Some(Action::ToggleSaved(kind.uri(id)));
            }
            if let Some((name, credits)) = credits {
                let mut labels = ui.new_child(
                    egui::UiBuilder::new()
                        .id(card.response.id.with("credits"))
                        .max_rect(card.subtitle_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true)),
                );
                labels.set_clip_rect(card.subtitle_rect.intersect(ui.clip_rect()));
                let mut actions = Vec::new();
                widgets::artist_links(
                    &mut labels,
                    &mut actions,
                    &crate::app::navidrome::view_artists(credits, name),
                    theme::regular(12.5),
                    app.palette.secondary,
                );
                if let Some(action) = actions.pop() {
                    return Some(action);
                }
            }
        }
        if card.play && kind == FavoriteKind::Album {
            return Some(Action::PlayContext {
                uri: kind.uri(id),
                offset_index: None,
                offset_uri: None,
            });
        }
        (card.clicked || card.play).then(|| match kind {
            FavoriteKind::Song => Action::PlayFavorite(id.clone()),
            FavoriteKind::Album => Action::Open(Page::Album(encode(id))),
            FavoriteKind::Artist => Action::Open(Page::Artist(encode(id))),
        })
    })
    .inner
}
