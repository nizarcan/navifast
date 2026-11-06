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

impl Host for egui::Context {
    fn repaint_after(&self, delay: Duration) {
        self.request_repaint_after(delay);
    }

    fn insert_cover(&self, app: &App, key: String, cover: Cover) {
        app.artwork.insert(self, key, cover);
        self.request_repaint();
    }

    fn local_action(&self, app: &mut App, action: Action) {
        let traversing = matches!(action, Action::Back | Action::Forward);
        app.presentation.apply(action, self);
        if traversing {
            crate::ui::restore_navidrome_scroll(self, app.presentation.page());
        }
    }

    fn show_window(&self) {
        self.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        self.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn close_window(&self) {
        self.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
