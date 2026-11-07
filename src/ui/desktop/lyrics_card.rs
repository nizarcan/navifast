//! A single Rune layout produces the PNG shown in the preview and exported locally.
use super::*;
use crate::app::navidrome::desktop::lyrics_share::{Background, Card, Rendered, pixel_digest};

const WIDTH: f32 = 600.;
const MAX_HEIGHT: f32 = 2048.;
const SCALE: f32 = 2.;

fn card(s: &Card, _: &mut Cx<Card>) -> El {
    let mut credit = row().w_full().items_center().gap(18.);
    if let Some(cover) = &s.cover {
        credit = credit.child(el().size(72., 72.).radius(6.).pixels(cover));
    }
    credit = credit.child(
        col()
            .grow(1.)
            .min_w(0.)
            .gap(5.)
            .child(label(&s.title, 20., 700., s.palette.text).wrap())
            .child(label(&s.artist, 15., 400., s.palette.secondary).wrap()),
    );
    let mut lines = col().w_full().gap(10.);
    for (index, line) in s.lines.iter().enumerate() {
        lines = lines.child(
            label(
                if line.is_empty() { "\u{a0}" } else { line },
                26.,
                600.,
                s.palette.text,
            )
            .key(format!("card-line-{index}"))
            .wrap(),
        );
    }
    col()
        .w(WIDTH)
        .padding(32.)
        .gap(28.)
        .fill(color(s.palette.panel))
        .child(credit)
        .child(el().w_full().h(1.).fill(color(s.palette.outline)))
        .child(lines)
        .child(el().key("card-end").h(1.))
}

fn layout(card_state: Card) -> Result<(rune::Runtime<Card>, u32), String> {
    if card_state.lines.is_empty()
        || card_state.lines.len() > crate::app::navidrome::desktop::lyrics_share::MAX_LINES
        || card_state.lines.iter().map(String::len).sum::<usize>() > 16 * 1024
        || card_state.title.len() + card_state.artist.len() > 4096
    {
        return Err("Select fewer or shorter lines for this image.".into());
    }
    let dark = card_state.palette.dark;
    let mut runtime = rune::Runtime::with_text(
        card_state,
        card,
        (WIDTH, MAX_HEIGHT),
        rune::Env::new(dark, SCALE),
        rune::text::TextSystem::new(&[crate::theme::INTER], ("Inter", "monospace")),
    );
    runtime.frame(0.);
    runtime.settle_all();
    let end = runtime
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("card-end"))
        .ok_or("Could not lay out the image. Try fewer lines.")?
        .rect;
    let height = (end[1] + end[3] + 32.).ceil();
    if !height.is_finite() || height > MAX_HEIGHT {
        return Err("These lines need a taller image. Select fewer or shorter lines.".into());
    }
    runtime.set_viewport((WIDTH, height), SCALE);
    runtime.frame(0.);
    runtime.settle_all();
    Ok((runtime, (height * SCALE) as u32))
}

pub(crate) fn render(card: Card) -> Result<Rendered, String> {
    use image::ImageEncoder;
    use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};
    let description = card.lines.join("\n");
    let (mut rt, height) = layout(card)?;
    let gpu = Gpu::try_new(Gpu::instance(), None)
        .map_err(|_| "Could not render the image. Check that a graphics device is available.")?;
    let width = (WIDTH * SCALE) as u32;
    let target = Offscreen::new(&gpu.device, width, height);
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
    renderer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut rt.text.atlas,
        &rt.instances,
        (width, height),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(&mut encoder, &target.view, rt.clear_color());
    target.copy(&mut encoder);
    renderer.submit(&gpu.queue, encoder);
    let rgba = target.read(&gpu.device);
    let digest = pixel_digest(&rgba);
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|_| "Could not encode the image. Try again.")?;
    let pixels = rune::Pixels::new();
    pixels.set(width, height, rgba);
    Ok(Rendered {
        png: png.into(),
        pixels,
        digest,
        description,
    })
}

pub(super) fn selection(v: &Paint<'_>) -> El {
    let draft = v.s.lyrics_share.draft.as_ref().unwrap();
    let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
    let rows = col()
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            draft.lines.len(),
            48.,
            |_: &State, i| format!("share-line-{i}").into(),
            move |s: &State, index| {
                let v = Paint {
                    s,
                    p,
                    scale,
                    zoom,
                    width,
                    height,
                };
                let Some(draft) = &s.lyrics_share.draft else {
                    return el();
                };
                let selected = draft.selected.contains(&index);
                row()
                    .key(format!("share-line-{index}"))
                    .w_full()
                    .py(10.)
                    .px(6.)
                    .gap(10.)
                    .radius(5.)
                    .fill(color(if selected { p.surface_active } else { p.panel }))
                    .hover(color(p.surface_hover))
                    .role("checkbox")
                    .focusable(true)
                    .checked(selected)
                    .label(&draft.lines[index])
                    .child(
                        el().size(18., 18.)
                            .mt(3.)
                            .radius(4.)
                            .border(1., color(p.outline))
                            .center()
                            .child(if selected {
                                v.icon(Icon::Check, 14., p.accent)
                            } else {
                                el()
                            }),
                    )
                    .child(
                        label(&draft.lines[index], 19., 400., p.text)
                            .grow(1.)
                            .min_w(0.)
                            .wrap(),
                    )
                    .on_press_tx(move |s: &mut State, _| s.select_lyric_line(index))
            },
        );
    let mut body = col()
        .grow(1.)
        .min_h(0.)
        .w_full()
        .gap(10.)
        .child(
            label(
                format!("{} · {}", draft.title, draft.artist),
                13.,
                600.,
                v.p.secondary,
            )
            .wrap(),
        )
        .child(
            label(
                v.tr(mark("Select lines for your image.")),
                13.,
                400.,
                v.p.dim,
            )
            .wrap(),
        )
        .child(
            col()
                .key("lyric-selection")
                .grow(1.)
                .min_h(0.)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows),
        );
    if let Some(error) = &draft.error {
        body = body.child(label(error, 13., 400., v.p.danger).wrap());
    }
    body.child(
        row()
            .w_full()
            .gap(8.)
            .items_center()
            .child(
                label(
                    format!(
                        "{} / {}",
                        draft.selected.len(),
                        crate::app::navidrome::desktop::lyrics_share::MAX_LINES
                    ),
                    12.,
                    400.,
                    v.p.dim,
                )
                .grow(1.),
            )
            .child(
                v.button(&v.tr(mark("Cancel")), false, |s, _| {
                    s.cancel_lyric_selection()
                })
                .key("lyrics-selection-cancel"),
            )
            .child(
                v.button(&v.tr(mark("Preview")), true, |s, _| s.preview_lyrics())
                    .key("lyrics-preview")
                    .disabled(draft.selected.is_empty()),
            ),
    )
}

pub(super) fn preview(v: &Paint<'_>) -> El {
    let draft = v.s.lyrics_share.draft.as_ref().unwrap();
    let width = 600_f32.min(v.width - 32.);
    let busy = v.s.lyrics_share.saving;
    let mut controls = row().w_full().gap(8.).items_center().flex_wrap();
    for (background, key, name) in [
        (Background::Theme, "theme", v.tr(mark("Theme"))),
        (
            Background::Dark,
            "dark",
            ThemeChoice::Dark
                .label(v.s.app.presentation.locale)
                .into_owned(),
        ),
        (
            Background::Light,
            "light",
            ThemeChoice::Light
                .label(v.s.app.presentation.locale)
                .into_owned(),
        ),
    ] {
        controls = controls.child(
            v.button(&name, draft.background == background, move |s, _| {
                s.lyric_background(background)
            })
            .key(format!("lyrics-background-{key}"))
            .disabled(busy),
        );
    }
    let mut contents = col()
        .w_full()
        .gap(12.)
        .child(label(v.tr(mark("Background")), 13., 600., v.p.secondary))
        .child(controls);
    if draft.cover.is_some() {
        let name = v.tr(mark("Include cover art"));
        contents = contents.child(
            row()
                .w_full()
                .items_center()
                .h(32.)
                .gap(12.)
                .child(label(&name, 13., 500., v.p.secondary).grow(1.))
                .child(
                    preferences::switch(v, &name, draft.show_cover, State::lyric_cover)
                        .key("lyrics-cover")
                        .disabled(busy),
                ),
        );
    }
    if let Some(image) = &draft.ready {
        let (w, h) = image.pixels.size();
        contents = contents.child(
            el().key("lyrics-image-preview")
                .w(width - 48.)
                .h((width - 48.) * h as f32 / w as f32)
                .pixels(&image.pixels)
                .role("image")
                .label(&image.description),
        );
    } else if draft.error.is_none() {
        contents = contents.child(v.loading().py(30.));
    }
    if draft.ready.is_none() && draft.error.is_some() {
        contents = contents.child(
            v.button(&v.tr(mark("Try again")), false, |s, _| s.preview_lyrics())
                .key("lyrics-render-retry"),
        );
    }
    let has_status = draft.error.is_some() || draft.notice.is_some();
    let reserved = 210. + if width < 480. { 40. } else { 0. } + if has_status { 64. } else { 0. };
    let mut panel = col()
        .w(width)
        .padding(24.)
        .radius(12.)
        .border(1., color(v.p.outline))
        .fill(color(v.p.overlay))
        .shadow(10., 40., color(v.p.shadow))
        .modal(true, |s: &mut State| s.edit_lyric_selection())
        .role("dialog")
        .label(v.tr(mark("Lyric image")))
        .child(
            row()
                .w_full()
                .items_center()
                .mb(18.)
                .child(label(v.tr(mark("Lyric image")), 20., 700., v.p.text).grow(1.))
                .child(
                    v.icon_button(&v.tr(mark("Close")), Icon::X, 18., |s, _| {
                        s.edit_lyric_selection()
                    })
                    .key("lyrics-preview-close"),
                ),
        )
        .child(
            contents
                .max_h((v.height - reserved).max(80.))
                .scroll(ScrollSpec {
                    stick_bottom: false,
                }),
        );
    if let Some(error) = &draft.error {
        panel = panel.child(
            label(error, 13., 400., v.p.danger)
                .wrap()
                .mt(12.)
                .role("alert"),
        );
    }
    if let Some(notice) = draft.notice {
        panel = panel.child(
            label(v.tr(notice), 13., 400., v.p.secondary)
                .mt(12.)
                .role("status"),
        );
    }
    panel = panel.child(
        row()
            .w_full()
            .flex_wrap()
            .gap(8.)
            .mt(18.)
            .child(
                v.button(&v.tr(mark("Edit lines")), false, |s, _| {
                    s.edit_lyric_selection()
                })
                .key("lyrics-edit-lines"),
            )
            .child(el().grow(1.))
            .child(
                v.button(&v.tr(mark("Copy image")), false, |s, tx| {
                    s.copy_lyric_image(tx)
                })
                .key("lyrics-copy")
                .disabled(busy || draft.ready.is_none()),
            )
            .child(
                v.button(&v.tr(mark("Save image")), true, |s, tx| {
                    s.save_lyric_image(tx)
                })
                .key("lyrics-save")
                .disabled(busy || draft.ready.is_none()),
            ),
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Card {
        Card { title:"A song with words".into(), artist:"Demo artist".into(),
            lines: vec!["We can take the long way home, past the trees and city lights. ".repeat(3),
                "مرحبا hello עולם, we can take the long way home together\nCarry daylight through the night".into()],
            cover:None, palette:Palette::dark() }
    }

    #[test]
    fn layout_wraps_the_complete_passage_and_preserves_logical_text() {
        for palette in [Palette::dark(), Palette::light()] {
            let mut input = fixture();
            input.palette = palette;
            let expected = input.lines.clone();
            let (rt, height) = layout(input).unwrap();
            assert!((600..4096).contains(&height));
            assert_eq!(rt.state.lines, expected);
            let elements = rt.elements();
            let first = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("card-line-0"))
                .unwrap()
                .rect;
            let second = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("card-line-1"))
                .unwrap()
                .rect;
            assert!(first[3] > 60., "long line must wrap");
            assert!(second[1] >= first[1] + first[3]);
            assert!(second[1] + second[3] < height as f32 / SCALE);
            assert!(first[0] >= 32. && first[2] <= WIDTH - 64.);
        }
    }

    #[test]
    fn a_passage_too_tall_is_rejected_without_small_or_clipped_text() {
        let mut input = fixture();
        input.lines = vec!["A long lyric line that must remain legible. ".repeat(250)];
        assert!(layout(input).is_err());
    }

    #[test]
    #[ignore = "requires a native graphics adapter; run for macOS export validation"]
    fn png_matches_preview_pixels() {
        for palette in [Palette::dark(), Palette::light()] {
            let mut input = fixture();
            input.palette = palette;
            let image = render(input).unwrap();
            let decoded = image::load_from_memory_with_format(&image.png, image::ImageFormat::Png)
                .unwrap()
                .to_rgba8();
            assert_eq!(decoded.dimensions(), image.pixels.size());
            assert_eq!(pixel_digest(decoded.as_raw()), image.digest);
            assert!(decoded.pixels().all(|p| p[3] == 255));
            if let Some(path) = std::env::var_os("NAVIFAST_LYRIC_CARD_CAPTURE") {
                std::fs::create_dir_all(&path).unwrap();
                std::fs::write(
                    std::path::Path::new(&path).join(if palette.dark {
                        "card-dark.png"
                    } else {
                        "card-light.png"
                    }),
                    &image.png,
                )
                .unwrap();
            }
        }
    }
}
