use crate::color::Srgba;
const SPECTRUM_ALPHA: (f32, f32) = (0.28, 0.08);
const GLOW_ALPHA: f32 = 0.12;
const PEAK_ALPHA: f32 = 0.7;
const PULSE_ALPHA: f32 = 0.35;
const PEAK_HEIGHT: f32 = 2.0;
const PEAK_GAP: f32 = 2.0;
pub(crate) const WAVE_LAYERS: [(f32, f32); 3] = [(9.0, 0.06), (4.0, 0.16), (1.6, 0.85)];
const SPECTRUM_GAP: f32 = 2.0;
pub(crate) const VIS_FRAME: std::time::Duration = std::time::Duration::from_micros(16_667);
const DARK_CEILING: f32 = 0.35;
const LIGHT_FLOOR: f32 = 0.2;
const LIGHT_CEILING: f32 = 0.4;
pub(crate) fn vis_colours(base: Srgba, dark: bool) -> (Srgba, Srgba) {
    let [r, g, b, a] = base.linear();
    let gain = if a > 0. { 1. / a } else { 1. };
    let [r, g, b] = [r, g, b].map(|v| v * gain);
    let max = r.max(g).max(b);
    let span = max - r.min(g).min(b);
    let hue = if span == 0. {
        0.
    } else if max == r {
        ((g - b) / span).rem_euclid(6.) / 6.
    } else if max == g {
        ((b - r) / span + 2.) / 6.
    } else {
        ((r - g) / span + 4.) / 6.
    };
    let saturation = if max == 0. {
        0.55
    } else {
        (span / max).max(0.55)
    };
    let keep = |h: f32| {
        let rgb = [0., 4., 2.].map(|phase| {
            let triangle = ((h * 6. + phase).rem_euclid(6.) - 3.).abs() - 1.;
            1. - saturation + saturation * triangle.clamp(0., 1.)
        });
        within_brightness(Srgba::from_linear([rgb[0], rgb[1], rgb[2], 1.]), dark)
    };
    (keep(hue), keep((hue + 1. / 6.).fract()))
}
fn within_brightness(colour: Srgba, dark: bool) -> Srgba {
    let [r, g, b, _] = colour.linear();
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let ceiling = if dark { DARK_CEILING } else { LIGHT_CEILING };
    let adjusted = if luminance > ceiling {
        [r, g, b].map(|v| v * (ceiling / luminance))
    } else if !dark && luminance < LIGHT_FLOOR {
        let toward_white = (LIGHT_FLOOR - luminance) / (1.0 - luminance).max(f32::EPSILON);
        [r, g, b].map(|v| v + (1. - v) * toward_white)
    } else {
        [r, g, b]
    };
    Srgba::from_linear([adjusted[0], adjusted[1], adjusted[2], 1.])
}
pub(crate) fn spectrum_quads(
    [x, y, w, h]: [f32; 4],
    levels: &[f32],
    peaks: &[f32],
    (low, high): (Srgba, Srgba),
    strength: f32,
    mut quad: impl FnMut([f32; 4], Srgba, Srgba),
) {
    let bands = levels.len() as f32;
    let width = (w - SPECTRUM_GAP * (bands - 1.0)) / bands;
    // A full band reaches the top, with room above for its cap.
    let reach = h - PEAK_GAP - PEAK_HEIGHT - 1.0;

    // The bass pulse: a glow along the foot, as strong as the low bands.
    let bass = levels.iter().take(levels.len() / 8).copied().sum::<f32>()
        / (levels.len() / 8).max(1) as f32;
    let pulse = [x, y + h / 2., w, h / 2.];
    quad(
        pulse,
        Srgba::TRANSPARENT,
        low.gamma_multiply(PULSE_ALPHA * strength * bass * bass),
    );

    for (index, (level, peak)) in levels.iter().zip(peaks).enumerate() {
        let left = x + index as f32 * (width + SPECTRUM_GAP);
        let colour = low.lerp_to_gamma(high, index as f32 / (bands - 1.0));
        let height = level * reach;
        if height >= 1.0 {
            let bar = [left, y + h - height, width, height];
            // The glow: the bar again, wider and fainter.
            quad(
                [
                    left - SPECTRUM_GAP,
                    bar[1] - 3.,
                    width + 2. * SPECTRUM_GAP,
                    height + 6.,
                ],
                colour.gamma_multiply(GLOW_ALPHA * strength * 0.3),
                colour.gamma_multiply(GLOW_ALPHA * strength),
            );
            let (foot, top) = (SPECTRUM_ALPHA.0 * strength, SPECTRUM_ALPHA.1 * strength);
            quad(
                bar,
                colour.gamma_multiply(foot + (top - foot) * level),
                colour.gamma_multiply(foot),
            );
        }
        let cap = peak * reach;
        if cap >= 2.0 {
            let y = y + h - cap - PEAK_GAP;
            quad(
                [left, y - PEAK_HEIGHT, width, PEAK_HEIGHT],
                colour.gamma_multiply(PEAK_ALPHA),
                colour.gamma_multiply(PEAK_ALPHA),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn spectrum_palette_matches_the_frozen_reference() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/palette/compatibility.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let stored: [u8; 4] = serde_json::from_value(case["stored"].clone()).unwrap();
            let c =
                super::Srgba::from_rgba_premultiplied(stored[0], stored[1], stored[2], stored[3]);
            let (a, b) = super::vis_colours(c, true);
            let (d, e) = super::vis_colours(c, false);
            assert_eq!(
                serde_json::json!([a, b, d, e]),
                case["spectrum"],
                "input {}",
                case["input"]
            );
        }
    }
}
