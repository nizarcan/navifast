//! Rune views and their shared layout rules.
use crate::color::Srgba;
pub mod collection;
pub mod desktop;
pub mod keys;
pub mod navidrome_artist;
pub mod player_bar;
pub mod sidebar;
pub mod topbar;
pub mod widgets;
pub fn blend(base: Srgba, tint: Srgba, amount: f32) -> Srgba {
    let a = base.linear();
    let b = tint.linear();
    Srgba::from_linear(std::array::from_fn(|i| {
        a[i] * (1. - amount) + b[i] * amount
    }))
    .opaque()
}
