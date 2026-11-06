//! The Navidrome results shown below the global search field.
use super::widgets;
use crate::app::{
    App,
    navidrome::search::{Action as SearchAction, Hit},
};
use crate::i18n::gettext;
use crate::model::{Action, Loadable};
use crate::navidrome::api::{ArtistCredits, Collection, FavoriteKind, SearchPart, Song};
use crate::theme::{self, Icon, Palette};

pub(super) fn show(app: &mut App, ui: &mut egui::Ui) {
    let locale = app.locale;
    let palette = app.palette;
    // The field is drawn first. Never paint or activate a row from the
    // previous query in the frame in which the edit is queued.
    if app
        .actions
        .iter()
        .any(|action| matches!(action, Action::NavidromeSearch(SearchAction::Edit(_))))
    {
        return;
    }
    if app.search.committed.is_empty() {
        super::search::recent(app, ui);
        return;
    }
    let state = &app.navidrome_search;
    if app.search.typed_at.is_some() || state.catalogue.is_loading() || state.playlists.is_loading()
    {
        widgets::loading_row(ui, &palette, locale);
    }
    if let Loadable::Failed(error) = &state.catalogue {
        let groups = [
            gettext(locale, "Songs"),
            gettext(locale, "Albums"),
            gettext(locale, "Artists"),
        ]
        .join(" · ");
        theme::section_title(ui, &palette, &groups);
        if retry(ui, &palette, locale, error) {
            app.actions
                .push(Action::NavidromeSearch(SearchAction::Retry(
                    SearchPart::Catalogue,
                )));
        }
    }
    let mut previous = None;
    let scroll_id = ui.id().with("keyboard-result");
    let last = ui.data(|data| data.get_temp::<(u64, Option<Hit>)>(scroll_id));
    for (hit, name, by) in state.rows() {
        let (heading, icon) = match hit {
            Hit::Song(_) => (gettext(locale, "Songs"), Icon::Music),
            Hit::Album(_) => (gettext(locale, "Albums"), Icon::Disc),
            Hit::Artist(_) => (gettext(locale, "Artists"), Icon::User),
            Hit::Playlist(_) => (gettext(locale, "Playlists"), Icon::ListMusic),
        };
        if previous != Some(icon) {
            ui.add_space(10.0);
            theme::section_title(ui, &palette, &heading);
            previous = Some(icon);
        }
        let selected = state.selected == Some(hit);
        let uri = state.catalogue.get().and_then(|c| match hit {
            Hit::Song(i) => Some(FavoriteKind::Song.uri(&c.song[i].id)),
            Hit::Album(i) => Some(FavoriteKind::Album.uri(&c.album[i].id)),
            Hit::Artist(i) => Some(FavoriteKind::Artist.uri(&c.artist[i].id)),
            Hit::Playlist(_) => None,
        });
        let response = super::navidrome_favorites::row(
            ui,
            &app.navidrome_favorites,
            &mut app.actions,
            (&palette, locale),
            uri.as_deref(),
            |ui, actions| row(ui, actions, &palette, icon, name, by, selected),
        );
        if selected && last != Some((app.search.serial, state.selected)) {
            response.scroll_to_me(None);
        }
        if response.clicked() {
            app.actions
                .push(Action::NavidromeSearch(SearchAction::Activate(hit)));
        }
    }
    ui.data_mut(|data| data.insert_temp(scroll_id, (app.search.serial, state.selected)));
    if let Loadable::Failed(error) = &state.playlists {
        ui.add_space(10.0);
        theme::section_title(ui, &palette, &gettext(locale, "Playlists"));
        if retry(ui, &palette, locale, error) {
            app.actions
                .push(Action::NavidromeSearch(SearchAction::Retry(
                    SearchPart::Playlists,
                )));
        }
    }
    if state.catalogue.get().is_some() && state.playlists.get().is_some() && previous.is_none() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Search,
            &gettext(locale, "No results for “{query}”").replace("{query}", &app.search.committed),
            &gettext(locale, "Check the spelling, or try fewer words."),
        );
    }
}

pub(super) fn retry(
    ui: &mut egui::Ui,
    palette: &Palette,
    locale: crate::i18n::Locale,
    message: &str,
) -> bool {
    ui.horizontal_wrapped(|ui| {
        theme::subtle(ui, palette, message);
        theme::soft_button(
            ui,
            palette,
            Some(Icon::Refresh),
            &gettext(locale, "Retry"),
            false,
        )
        .clicked()
    })
    .inner
}

pub(super) fn row(
    ui: &mut egui::Ui,
    actions: &mut Vec<Action>,
    palette: &Palette,
    icon: Icon,
    name: &str,
    by: Option<(&str, &ArtistCredits)>,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if selected || response.hovered() {
            ui.painter().rect_filled(rect, 5.0, palette.surface_hover);
        }
        theme::paint_icon(
            ui,
            icon,
            egui::Rect::from_min_size(rect.min, egui::Vec2::splat(32.0)),
            16.0,
            if selected {
                palette.accent
            } else {
                palette.secondary
            },
        );
        let area = egui::Rect::from_min_max(
            rect.min + egui::vec2(36.0, 0.0),
            rect.max - egui::vec2(8.0, 0.0),
        );
        let artists = by
            .map(|(name, credits)| crate::app::navidrome::view_artists(credits, name))
            .unwrap_or_default();
        let mut labels = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        labels.set_clip_rect(area.intersect(ui.clip_rect()));
        labels.spacing_mut().item_spacing.x = 5.0;
        let title_width = if artists.is_empty() {
            area.width()
        } else {
            area.width() * 0.6
        };
        let title = crate::bidi::layout(
            labels.painter(),
            name,
            theme::regular(14.0),
            palette.text,
            title_width.max(1.0),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        let (title_rect, _) = labels.allocate_exact_size(title.size(), egui::Sense::hover());
        labels.painter().galley(
            crate::bidi::galley_pos(title_rect, &title),
            title,
            palette.text,
        );
        if !artists.is_empty() {
            theme::text(&mut labels, "·", theme::regular(14.0), palette.secondary);
            widgets::artist_links(
                &mut labels,
                actions,
                &artists,
                theme::regular(14.0),
                palette.secondary,
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                if artists.is_empty() {
                    name.to_owned()
                } else {
                    format!(
                        "{name} · {}",
                        artists
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
            )
        });
    }
    theme::focus_ring(ui, &response);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn collection(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    match &app.navidrome_search.collection {
        Loadable::Loading | Loadable::NotLoaded => widgets::loading_row(ui, &palette, locale),
        Loadable::Failed(error) => {
            if retry(ui, &palette, locale, error) {
                app.actions.push(Action::Reload(app.page().clone()));
            }
        }
        Loadable::Loaded(Collection::Artist { .. }) => super::navidrome_artist::show(app, ui),
        Loadable::Loaded(Collection::Playlist { name, songs }) => {
            theme::section_title(ui, &palette, name);
            theme::subtle(ui, &palette, &gettext(locale, "Songs"));
            song_rows(
                ui,
                &app.navidrome_favorites,
                &mut app.actions,
                &palette,
                locale,
                songs,
            );
        }
    }
}

fn song_rows(
    ui: &mut egui::Ui,
    favorites: &crate::app::navidrome::favorites::State,
    actions: &mut Vec<Action>,
    palette: &Palette,
    locale: crate::i18n::Locale,
    songs: &[Song],
) {
    if songs.is_empty() {
        theme::subtle(ui, palette, &gettext(locale, "Nothing here yet."));
    }
    for (index, song) in songs.iter().enumerate() {
        if super::navidrome_favorites::row(
            ui,
            favorites,
            actions,
            (palette, locale),
            Some(&FavoriteKind::Song.uri(&song.id)),
            |ui, actions| {
                row(
                    ui,
                    actions,
                    palette,
                    Icon::Music,
                    &song.title,
                    Some((&song.artist, &song.credits)),
                    false,
                )
            },
        )
        .clicked()
        {
            actions.push(Action::NavidromeSearch(SearchAction::PlayEntry(index)));
        }
    }
}
