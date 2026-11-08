//! Full artwork uses the same modal surfaces and controls as other detail dialogs.
use super::*;

pub(super) fn dialog(v: &Paint<'_>) -> El {
    let Some(art) = &v.s.artwork_viewer.open else {
        return el();
    };
    let width = 900_f32.min(v.width - 16.);
    let image_height = (v.height - 230.).max(120.);
    let artwork = art
        .id
        .as_deref()
        .map(|id| v.s.cover_for_view(id, CoverSize::Original));
    let mut image = el().key("artwork-image").w_full().h(image_height).center();
    if let Some(pixels) = artwork.as_ref().and_then(|art| art.pixels.as_ref()) {
        image = image.pixels_fit(pixels, rune::SurfaceFit::Fit);
    } else {
        image = image.child(v.icon(Icon::Music, 72., v.p.dim));
    }
    image = image.on_visible(move |_: &mut State, _, tx| {
        let _ = &artwork;
        tx.live_only();
    });
    let failed = v.s.artwork_failed();
    let ready = v.s.artwork_bytes().is_some();
    let status = if let Some(error) = art.error {
        error
    } else if failed {
        mark("Could not load artwork. Try again.")
    } else if art.id.is_none() {
        mark("No artwork available")
    } else if art.saved {
        mark("Image saved")
    } else if !ready {
        mark("Loading artwork…")
    } else {
        ""
    };
    let mut actions = row().gap(8.).justify_end().w_full();
    if failed {
        actions = actions.child(
            v.button(&v.tr(mark("Retry")), false, |s, _| s.retry_artwork())
                .key("artwork-retry"),
        );
    }
    actions = actions
        .child(
            v.button(&v.tr(mark("Close")), false, |s, _| s.close_artwork())
                .key("artwork-close"),
        )
        .child(
            v.button(&v.tr(mark("Save image")), true, |s, tx| s.save_artwork(tx))
                .disabled(!ready || v.s.artwork_viewer.saving)
                .key("artwork-save"),
        );
    let panel = col()
        .key("artwork-viewer")
        .w(width)
        .padding(24.)
        .gap(16.)
        .radius(12.)
        .border(1., color(v.p.outline))
        .fill(color(v.p.overlay))
        .shadow(10., 40., color(v.p.shadow))
        .modal(true, State::close_artwork)
        .focus_return("collection-art")
        .role("dialog")
        .label(v.tr(mark("Artwork")))
        .child(label(&art.title, 20., 700., v.p.text).truncate())
        .child(image)
        .child(
            label(
                if status.is_empty() {
                    String::new()
                } else {
                    v.tr(status)
                },
                13.,
                400.,
                if failed || art.error.is_some() {
                    v.p.danger
                } else {
                    v.p.secondary
                },
            )
            .wrap()
            .min_h(18.),
        )
        .child(actions);
    col()
        .absolute(0., 0.)
        .w_full()
        .h_full()
        .center()
        .fill(color(Srgba::from_black_alpha(if v.p.dark {
            150
        } else {
            80
        })))
        .child(panel)
}
