//! Shared palette, typography, icons, and base widgets.
//!
//! Inter provides real font weights, and Lucide provides a consistent icon set.
//! All colors use [`Palette`] so light, dark, and album-art-tinted themes stay
//! consistent.

#[cfg(target_os = "linux")]
mod omarchy;

use crate::color::Srgba;
use crate::i18n::Locale;
use std::borrow::Cow;

mod custom;
mod presets;
pub use custom::{Catalog, CustomTheme, display_name, read_cached_theme};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    pub dark: bool,
    pub window: Srgba,
    pub panel: Srgba,
    pub surface: Srgba,
    pub surface_hover: Srgba,
    pub surface_active: Srgba,
    pub outline: Srgba,
    pub text: Srgba,
    pub secondary: Srgba,
    pub dim: Srgba,
    pub accent: Srgba,
    pub accent_hover: Srgba,
    pub on_accent: Srgba,
    pub danger: Srgba,
    pub warning: Srgba,
    pub overlay: Srgba,
    pub shadow: Srgba,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            dark: true,
            window: Srgba::from_rgb(0x0f, 0x11, 0x14),
            panel: Srgba::from_rgb(0x15, 0x18, 0x1c),
            surface: Srgba::from_rgb(0x1d, 0x21, 0x27),
            surface_hover: Srgba::from_rgb(0x26, 0x2b, 0x33),
            surface_active: Srgba::from_rgb(0x2f, 0x35, 0x3f),
            outline: Srgba::from_rgb(0x2a, 0x30, 0x38),
            text: Srgba::from_rgb(0xf2, 0xf4, 0xf6),
            secondary: Srgba::from_rgb(0xa9, 0xb1, 0xbc),
            dim: Srgba::from_rgb(0x6e, 0x77, 0x84),
            accent: Srgba::from_rgb(0x1e, 0xd7, 0x60),
            accent_hover: Srgba::from_rgb(0x3c, 0xe8, 0x7a),
            on_accent: Srgba::from_rgb(0x0a, 0x14, 0x0e),
            danger: Srgba::from_rgb(0xf5, 0x71, 0x7f),
            warning: Srgba::from_rgb(0xf2, 0xb8, 0x5c),
            overlay: Srgba::from_rgb(0x22, 0x27, 0x2e),
            shadow: Srgba::from_black_alpha(140),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            window: Srgba::from_rgb(0xf8, 0xf9, 0xfb),
            panel: Srgba::from_rgb(0xff, 0xff, 0xff),
            surface: Srgba::from_rgb(0xee, 0xf0, 0xf3),
            surface_hover: Srgba::from_rgb(0xe3, 0xe6, 0xeb),
            surface_active: Srgba::from_rgb(0xd7, 0xdb, 0xe1),
            outline: Srgba::from_rgb(0xdd, 0xe1, 0xe6),
            text: Srgba::from_rgb(0x14, 0x17, 0x1a),
            secondary: Srgba::from_rgb(0x53, 0x5b, 0x66),
            dim: Srgba::from_rgb(0x8b, 0x93, 0x9e),
            accent: Srgba::from_rgb(0x15, 0xa6, 0x4a),
            accent_hover: Srgba::from_rgb(0x12, 0x8f, 0x40),
            on_accent: Srgba::WHITE,
            danger: Srgba::from_rgb(0xd6, 0x3b, 0x4c),
            warning: Srgba::from_rgb(0xb8, 0x7a, 0x14),
            overlay: Srgba::from_rgb(0xff, 0xff, 0xff),
            shadow: Srgba::from_black_alpha(50),
        }
    }

    /// A colour derived from album art, softened so it can sit behind text.
    pub fn tint_from_art(&self, rgb: [u8; 3]) -> Srgba {
        let [r, g, b] = rgb.map(|c| c as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let lightness = (max + min) / 2.0;
        let target = if self.dark { 0.30 } else { 0.72 };
        let (r, g, b) = if lightness < 0.01 {
            (target, target, target)
        } else {
            let scale = target / lightness;
            (
                (r * scale).min(1.0),
                (g * scale).min(1.0),
                (b * scale).min(1.0),
            )
        };
        Srgba::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
    }
}

/// Whether a launch may follow the desktop's themes with this themes
/// folder. An updater trial of the legacy Fastpotify profile does not, so the
/// old hook and profile stay together until the update is accepted.
fn desktop_themes_allowed(themes: &std::path::Path) -> bool {
    themes != crate::paths::AppDirs::legacy().config.join("themes")
}

/// Adds the desktop's palettes to a normal launch: the eight shared palettes,
/// installed into the themes folder as files on the first launch, and
/// Omarchy's on Linux, with the packaged template and hook installed for the
/// user.
pub fn enable_desktop_themes(catalog: &mut Catalog, themes: &std::path::Path) {
    if !desktop_themes_allowed(themes) {
        return;
    }
    #[cfg(target_os = "linux")]
    omarchy::upgrade_legacy_hook();
    catalog.enable_desktop_themes();
}

/// The status line under the Theme setting, empty when all is well.
pub fn catalog_detail(
    catalog: &Catalog,
    locale: Locale,
    selected: Option<&str>,
) -> Cow<'static, str> {
    catalog.detail_in(locale, selected)
}

pub const RADIUS: u8 = 8;
pub const RADIUS_SMALL: u8 = 4;
pub const ROW_HEIGHT: f32 = 56.0;
pub const COMPACT_ROW_HEIGHT: f32 = 48.0;
/// The compact track list: one line, no cover.
pub const THIN_ROW_HEIGHT: f32 = 36.0;
pub const PLAYER_BAR_HEIGHT: f32 = 88.0;
/// The narrowest either right-hand panel goes. The queue and the lyrics
/// take the same edge and swap places there, so a width that suits one
/// has to suit the other, or the window would jump on the swap.
pub const SIDE_PANEL_MIN_WIDTH: f32 = 280.0;
pub const TOP_BAR_HEIGHT: f32 = 56.0;

/// macOS hides the titlebar and draws the window content all the way to the
/// top edge, so whatever sits at the top of the window has to leave room for
/// the traffic lights. Zero everywhere else, and in fullscreen, where the
pub const INTER: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");
macro_rules! icons {
    ($($name:ident => $file:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon { $($name),+ }

        impl Icon {
            pub const ALL: &[Self] = &[$(Self::$name),+];
            pub const fn bytes(self) -> &'static [u8] {
                match self { $(Self::$name => include_bytes!(concat!("../assets/icons/", $file, ".svg"))),+ }
            }
            pub const fn uri(self) -> &'static str {
                match self { $(Self::$name => concat!("bytes://navifast-icon-", $file, ".svg")),+ }
            }

        }
    };
}

icons! {
    AudioLines => "audio-lines",
    Check => "check",
    ChevronDown => "chevron-down",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    ChevronUp => "chevron-up",
    CircleAlert => "circle-alert",
    CircleCheck => "circle-check",
    Clock => "clock",
    Disc => "disc-3",
    Ellipsis => "ellipsis",
    Star => "star",
    StarFilled => "star-filled",
    House => "house",
    Info => "info",
    Image => "image",
    Library => "library",
    LayoutGrid => "layout-grid",
    ListMusic => "list-music",
    ListVideo => "list-video",
    LogOut => "log-out",
    Mic => "mic",
    Minus => "minus",
    Music => "music",
    PauseFilled => "pause-filled",
    PanelLeft => "panel-left",
    Play => "play",
    PlayFilled => "play-filled",
    Plus => "plus",
    Refresh => "refresh-cw",
    Repeat => "repeat",
    Search => "search",
    Settings => "settings",
    Shuffle => "shuffle",
    SkipBackFilled => "skip-back-filled",
    SkipForwardFilled => "skip-forward-filled",
    Speaker => "speaker",
    User => "user",
    Volume => "volume",
    Volume1 => "volume-1",
    Volume2 => "volume-2",
    VolumeX => "volume-x",
    X => "x",
}

pub fn play_glyph_offset(icon: Icon, icon_size: f32) -> [f32; 2] {
    if matches!(icon, Icon::PlayFilled | Icon::Play) {
        [icon_size * (0.03 - 1.0 / 24.0), 0.0]
    } else {
        [0., 0.]
    }
}
pub(crate) const SPINNER_TURNS_PER_SECOND: f64 = 1.2;
pub(crate) fn spinner_points(size: f32, start: f64) -> [[f32; 2]; 20] {
    let radius = size / 2.0 - 2.0;
    std::array::from_fn(|i| {
        let (sin, cos) = (start + 250_f64.to_radians() * i as f64 / 19.).sin_cos();
        [radius * cos as f32, radius * sin as f32]
    })
}
