//! The reference client stores its viewport as a RON string inside app.ron.
//! Keep the other entries (including egui memory) opaque when updating it.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Point {
    x: f32,
    y: f32,
}
impl Point {
    fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    fn scaled(self, scale: f32) -> Self {
        Self::new(self.x * scale, self.y * scale)
    }
    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Geometry {
    inner_position_pixels: Option<Point>,
    outer_position_pixels: Option<Point>,
    fullscreen: bool,
    maximized: bool,
    inner_size_points: Option<Point>,
}

#[derive(Default, Clone, Copy)]
pub(super) struct WindowMode {
    fullscreen: bool,
    maximized: bool,
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
        let size = self.geometry.inner_size_points?.scaled(zoom);
        (size.is_finite() && size.x > 0. && size.y > 0.).then_some([
            size.x.max(crate::window::MAIN_MIN_SIZE[0]),
            size.y.max(crate::window::MAIN_MIN_SIZE[1]),
        ])
    }
    pub fn mode(&self) -> WindowMode {
        WindowMode {
            fullscreen: self.geometry.fullscreen,
            maximized: self.geometry.maximized,
        }
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
            self.geometry.inner_size_points = Some(Point::new(width, height).scaled(1. / zoom));
        }
    }
    pub fn observe(&mut self, handle: &rune::WindowHandle) {
        if let Some((origin, mode)) = native_frame(handle) {
            self.geometry.outer_position_pixels = Some(origin);
            self.geometry.inner_position_pixels = Some(origin);
            self.geometry.maximized = mode.maximized;
            self.geometry.fullscreen = mode.fullscreen;
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
fn native_frame(handle: &rune::WindowHandle) -> Option<(Point, WindowMode)> {
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
                Point::new(
                    (frame.origin.x * scale) as f32,
                    ((primary - frame.origin.y - frame.size.height) * scale) as f32,
                ),
                WindowMode {
                    maximized: window.isZoomed(),
                    fullscreen: window
                        .styleMask()
                        .contains(objc2_app_kit::NSWindowStyleMask::FullScreen),
                },
            ))
        })
        .ok()
        .flatten()
}

#[cfg(windows)]
fn native_frame(handle: &rune::WindowHandle) -> Option<(Point, WindowMode)> {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{
            GWL_STYLE, GetWindowLongW, GetWindowRect, IsZoomed, WS_OVERLAPPEDWINDOW,
        },
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
                        Point::new(rect.left as f32, rect.top as f32),
                        WindowMode {
                            maximized: IsZoomed(hwnd) != 0,
                            fullscreen: GetWindowLongW(hwnd, GWL_STYLE) as u32
                                & WS_OVERLAPPEDWINDOW
                                == 0,
                        },
                    )
                })
            }
        })
        .ok()
        .flatten()
}

#[cfg(target_os = "linux")]
fn native_frame(handle: &rune::WindowHandle) -> Option<(Point, WindowMode)> {
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
            let atoms = x11_modes(&conn)?;
            let property = conn
                .get_property(
                    false,
                    id,
                    atoms[0],
                    x11rb::protocol::xproto::AtomEnum::ATOM,
                    0,
                    128,
                )
                .ok()?
                .reply()
                .ok()?;
            let states: Vec<_> = property.value32().into_iter().flatten().collect();
            Some((
                Point::new(f32::from(position.dst_x), f32::from(position.dst_y)),
                WindowMode {
                    fullscreen: states.contains(&atoms[1]),
                    maximized: states.contains(&atoms[2]) && states.contains(&atoms[3]),
                },
            ))
        })
        .ok()
        .flatten()
}

#[cfg(target_os = "macos")]
fn with_window<T>(
    handle: &rune::WindowHandle,
    f: impl FnOnce(&objc2_app_kit::NSWindow) -> T,
) -> Option<T> {
    use raw_window_handle::RawWindowHandle;
    handle
        .with_raw_handle(|raw| {
            let RawWindowHandle::AppKit(raw) = raw.as_raw() else {
                return None;
            };
            let _main = objc2::MainThreadMarker::new()?;
            // The callback borrows the current window's live NSView on the main thread.
            let view = unsafe { raw.ns_view.cast::<objc2_app_kit::NSView>().as_ref() };
            let window = view.window()?;
            Some(f(&window))
        })
        .ok()
        .flatten()
}

#[cfg(target_os = "macos")]
pub(super) fn fullscreen(handle: &rune::WindowHandle) -> bool {
    with_window(handle, |w| {
        w.styleMask()
            .contains(objc2_app_kit::NSWindowStyleMask::FullScreen)
    })
    .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn x11_modes(conn: &x11rb::rust_connection::RustConnection) -> Option<[u32; 4]> {
    use x11rb::protocol::xproto::ConnectionExt;
    let atom = |name: &[u8]| Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom);
    Some([
        atom(b"_NET_WM_STATE")?,
        atom(b"_NET_WM_STATE_FULLSCREEN")?,
        atom(b"_NET_WM_STATE_MAXIMIZED_VERT")?,
        atom(b"_NET_WM_STATE_MAXIMIZED_HORZ")?,
    ])
}

impl WindowMode {
    pub fn restore(self, handle: &rune::WindowHandle) {
        if !self.maximized && !self.fullscreen {
            return;
        }
        #[cfg(target_os = "macos")]
        with_window(handle, |w| {
            if self.fullscreen {
                if !w
                    .styleMask()
                    .contains(objc2_app_kit::NSWindowStyleMask::FullScreen)
                {
                    w.toggleFullScreen(None);
                }
            } else if !w.isZoomed() {
                w.zoom(None);
            }
        });
        #[cfg(windows)]
        let _ = handle.with_raw_handle(|raw| {
            use raw_window_handle::RawWindowHandle;
            use windows_sys::Win32::{Graphics::Gdi::*, UI::WindowsAndMessaging::*};
            let RawWindowHandle::Win32(raw) = raw.as_raw() else {
                return;
            };
            let hwnd = raw.hwnd.get() as *mut _;
            // All calls target the live HWND borrowed for this callback.
            unsafe {
                if self.fullscreen {
                    let mut monitor = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    if GetMonitorInfoW(
                        MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                        &mut monitor,
                    ) != 0
                    {
                        let rect = monitor.rcMonitor;
                        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                        SetWindowLongW(hwnd, GWL_STYLE, (style & !WS_OVERLAPPEDWINDOW) as i32);
                        SetWindowPos(
                            hwnd,
                            std::ptr::null_mut(),
                            rect.left,
                            rect.top,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                            SWP_NOZORDER | SWP_FRAMECHANGED,
                        );
                    }
                } else {
                    ShowWindow(hwnd, SW_MAXIMIZE);
                }
            }
        });
        #[cfg(target_os = "linux")]
        let _ = handle.with_raw_handle(|raw| -> Option<()> {
            use raw_window_handle::RawWindowHandle;
            use x11rb::{connection::Connection, protocol::xproto::*};
            let id = match raw.as_raw() {
                RawWindowHandle::Xlib(x) => u32::try_from(x.window).ok()?,
                RawWindowHandle::Xcb(x) => x.window.get(),
                _ => return None,
            };
            let (conn, screen) = x11rb::connect(None).ok()?;
            let atoms = x11_modes(&conn)?;
            let flags = if self.fullscreen {
                [atoms[1], 0]
            } else {
                [atoms[2], atoms[3]]
            };
            let root = conn.setup().roots.get(screen)?.root;
            let event = ClientMessageEvent::new(32, id, atoms[0], [1, flags[0], flags[1], 1, 0]);
            conn.send_event(
                false,
                root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .ok()?;
            conn.flush().ok()
        });
    }
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
            Some(Point::new(960., 600.))
        );
        std::fs::remove_file(path).unwrap();
    }
}
