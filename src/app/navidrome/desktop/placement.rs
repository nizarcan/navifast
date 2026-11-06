//! The reference client stores its viewport as a RON string inside app.ron.
//! Keep the other entries (including egui memory) opaque when updating it.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Geometry {
    inner_position_pixels: Option<egui::Pos2>,
    outer_position_pixels: Option<egui::Pos2>,
    fullscreen: bool,
    maximized: bool,
    inner_size_points: Option<egui::Vec2>,
}

pub(super) struct Placement {
    path: PathBuf,
    entries: BTreeMap<String, String>,
    geometry: Geometry,
    original: Geometry,
}

impl Placement {
    pub fn load(path: PathBuf) -> Option<Self> {
        let entries: BTreeMap<String, String> = match std::fs::read_to_string(&path) {
            Ok(text) => ron::from_str(&text).ok()?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(_) => return None,
        };
        let geometry: Geometry = match entries.get("window") {
            Some(ron) => ron::from_str(ron).ok()?,
            None => Geometry::default(),
        };
        Some(Self {
            path,
            entries,
            original: geometry.clone(),
            geometry,
        })
    }
    pub fn size(&self, zoom: f32) -> Option<[f32; 2]> {
        let size = self.geometry.inner_size_points? * zoom;
        (size.is_finite() && size.x > 0. && size.y > 0.).then_some([
            size.x.max(crate::window::MAIN_MIN_SIZE[0]),
            size.y.max(crate::window::MAIN_MIN_SIZE[1]),
        ])
    }
    pub fn position(&self) -> Option<(i32, i32)> {
        let pos = if cfg!(target_os = "macos") {
            self.geometry.inner_position_pixels
        } else {
            self.geometry.outer_position_pixels
        }?;
        crate::window::can_restore([pos.x, pos.y], 1.)
            .then_some((pos.x.round() as i32, pos.y.round() as i32))
    }
    pub fn resize(&mut self, [width, height]: [f32; 2], zoom: f32) {
        if width.is_finite()
            && height.is_finite()
            && width >= crate::window::MAIN_MIN_SIZE[0]
            && height >= crate::window::MAIN_MIN_SIZE[1]
        {
            self.geometry.inner_size_points = Some(egui::vec2(width, height) / zoom);
        }
    }
    pub fn observe(&mut self, handle: &rune::WindowHandle) {
        if let Some((origin, maximized)) = native_frame(handle) {
            self.geometry.outer_position_pixels = Some(origin);
            self.geometry.inner_position_pixels = Some(origin);
            self.geometry.maximized = maximized;
        }
    }
    pub fn update(&mut self) -> Option<(PathBuf, String)> {
        if self.geometry == self.original {
            return None;
        }
        self.entries
            .insert("window".into(), ron::to_string(&self.geometry).ok()?);
        let text = ron::ser::to_string_pretty(&self.entries, Default::default()).ok()?;
        self.original = self.geometry.clone();
        Some((self.path.clone(), text))
    }
}

#[cfg(target_os = "macos")]
fn native_frame(handle: &rune::WindowHandle) -> Option<(egui::Pos2, bool)> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSView};
    use raw_window_handle::RawWindowHandle;
    handle
        .with_raw_handle(|raw| {
            let RawWindowHandle::AppKit(raw) = raw.as_raw() else {
                return None;
            };
            let mtm = MainThreadMarker::new()?;
            // Rune supplies a live NSView and calls this observer on the main thread.
            let view = unsafe { raw.ns_view.cast::<NSView>().as_ref() };
            let window = view.window()?;
            let primary = NSScreen::screens(mtm).firstObject()?.frame().size.height;
            let frame = window.frame();
            let scale = window.backingScaleFactor();
            Some((
                egui::pos2(
                    (frame.origin.x * scale) as f32,
                    ((primary - frame.origin.y - frame.size.height) * scale) as f32,
                ),
                window.isZoomed(),
            ))
        })
        .ok()
        .flatten()
}

#[cfg(windows)]
fn native_frame(handle: &rune::WindowHandle) -> Option<(egui::Pos2, bool)> {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{GetWindowRect, IsZoomed},
    };
    handle
        .with_raw_handle(|raw| {
            let RawWindowHandle::Win32(raw) = raw.as_raw() else {
                return None;
            };
            let hwnd = raw.hwnd.get() as *mut _;
            let mut rect = RECT::default();
            // The borrowed window handle identifies a live HWND for these synchronous queries.
            unsafe {
                (GetWindowRect(hwnd, &mut rect) != 0).then(|| {
                    (
                        egui::pos2(rect.left as f32, rect.top as f32),
                        IsZoomed(hwnd) != 0,
                    )
                })
            }
        })
        .ok()
        .flatten()
}

#[cfg(target_os = "linux")]
fn native_frame(handle: &rune::WindowHandle) -> Option<(egui::Pos2, bool)> {
    use raw_window_handle::RawWindowHandle;
    use x11rb::{connection::Connection, protocol::xproto::ConnectionExt};
    // Wayland owns window placement. Only read desktop coordinates on X11.
    handle
        .with_raw_handle(|raw| {
            let id = match raw.as_raw() {
                RawWindowHandle::Xlib(x) => u32::try_from(x.window).ok()?,
                RawWindowHandle::Xcb(x) => x.window.get(),
                _ => return None,
            };
            let (conn, screen) = x11rb::connect(None).ok()?;
            let root = conn.setup().roots.get(screen)?.root;
            let position = conn
                .translate_coordinates(id, root, 0, 0)
                .ok()?
                .reply()
                .ok()?;
            Some((
                egui::pos2(f32::from(position.dst_x), f32::from(position.dst_y)),
                false,
            ))
        })
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_other_profile_entries_and_applies_zoom_once() {
        let path = std::env::temp_dir().join(format!(
            "navifast-placement-{:016x}.ron",
            rand::random::<u64>()
        ));
        std::fs::write(
            &path,
            r#"{"egui":"opaque","window":"(inner_size_points:Some((x:800.0,y:600.0)))"}"#,
        )
        .unwrap();
        let mut store = Placement::load(path.clone()).unwrap();
        assert_eq!(store.size(1.25), Some([1000., 750.]));
        assert!(store.update().is_none());
        store.resize([1200., 750.], 1.25);
        let (_, text) = store.update().unwrap();
        let entries: BTreeMap<String, String> = ron::from_str(&text).unwrap();
        assert_eq!(entries["egui"], "opaque");
        assert_eq!(
            ron::from_str::<Geometry>(&entries["window"])
                .unwrap()
                .inner_size_points,
            Some(egui::vec2(960., 600.))
        );
        std::fs::remove_file(path).unwrap();
    }
}
