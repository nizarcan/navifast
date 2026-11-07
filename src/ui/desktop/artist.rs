use super::*;
use crate::model::Loadable;
use crate::navidrome::api::{Album, FavoriteKind};
use crate::ui::navidrome_artist::{
    PORTRAIT_STOPS, ReleaseGroup, grid_size, release_group, release_summary,
};

#[derive(Default, Clone, Copy)]
pub(crate) struct Options {
    group: ReleaseGroup,
    releases: bool,
    biography: bool,
    similar: bool,
    features: bool,
}

impl State {
    fn artist_options(&mut self) -> &mut Options {
        self.artist_views
            .entry(self.app.presentation.page().clone())
            .or_default()
    }
}

fn shelf(
    v: &Paint<'_>,
    name: &'static str,
    count: usize,
    cols: usize,
    expanded: bool,
    field: fn(&mut Options) -> &mut bool,
) -> El {
    let mut head = row()
        .w_full()
        .items_center()
        .child(label(v.tr(name), 26., 700., v.p.text).grow(1.));
    if count > cols {
        head = head.child(
            v.button(
                &v.tr(if expanded { "Show less" } else { "See more" }),
                false,
                move |s, _| {
                    let open = field(s.artist_options());
                    *open = !*open;
                },
            )
            .fill(Color::TRANSPARENT),
        );
    }
    head
}

fn status<T>(v: &Paint<'_>, data: &Loadable<T>, empty: &'static str) -> El {
    match data {
        Loadable::Loading => v.loading(),
        Loadable::Failed(error) => row()
            .gap(8.)
            .items_center()
            .child(label(error, 14., 400., v.p.secondary).wrap())
            .child(v.button(&v.tr("Retry"), false, |s, tx| {
                s.dispatch(Action::Reload(s.app.presentation.page().clone()), tx)
            })),
        _ => label(v.tr(empty), 14., 400., v.p.secondary),
    }
}

fn tile(v: &Paint<'_>, album: &Album, side: f32, appearance: bool) -> El {
    let mut meta = Vec::new();
    if appearance && !album.artist.is_empty() {
        meta.push(album.artist.clone());
    }
    if album.year > 0 {
        meta.push(album.year.to_string());
    }
    if !appearance && let Some(kind) = album.release_types.first() {
        meta.push(match kind.to_lowercase().as_str() {
            "album" => v.tr("Album"),
            "single" => v.tr("Single"),
            "ep" => v.tr("EP"),
            _ => kind.to_owned(),
        });
    }
    if meta.is_empty() && album.song_count > 0 {
        meta.push(v.s.app.presentation.locale.song_count(album.song_count));
    }
    let id = navidrome::encode(&album.id);
    let tile = col()
        .w(side)
        .gap(3.)
        .role("button")
        .label(&album.name)
        .focusable(true)
        .child(v.cover(album.cover_art.as_deref(), CoverSize::Sleeve, side, 5.))
        .child(label(&album.name, 15., 600., v.p.text).mt(5.).max_lines(2))
        .child(label(meta.join("  ·  "), 12.5, 400., v.p.secondary).truncate())
        .on_press_tx(move |s: &mut State, tx| {
            s.dispatch(Action::Open(Page::Album(id.clone())), tx)
        });
    library::favorite_context(tile, FavoriteKind::Album, &album.id)
}

/// A small reusable texture for the reference's vertex-colored mesh. Only the
/// artist tint changes its pixels. The photograph remains a separate cached image.
#[derive(Default)]
pub(crate) struct PortraitFade {
    colors: Option<(Color32, Color32)>,
    pixels: rune::Pixels,
}

impl PortraitFade {
    fn texture(&mut self, top: Color32, bottom: Color32) -> &rune::Pixels {
        if self.colors != Some((top, bottom)) {
            let (width, height) = (256, 64);
            let mut rgba = Vec::with_capacity(width * height * 4);
            for y in 0..height {
                for x in 0..width {
                    let u = x as f32 / (width - 1) as f32;
                    let v = y as f32 / (height - 1) as f32;
                    rgba.extend(portrait_shade(top, bottom, u, v).to_srgba_unmultiplied());
                }
            }
            self.pixels.set(width as u32, height as u32, rgba);
            self.colors = Some((top, bottom));
        }
        &self.pixels
    }
}

fn portrait_shade(top: Color32, bottom: Color32, u: f32, v: f32) -> Color32 {
    let band = PORTRAIT_STOPS.partition_point(|&(x, _)| x < u).clamp(1, 3);
    let (left, a) = PORTRAIT_STOPS[band - 1];
    let (right, b) = PORTRAIT_STOPS[band];
    let x = (u - left) / (right - left);
    // Interpolate premultiplied sRGB in the same two triangles as the reference.
    let (corner, weight, other, across, down) = if x + v <= 1. {
        (
            top.gamma_multiply_u8(a),
            1. - x - v,
            top.gamma_multiply_u8(b),
            x,
            v,
        )
    } else {
        (
            bottom.gamma_multiply_u8(b),
            x + v - 1.,
            top.gamma_multiply_u8(b),
            1. - v,
            1. - x,
        )
    };
    let lower_left = bottom.gamma_multiply_u8(a);
    let [r, g, b, a] = std::array::from_fn(|i| {
        (f32::from(corner[i]) * weight
            + f32::from(other[i]) * across
            + f32::from(lower_left[i]) * down)
            .round() as u8
    });
    Color32::from_rgba_premultiplied(r, g, b, a)
}

fn record(v: &Paint<'_>, albums: &[Album], radius: f32, plum: Color32, ink: Color32) -> El {
    let mut record = el()
        .size(radius * 2., radius * 2.)
        .center()
        .radius(radius)
        .fill(color(Color32::from_rgb(40, 34, 58)));
    for i in 0..30 {
        let r = radius * (0.38 + i as f32 * 0.021);
        record = record.child(
            el().absolute(radius - r, radius - r)
                .size(2. * r, 2. * r)
                .radius(r)
                .border(1., Color::WHITE.alpha(27. / 255.)),
        );
    }
    let label = match albums.iter().find_map(|a| a.cover_art.as_deref()) {
        Some(id) => v.cover(Some(id), CoverSize::Sleeve, radius * 0.70, radius * 0.35),
        None => el()
            .size(radius * 0.70, radius * 0.70)
            .radius(radius * 0.35)
            .fill(color(plum)),
    };
    record.child(label).child(
        el().absolute(radius - 3.5, radius - 3.5)
            .size(7., 7.)
            .radius(3.5)
            .fill(color(ink)),
    )
}

fn hero(
    v: &Paint<'_>,
    name: &str,
    albums: &[Album],
    portrait: Option<&str>,
    w: f32,
    gutter: f32,
) -> El {
    let text_width = ((w - 2. * gutter) * 0.70).max(1.);
    let title_size = (w * 0.065).clamp(38., 84.);
    // The reference uses Inter's default optical cut even at display sizes.
    let title_style = TextStyle::new(title_size, 700.)
        .optical_sizing(rune::OpticalSize::FontDefault)
        .lh(title_size * 1.21);
    let (_, measured) = rune::text::measure_with(
        name,
        &[],
        title_style,
        Some(text_width),
        rune::text::RichLayoutOptions::new().wrap_anywhere(),
    );
    let title_height = measured.min(2. * title_style.line_px());
    let height = (w * 0.24).clamp(216., 324.).max(title_height + 81.);
    let p = &v.s.app.presentation;
    let tint = p
        .navidrome_search
        .artist_cover
        .as_ref()
        .filter(|_| p.settings.accent_from_art)
        .and_then(|uri| v.s.cover_tint(uri, v.p))
        .unwrap_or(Color32::from_rgb(111, 81, 128));
    let ink = crate::ui::blend(Color32::from_rgb(12, 13, 28), tint, 0.16);
    let plum = crate::ui::blend(Color32::from_rgb(35, 26, 39), tint, 0.56);
    let mut hero = el()
        .w_full()
        .h(height)
        .clip_soft(0.)
        .gradient(color(plum), color(ink));
    let art = (w * 0.63).min(height * 2.35);
    // Clip the fallback to the same artwork area as the photo. Keep a missing
    // portrait's visibility callback mounted so its fetch and retry still run.
    let mut picture = el().absolute(w - art, 0.).size(art, height).clip_soft(0.);
    let loaded = portrait.is_some_and(|id| v.s.cover_image(id, CoverSize::Portrait).1.is_some());
    if !loaded {
        let r = height * 0.72;
        picture = picture
            .child(record(v, albums, r, plum, ink).absolute(art * 0.60 - r, height / 2. - r));
    }
    if let Some(id) = portrait {
        picture = picture.child(
            v.cover(Some(id), CoverSize::Portrait, art, 0.)
                .absolute(0., 0.)
                .size(art, height)
                .opacity(if loaded { 1. } else { 0. }),
        );
    }
    let mut fade = v.s.portrait_fade.borrow_mut();
    hero = hero.child(
        picture.child(
            el().absolute(0., 0.)
                .size(art, height)
                .pixels(fade.texture(plum, ink))
                .a11y_hidden(),
        ),
    );
    let mut meta = row().items_center().gap(8.).child(label(
        release_summary(v.s.app.presentation.locale, albums),
        14.,
        500.,
        Color32::from_gray(235),
    ));
    if let Page::Artist(id) = v.s.app.presentation.page()
        && let Some(id) = navidrome::decode(id)
    {
        meta = meta.child(library::star(v, FavoriteKind::Artist, &id, 18.));
    }
    hero.child(
        col()
            .absolute(gutter, (height - title_height - 73.).max(8.))
            .w(text_width)
            .gap(2.)
            .child(label(v.tr("Artist"), 14., 600., Color32::from_gray(236)).h(17.))
            .child(
                text(name.to_owned())
                    .font(title_style)
                    .color(Color::WHITE)
                    .wrap_anywhere()
                    .max_lines(2),
            )
            .child(meta),
    )
}

pub(super) fn show(v: &Paint<'_>, name: &str, albums: &[Album], portrait: Option<&str>) -> El {
    let page = v.s.app.presentation.page();
    let options = v.s.artist_views.get(page).copied().unwrap_or_default();
    let state = &v.s.app.presentation.navidrome_search;
    let w = v.content_width() + 48.;
    let gutter = (w * 0.033).clamp(12., 48.).max((w - 1380.) / 2.);
    let body_width = w - 2. * gutter;
    let (cols, side) = grid_size(body_width, 152.);
    let filters = albums
        .iter()
        .any(|a| release_group(a) == ReleaseGroup::Album)
        && albums
            .iter()
            .any(|a| release_group(a) == ReleaseGroup::Short);
    let releases: Vec<_> = albums
        .iter()
        .filter(|a| {
            !filters || options.group == ReleaseGroup::All || release_group(a) == options.group
        })
        .collect();
    let mut body = col().w_full().px(gutter).pt(20.).gap(8.).child(shelf(
        v,
        "Discography",
        releases.len(),
        cols,
        options.releases,
        |o| &mut o.releases,
    ));
    if filters {
        body = body.child(
            row().gap(6.).children(
                [
                    ("All", ReleaseGroup::All),
                    ("Albums", ReleaseGroup::Album),
                    ("Singles & EPs", ReleaseGroup::Short),
                ]
                .into_iter()
                .map(|(name, group)| {
                    v.button(&v.tr(name), options.group == group, move |s, _| {
                        let o = s.artist_options();
                        o.group = group;
                        o.releases = false;
                    })
                }),
            ),
        );
    }
    let limit = if options.releases {
        releases.len()
    } else {
        cols.min(releases.len())
    };
    for line in releases[..limit].chunks(cols) {
        body = body.child(
            row()
                .gap(14.)
                .children(line.iter().map(|a| tile(v, a, side, false))),
        );
    }
    body = body.child(label(v.tr("About"), 26., 700., v.p.text).mt(16.));
    if let Some(bio) = state
        .artist_info
        .get()
        .and_then(|info| info.biography.as_deref())
        .filter(|bio| !bio.is_empty())
    {
        let mut copy = col()
            .w_full()
            .padding(18.)
            .gap(8.)
            .radius(8.)
            .fill(color(v.p.surface))
            .child(
                label(bio, 16., 400., v.p.text)
                    .wrap()
                    .max_lines(if options.biography { usize::MAX } else { 5 }),
            );
        if bio.chars().count() as f32 * 8. > (body_width - 36.) * 5. {
            copy = copy.child(v.button(
                &v.tr(if options.biography {
                    "Show less"
                } else {
                    "See more"
                }),
                false,
                |s, _| {
                    let o = s.artist_options();
                    o.biography = !o.biography;
                },
            ));
        }
        body = body.child(copy);
    } else {
        body = body.child(status(v, &state.artist_info, "No biography available."));
    }
    let related = state
        .artist_info
        .get()
        .map(|i| i.similar_artist.as_slice())
        .unwrap_or_default();
    let (artist_cols, artist_side) = grid_size(body_width, 128.);
    body = body.child(
        shelf(
            v,
            "Similar artists",
            related.len(),
            artist_cols,
            options.similar,
            |o| &mut o.similar,
        )
        .mt(16.),
    );
    if related.is_empty() {
        body = body.child(status(
            v,
            &state.artist_info,
            "No similar artists in your library.",
        ));
    }
    let limit = if options.similar {
        related.len()
    } else {
        artist_cols.min(related.len())
    };
    for line in related[..limit].chunks(artist_cols) {
        body = body.child(row().gap(14.).children(line.iter().map(|a| {
            let tile = col()
                .w(artist_side)
                .gap(8.)
                .child(v.cover(
                    a.cover_art.as_deref(),
                    CoverSize::Sleeve,
                    artist_side,
                    artist_side / 2.,
                ))
                .child(label(&a.name, 15., 600., v.p.text).truncate());
            if let Some(id) = &a.id {
                let page = Page::Artist(navidrome::encode(id));
                library::favorite_context(
                    tile.role("button")
                        .focusable(true)
                        .label(&a.name)
                        .on_press_tx(move |s: &mut State, tx| {
                            s.dispatch(Action::Open(page.clone()), tx)
                        }),
                    FavoriteKind::Artist,
                    id,
                )
            } else {
                tile
            }
        })));
    }
    let appearances = state
        .appearances
        .get()
        .map(Vec::as_slice)
        .unwrap_or_default();
    body = body.child(
        shelf(
            v,
            "Appears on",
            appearances.len(),
            cols,
            options.features,
            |o| &mut o.features,
        )
        .mt(16.),
    );
    if appearances.is_empty() {
        body = body.child(status(
            v,
            &state.appearances,
            "No featured releases in your library.",
        ));
    }
    let limit = if options.features {
        appearances.len()
    } else {
        cols.min(appearances.len())
    };
    for line in appearances[..limit].chunks(cols) {
        body = body.child(
            row()
                .gap(14.)
                .children(line.iter().map(|a| tile(v, a, side, true))),
        );
    }
    col()
        .w_full()
        .child(hero(v, name, albums, portrait, w, gutter))
        .child(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artist_portrait_fades_from_the_header_tint_to_clear() {
        let top = Color32::from_rgb(40, 62, 82);
        let bottom = Color32::from_rgb(14, 18, 20);
        for (x, alpha) in PORTRAIT_STOPS {
            assert_eq!(
                portrait_shade(top, bottom, x, 0.),
                top.gamma_multiply_u8(alpha)
            );
            assert_eq!(
                portrait_shade(top, bottom, x, 1.),
                bottom.gamma_multiply_u8(alpha)
            );
        }
        assert_eq!(portrait_shade(top, bottom, 0.51, 0.5).a(), 128);
    }
}
