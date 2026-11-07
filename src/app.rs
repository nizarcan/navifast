//! Presentation state shared by the Rune views and Navidrome controller.
pub mod navidrome;
use crate::{
    model::{Dialog, Page, SearchState, TableSort, Toast, ToastKind},
    settings::Settings,
    theme,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
const TOAST_LIFETIME: Duration = Duration::from_millis(3200);

pub struct App {
    pub settings: Settings,
    dirs: crate::paths::AppDirs,
    pub locale: crate::i18n::Locale,
    pub custom_themes: theme::Catalog,
    pub history: Vec<Page>,
    pub history_index: usize,
    pub recent_contexts: Vec<String>,
    pub navidrome_favorites: navidrome::favorites::State,
    pub navidrome_search: navidrome::search::State,
    pub navidrome_playlists: navidrome::playlists::State,
    pub search: SearchState,
    pub table_sorts: HashMap<Page, TableSort>,
    pub dialog: Option<Dialog>,
    pub toasts: Vec<Toast>,
    pub accents: HashMap<String, crate::color::Srgba>,
}
impl App {
    fn new(dirs: crate::paths::AppDirs, settings: Settings) -> Self {
        let session = crate::settings::SessionState::load(&dirs.session_file());
        Self {
            locale: settings.language.resolve(),
            settings,
            dirs,
            custom_themes: Default::default(),
            history: vec![Page::Albums],
            history_index: 0,
            recent_contexts: Vec::new(),
            navidrome_favorites: Default::default(),
            navidrome_search: Default::default(),
            navidrome_playlists: Default::default(),
            search: Default::default(),
            table_sorts: session
                .sorts
                .iter()
                .filter_map(|(page, sort)| Some((Page::decode(page)?, *sort)))
                .collect(),
            dialog: None,
            toasts: Vec::new(),
            accents: HashMap::new(),
        }
    }
    pub fn open(&mut self, page: Page) {
        if self.page() == &page {
            return;
        }
        self.history.truncate(self.history_index + 1);
        self.history.push(page);
        if self.history.len() > 60 {
            self.history.remove(0);
        }
        self.history_index = self.history.len() - 1;
    }
    pub(crate) fn apply_local(&mut self, action: crate::model::Action, wake: rune::Waker) {
        let p = self;
        match action {
            crate::model::Action::Back if p.can_go_back() => p.history_index -= 1,
            crate::model::Action::Forward if p.can_go_forward() => p.history_index += 1,
            crate::model::Action::Open(Page::Search) | crate::model::Action::FocusSearch => {
                p.open(Page::Search)
            }
            crate::model::Action::SetTheme(theme) => {
                p.settings.theme = theme;
                p.settings.custom_theme = None;
                p.settings.custom_theme_cache = None;
            }
            crate::model::Action::SetCustomTheme(name) => {
                if let Some(theme) = p.custom_themes.find(&name) {
                    p.settings.custom_theme_cache = Some(theme.clone());
                    p.settings.custom_theme = Some(name);
                }
            }
            crate::model::Action::SetLanguage(language) => {
                p.settings.language = language;
                p.locale = language.resolve();
            }
            crate::model::Action::ToggleSidebar => {
                p.settings.sidebar_visible = !p.settings.sidebar_visible
            }
            crate::model::Action::SetLibraryGrid(grid) => p.settings.sidebar_grid = grid,
            crate::model::Action::CyclePlayerBarVis => {
                p.settings.player_bar_vis = p.settings.player_bar_vis.next()
            }
            crate::model::Action::ReloadThemes => {
                p.custom_themes.start(
                    p.dirs.config.join("themes"),
                    p.settings.custom_theme.clone(),
                    wake,
                );
            }
            crate::model::Action::ShowDialog(dialog) => p.dialog = Some(dialog),
            crate::model::Action::CloseDialog => p.dialog = None,
            _ => {}
        }
    }
    pub fn page(&self) -> &Page {
        &self.history[self.history_index]
    }
    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }
    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }
    pub fn enable_desktop_themes(&mut self) {
        theme::enable_desktop_themes(&mut self.custom_themes, &self.dirs.config.join("themes"));
    }
    fn refresh_theme_cache(&mut self) -> bool {
        let mut changed = false;
        if let Some(filename) = &self.settings.custom_theme
            && let Some(theme) = self.custom_themes.find(filename)
            && self.settings.custom_theme_cache.as_ref() != Some(theme)
        {
            // Use the current selection, never the selection captured by the scan.
            self.settings.custom_theme_cache = Some(theme.clone());
            changed = true;
        }
        if self.custom_themes.follows_omarchy() {
            if let Some(theme) = self.custom_themes.system_theme()
                && self.settings.system_theme_cache.as_ref() != Some(theme)
            {
                self.settings.system_theme_cache = Some(theme.clone());
                changed = true;
            }
        } else if self.settings.system_theme_cache.take().is_some() {
            changed = true;
        }
        changed
    }
    pub fn toast_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        log::warn!("{message}");
        self.toasts.push(Toast {
            message,
            kind: ToastKind::Error,
            created: Instant::now(),
        });
    }
    pub fn report_update_failure(&mut self, error: String) {
        self.toast_error(error);
    }
}
pub fn eq_settings(settings: &Settings) -> crate::eq::EqSettings {
    crate::eq::EqSettings {
        on: settings.eq_on,
        preamp_db: settings.eq_preamp_db,
        bands_db: settings.eq_bands_db,
        balance: settings.balance,
        mono: settings.mono,
    }
    .clamped()
}
pub fn volume_to_percent(volume: u16) -> u8 {
    ((u32::from(volume) * 100 + u32::from(u16::MAX) / 2) / u32::from(u16::MAX)) as u8
}
pub fn percent_to_volume(percent: u8) -> u16 {
    ((u32::from(percent.min(100)) * u32::from(u16::MAX)) / 100) as u16
}
