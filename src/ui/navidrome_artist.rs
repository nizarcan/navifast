use crate::{
    i18n::{Locale, ngettext},
    navidrome::api::Album,
};
const TILE_GAP: f32 = 14.;
pub(crate) const PORTRAIT_STOPS: [(f32, u8); 4] = [(0., 255), (0.29, 226), (0.73, 30), (1., 0)];

pub(super) fn grid_size(width: f32, target: f32) -> (usize, f32) {
    let cols = ((width + TILE_GAP) / (target + TILE_GAP)).round().max(1.0) as usize;
    (
        cols,
        ((width - (cols - 1) as f32 * TILE_GAP) / cols as f32).clamp(1.0, target * 1.28),
    )
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
