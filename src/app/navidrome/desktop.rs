//! The Rune window for the active Navidrome controller.
mod artwork;
mod placement;
mod tracks;
use super::{App, Host, presentation};
use crate::ui::desktop as views;
use crate::{
    model::{Action, Page},
    navidrome::{
        api::CoverSize,
        backend::{Cover, Request},
    },
    settings::ThemeChoice,
    theme::Palette,
};
use rune::{AppSpec, Cx, Live, TextEdit, Tx, Waker};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

pub struct Launch {
    pub dirs: crate::paths::AppDirs,
    pub link: Option<String>,
    pub demo: bool,
    pub size: Option<[f32; 2]>,
    pub page: Option<String>,
    pub flags: Option<String>,
    pub snapshot: Option<PathBuf>,
}

struct Window {
    handle: rune::AppHandle<State>,
    wake: Waker,
    delay: Cell<Option<Duration>>,
    covers: RefCell<artwork::Covers>,
}

impl Host for Window {
    fn repaint_after(&self, delay: Duration) {
        self.delay
            .set(Some(self.delay.get().map_or(delay, |d| d.min(delay))));
    }
    fn insert_cover(&self, _: &App, key: String, cover: Cover) {
        self.covers.borrow_mut().insert(key, cover);
    }
    fn retry_cover(&self, key: &str) -> bool {
        self.covers
            .borrow_mut()
            .failed(key, std::time::Instant::now());
        true
    }
    fn show_window(&self) {
        self.handle.open()
    }
    fn close_window(&self) {
        self.handle.quit()
    }
    fn local_action(&self, app: &mut App, action: Action) {
        let p = &mut app.presentation;
        match action {
            Action::Back if p.can_go_back() => p.history_index -= 1,
            Action::Forward if p.can_go_forward() => p.history_index += 1,
            Action::Open(Page::Search) | Action::FocusSearch => p.open(Page::Search),
            Action::SetTheme(theme) => {
                p.settings.theme = theme;
                p.settings.custom_theme = None;
                p.settings.custom_theme_cache = None;
            }
            Action::SetCustomTheme(name) => {
                if let Some(theme) = p.custom_themes.find(&name) {
                    p.settings.custom_theme_cache = Some(theme.clone());
                    p.settings.custom_theme = Some(name);
                }
            }
            Action::SetLanguage(language) => {
                p.settings.language = language;
                p.locale = language.resolve();
            }
            Action::ToggleSidebar => p.settings.sidebar_visible = !p.settings.sidebar_visible,
            Action::SetLibraryGrid(grid) => p.settings.sidebar_grid = grid,
            Action::CyclePlayerBarVis => {
                p.settings.player_bar_vis = p.settings.player_bar_vis.next()
            }
            Action::ReloadThemes => {
                let wake = self.wake.clone();
                p.custom_themes.start(
                    app.dirs.config.join("themes"),
                    p.settings.custom_theme.clone(),
                    &fastframe_theme::Waker::new(move || wake()),
                );
            }
            Action::ShowDialog(dialog) => p.dialog = Some(dialog),
            Action::CloseDialog => p.dialog = None,
            Action::Quit | Action::CloseWindow => self.close_window(),
            Action::ShowWindow => self.show_window(),
            _ => {}
        }
    }
}

pub(crate) struct State {
    pub(crate) app: App,
    window: Window,
    placement: Option<placement::Placement>,
    native_window: std::rc::Rc<RefCell<Option<rune::WindowHandle>>>,
    pub(crate) server: TextEdit,
    pub(crate) user: TextEdit,
    pub(crate) password: TextEdit,
    pub(crate) query: TextEdit,
    pub(crate) filter: TextEdit,
    pub(crate) library_filter: TextEdit,
    pub(crate) library_search: bool,
    pub(crate) images: rune::ImageCache,
    pub(crate) icons: RefCell<HashMap<(crate::theme::Icon, egui::Color32, u32), rune::Image>>,
    pub(crate) logos: RefCell<HashMap<u32, rune::Pixels>>,
    pub(crate) table: tracks::Tracks,
    history_tables: HashMap<Page, tracks::Tracks>,
    pub(crate) offsets: HashMap<Page, Live<f32>>,
    analyser: crate::vis::WideAnalyser,
    samples: Vec<f32>,
    next_visual_frame: Option<(crate::settings::PlayerBarVis, std::time::Instant)>,
    pub(crate) trace: Vec<f32>,
    pub(crate) caps: [Live<[f32; 4]>; crate::vis::WIDE_BANDS],
    pub(crate) bars: [Live<[f32; 4]>; crate::vis::WIDE_BANDS],
    pub(crate) plot_width: f32,
    pub(crate) seek_preview: Option<(u64, f64)>,
    pub(crate) artist_views: HashMap<Page, views::artist::Options>,
    pub(crate) favorite_menu: Option<(String, [f32; 2])>,
    pub(crate) menu: Option<&'static str>,
}

impl App {
    pub(crate) fn page_songs(&self) -> Option<&[crate::navidrome::api::Song]> {
        use crate::navidrome::api::Collection;
        match self.presentation.page() {
            Page::Album(_) => self.album.as_ref().map(|a| a.song.as_slice()),
            Page::Playlist(_) => match self.presentation.navidrome_search.collection.get() {
                Some(Collection::Playlist { songs, .. }) => Some(songs),
                _ => None,
            },
            _ => None,
        }
    }
}
impl State {
    fn new(
        launch: Launch,
        instance: Option<crate::single_instance::Guard>,
        handle: rune::AppHandle<Self>,
    ) -> Self {
        let notifier = handle.clone();
        let wake: Waker = Arc::new(move || notifier.wake());
        let window = Window {
            handle,
            delay: Cell::default(),
            covers: RefCell::default(),
            wake: wake.clone(),
        };
        let app = App::with_host(
            launch.dirs,
            instance,
            launch.demo,
            &Default::default(),
            wake.clone(),
        );
        #[cfg(feature = "demo")]
        let app = {
            let mut app = app;
            if launch.demo {
                app.populate_demo_model(launch.page.as_deref(), launch.flags.as_deref(), &window);
            }
            app
        };
        let flags = launch
            .flags
            .as_deref()
            .unwrap_or("")
            .split(',')
            .collect::<Vec<_>>();
        let filter = if flags.contains(&"appearance") {
            "Appearance"
        } else if flags.contains(&"about") {
            "About"
        } else {
            ""
        };
        let page = app.presentation.page().clone();
        let mut state = Self {
            server: TextEdit::new(&app.login.server),
            user: TextEdit::new(&app.login.username),
            password: TextEdit::new("").secure(),
            query: TextEdit::new(&app.presentation.search.query),
            filter: TextEdit::new(filter),
            library_filter: TextEdit::new(""),
            library_search: false,
            app,
            window,
            placement: None,
            native_window: Default::default(),
            images: rune::ImageCache::new(wake),
            icons: RefCell::default(),
            logos: RefCell::default(),
            table: Default::default(),
            history_tables: Default::default(),
            offsets: HashMap::from([(page, Live::new(0.))]),
            analyser: Default::default(),
            samples: Vec::new(),
            next_visual_frame: None,
            trace: Vec::new(),
            caps: std::array::from_fn(|_| Live::new([0.; 4])),
            bars: std::array::from_fn(|_| Live::new([0.; 4])),
            plot_width: 0.,
            seek_preview: None,
            artist_views: Default::default(),
            favorite_menu: None,
            menu: None,
        };
        state.sync_tracks();
        state
    }

    pub(crate) fn resized(&mut self, width: f32, height: f32) {
        let Some(placement) = &mut self.placement else {
            return;
        };
        placement.resize([width, height], self.app.presentation.settings.zoom);
    }
    fn save_placement(&mut self) {
        let Some(placement) = &mut self.placement else {
            return;
        };
        if let Some(handle) = self.native_window.borrow().as_ref() {
            placement.observe(handle);
        }
        if let Some((path, json)) = placement.update() {
            self.app.backend.send(Request::Save { path, json });
        }
    }
    fn sync_tracks(&mut self) {
        if let Some(songs) = self.app.page_songs() {
            self.table.update(songs);
        }
    }
    pub(crate) fn dispatch(&mut self, action: Action, tx: &mut Tx) {
        let previous = self.app.presentation.page().clone();
        let session = self.app.session;
        self.app.ui_action(action, &self.window);
        self.menu = None;
        self.favorite_menu = None;
        if session != self.app.session {
            self.window.covers.replace(Default::default());
            self.offsets.clear();
            self.history_tables.clear();
            self.artist_views.clear();
            self.table = Default::default();
            self.library_filter.clear();
            self.library_search = false;
            self.password.clear();
            self.query.clear();
        }
        let page = self.app.presentation.page().clone();
        if previous != page {
            self.query.focused = page == Page::Search;
            if page != Page::Search {
                tx.blur();
            }
            let next = self.history_tables.remove(&page).unwrap_or_default();
            let old = std::mem::replace(&mut self.table, next);
            if session == self.app.session {
                self.history_tables.insert(previous, old);
            }
            self.history_tables
                .retain(|p, _| self.app.presentation.history.contains(p));
            self.sync_tracks();
            tx.scroll_to_y(
                "page",
                self.offsets
                    .entry(page)
                    .or_insert_with(|| Live::new(0.))
                    .get(),
            );
        }
        tx.instant();
    }

    pub(crate) fn palette(&self, cx: &Cx<Self>) -> Palette {
        let s = &self.app.presentation.settings;
        s.cached_palette().unwrap_or_else(|| {
            if matches!(s.theme, ThemeChoice::Dark)
                || (s.theme == ThemeChoice::System && cx.system_dark())
            {
                Palette::dark()
            } else {
                Palette::light()
            }
        })
    }

    pub(crate) fn cover_image(&self, id: &str, size: CoverSize) -> (String, Option<rune::Pixels>) {
        let uri = presentation::artwork_uri(self.app.session, id, size);
        let covers = self.window.covers.borrow();
        (covers.view_key(&uri), covers.get(&uri))
    }

    pub(crate) fn request_cover(&mut self, id: &str, size: CoverSize) {
        let key = presentation::artwork_uri(self.app.session, id, size);
        let Some(cached) = self.window.covers.borrow_mut().visible(&key) else {
            return;
        };
        if let Some(client) = &self.app.client
            && self.app.requested_covers.insert(key)
        {
            self.app.backend.send(Request::Cover {
                session: self.app.session,
                client: client.clone(),
                size,
                id: id.to_owned(),
                cached,
            });
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.save_placement();
        self.app.save();
    }
}

fn poll(s: &mut State, _time: f64, tx: &mut Tx) -> bool {
    let restoring = s.app.restoring_login;
    let mut changed = s.app.poll(&s.window);
    if changed {
        s.sync_tracks();
    }
    let (retry, deadline) = s
        .window
        .covers
        .borrow_mut()
        .poll_retries(std::time::Instant::now());
    changed |= retry;
    if let Some(delay) = deadline {
        tx.wake_after(delay);
    }
    if restoring && !s.app.restoring_login {
        s.server.replace_text(&s.app.login.server);
        s.user.replace_text(&s.app.login.username);
        if !s.app.connected {
            s.password.replace_text(&s.app.login.password);
        }
    }
    if s.app.presentation.custom_themes.needs_reload() {
        s.window.local_action(&mut s.app, Action::ReloadThemes);
    }
    if s.app.presentation.custom_themes.poll() {
        if s.app.presentation.refresh_theme_cache() {
            s.app.mark_dirty();
        }
        changed = true;
    }
    let toasts = &mut s.app.presentation.toasts;
    let old = toasts.len();
    toasts.retain(|t| t.created.elapsed() < crate::app::TOAST_LIFETIME);
    changed |= toasts.len() != old;
    if let Some(t) = toasts.first() {
        tx.wake_after(crate::app::TOAST_LIFETIME.saturating_sub(t.created.elapsed()));
    }
    // Align widgets with a completed credential restore; sensitive entry belongs
    // only to the login form and is cleared on leaving it.
    if s.query.focused && s.app.connected && s.app.presentation.page() != &Page::Search {
        s.dispatch(Action::Open(Page::Search), tx);
        changed = true;
    }
    if s.app.connected && !s.password.text.is_empty() {
        s.password.clear();
    }
    if s.app.dirty.is_some() {
        tx.wake_after(Duration::from_millis(500));
    }
    if let Some(delay) = s.window.delay.take() {
        tx.wake_after(delay);
    }
    if s.window.handle.is_visible() && s.plot_width > 0. {
        let sounding = !s.app.transport.paused && s.app.transport.song().is_some();
        let mode = s.app.presentation.settings.player_bar_vis;
        use crate::{settings::PlayerBarVis, vis};
        let now = std::time::Instant::now();
        if let Some((previous, deadline)) = s.next_visual_frame
            && previous == mode
            && now < deadline
        {
            tx.wake_after(deadline - now);
            return changed;
        }
        s.next_visual_frame = None;
        match mode {
            PlayerBarVis::Spectrum if sounding || !s.analyser.settled() => {
                s.samples.clear();
                if sounding {
                    s.app
                        .tap
                        .window_into(vis::WIDE_SAMPLES, vis::LAG, &mut s.samples);
                }
                let levels = s.analyser.step(&s.samples, now);
                let peaks = s.analyser.peaks();
                let stride = (s.plot_width + 2.) / vis::WIDE_BANDS as f32;
                let height = crate::theme::PLAYER_BAR_HEIGHT;
                for (i, level) in levels.iter().enumerate() {
                    let x = i as f32 * stride;
                    let band = (height - 5.) * level;
                    let peak = (height - 5.) * peaks[i];
                    s.bars[i].set([x, height - band, (stride - 2.).max(0.), band]);
                    s.caps[i].set([
                        x,
                        height - peak - 4.,
                        (stride - 2.).max(0.),
                        if peak >= 2. { 2. } else { 0. },
                    ]);
                }
                s.next_visual_frame = Some((mode, now + crate::ui::player_bar::VIS_FRAME));
                tx.wake_after(crate::ui::player_bar::VIS_FRAME);
            }
            PlayerBarVis::Waveform if sounding => {
                let points = (s.plot_width / 8.).clamp(75., 320.) as usize;
                s.app
                    .tap
                    .window_into(points * vis::SCOPE_STEP, vis::LAG, &mut s.samples);
                s.trace = vis::scope_line(&s.samples, points);
                s.next_visual_frame = Some((mode, now + crate::ui::player_bar::VIS_FRAME));
                tx.wake_after(crate::ui::player_bar::VIS_FRAME);
                changed = true;
            }
            _ if !s.trace.is_empty() => {
                s.trace.clear();
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

pub fn run(launch: Launch, prepare: impl FnOnce(&crate::paths::AppDirs)) -> Result<(), String> {
    let mut args = Vec::new();
    if let Some(path) = &launch.snapshot {
        args.extend([
            "--snap".to_owned(),
            path.to_string_lossy().into_owned(),
            "--at".into(),
            "1".into(),
            "--scale".into(),
            "2".into(),
        ]);
    }
    let placement = (!launch.demo)
        .then(|| {
            eframe::storage_dir("navifast")
                .and_then(|dir| placement::Placement::load(dir.join("app.ron")))
        })
        .flatten();
    let restored = placement.as_ref().and_then(|p| {
        p.size(
            crate::settings::Settings::load(&launch.dirs.settings_file())
                .zoom
                .clamp(0.5, 2.5),
        )
    });
    let position = placement.as_ref().and_then(placement::Placement::position);
    let [w, h] = launch.size.or(restored).unwrap_or([1240., 800.]);
    let spec = AppSpec {
        poll: Some(poll),
        on_key: Some(views::keys),
        fonts: &[fastframe_fonts::INTER],
        min_size: Some((
            crate::window::MAIN_MIN_SIZE[0],
            crate::window::MAIN_MIN_SIZE[1],
        )),
        ..AppSpec::new(
            "Navifast",
            (w, h),
            || unreachable!("state is supplied by the launcher"),
            views::view,
        )
    };
    #[cfg(target_os = "linux")]
    {
        static DESKTOP_ENTRY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        rune::set_app_id(DESKTOP_ENTRY.get_or_init(crate::media_controls::desktop_entry));
    }
    let app = rune::App::new(spec).chrome(rune::Chrome {
        drag_regions: true,
        ..rune::Chrome::DEFAULT
    });
    let native_window = std::rc::Rc::new(RefCell::new(None));
    let observe = native_window.clone();
    let app = app
        .window_options(rune::WindowOptions {
            position,
            ..Default::default()
        })
        .on_window(move |handle| {
            *observe.borrow_mut() = Some(handle.clone());
        })
        .on_close(|s, _| s.save_placement())
        .on_focus(|s, on, _| {
            if !on {
                s.save_placement();
            }
        });
    let handle = app.handle();
    let notify = handle.clone();
    let instance = if launch.demo {
        None
    } else {
        match crate::single_instance::acquire_with_wake(
            &fastframe_theme::Waker::new(move || notify.wake()),
            launch.link.as_deref(),
        ) {
            crate::single_instance::Outcome::Only(guard) => Some(guard),
            crate::single_instance::Outcome::Surfaced => return Ok(()),
        }
    };
    prepare(&launch.dirs);
    app.args(args)
        .init(move || {
            let mut state = State::new(launch, instance, handle);
            state.native_window = native_window;
            state.placement = placement;
            state
        })
        .try_run()
}

#[cfg(all(test, feature = "demo"))]
mod tests {
    use super::*;
    use rune::{Env, Key, KeyEvent, Mods, Runtime};

    fn runtime(page: &str, flags: &str) -> Runtime<State> {
        let root =
            std::env::temp_dir().join(format!("navifast-rune-{:016x}", rand::random::<u64>()));
        let launch = Launch {
            dirs: crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            link: None,
            demo: true,
            size: Some([1280., 800.]),
            page: Some(page.into()),
            flags: Some(flags.into()),
            snapshot: None,
        };
        let spec = AppSpec::new("Navifast", (1280., 800.), || unreachable!(), views::view);
        let app = rune::App::new(spec);
        let state = State::new(launch, None, app.handle());
        let mut rt = Runtime::new(state, views::view, (1280., 800.), Env::new(true, 2.));
        rt.frame(0.);
        rt.settle_all();
        rt
    }

    fn click_row(rt: &mut Runtime<State>, i: usize) {
        let key = rt.state.table.key(i).to_owned();
        let el = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_ref() == Some(&key))
            .unwrap();
        let [x, y, _, h] = el.rect;
        rt.pointer_down(x + 10., y + h / 2.);
        rt.pointer_up(x + 10., y + h / 2.);
        rt.frame(0.);
    }

    #[test]
    fn album_keyboard_plays_the_selected_occurrence_after_scrolling() {
        let mut rt = runtime("album:alb0", "");
        // Make the two duplicate entries reachable only after the header scrolls out.
        rt.update(|s, _| {
            let album = s.app.album.as_mut().unwrap();
            let track = album.song[0].clone();
            album.song = vec![track; 100];
            s.sync_tracks();
        });
        rt.update(|s, tx| tx.scroll_into_view(s.table.key(76)));
        rt.frame(0.);
        rt.settle_all();
        click_row(&mut rt, 76);
        assert_eq!(rt.state.table.cursor, Some(76));
        for key in [Key::Down, Key::Enter] {
            rt.key(KeyEvent {
                key,
                mods: Mods::default(),
            });
            rt.frame(0.);
        }
        assert_eq!(rt.state.table.cursor, Some(77));
        assert_eq!(rt.state.app.transport.index, Some(77));
        let old = rt.state.offsets[rt.state.app.presentation.page()].get();
        assert!(old > 1000.);
        rt.update(|s, tx| s.dispatch(Action::Open(Page::Settings), tx));
        rt.update(|s, tx| s.dispatch(Action::Back, tx));
        rt.frame(0.);
        assert_eq!(rt.state.table.cursor, Some(77));
    }

    #[test]
    fn a_filtered_equalizer_exposes_controls_and_updates_the_playback_filter() {
        let mut rt = runtime("settings", "");
        rt.update(|s, _| s.filter.replace_text("Equalizer"));
        rt.frame(0.);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("Pre"));
        assert!(!tree.contains("Open themes folder"));
        assert!(rt.accessibility_action("eq-1", rune::accesskit::Action::Increment, None));
        rt.frame(0.);
        assert_eq!(rt.state.app.presentation.settings.eq_bands_db[0], 0.1);
        assert_eq!(rt.state.app.eq.lock().unwrap().bands_db[0], 0.1);
    }

    #[test]
    fn transport_accessibility_press_toggles_real_controller_state() {
        let mut rt = runtime("album:alb0", "");
        let previous = rt.state.app.transport.paused;
        assert!(rt.accessibility_action("toggle", rune::accesskit::Action::Click, None));
        rt.frame(0.);
        assert_ne!(rt.state.app.transport.paused, previous);
    }

    #[test]
    fn login_exposes_a_named_password_field_and_connect_action() {
        let rt = runtime("albums", "signed-out");
        let tree = rt.accessibility_json();
        fn named<'a>(tree: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
            if tree["name"] == name {
                return Some(tree);
            }
            tree["children"]
                .as_array()?
                .iter()
                .find_map(|node| named(node, name))
        }
        assert!(named(&tree, "Password").is_some());
        let button = named(&tree, "Connect").unwrap();
        assert_eq!(button["role"], "button");
        assert!(
            button["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a == "click")
        );
    }
}
