//! Recorded byte values from the previous desktop renderer, without that dependency.
use navifast::color::Srgba;
use serde::Deserialize;
#[derive(Deserialize)]
struct Case {
    input: [u8; 4],
    stored: [u8; 4],
    straight: [u8; 4],
    fade: [u8; 4],
    lerp: [u8; 4],
    blend: [u8; 4],
}
#[test]
fn saved_palette_bytes_and_compositing_match_the_installed_format() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/palette/compatibility.json")).unwrap();
    for c in cases {
        let [r, g, b, a] = c.input;
        let actual = Srgba::from_rgba_unmultiplied(r, g, b, a);
        assert_eq!(actual.to_array(), c.stored, "stored {:?}", c.input);
        assert_eq!(
            actual.to_srgba_unmultiplied(),
            c.straight,
            "straight {:?}",
            c.input
        );
        assert_eq!(
            actual.gamma_multiply(0.28).to_array(),
            c.fade,
            "fade {:?}",
            c.input
        );
        assert_eq!(
            actual.lerp_to_gamma(Srgba::WHITE, 0.56).to_array(),
            c.lerp,
            "lerp {:?}",
            c.input
        );
        assert_eq!(
            navifast::ui::blend(actual, Srgba::from_rgb(35, 26, 39), 0.56).to_array(),
            c.blend,
            "blend {:?}",
            c.input
        );
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(c.stored).unwrap()
        );
    }
}
