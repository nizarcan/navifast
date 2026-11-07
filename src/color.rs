//! Palette colors stored as premultiplied sRGB bytes.
//!
//! The four-byte representation also preserves existing cached theme JSON.
//! Rune consumes linear colors, so conversion happens at the drawing boundary.

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct Srgba([u8; 4]);

impl Srgba {
    pub const WHITE: Self = Self([255; 4]);
    pub const BLACK: Self = Self([0, 0, 0, 255]);
    pub const TRANSPARENT: Self = Self([0; 4]);

    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }
    pub const fn from_gray(v: u8) -> Self {
        Self::from_rgb(v, v, v)
    }
    pub const fn from_black_alpha(a: u8) -> Self {
        Self([0, 0, 0, a])
    }
    pub const fn from_white_alpha(a: u8) -> Self {
        Self([a; 4])
    }
    pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    pub fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        let opacity = f32::from(a) / 255.;
        let [r, g, b] = [r, g, b].map(|v| (f32::from(v) * opacity).round() as u8);
        Self([r, g, b, a])
    }
    pub fn to_array(self) -> [u8; 4] {
        self.0
    }
    pub fn r(self) -> u8 {
        self.0[0]
    }
    pub fn g(self) -> u8 {
        self.0[1]
    }
    pub fn b(self) -> u8 {
        self.0[2]
    }
    pub fn a(self) -> u8 {
        self.0[3]
    }
    pub fn to_srgba_unmultiplied(self) -> [u8; 4] {
        let mut channels = self.0;
        if self.a() != 0 && self.a() != 255 {
            let scale = 255. / f32::from(self.a());
            for value in &mut channels[..3] {
                *value = (scale * f32::from(*value)).round() as u8;
            }
        }
        channels
    }
    pub fn gamma_multiply(self, opacity: f32) -> Self {
        Self(self.0.map(|v| (f32::from(v) * opacity).round() as u8))
    }
    pub fn gamma_multiply_u8(self, opacity: u8) -> Self {
        self.gamma_multiply(f32::from(opacity) / 255.)
    }
    pub fn lerp_to_gamma(self, other: Self, t: f32) -> Self {
        Self(std::array::from_fn(|i| {
            ((1. - t) * f32::from(self.0[i]) + t * f32::from(other.0[i])).round() as u8
        }))
    }

    pub(crate) fn linear(self) -> [f32; 4] {
        let alpha = f32::from(self.a()) / 255.;
        let divisor = if alpha == 0. { 255. } else { 255. * alpha };
        let gain = if alpha == 0. { 1. } else { alpha };
        let [r, g, b] = [self.r(), self.g(), self.b()]
            .map(|v| rune::color::srgb_decode(f32::from(v) / divisor) * gain);
        [r, g, b, alpha]
    }
    pub(crate) fn from_linear([r, g, b, alpha]: [f32; 4]) -> Self {
        let gain = if alpha == 0. { 1. } else { alpha };
        let [r, g, b] = [r, g, b].map(|v| {
            let byte = (255. * rune::color::srgb_encode(v / gain)).round() as u8;
            (f32::from(byte) * gain).round() as u8
        });
        Self([r, g, b, (alpha * 255.).round() as u8])
    }
    pub(crate) fn opaque(mut self) -> Self {
        self.0[3] = 255;
        self
    }
}
