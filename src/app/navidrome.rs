//! Stage-one Navidrome application state. Views return actions; apply them here.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

mod presentation;
#[cfg(feature = "demo")]
use egui::load::ImageLoader;
use presentation::Artwork;

type PresentationRevision = (u64, u64, u64, usize, bool, bool, bool, Option<String>);

use crate::navidrome::{
    api::{Album, Client, PAGE_SIZE, Song},
    backend::{Backend, Event, Request},
    player::{self, Player},
};

#[derive(Clone, Default)]
pub struct Login {
    pub server: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub server: String,
    pub username: String,
    pub light: bool,
    pub volume: f32,
    pub eq_on: bool,
    pub bands: [f32; 10],
    pub preamp: f32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            server: String::new(),
            username: String::new(),
            light: false,
            volume: 0.7,
            eq_on: false,
            bands: [0.0; 10],
            preamp: 0.0,
        }
    }
}

pub enum Action {
    EditLogin(Login),
    Login,
    Cancel,
    SignOut,
    Browse(bool),
    More,
    Open(String),
    Back,
    Retry,
    Play(usize),
    Toggle,
    Pause(bool),
    Next,
    Previous,
    Seek(f64),
    Volume(f32),
    Theme,
    Equalizer,
    SetEq {
        on: bool,
        bands: [f32; 10],
        preamp: f32,
    },
    Cover(String),
}

#[derive(Default)]
pub struct Transport {
    pub songs: Vec<Song>,
    pub index: Option<usize>,
    pub position: f64,
    pub paused: bool,
    pub loading: bool,
    pub generation: u64,
    pub error: Option<String>,
}

impl Transport {
    pub fn song(&self) -> Option<&Song> {
        self.index.and_then(|index| self.songs.get(index))
    }
    fn media_uri(&self, session: u64) -> Option<String> {
        self.song()
            .map(|song| format!("navidrome:track:{session}:{}", song.id))
    }
    pub fn next_index(&self) -> Option<usize> {
        self.index
            .and_then(|index| (index + 1 < self.songs.len()).then_some(index + 1))
    }
    fn select(&mut self, index: usize, position: f64, paused: bool) -> Option<Song> {
        let song = self.songs.get(index)?.clone();
        self.index = Some(index);
        self.position = position.max(0.0).min(song.duration.max(0.0));
        self.paused = paused;
        self.loading = true;
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
        Some(song)
    }
    fn event(&mut self, event: player::Event) -> Option<usize> {
        match event {
            player::Event::Position {
                generation,
                seconds,
                started,
                ..
            } if generation == self.generation => {
                // Pause is the user's intention; an older output report cannot undo it.
                self.position = seconds.min(self.song().map_or(seconds, |song| song.duration));
                if started {
                    self.loading = false;
                }
            }
            player::Event::Finished(generation) if generation == self.generation => {
                if let Some(index) = self.next_index() {
                    return Some(index);
                }
                self.paused = true;
                self.loading = false;
                self.position = self.song().map_or(0.0, |song| song.duration);
            }
            player::Event::Failed {
                generation,
                message,
            } if generation == self.generation => {
                self.paused = true;
                self.loading = false;
                self.error = Some(message);
            }
            _ => {}
        }
        None
    }
}

pub struct App {
    presentation: Box<super::App>,
    artwork: Arc<Artwork>,
    presentation_revision: Option<PresentationRevision>,
    pending_play: Option<(String, usize)>,
    playing_album: Option<Album>,
    pub login: Login,
    pub connecting: bool,
    pub connected: bool,
    pub preferences: Preferences,
    pub albums: Vec<Album>,
    pub album: Option<Album>,
    pub list_loading: bool,
    pub album_loading: bool,
    pub has_more: bool,
    pub newest: bool,
    pub error: Option<String>,
    pub transport: Transport,
    pub requested_covers: HashSet<String>,
    pub show_eq: bool,
    pub spectrum: [f32; crate::vis::WIDE_BANDS],
    pub palette: crate::theme::Palette,
    backend: Backend,
    player: Player,
    player_events: mpsc::Receiver<player::Event>,
    client: Option<Client>,
    session: u64,
    list_request: u64,
    album_request: u64,
    eq: crate::eq::SharedEq,
    tap: Arc<crate::vis::AudioTap>,
    analyser: crate::vis::WideAnalyser,
    dirs: crate::paths::AppDirs,
    dirty: Option<Instant>,
    proxy: crate::settings::ProxyConfig,
    commands: Option<Arc<Mutex<Vec<crate::single_instance::ControlCommand>>>>,
    now_slot: Option<Arc<Mutex<String>>>,
    media: Option<crate::media_controls::MediaService>,
    demo: bool,
    _instance: Option<crate::single_instance::Guard>,
    #[cfg(feature = "demo")]
    pub shot: Option<(std::path::PathBuf, Instant, bool)>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        dirs: crate::paths::AppDirs,
        instance: Option<crate::single_instance::Guard>,
        demo: bool,
    ) -> Self {
        crate::theme::install(&cc.egui_ctx);
        let mut preferences: Preferences = std::fs::read(dirs.config.join("navidrome.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        preferences.volume = preferences.volume.clamp(0.0, 1.0);
        let mut settings = crate::settings::Settings::load(&dirs.settings_file());
        if !dirs.settings_file().exists() {
            settings.volume = (preferences.volume * f32::from(u16::MAX)).round() as u16;
            settings.eq_on = preferences.eq_on;
            settings.eq_bands_db = preferences.bands;
            settings.eq_preamp_db = preferences.preamp;
            if preferences.light {
                settings.theme = crate::settings::ThemeChoice::Light;
            }
        }
        // Keep the original interface state, but never authorize its Spotify backend.
        settings.check_for_updates = false;
        settings.winamp_window = false;
        preferences.volume = f32::from(settings.volume) / f32::from(u16::MAX);
        preferences.eq_on = settings.eq_on;
        preferences.bands = settings.eq_bands_db;
        preferences.preamp = settings.eq_preamp_db;
        let waker = crate::backend::Waker::default();
        waker.attach(&cc.egui_ctx);
        let mut presentation = Box::new(super::App::new(
            &waker,
            dirs.clone(),
            settings,
            super::AppOptions {
                restore_sign_in: false,
                media_controls: false,
                tray: false,
            },
        ));
        presentation.backend.set_offline(true);
        presentation.offline = true;
        presentation.navidrome_mode = true;
        presentation.history = vec![crate::model::Page::Albums];
        presentation.history_index = 0;
        presentation.attach(&cc.egui_ctx);
        if !demo {
            presentation.enable_desktop_themes();
        }
        presentation.load_custom_themes(&waker);
        let eq = presentation.winamp.eq.clone();
        let tap = presentation.winamp.tap.clone();
        let artwork = Arc::new(Artwork::default());
        cc.egui_ctx.add_image_loader(artwork.clone());
        let backend = Backend::spawn(cc.egui_ctx.clone());
        // System proxy honors normal desktop configuration. Advanced proxy UI
        // is retained in the old modules until the full settings migration.
        let proxy = crate::settings::ProxyConfig::System;
        let (events, player_events) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        let player = Player::spawn(
            proxy.clone(),
            eq.clone(),
            tap.clone(),
            Arc::new(move |event| {
                let _ = events.send(event);
                ctx.request_repaint();
            }),
        );
        player.send(player::Command::Volume(preferences.volume));
        let ctx = cc.egui_ctx.clone();
        let media = (!demo)
            .then(|| crate::media_controls::MediaService::spawn(move || ctx.request_repaint()));
        let newest = presentation
            .settings
            .library_sort
            .get(&crate::settings::LibraryShelf::Albums)
            != Some(&crate::settings::LibrarySort::Name);
        Self {
            presentation,
            artwork,
            presentation_revision: None,
            pending_play: None,
            playing_album: None,
            login: Login {
                server: preferences.server.clone(),
                username: preferences.username.clone(),
                password: String::new(),
            },
            connecting: false,
            connected: false,
            preferences,
            albums: Vec::new(),
            album: None,
            list_loading: false,
            album_loading: false,
            has_more: false,
            newest,
            error: None,
            transport: Transport {
                paused: true,
                ..Default::default()
            },
            requested_covers: HashSet::new(),
            show_eq: false,
            spectrum: [0.0; crate::vis::WIDE_BANDS],
            palette: crate::theme::Palette::dark(),
            backend,
            player,
            player_events,
            client: None,
            session: 0,
            list_request: 0,
            album_request: 0,
            eq,
            tap,
            analyser: Default::default(),
            dirs,
            dirty: None,
            proxy,
            commands: instance.as_ref().map(|guard| guard.commands()),
            now_slot: instance.as_ref().map(|guard| guard.now_playing_slot()),
            media,
            demo,
            _instance: instance,
            #[cfg(feature = "demo")]
            shot: None,
        }
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::EditLogin(login) => self.login = login,
            Action::Login => {
                if self.demo {
                    self.connected = true;
                    return;
                }
                if let Err(message) = crate::navidrome::api::server_url(&self.login.server) {
                    self.error = Some(message);
                    return;
                }
                self.session = self.session.wrapping_add(1);
                self.backend.send(Request::Cancel);
                self.connecting = true;
                self.error = None;
                self.backend.send(Request::Login {
                    session: self.session,
                    server: self.login.server.clone(),
                    username: self.login.username.clone(),
                    password: self.login.password.clone(),
                    proxy: self.proxy.clone(),
                });
            }
            Action::Cancel => {
                self.session = self.session.wrapping_add(1);
                self.backend.send(Request::Cancel);
                self.connecting = false;
            }
            Action::SignOut => {
                self.session = self.session.wrapping_add(1);
                self.backend.send(Request::Cancel);
                self.player.send(player::Command::Stop);
                self.client = None;
                self.connected = false;
                self.connecting = false;
                self.login.password.clear();
                self.albums.clear();
                self.album = None;
                self.requested_covers.clear();
                let generation = self.transport.generation.wrapping_add(1);
                self.playing_album = None;
                self.transport = Transport {
                    generation,
                    paused: true,
                    ..Default::default()
                };
                self.list_loading = false;
                self.album_loading = false;
                self.error = None;
                self.tap.clear();
            }
            Action::Browse(newest) => {
                self.newest = newest;
                // Reordering the sidebar does not navigate away from an album
                // or erase the visible library while its replacement loads.
                self.list_request += 1;
                self.fetch_albums(0);
            }
            Action::More => {
                if !self.list_loading {
                    self.fetch_albums(self.albums.len());
                }
            }
            Action::Open(id) => {
                self.album_request += 1;
                self.album_loading = true;
                self.error = None;
                self.album = self.albums.iter().find(|album| album.id == id).cloned();
                if let Some(client) = &self.client {
                    self.backend.send(Request::Album {
                        session: self.session,
                        request: self.album_request,
                        client: client.clone(),
                        id,
                    });
                }
                if self.demo {
                    self.album_loading = false;
                }
            }
            Action::Back => {
                self.album = None;
                self.album_loading = false;
                self.album_request += 1;
                self.error = None;
            }
            Action::Retry => {
                if let Some(album) = &self.album {
                    self.apply(Action::Open(album.id.clone()));
                } else {
                    self.fetch_albums(self.albums.len());
                }
            }
            Action::Play(index) => {
                if let Some(album) = &self.album {
                    self.transport.songs = album.song.clone();
                    self.load(index, 0.0, false);
                }
            }
            Action::Toggle => {
                if self.transport.error.is_some()
                    || self
                        .transport
                        .song()
                        .is_some_and(|song| self.transport.position >= song.duration)
                {
                    if let Some(index) = self.transport.index {
                        let position = if self.transport.error.is_some() {
                            self.transport.position
                        } else {
                            0.0
                        };
                        self.load(index, position, false);
                    }
                } else {
                    self.apply(Action::Pause(!self.transport.paused));
                }
            }
            Action::Pause(paused) => {
                if !paused
                    && (self.transport.error.is_some()
                        || self
                            .transport
                            .song()
                            .is_some_and(|song| self.transport.position >= song.duration))
                {
                    self.apply(Action::Toggle);
                    return;
                }
                self.transport.paused = paused;
                self.player.send(player::Command::Pause(paused));
            }
            Action::Next => {
                if let Some(index) = self.transport.next_index() {
                    self.load(index, 0.0, self.transport.paused);
                }
            }
            Action::Previous => {
                if let Some(index) = self.transport.index {
                    let index = if self.transport.position > 3.0 {
                        index
                    } else {
                        index.saturating_sub(1)
                    };
                    self.load(index, 0.0, self.transport.paused);
                }
            }
            Action::Seek(seconds) => {
                if let Some(index) = self.transport.index {
                    self.load(index, seconds, self.transport.paused);
                    if let Some(media) = &self.media {
                        media.seeked((self.transport.position * 1000.0) as u32);
                    }
                }
            }
            Action::Volume(volume) => {
                self.preferences.volume = volume.clamp(0.0, 1.0);
                self.player
                    .send(player::Command::Volume(self.preferences.volume));
                self.mark_dirty();
            }
            Action::Theme => {
                self.preferences.light = !self.preferences.light;
                self.mark_dirty();
            }
            Action::Equalizer => self.show_eq = !self.show_eq,
            Action::SetEq { on, bands, preamp } => {
                let settings = crate::eq::EqSettings {
                    on,
                    bands_db: bands,
                    preamp_db: preamp,
                    ..Default::default()
                }
                .clamped();
                self.preferences.eq_on = settings.on;
                self.preferences.bands = settings.bands_db;
                self.preferences.preamp = settings.preamp_db;
                *self.eq.lock().unwrap_or_else(|p| p.into_inner()) = settings;
                self.mark_dirty();
            }
            Action::Cover(id) => {
                if self.requested_covers.insert(id.clone())
                    && let Some(client) = &self.client
                {
                    self.backend.send(Request::Cover {
                        session: self.session,
                        client: client.clone(),
                        cached: self.artwork.cached_bytes(&self.cover_uri(&id)),
                        id,
                    });
                }
            }
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty.get_or_insert_with(Instant::now);
    }

    fn save(&mut self) {
        if !self.demo
            && let Ok(json) = serde_json::to_string_pretty(&self.preferences)
        {
            self.backend.send(Request::Save {
                path: self.dirs.config.join("navidrome.json"),
                json,
            });
            if let Ok(json) = serde_json::to_string_pretty(&self.presentation.settings) {
                self.backend.send(Request::Save {
                    path: self.dirs.settings_file(),
                    json,
                });
            }
        }
        self.presentation.settings_dirty = false;
        self.dirty = None;
    }

    fn fetch_albums(&mut self, offset: usize) {
        self.error = None;
        self.list_loading = true;
        if let Some(client) = &self.client {
            self.backend.send(Request::Albums {
                session: self.session,
                request: self.list_request,
                client: client.clone(),
                offset,
                newest: self.newest,
            });
        }
        if self.demo {
            self.list_loading = false;
        }
    }

    fn load(&mut self, index: usize, position: f64, paused: bool) {
        self.pending_play = None;
        let Some(song) = self.transport.select(index, position, paused) else {
            return;
        };
        if let Some(album) = &self.album
            && album.song.iter().any(|item| item.id == song.id)
        {
            self.playing_album = Some(album.clone());
            let uri = format!("navidrome:album:{}", presentation::encode(&album.id));
            self.presentation.recent_contexts.retain(|old| old != &uri);
            self.presentation.recent_contexts.insert(0, uri);
            self.presentation.recent_contexts.truncate(100);
        }
        self.tap.clear();
        if let Some(client) = &self.client {
            self.player.send(player::Command::Load {
                generation: self.transport.generation,
                client: client.clone(),
                song: Box::new(song),
                position: self.transport.position,
                paused,
            });
        }
        if self.demo {
            self.transport.loading = false;
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        for event in self.backend.events.try_iter().collect::<Vec<_>>() {
            match event {
                Event::Login { session, result } if session == self.session => {
                    self.connecting = false;
                    match result {
                        Ok(client) => {
                            self.preferences.server = client.base_url().into();
                            self.preferences.username = client.username().into();
                            self.login.password.clear();
                            self.client = Some(client);
                            self.connected = true;
                            self.mark_dirty();
                            self.list_request += 1;
                            self.fetch_albums(0);
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Event::Albums {
                    session,
                    request,
                    offset,
                    result,
                } if session == self.session && request == self.list_request => {
                    self.list_loading = false;
                    match result {
                        Ok(albums) => {
                            self.has_more = albums.len() == PAGE_SIZE;
                            if offset == 0 {
                                self.albums = albums;
                            } else if offset == self.albums.len() {
                                self.albums.extend(albums);
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Event::Album {
                    session,
                    request,
                    result,
                } if session == self.session && request == self.album_request => {
                    self.album_loading = false;
                    match result {
                        Ok(album) => {
                            let play = self.pending_play.take().filter(|(id, _)| *id == album.id);
                            self.album = Some(album);
                            if let Some((_, index)) = play {
                                self.transport.songs = self.album.as_ref().unwrap().song.clone();
                                self.load(index, 0.0, false);
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Event::Cover {
                    session,
                    id,
                    result: Ok(cover),
                } if session == self.session => {
                    self.requested_covers.remove(&id);
                    let uri = self.cover_uri(&id);
                    if let Some(color) = cover.color {
                        let tint = self.presentation.palette.tint_from_art(color);
                        self.presentation.accents.insert(uri.clone(), tint);
                    }
                    self.artwork.insert(ctx, uri, cover);
                    ctx.request_repaint();
                }
                Event::SaveFailed => self.error = Some(
                    "Your preferences could not be saved. Check the config directory permissions."
                        .into(),
                ),
                _ => {}
            }
        }
        for event in self.player_events.try_iter().collect::<Vec<_>>() {
            if let player::Event::Failed {
                generation,
                message,
            } = &event
                && *generation == self.transport.generation
            {
                self.presentation.toast_error(message.clone());
            }
            if let Some(index) = self.transport.event(event) {
                self.load(index, 0.0, self.transport.paused);
            }
        }
        if self
            .dirty
            .is_some_and(|since| since.elapsed() > Duration::from_millis(500))
        {
            self.save();
        }
        self.poll_controls(ctx);
        self.sync_media();
        self.spectrum = self.analyser.step(
            &self.tap.window(crate::vis::WIDE_SAMPLES, crate::vis::LAG),
            Instant::now(),
        );
    }

    fn poll_controls(&mut self, ctx: &egui::Context) {
        use crate::media::MediaCommand as M;
        if let Some(media) = &self.media {
            for command in media.drain_commands() {
                let action = match command {
                    M::Play => Some(Action::Pause(false)),
                    M::Pause | M::Stop => Some(Action::Pause(true)),
                    M::PlayPause => Some(Action::Toggle),
                    M::Next => Some(Action::Next),
                    M::Previous => Some(Action::Previous),
                    M::SeekBy(ms) => {
                        Some(Action::Seek(self.transport.position + ms as f64 / 1000.0))
                    }
                    M::SetPosition {
                        track_uri,
                        position_ms,
                    } => (self.transport.media_uri(self.session).as_deref()
                        == Some(track_uri.as_str()))
                    .then_some(Action::Seek(f64::from(position_ms) / 1000.0)),
                    M::SetVolume(volume) => Some(Action::Volume(volume as f32)),
                    M::Raise => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        None
                    }
                    M::Quit => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        None
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    self.apply(action);
                }
            }
        }
        let commands = self
            .commands
            .as_ref()
            .map(|commands| {
                std::mem::take(&mut *commands.lock().unwrap_or_else(|p| p.into_inner()))
            })
            .unwrap_or_default();
        for command in commands {
            use crate::single_instance::ControlCommand as C;
            let action = match command {
                C::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    None
                }
                C::ReloadThemes => {
                    self.ui_action(crate::model::Action::ReloadThemes, ctx);
                    None
                }
                C::PlayPause => Some(Action::Toggle),
                C::Play => Some(Action::Pause(false)),
                C::Pause => Some(Action::Pause(true)),
                C::Next => Some(Action::Next),
                C::Previous => Some(Action::Previous),
                C::SeekBy(ms) => Some(Action::Seek(self.transport.position + ms as f64 / 1000.0)),
                C::SeekTo(ms) => Some(Action::Seek(f64::from(ms) / 1000.0)),
                C::SetVolume(percent) => Some(Action::Volume(f32::from(percent) / 100.0)),
                C::VolumeBy(percent) => Some(Action::Volume(
                    self.preferences.volume + f32::from(percent) / 100.0,
                )),
                C::ToggleMute => Some(Action::Volume(if self.preferences.volume == 0.0 {
                    0.7
                } else {
                    0.0
                })),
                _ => None,
            };
            if let Some(action) = action {
                self.apply(action);
            }
        }
    }

    fn sync_media(&mut self) {
        let song = self.transport.song();
        if let Some(slot) = &self.now_slot {
            let value = if let Some(song) = song {
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\tfalse\toff\t\tfalse\tThis computer",
                    if self.transport.paused {
                        "paused"
                    } else {
                        "playing"
                    },
                    song.title.replace(['\t', '\n'], " "),
                    song.artist.replace(['\t', '\n'], " "),
                    song.album.replace(['\t', '\n'], " "),
                    (self.transport.position * 1000.0) as u64,
                    (song.duration * 1000.0) as u64,
                    (self.preferences.volume * 100.0) as u32
                )
            } else {
                "stopped".into()
            };
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = value;
        }
        let state = crate::media::MediaState {
            playback: if song.is_none() {
                crate::player::Playback::Stopped
            } else if self.transport.paused {
                crate::player::Playback::Paused
            } else if self.transport.loading {
                crate::player::Playback::Loading
            } else {
                crate::player::Playback::Playing
            },
            track: song.map(|song| crate::media::MediaTrack {
                uri: self.transport.media_uri(self.session).unwrap_or_default(),
                title: song.title.clone(),
                artists: vec![song.artist.clone()],
                album: song.album.clone(),
                duration_ms: (song.duration * 1000.0) as u32,
                ..Default::default()
            }),
            position_ms: (self.transport.position * 1000.0) as u32,
            volume: f64::from(self.preferences.volume),
            shuffle: false,
            repeat: crate::player::RepeatMode::Off,
            can_control: song.is_some(),
        };
        if let Some(media) = &mut self.media {
            media.update(state);
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll(ctx);
        self.presentation.poll_custom_themes(ctx);
        if self.presentation.settings_dirty {
            self.mark_dirty();
        }
        if !self.presentation.zoom_applied {
            self.presentation.zoom_applied = true;
            ctx.set_zoom_factor(self.presentation.settings.zoom.clamp(0.5, 2.5));
        } else if (ctx.zoom_factor() - self.presentation.settings.zoom).abs() > 0.001 {
            self.presentation.settings.zoom = ctx.zoom_factor();
            self.mark_dirty();
        }
        self.presentation
            .toasts
            .retain(|toast| toast.created.elapsed() < super::TOAST_LIFETIME);
        if self.transport.song().is_some() && !self.transport.paused {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        #[cfg(feature = "demo")]
        self.capture(ctx);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.draw(ui);
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.save();
        self.player.send(player::Command::Stop);
    }
}

#[cfg(feature = "demo")]
impl App {
    pub fn populate_demo(&mut self, page: Option<&str>, flags: Option<&str>, ctx: &egui::Context) {
        let flags: Vec<_> = flags.unwrap_or_default().split(',').collect();
        self.preferences.light = flags.contains(&"light");
        self.preferences.username = "Demo listener".into();
        self.login.server = "https://music.example.com".into();
        self.login.username = "listener".into();
        self.connected = !flags.contains(&"signed-out") && !flags.contains(&"connecting");
        self.connecting = flags.contains(&"connecting");
        // Share catalogue fixtures with the inherited demo so UI comparisons
        // show the same album names, rows and playback state.
        for index in 0..crate::demo::ALBUMS.len() {
            let reference = crate::demo::album(index);
            let id = reference.id.clone();
            let songs = (0..12)
                .map(|track| {
                    let reference = crate::demo::track(track);
                    Song {
                        id: reference.id.unwrap_or_default(),
                        title: reference.name,
                        artist: reference
                            .artists
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        album: reference.album.map_or_else(String::new, |a| a.name),
                        cover_art: Some(id.clone()),
                        duration: f64::from(reference.duration_ms) / 1000.0,
                        suffix: "flac".into(),
                        track: track as u32 + 1,
                        disc_number: 1,
                    }
                })
                .collect();
            self.albums.push(Album {
                id: id.clone(),
                name: reference.name,
                artist: reference
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                cover_art: Some(id.clone()),
                song_count: 12,
                year: crate::demo::ALBUMS[index].2.parse().unwrap_or_default(),
                song: songs,
            });
            let colors: [[u8; 3]; 6] = [
                [35, 85, 112],
                [128, 79, 60],
                [41, 111, 104],
                [145, 106, 47],
                [70, 64, 120],
                [110, 65, 96],
            ];
            let base = colors[index % colors.len()];
            let pixels = (0..128 * 128)
                .map(|n| {
                    let x = (n % 128) as f32;
                    let y = (n / 128) as f32;
                    let stripe =
                        ((x * 0.04 + y * 0.03 + index as f32).sin() * 0.3 + 0.7).clamp(0.0, 1.0);
                    egui::Color32::from_rgb(
                        (f32::from(base[0]) * stripe) as u8,
                        (f32::from(base[1]) * stripe) as u8,
                        (f32::from(base[2]) * stripe) as u8,
                    )
                })
                .collect();
            let image = egui::ColorImage::new([128, 128], pixels);
            self.artwork.insert(
                ctx,
                self.cover_uri(&id),
                crate::navidrome::backend::Cover {
                    preview: image.clone(),
                    image,
                    bytes: Arc::from([]),
                    color: None,
                },
            );
        }
        if page.is_some_and(|page| page.starts_with("album:")) {
            self.album = self.albums.first().cloned();
        }
        self.playing_album = Some(self.albums[0].clone());
        self.transport.songs = self.albums[0].song.clone();
        self.transport.select(0, 83.0, false);
        self.transport.loading = false;
        if flags.contains(&"no-art") {
            self.artwork.forget_all();
        }
        if flags.contains(&"empty") {
            self.albums.clear();
            self.album = None;
        }
        if flags.contains(&"error") {
            self.error = Some(
                "The server did not respond in time. Check the connection and try again.".into(),
            );
        }
        if flags.contains(&"loading") {
            self.album_loading = self.album.is_some();
            self.list_loading = self.album.is_none();
        }
        self.show_eq = flags.contains(&"equalizer");
        if self.preferences.light {
            self.presentation.settings.theme = crate::settings::ThemeChoice::Light;
        }
        ctx.set_theme(self.presentation.theme_preference());
        if page == Some("settings") || self.show_eq {
            self.presentation.history = vec![crate::model::Page::Settings];
        } else if page.is_some_and(|p| p.starts_with("album:")) {
            self.presentation.history = vec![crate::model::Page::Album(presentation::encode(
                &self.albums[0].id,
            ))];
        }
        if flags.contains(&"appearance") {
            self.presentation.history = vec![crate::model::Page::Settings];
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("settings-filter"), "Appearance".to_string())
            });
        }
        if flags.contains(&"about") {
            self.presentation.history = vec![crate::model::Page::Settings];
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("settings-filter"), "About".to_string())
            });
        }
    }

    fn capture(&mut self, ctx: &egui::Context) {
        let Some((path, due, asked)) = &mut self.shot else {
            return;
        };
        ctx.request_repaint();
        if !*asked && Instant::now() >= *due {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            *asked = true;
        }
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let rgba: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(|pixel| pixel.to_srgba_unmultiplied())
                .collect();
            if let Some(image) =
                image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, rgba)
                && let Err(error) = image.save(path)
            {
                log::error!("Could not save demo screenshot: {error}");
            }
            self.shot = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transport() -> Transport {
        Transport {
            songs: vec![
                Song {
                    id: "a".into(),
                    duration: 60.0,
                    ..Default::default()
                },
                Song {
                    id: "a".into(),
                    duration: 60.0,
                    ..Default::default()
                },
                Song {
                    id: "b".into(),
                    duration: 60.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn duplicate_occurrences_progress_by_index_and_stale_completions_cannot_skip() {
        let mut player = transport();
        player.select(0, 0.0, false);
        let old = player.generation;
        assert_eq!(player.event(player::Event::Finished(old)), Some(1));
        player.select(1, 0.0, false);
        assert_eq!(player.event(player::Event::Finished(old)), None);
        assert_eq!(player.index, Some(1));
        assert_eq!(
            player.event(player::Event::Finished(player.generation)),
            Some(2)
        );
    }
    #[test]
    fn selecting_and_seeking_are_immediate_and_late_position_reports_are_ignored() {
        let mut player = transport();
        player.select(0, 0.0, false);
        let old = player.generation;
        player.select(2, 30.0, true);
        player.event(player::Event::Position {
            generation: old,
            seconds: 2.0,
            paused: false,
            started: true,
        });
        assert_eq!(player.position, 30.0);
        assert_eq!(player.song().unwrap().id, "b");
        assert!(player.paused);
        player.event(player::Event::Position {
            generation: player.generation,
            seconds: 30.1,
            paused: false,
            started: true,
        });
        assert!(player.paused);
    }
    #[test]
    fn preferences_contain_no_password_field() {
        let json = serde_json::to_string(&Preferences::default()).unwrap();
        assert!(!json.contains("password"));
        let old: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(old.volume, 0.7);
    }

    #[test]
    fn desktop_track_identity_changes_after_signing_in_again() {
        let mut player = transport();
        player.select(0, 0.0, false);
        assert_ne!(player.media_uri(1), player.media_uri(2));
        assert_eq!(player.media_uri(2).as_deref(), Some("navidrome:track:2:a"));
    }

    #[test]
    fn login_fields_stay_vertical_and_inside_narrow_and_normal_windows() {
        fn texts(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::epaint::Shape::Text(text) => out.push((
                    text.galley.job.text.clone(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                egui::epaint::Shape::Vec(shapes) => {
                    shapes.iter().for_each(|shape| texts(shape, out));
                }
                _ => {}
            }
        }
        let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
        let root = std::env::temp_dir().join(format!(
            "navifast-login-layout-{:016x}",
            rand::random::<u64>()
        ));
        let mut app = App::new(
            &cc,
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            None,
            true,
        );
        for width in [760.0, 1280.0] {
            for _ in 0..2 {
                let mut output = cc.egui_ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        app.draw(ui);
                    },
                );
                output.textures_delta.clear();
                let mut drawn = Vec::new();
                for shape in &output.shapes {
                    texts(&shape.shape, &mut drawn);
                }
                let fields: Vec<_> = ["Server URL", "Username", "Password"]
                    .iter()
                    .map(|label| {
                        drawn
                            .iter()
                            .find(|(text, _)| text == label)
                            .map(|(_, rect)| *rect)
                            .expect("login field label is visible")
                    })
                    .collect();
                assert!(fields[0].bottom() < fields[1].top());
                assert!(fields[1].bottom() < fields[2].top());
                assert!(fields.iter().all(|rect| {
                    rect.left() >= 0.0 && rect.right() <= width && rect.bottom() < 800.0
                }));
            }
        }
        let title = "A long song title that must leave room for its duration";
        app.connected = true;
        app.album = Some(Album {
            name: "Layout fixture".into(),
            song: vec![Song {
                title: title.into(),
                duration: 217.0,
                ..Default::default()
            }],
            ..Default::default()
        });
        app.presentation.history = vec![crate::model::Page::Album(String::new())];
        for width in [760.0, 1280.0] {
            for _ in 0..2 {
                let mut output = cc.egui_ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        app.draw(ui);
                    },
                );
                output.textures_delta.clear();
                let mut drawn = Vec::new();
                for shape in &output.shapes {
                    texts(&shape.shape, &mut drawn);
                }
                let duration = drawn.iter().find(|(text, _)| text == "3:37").unwrap().1;
                let name = drawn.iter().find(|(text, _)| text == title).unwrap().1;
                assert!(duration.right() > width - 80.0);
                assert!(name.right() < duration.left());
            }
        }
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }
}
