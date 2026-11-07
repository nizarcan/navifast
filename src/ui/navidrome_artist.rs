//! An artist's releases, biography, library connections and guest appearances.
use egui::{Align, Color32, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use super::widgets;
use crate::app::{
    App,
    navidrome::{encode, search::State},
};
use crate::i18n::{Locale, gettext, ngettext, pgettext};
use crate::model::{Action, Loadable, Page};
use crate::navidrome::api::{Album, Collection, FavoriteKind};
use crate::theme::{self, Icon, Palette};

/// Opacity of the title background across the artist artwork, from its left edge.
pub(crate) const PORTRAIT_STOPS: [(f32, u8); 4] = [(0., 255), (0.29, 226), (0.73, 30), (1., 0)];

pub(super) fn show(app: &mut App, ui: &mut Ui) {
    let artist_page = app.page().clone();
    let state = &app.navidrome_search;
    let Some(Collection::Artist { name, albums, .. }) = state.collection.get() else {
        return;
    };
    let tint = state
        .artist_cover
        .as_ref()
        .filter(|_| app.settings.accent_from_art)
        .and_then(|uri| app.accents.get(uri))
        .copied();
    let mut page = ArtistPage {
        palette: app.palette,
        locale: app.locale,
        state,
        favorites: &app.navidrome_favorites,
        artist: &artist_page,
        actions: &mut app.actions,
    };
    let width = ui.available_width();
    let gutter = (width * 0.033)
        .clamp(12.0, 48.0)
        .max((width - 1380.0) * 0.5);
    let body_width = (width - 2.0 * gutter).max(1.0);
    ui.push_id(&artist_page, |ui| {
        page.hero(ui, name, albums, gutter, tint);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(gutter);
            ui.vertical(|ui| {
                ui.set_width(body_width);
                ui.spacing_mut().item_spacing = vec2(10.0, 4.0);
                ui.add_space(20.0);
                page.discography(ui, albums);
                ui.add_space(22.0);
                page.heading(ui, &gettext(page.locale, "About"));
                if let Some(bio) = state
                    .artist_info
                    .get()
                    .and_then(|i| i.biography.as_deref())
                    .filter(|s| !s.is_empty())
                {
                    page.about(ui, bio);
                } else {
                    page.section_status(
                        ui,
                        &state.artist_info,
                        &gettext(page.locale, "No biography available."),
                    );
                }
                ui.add_space(24.0);
                page.related(ui);
                ui.add_space(24.0);
                page.appearances(ui);
            });
        });
    });
}

struct ArtistPage<'a> {
    palette: Palette,
    locale: Locale,
    state: &'a State,
    favorites: &'a crate::app::navidrome::favorites::State,
    artist: &'a Page,
    actions: &'a mut Vec<Action>,
}

impl ArtistPage<'_> {
    fn heading(&self, ui: &mut Ui, label: &str) {
        theme::text(ui, label, theme::bold(26.0), self.palette.text);
    }

    fn hero(
        &mut self,
        ui: &mut Ui,
        name: &str,
        albums: &[Album],
        gutter: f32,
        tint: Option<Color32>,
    ) {
        let width = ui.available_width();
        let text_width = ((width - 2.0 * gutter) * 0.70).max(1.0);
        let title = widgets::ellipsized(
            ui,
            name,
            theme::bold((width * 0.065).clamp(38.0, 84.0)),
            Color32::WHITE,
            text_width,
            2,
        );
        let meta = release_summary(self.locale, albums);
        let content_height = title.size().y + 50.0;
        let height = (width * 0.24)
            .clamp(216.0, 324.0)
            .max(content_height + 31.0);
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let tint = tint.unwrap_or(Color32::from_rgb(111, 81, 128));
        let ink = super::blend(Color32::from_rgb(12, 13, 28), tint, 0.16);
        let plum = super::blend(Color32::from_rgb(35, 26, 39), tint, 0.56);
        widgets::paint_vertical_gradient(ui, rect, plum, ink);

        let art_width = (width * 0.63).min(height * 2.35);
        let photo = Rect::from_min_max(pos2(rect.right() - art_width, rect.top()), rect.max);
        if !self.state.artist_cover.as_deref().is_some_and(|uri| {
            widgets::paint_cover_url(ui, uri, photo, egui::CornerRadius::ZERO, None, false)
        }) {
            // A record on the turntable also works for a completely unillustrated library.
            let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
            let centre = pos2(rect.right() - art_width * 0.40, rect.center().y);
            let r = height * 0.72;
            painter.circle_filled(centre, r, Color32::from_rgb(40, 34, 58));
            for i in 0..30 {
                painter.circle_stroke(
                    centre,
                    r * (0.38 + i as f32 * 0.021),
                    (1.0, Color32::from_white_alpha(27)),
                );
            }
            let label = Rect::from_center_size(centre, egui::Vec2::splat(r * 0.70));
            if let Some(cover) = albums
                .iter()
                .find_map(|a| self.state.artwork_uri(a.cover_art.as_deref()))
            {
                widgets::paint_cover(
                    ui,
                    &self.palette,
                    Some(&cover),
                    label,
                    label.width() / 2.0,
                    Icon::Disc,
                    None,
                );
            } else {
                painter.circle_filled(centre, label.width() / 2.0, plum);
            }
            painter.circle_filled(centre, 3.5, ink);
        }

        // Leave the photograph clear at its far edge and give the title a solid reading area.
        let mut mesh = egui::Mesh::default();
        let start = photo.left();
        for (x, opacity) in std::iter::once((rect.left(), 255))
            .chain(PORTRAIT_STOPS.map(|(t, a)| (start + art_width * t, a)))
        {
            for (y, c) in [(rect.top(), plum), (rect.bottom(), ink)] {
                mesh.colored_vertex(
                    pos2(x, y),
                    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), opacity),
                );
            }
        }
        for i in [0, 2, 4, 6] {
            mesh.add_triangle(i, i + 2, i + 1);
            mesh.add_triangle(i + 1, i + 2, i + 3);
        }
        ui.painter().add(egui::Shape::mesh(mesh));

        let left = rect.left() + gutter;
        let top = rect.bottom() - content_height - 23.0;
        let area = Rect::from_min_max(
            pos2(left, top.max(rect.top() + 8.0)),
            pos2(left + text_width, rect.bottom() - 8.0),
        );
        let mut copy = ui.new_child(
            UiBuilder::new()
                .max_rect(area)
                .layout(Layout::top_down(Align::Min)),
        );
        copy.set_clip_rect(area.intersect(ui.clip_rect()));
        copy.spacing_mut().item_spacing.y = 2.0;
        theme::text(
            &mut copy,
            gettext(self.locale, "Artist"),
            theme::semibold(14.0),
            Color32::from_gray(236),
        );
        copy.add(egui::Label::new(title).selectable(false))
            .on_hover_text(name);
        copy.horizontal(|ui| {
            theme::text(ui, meta, theme::medium(14.0), Color32::from_gray(235));
            if let Page::Artist(id) = self.artist {
                let uri = format!("navidrome:artist:{id}");
                if super::navidrome_favorites::toggle(
                    ui,
                    self.favorites,
                    (&self.palette, self.locale),
                    &uri,
                    18.0,
                ) {
                    self.actions.push(Action::ToggleSaved(uri));
                }
            }
        });
    }

    fn discography(&mut self, ui: &mut Ui, albums: &[Album]) {
        let (cols, side) = grid_size(ui.available_width(), 152.0);
        let all_id = ui.make_persistent_id("all-releases");
        let filter_id = ui.make_persistent_id("release-filter");
        let mut filter = ui
            .data(|d| d.get_temp::<ReleaseGroup>(filter_id))
            .unwrap_or_default();
        let has = |group| albums.iter().any(|a| release_group(a) == group);
        let filters = has(ReleaseGroup::Album) && has(ReleaseGroup::Short);
        if !filters {
            filter = ReleaseGroup::All;
        }
        let matched = |a: &&Album| filter == ReleaseGroup::All || release_group(a) == filter;
        let mut limit = self.shelf_heading(
            ui,
            &gettext(self.locale, "Discography"),
            "all-releases",
            albums.iter().filter(matched).count(),
            cols,
        );
        if albums.is_empty() {
            theme::subtle(ui, &self.palette, &gettext(self.locale, "No albums"));
            return;
        }
        if filters {
            ui.add_space(2.0);
            let (any, lp, short) = (
                pgettext(self.locale, "filter", "All"),
                gettext(self.locale, "Albums"),
                gettext(self.locale, "Singles & EPs"),
            );
            if let Some(next) = widgets::chips(
                ui,
                &self.palette,
                &[
                    (ReleaseGroup::All, &any),
                    (ReleaseGroup::Album, &lp),
                    (ReleaseGroup::Short, &short),
                ],
                filter,
            ) {
                filter = next;
                limit = cols;
                ui.data_mut(|d| {
                    d.insert_temp(filter_id, next);
                    d.insert_temp(all_id, false);
                });
            }
        }
        ui.add_space(6.0);
        let releases: Vec<_> = albums
            .iter()
            .filter(|a| filter == ReleaseGroup::All || release_group(a) == filter)
            .take(limit)
            .collect();
        for row in releases.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = TILE_GAP;
                for album in row {
                    self.album_tile(ui, album, side, false);
                }
            });
        }
    }

    fn album_tile(&mut self, ui: &mut Ui, album: &Album, side: f32, appearance: bool) {
        let uri = self.state.artwork_uri(album.cover_art.as_deref());
        let kind = album
            .release_types
            .first()
            .map(String::as_str)
            .unwrap_or_default();
        let kind = match kind.to_ascii_lowercase().as_str() {
            "album" => gettext(self.locale, "Album").into_owned(),
            "single" => gettext(self.locale, "Single").into_owned(),
            "ep" => gettext(self.locale, "EP").into_owned(),
            _ => kind.to_owned(),
        };
        let mut detail = Vec::new();
        if appearance && !album.artist.is_empty() {
            detail.push(album.artist.clone());
        }
        if album.year > 0 {
            detail.push(album.year.to_string());
        }
        if !appearance && !kind.is_empty() {
            detail.push(kind);
        }
        if detail.is_empty() && album.song_count > 0 {
            detail.push(self.locale.song_count(album.song_count));
        }
        let response = tile(
            ui,
            &self.palette,
            side,
            &album.name,
            &detail.join("  ·  "),
            uri.as_deref(),
            false,
        );
        self.favorite_menu(ui, &response, FavoriteKind::Album.uri(&album.id));
        if response.clicked() {
            self.actions
                .push(Action::Open(Page::Album(encode(&album.id))));
        }
    }

    fn favorite_menu(&mut self, ui: &mut Ui, response: &egui::Response, uri: String) {
        egui::Popup::context_menu(response)
            .id(ui.make_persistent_id(("favorite", &uri)))
            .frame(widgets::menu_frame(&self.palette))
            .show(|ui| {
                if super::navidrome_favorites::menu(
                    ui,
                    self.favorites,
                    (&self.palette, self.locale),
                    &uri,
                ) {
                    self.actions.push(Action::ToggleSaved(uri));
                }
            });
    }

    fn section_status<T>(&mut self, ui: &mut Ui, data: &Loadable<T>, empty: &str) {
        match data {
            Loadable::Loading => widgets::loading_row(ui, &self.palette, self.locale),
            Loadable::Failed(error) => {
                if super::navidrome_search::retry(ui, &self.palette, self.locale, error) {
                    self.actions.push(Action::Reload(self.artist.clone()));
                }
            }
            _ => {
                theme::subtle(ui, &self.palette, empty);
            }
        }
    }

    fn shelf_heading(
        &self,
        ui: &mut Ui,
        label: &str,
        key: &str,
        count: usize,
        columns: usize,
    ) -> usize {
        let id = ui.make_persistent_id(key);
        let mut expanded = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or_default();
        ui.horizontal(|ui| {
            self.heading(ui, label);
            if count > columns {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if more_link(ui, &self.palette, self.locale, expanded) {
                        expanded = !expanded;
                        ui.data_mut(|d| d.insert_temp(id, expanded));
                    }
                });
            }
        });
        if expanded { count } else { count.min(columns) }
    }

    fn appearances(&mut self, ui: &mut Ui) {
        let (cols, side) = grid_size(ui.available_width(), 152.0);
        let albums = self
            .state
            .appearances
            .get()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let count = self.shelf_heading(
            ui,
            &gettext(self.locale, "Appears on"),
            "all-appearances",
            albums.len(),
            cols,
        );
        if albums.is_empty() {
            self.section_status(
                ui,
                &self.state.appearances,
                &gettext(self.locale, "No featured releases in your library."),
            );
            return;
        }
        ui.add_space(8.0);
        for row in albums[..count].chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = TILE_GAP;
                for album in row {
                    self.album_tile(ui, album, side, true);
                }
            });
        }
    }

    fn about(&self, ui: &mut Ui, bio: &str) {
        ui.add_space(8.0);
        let width = ui.available_width().min(980.0);
        let portrait = self.state.artist_cover.as_deref();
        egui::Frame::new().fill(self.palette.panel).show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if width >= 660.0
                && let Some(uri) = portrait
            {
                ui.horizontal_top(|ui| {
                    let photo_width = (width * 0.37).round();
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(photo_width, 174.0), Sense::hover());
                    widgets::paint_cover(ui, &self.palette, Some(uri), rect, 0.0, Icon::User, None);
                    self.biography(ui, bio, width - photo_width);
                });
            } else {
                if let Some(uri) = portrait {
                    let (rect, _) = ui.allocate_exact_size(
                        vec2(width, (width * 0.32).clamp(85.0, 167.0)),
                        Sense::hover(),
                    );
                    widgets::paint_cover(ui, &self.palette, Some(uri), rect, 0.0, Icon::User, None);
                }
                self.biography(ui, bio, width);
            }
        });
    }

    fn biography(&self, ui: &mut Ui, bio: &str, width: f32) {
        let id = ui.make_persistent_id("biography");
        let all = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or_default();
        egui::Frame::new().inner_margin(18).show(ui, |ui| {
            ui.set_width((width - 36.0).max(1.0));
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                let galley = widgets::ellipsized(
                    ui,
                    bio,
                    theme::regular(16.0),
                    self.palette.text,
                    ui.available_width(),
                    if all { usize::MAX } else { 5 },
                );
                let truncated = galley.elided || galley.job.text.ends_with(crate::bidi::ELLIPSIS);
                ui.add(egui::Label::new(galley));
                if truncated || all {
                    ui.add_space(8.0);
                    if more_link(ui, &self.palette, self.locale, all) {
                        ui.data_mut(|d| d.insert_temp(id, !all));
                    }
                }
            });
        });
    }

    fn related(&mut self, ui: &mut Ui) {
        let artists = self
            .state
            .artist_info
            .get()
            .map(|info| info.similar_artist.as_slice())
            .unwrap_or_default();
        let (cols, side) = grid_size(ui.available_width(), 128.0);
        let count = self.shelf_heading(
            ui,
            &gettext(self.locale, "Similar artists"),
            "all-similar",
            artists.len(),
            cols,
        );
        if artists.is_empty() {
            self.section_status(
                ui,
                &self.state.artist_info,
                &gettext(self.locale, "No similar artists in your library."),
            );
            return;
        }
        ui.add_space(8.0);
        for row in artists[..count].chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = TILE_GAP;
                for artist in row {
                    let Some(id) = &artist.id else { continue };
                    let uri = self.state.artwork_uri(artist.cover_art.as_deref());
                    let response = tile(
                        ui,
                        &self.palette,
                        side,
                        &artist.name,
                        "",
                        uri.as_deref(),
                        true,
                    );
                    self.favorite_menu(ui, &response, FavoriteKind::Artist.uri(id));
                    if response.clicked() {
                        self.actions.push(Action::Open(Page::Artist(encode(id))));
                    }
                }
            });
        }
    }
}

const TILE_GAP: f32 = 14.0;

pub(super) fn grid_size(width: f32, target: f32) -> (usize, f32) {
    let cols = ((width + TILE_GAP) / (target + TILE_GAP)).round().max(1.0) as usize;
    (
        cols,
        ((width - (cols - 1) as f32 * TILE_GAP) / cols as f32).clamp(1.0, target * 1.28),
    )
}

fn tile(
    ui: &mut Ui,
    p: &Palette,
    side: f32,
    name: &str,
    detail: &str,
    uri: Option<&str>,
    portrait: bool,
) -> egui::Response {
    let (r, response) = ui.allocate_exact_size(
        vec2(side, side + if portrait { 48.0 } else { 66.0 }),
        Sense::click(),
    );
    if ui.is_rect_visible(r) {
        let art = Rect::from_min_size(r.min, egui::Vec2::splat(side));
        let radius = if portrait { (side * 0.5) as u8 } else { 5 };
        if !uri
            .is_some_and(|uri| widgets::paint_cover_url(ui, uri, art, radius.into(), None, false))
        {
            // A monogram keeps even a whole row of missing sleeves distinguishable.
            let tones = [
                Color32::from_rgb(66, 75, 101),
                Color32::from_rgb(96, 64, 77),
                Color32::from_rgb(46, 84, 78),
                Color32::from_rgb(84, 68, 99),
                Color32::from_rgb(94, 78, 58),
            ];
            let tone = tones[egui::Id::new(name).value() as usize % tones.len()];
            ui.painter().rect_filled(art, radius, tone);
            let initials: String = name
                .split_whitespace()
                .filter_map(|word| word.chars().next())
                .take(2)
                .collect();
            let text = crate::bidi::layout_line(
                ui.painter(),
                initials,
                theme::bold(side * 0.30),
                Color32::from_white_alpha(188),
            );
            ui.painter()
                .galley(art.center() - text.size() * 0.5, text, Color32::WHITE);
        }
        if response.hovered() {
            ui.painter().rect_stroke(
                art,
                if portrait { (side / 2.0) as u8 } else { 5 },
                (2.0, p.accent),
                egui::StrokeKind::Inside,
            );
        }
        let title = widgets::ellipsized(ui, name, theme::semibold(15.0), p.text, side, 2);
        let meta_offset = title.size().y + 2.0;
        let label = Rect::from_min_max(pos2(r.left(), art.bottom() + 8.0), r.max);
        ui.painter()
            .galley(crate::bidi::galley_pos(label, &title), title, p.text);
        if !detail.is_empty() {
            let meta = widgets::ellipsized(ui, detail, theme::regular(12.5), p.secondary, side, 1);
            ui.painter().galley(
                crate::bidi::galley_pos(label.translate(vec2(0.0, meta_offset)), &meta),
                meta,
                p.secondary,
            );
        }
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    theme::focus_ring(ui, &response);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(name)
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) enum ReleaseGroup {
    #[default]
    All,
    Album,
    Short,
}

pub(super) fn release_group(album: &Album) -> ReleaseGroup {
    if album
        .release_types
        .iter()
        .any(|s| s.eq_ignore_ascii_case("single") || s.eq_ignore_ascii_case("ep"))
    {
        ReleaseGroup::Short
    } else if album
        .release_types
        .iter()
        .any(|s| s.eq_ignore_ascii_case("album"))
    {
        ReleaseGroup::Album
    } else {
        ReleaseGroup::All
    }
}

pub(super) fn release_summary(locale: Locale, albums: &[Album]) -> String {
    let count = albums.len() as u32;
    let mut text = ngettext(locale, "{count} release", "{count} releases", count)
        .replace("{count}", &count.to_string());
    let mut years = albums.iter().map(|a| a.year).filter(|y| *y > 0);
    if let Some(first) = years.next() {
        let (lo, hi) = years.fold((first, first), |(lo, hi), y| (lo.min(y), hi.max(y)));
        text.push_str(&format!("   ·   {lo}"));
        if hi > lo {
            text.push_str(&format!("–{hi}"));
        }
    }
    text
}

fn more_link(ui: &mut Ui, p: &Palette, locale: Locale, expanded: bool) -> bool {
    let label = if expanded {
        gettext(locale, "Show less")
    } else {
        gettext(locale, "See more")
    };
    theme::link(ui, label, theme::semibold(13.0), p.secondary).clicked()
}
