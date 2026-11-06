//! The Navidrome results shown below the global search field.
use super::widgets;
use crate::app::{
    App,
    navidrome::search::{Action as SearchAction, Hit},
};
use crate::i18n::gettext;
use crate::model::{Action, Loadable, Page};
use crate::navidrome::api::{Collection, SearchPart};
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
        let response = row(ui, &palette, icon, name, by, selected);
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

fn retry(ui: &mut egui::Ui, palette: &Palette, locale: crate::i18n::Locale, message: &str) -> bool {
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

fn row(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    name: &str,
    by: &str,
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
        let text = if by.is_empty() {
            name.to_string()
        } else {
            format!("{name}  ·  {by}")
        };
        let area = egui::Rect::from_min_max(
            rect.min + egui::vec2(36.0, 0.0),
            rect.max - egui::vec2(8.0, 0.0),
        );
        let galley = crate::bidi::layout(
            ui.painter(),
            &text,
            theme::regular(14.0),
            palette.text,
            area.width().max(1.0),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        let pos = crate::bidi::galley_pos(area, &galley)
            + egui::vec2(0.0, (area.height() - galley.size().y) / 2.0);
        ui.painter().galley(pos, galley, palette.text);
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                &text,
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
        Loadable::Loaded(Collection::Artist { name, albums }) => {
            theme::section_title(ui, &palette, name);
            theme::subtle(ui, &palette, &gettext(locale, "Albums"));
            if albums.is_empty() {
                theme::subtle(ui, &palette, &gettext(locale, "No albums"));
            }
            for album in albums {
                if row(ui, &palette, Icon::Disc, &album.name, &album.artist, false).clicked() {
                    app.actions
                        .push(Action::Open(Page::Album(crate::app::navidrome::encode(
                            &album.id,
                        ))));
                }
            }
        }
        Loadable::Loaded(Collection::Playlist { name, songs }) => {
            theme::section_title(ui, &palette, name);
            theme::subtle(ui, &palette, &gettext(locale, "Songs"));
            if songs.is_empty() {
                theme::subtle(ui, &palette, &gettext(locale, "Nothing here yet."));
            }
            for (index, song) in songs.iter().enumerate() {
                if row(ui, &palette, Icon::Music, &song.title, &song.artist, false).clicked() {
                    app.actions
                        .push(Action::NavidromeSearch(SearchAction::PlayEntry(index)));
                }
            }
        }
    }
}
