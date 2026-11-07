//! Effects that the Navidrome controller hands back to the active window.

use super::App;
use crate::model::Action;
use crate::navidrome::backend::Cover;
use std::time::Duration;

pub(super) trait Host {
    fn repaint_after(&self, delay: Duration);
    fn insert_cover(&self, app: &App, key: String, cover: Cover);
    /// A renderer with retryable artwork can release its in-flight request.
    fn retry_cover(&self, _key: &str) -> bool {
        false
    }
    fn local_action(&self, app: &mut App, action: Action);
    fn show_window(&self);
    fn close_window(&self);
}

#[cfg(test)]
pub(super) struct TestHost;
#[cfg(test)]
impl Host for TestHost {
    fn repaint_after(&self, _: Duration) {}
    fn insert_cover(&self, _: &App, _: String, _: Cover) {}
    fn local_action(&self, app: &mut App, action: Action) {
        app.presentation
            .apply_local(action, std::sync::Arc::new(|| {}));
    }
    fn show_window(&self) {}
    fn close_window(&self) {}
}
