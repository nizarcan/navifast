//! Platform policy for Rune middle-click scrolling.
/// Whether middle-click scrolling is on: always on Windows, on Linux only by
/// choice because a middle click there usually pastes, and never on macOS.
pub fn enabled(chosen_on_linux: bool) -> bool {
    enabled_for(cfg!(windows), cfg!(target_os = "linux"), chosen_on_linux)
}

/// Scroll points per second beyond the middle-click anchor's dead zone.
pub(crate) fn speed(distance: f32) -> f32 {
    (distance - 12.).max(0.) * 6.
}

const fn enabled_for(windows: bool, linux: bool, chosen_on_linux: bool) -> bool {
    windows || (linux && chosen_on_linux)
}

#[cfg(test)]
mod choice_tests {
    use super::enabled_for;

    #[test]
    fn windows_always_autoscrolls_linux_by_choice_and_macos_never() {
        assert!(enabled_for(true, false, false));
        assert!(enabled_for(true, false, true));
        assert!(!enabled_for(false, true, false));
        assert!(enabled_for(false, true, true));
        assert!(!enabled_for(false, false, true));
        assert!(!enabled_for(false, false, false));
    }
}
