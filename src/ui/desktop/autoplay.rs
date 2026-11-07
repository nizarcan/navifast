//! Exclusions reuse item menus and the existing Settings section.
use super::*;
use crate::navidrome::api::FavoriteKind;
const PAGE: usize = 8;
pub(super) fn menu_item(v: &Paint<'_>, uri: &str) -> El {
    let Some(excluded) = v.s.app.autoplay_excluded(uri) else {
        return el();
    };
    let uri = uri.to_owned();
    v.menu_item(
        &v.tr(if excluded {
            mark("Include in autoplay")
        } else {
            mark("Exclude from autoplay")
        }),
        None,
        move |s, _| {
            s.toggle_autoplay_exclusion(&uri);
        },
    )
    .key("autoplay-exclusion")
}
pub(super) fn rows(v: &Paint<'_>) -> Vec<El> {
    let mut rows = vec![label(v.tr(mark("These rules apply only to Autoplay for this account on this device. You can still play and queue these items yourself.")), 13., 400., v.p.secondary).wrap()];
    let rules = v.s.app.autoplay_exclusions();
    let count = rules.map_or(0, |r| r.len());
    if count == 0 {
        rows.push(label(v.tr(mark("No exclusions. Right-click a song, album or artist to exclude it from Autoplay.")), 14., 400., v.p.secondary).wrap());
        return rows;
    }
    rows.push(label(
        v.tr(mark("{count} exclusions"))
            .replace("{count}", &count.to_string()),
        15.,
        600.,
        v.p.text,
    ));
    rows.push(label(v.tr(mark("A song excluded through an album or artist stays excluded until that rule is removed.")), 13., 400., v.p.secondary).wrap());
    let page = v.s.exclusions_page.min((count - 1) / PAGE);
    for (kind, id, name) in rules.unwrap().entries().skip(page * PAGE).take(PAGE) {
        let kind_label = v.tr(match kind {
            FavoriteKind::Song => mark("Song"),
            FavoriteKind::Album => mark("Album"),
            FavoriteKind::Artist => mark("Artist"),
        });
        let id = id.to_owned();
        let key = kind.uri(&id);
        let copy = col()
            .min_w(0.)
            .grow(1.)
            .gap(3.)
            .child(label(name, 14., 600., v.p.text).truncate().help(name))
            .child(label(kind_label, 12., 400., v.p.secondary));
        rows.push(
            row()
                .w_full()
                .items_center()
                .gap(12.)
                .key(format!("exclusion-{key}"))
                .child(copy)
                .child(
                    v.button(&v.tr(mark("Include in autoplay")), false, move |s, _| {
                        s.include_in_autoplay(kind, &id);
                    })
                    .key(format!("include-{key}")),
                ),
        );
    }
    if count > PAGE {
        rows.push(
            row()
                .w_full()
                .gap(8.)
                .flex_wrap()
                .child(
                    v.button(&v.tr(mark("Previous")), false, move |s, _| {
                        s.exclusions_page = page.saturating_sub(1)
                    })
                    .disabled(page == 0)
                    .key("exclusions-previous"),
                )
                .child(
                    v.button(&v.tr(mark("Next")), false, move |s, _| {
                        s.exclusions_page = page + 1
                    })
                    .disabled((page + 1) * PAGE >= count)
                    .key("exclusions-next"),
                ),
        );
    }
    rows
}
