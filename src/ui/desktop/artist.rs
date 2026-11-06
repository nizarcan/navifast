use super::*;
use crate::model::Loadable;
use crate::navidrome::api::{Album, FavoriteKind};
use crate::ui::navidrome_artist::{ReleaseGroup, grid_size, release_group, release_summary};

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
        Loadable::Loading => label(v.tr("Loading…"), 14., 400., v.p.secondary),
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

fn hero(
    v: &Paint<'_>,
    name: &str,
    albums: &[Album],
    portrait: Option<&str>,
    w: f32,
    gutter: f32,
) -> El {
    let height = (w * 0.24).clamp(216., 324.);
    let p = &v.s.app.presentation;
    let tint = p
        .navidrome_search
        .artist_cover
        .as_ref()
        .filter(|_| p.settings.accent_from_art)
        .and_then(|uri| p.accents.get(uri))
        .copied()
        .unwrap_or(Color32::from_rgb(111, 81, 128));
    let ink = crate::ui::blend(Color32::from_rgb(12, 13, 28), tint, 0.16);
    let plum = crate::ui::blend(Color32::from_rgb(35, 26, 39), tint, 0.56);
    let mut hero = el()
        .w_full()
        .h(height)
        .clip_soft(0.)
        .gradient(color(plum), color(ink));
    let art = (w * 0.63).min(height * 2.35);
    let center = w - art * 0.40;
    let radius = height * 0.72;
    let mut record = el()
        .absolute(center - radius, height / 2. - radius)
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
                .border(1., Color::WHITE.alpha(0.055)),
        );
    }
    record = record.child(v.cover(
        albums.iter().find_map(|a| a.cover_art.as_deref()),
        CoverSize::Sleeve,
        radius * 0.7,
        radius * 0.35,
    ));
    hero = hero.child(record);
    if let Some(id) = portrait {
        let loaded = v.s.cover_image(id, CoverSize::Portrait).1.is_some();
        let image = v
            .cover(Some(id), CoverSize::Portrait, art, 0.)
            .size(art, height)
            .absolute(w - art, 0.);
        hero = hero.child(image.opacity(if loaded { 1. } else { 0. }));
    }
    let mut meta = row().items_center().gap(8.).child(label(
        release_summary(v.s.app.presentation.locale, albums),
        14.,
        600.,
        Color32::WHITE,
    ));
    if let Page::Artist(id) = v.s.app.presentation.page()
        && let Some(id) = navidrome::decode(id)
    {
        meta = meta.child(library::star(v, FavoriteKind::Artist, &id));
    }
    hero.child(
        col()
            .absolute(gutter, 0.)
            .w((w - 2. * gutter) * 0.70)
            .h(height)
            .pb(16.)
            .justify_end()
            .gap(2.)
            .child(label(v.tr("Artist"), 14., 600., Color32::WHITE))
            .child(label(name, (w * 0.065).clamp(38., 84.), 700., Color32::WHITE).max_lines(2))
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
