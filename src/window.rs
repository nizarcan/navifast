//! Validate saved desktop window coordinates before restoring them.
#[cfg(not(windows))]
pub fn can_restore(pos: [f32; 2], _pixels_per_point: f32) -> bool {
    // Wayland ignores window moves; keep the old limits on other platforms.
    (-1000.0..=5000.0).contains(&pos[0]) && (-1000.0..=5000.0).contains(&pos[1])
}

#[cfg(any(windows, test))]
fn titlebar_anchor(pos: [f32; 2], pixels_per_point: f32) -> Option<[f32; 2]> {
    // A maximized window can start at (-8, -8), so check a point inside
    // its title bar. Convert from logical points to pixels for Win32.
    let anchor = [
        (pos[0] + 32.) * pixels_per_point,
        (pos[1] + 16.) * pixels_per_point,
    ];
    (pixels_per_point.is_finite() && pixels_per_point > 0.0 && anchor.iter().all(|v| v.is_finite()))
        .then_some(anchor)
}

/// Checks that the saved position leaves the title bar in a monitor's work area.
#[cfg(windows)]
pub fn can_restore(pos: [f32; 2], pixels_per_point: f32) -> bool {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromPoint,
    };

    let Some(anchor) = titlebar_anchor(pos, pixels_per_point) else {
        return false;
    };
    let point = POINT {
        x: anchor[0] as i32,
        y: anchor[1] as i32,
    };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        // Asking for the nearest monitor would also accept off-screen positions.
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONULL);
        if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
            return false;
        }
    }
    // Use the work area so the title bar cannot end up behind the taskbar.
    let area = info.rcWork;
    (area.left as f32..=area.right as f32).contains(&anchor[0])
        && (area.top as f32..=area.bottom as f32).contains(&anchor[1])
}

pub const MAIN_MIN_SIZE: [f32; 2] = [760., 520.];

#[cfg(test)]
mod tests {
    #[test]
    fn titlebar_anchor_preserves_scale_and_rejects_invalid_coordinates() {
        assert_eq!(super::titlebar_anchor([-8., -8.], 1.), Some([24., 8.]));
        assert_eq!(super::titlebar_anchor([100., 200.], 2.), Some([264., 432.]));
        for scale in [0., -1., f32::NAN, f32::INFINITY] {
            assert!(super::titlebar_anchor([0., 0.], scale).is_none());
        }
        assert!(super::titlebar_anchor([f32::NAN, 0.], 1.).is_none());
    }
}
