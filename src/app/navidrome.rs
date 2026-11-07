//! Stage-one Navidrome application state. Views return actions; apply them here.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub mod autoplay;
pub mod browse;
pub mod desktop;
pub mod downloads;
pub mod favorites;
mod gapless;
pub mod home;
mod host;
pub mod lyrics;
pub mod playlists;
pub mod quality;
pub mod queue;
pub mod ratings;
pub mod scrobble;
mod session;
pub mod stats;
pub mod sync;
use host::Host;
mod presentation;
pub mod search;
pub(crate) use presentation::{decode, encode};

use crate::navidrome::{
    api::{Album, Client, CoverSize, PAGE_SIZE, Song},
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
    /// Album URIs, newest first, belonging to the server and account above.
    pub recent_contexts: Vec<String>,
    pub home_history: home::History,
    /// Submitted terms by the client's server and username scope.
    pub search_history: HashMap<String, Vec<String>>,
    pub light: bool,
    pub volume: f32,
    pub autoplay: bool,
    pub autoplay_exclusions: crate::navidrome::autoplay::exclusions::Store,
    pub streaming_quality: crate::navidrome::quality::Quality,
    pub audio_cache: crate::navidrome::cache::Config,
    pub eq_on: bool,
    pub bands: [f32; 10],
    pub preamp: f32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            server: String::new(),
            username: String::new(),
            recent_contexts: Vec::new(),
            home_history: Default::default(),
            search_history: HashMap::new(),
            light: false,
            volume: 0.7,
            autoplay: false,
            autoplay_exclusions: Default::default(),
            streaming_quality: Default::default(),
            audio_cache: Default::default(),
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
    Shuffle(bool),
    Repeat(crate::player::RepeatMode),
    Theme,
    Equalizer,
    SetEq {
        on: bool,
        bands: [f32; 10],
        preamp: f32,
    },
    Cover(String, CoverSize),
}

#[derive(Default)]
pub struct Transport {
    /// For a sorted collection, the original occurrence key of each item in playback order.
    collection_rows: Vec<String>,
    pub queue: crate::navidrome::queue::Queue,
    pub repeat: crate::player::RepeatMode,
    waiting_for_queue: bool,
    pub position: f64,
    pub paused: bool,
    pub loading: bool,
    pub seekable: bool,
    pub information: Option<crate::navidrome::audio::Playback>,
    pub generation: u64,
    pub error: Option<String>,
}

impl Transport {
    pub fn song(&self) -> Option<&Song> {
        self.queue
            .current()
            .and_then(|entry| self.queue.song(entry))
    }
    fn media_uri(&self, session: u64) -> Option<String> {
        self.song()
            .map(|song| format!("navidrome:track:{session}:{}", song.id))
    }
    fn prepare(&mut self, position: f64, paused: bool) -> Option<Song> {
        let song = self.song()?.clone();
        self.waiting_for_queue = false;
        self.position = position.max(0.0).min(song.duration.max(0.0));
        self.paused = paused;
        self.loading = true;
        self.information = None;
        self.error = None;
        self.generation = player::next_generation();
        Some(song)
    }
    fn event(&mut self, event: player::Event) -> bool {
        match event {
            player::Event::Information { generation, info } if generation == self.generation => {
                self.information = Some(info);
            }
            player::Event::Ready {
                generation,
                seekable,
            } if generation == self.generation => {
                self.seekable = seekable;
                if self.paused {
                    self.loading = false;
                }
            }
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
                if self.repeat == crate::player::RepeatMode::Track
                    || !self.queue.upcoming().is_empty()
                {
                    self.position = self.song().map_or(0.0, |song| song.duration);
                    return true;
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
                self.information = None;
            }
            _ => {}
        }
        false
    }
}

pub struct App {
    pub(crate) presentation: Box<super::App>,
    pending_play: Option<(String, usize)>,
    listening_session: session::State,
    pub(crate) playing_album: Option<Album>,
    pub(crate) playing_playlist: Option<String>,
    pub login: Login,
    pub connecting: bool,
    pub connected: bool,
    // Keep the main window visible through both credential lookup and ping.
    // This is presentation state, not permission to send authenticated requests.
    pub(crate) restoring_login: bool,
    pub preferences: Preferences,
    pub albums: Vec<Album>,
    pub album: Option<Album>,
    pub list_loading: bool,
    pub album_loading: bool,
    pub has_more: bool,
    pub newest: bool,
    pub error: Option<String>,
    pub transport: Transport,
    pub lyrics: lyrics::State,
    pub browse: browse::State,
    pub stats: stats::State,
    pub autoplay: autoplay::State,
    pub ratings: ratings::State,
    pub scrobbles: scrobble::State,
    gapless: gapless::State,
    pub quality: quality::State,
    pub requested_covers: HashSet<String>,
    pub downloads: downloads::State,
    pub sync: sync::State,
    pub audio_cache: crate::navidrome::cache::Summary,
    pub cache_pending: bool,
    cache_revision: u64,
    pub show_eq: bool,
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
    dirs: crate::paths::AppDirs,
    dirty: Option<Instant>,
    proxy: crate::settings::ProxyConfig,
    commands: Option<Arc<Mutex<Vec<crate::single_instance::ControlCommand>>>>,
    now_slot: Option<Arc<Mutex<String>>>,
    media: Option<crate::media_controls::MediaService>,
    demo: bool,
    _instance: Option<crate::single_instance::Guard>,
}

impl App {
    pub(crate) fn playback_loading(&self) -> bool {
        self.transport.loading || self.pending_play.is_some()
    }

    /// The playing album supplies its sleeve even when a song omits its own art.
    pub(crate) fn playing_cover(&self) -> Option<&str> {
        let song = self.transport.song()?;
        song.cover_art.as_deref().or_else(|| {
            self.transport.queue.context_index()?;
            self.playing_album.as_ref()?.cover_art.as_deref()
        })
    }
    fn with_host(
        dirs: crate::paths::AppDirs,
        instance: Option<crate::single_instance::Guard>,
        demo: bool,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let mut preferences: Preferences = std::fs::read(dirs.config.join("navidrome.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        preferences.volume = preferences.volume.clamp(0.0, 1.0);
        preferences.audio_cache = preferences.audio_cache.normalized();
        preferences.home_history.sanitize();
        preferences.autoplay_exclusions.sanitize();
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
        // Restore the existing presentation preferences.
        settings.check_for_updates = false;
        preferences.volume = f32::from(settings.volume) / f32::from(u16::MAX);
        preferences.eq_on = settings.eq_on;
        preferences.bands = settings.eq_bands_db;
        preferences.preamp = settings.eq_preamp_db;
        let mut presentation = Box::new(super::App::new(dirs.clone(), settings));
        presentation.recent_contexts = preferences.recent_contexts.clone();
        presentation.history = vec![crate::model::Page::Home];
        presentation.history_index = 0;
        if !demo {
            presentation.enable_desktop_themes();
        }
        let theme_wake = wake.clone();
        presentation.custom_themes.start(
            dirs.config.join("themes"),
            presentation.settings.custom_theme.clone(),
            theme_wake,
        );
        let eq = Arc::new(Mutex::new(super::eq_settings(&presentation.settings)));
        let tap = crate::vis::AudioTap::new();
        let backend = Backend::spawn(wake.clone(), dirs.clone());
        if !demo {
            backend.send(Request::Cache {
                root: dirs.cache.join("streams"),
                config: preferences.audio_cache,
                clear: false,
            });
            backend.send(Request::Restore { session: 0 });
        }
        // System proxy honors the existing native desktop configuration.
        let proxy = crate::settings::ProxyConfig::System;
        let (events, player_events) = mpsc::channel();
        let wake_player = wake.clone();
        let player = Player::spawn(
            proxy.clone(),
            backend.stream_resolver(),
            eq.clone(),
            tap.clone(),
            Arc::new(move |event| {
                let _ = events.send(event);
                wake_player();
            }),
        );
        player.send(player::Command::Volume(preferences.volume));
        let media = (!demo).then(|| crate::media_controls::MediaService::spawn(move || wake()));
        let newest = presentation
            .settings
            .library_sort
            .get(&crate::settings::LibraryShelf::Albums)
            != Some(&crate::settings::LibrarySort::Name);
        let quality = quality::State {
            active: preferences.streaming_quality,
            capabilities: demo.then_some(Ok(crate::navidrome::quality::Capabilities {
                transcoding: true,
                offset: true,
            })),
            ..Default::default()
        };
        Self {
            quality,
            presentation,
            pending_play: None,
            listening_session: Default::default(),
            playing_album: None,
            playing_playlist: None,
            login: Login {
                server: preferences.server.clone(),
                username: preferences.username.clone(),
                password: String::new(),
            },
            connecting: !demo,
            connected: false,
            restoring_login: !demo,
            preferences,
            albums: Vec::new(),
            album: None,
            list_loading: false,
            album_loading: false,
            has_more: false,
            newest,
            error: None,
            lyrics: Default::default(),
            browse: Default::default(),
            stats: Default::default(),
            autoplay: Default::default(),
            ratings: Default::default(),
            scrobbles: Default::default(),
            gapless: Default::default(),
            transport: Transport {
                paused: true,
                ..Default::default()
            },
            requested_covers: HashSet::new(),
            downloads: Default::default(),
            sync: Default::default(),
            audio_cache: Default::default(),
            cache_pending: !demo,
            cache_revision: u64::from(!demo),
            show_eq: false,
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
            dirs,
            dirty: None,
            proxy,
            commands: instance.as_ref().map(|guard| guard.commands()),
            now_slot: instance.as_ref().map(|guard| guard.now_playing_slot()),
            media,
            demo,
            _instance: instance,
        }
    }

    pub fn apply(&mut self, action: Action) {
        if matches!(action, Action::Seek(_)) && !self.can_seek() {
            return;
        }
        if matches!(
            &action,
            Action::Play(_)
                | Action::Toggle
                | Action::Pause(_)
                | Action::Next
                | Action::Previous
                | Action::Seek(_)
                | Action::Shuffle(_)
                | Action::Repeat(_)
        ) {
            self.edit_session();
        }
        match action {
            Action::EditLogin(mut login) => {
                if login.server != self.login.server || login.username != self.login.username {
                    login.password.clear();
                }
                self.login = login;
            }
            Action::Login => {
                if self.demo {
                    self.restoring_login = false;
                    self.connecting = false;
                    self.connected = true;
                    return;
                }
                if let Err(message) = crate::navidrome::api::server_url(&self.login.server) {
                    self.restoring_login = false;
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
                self.restoring_login = false;
            }
            Action::SignOut => {
                self.reset_autoplay();
                self.downloads = Default::default();
                self.sync = Default::default();
                self.browse = Default::default();
                self.stats = Default::default();
                self.ratings = Default::default();
                self.scrobbles.reset();
                self.forget_session();
                self.lyrics = Default::default();
                self.session = self.session.wrapping_add(1);
                self.backend.send(Request::Cancel);
                if !self.demo {
                    self.backend.send(Request::Forget {
                        session: self.session,
                    });
                }
                self.player.send(player::Command::Stop);
                self.client = None;
                self.connected = false;
                self.connecting = false;
                self.restoring_login = false;
                self.login.password.clear();
                self.presentation.search = Default::default();
                self.presentation.navidrome_search = Default::default();
                self.presentation.navidrome_playlists = Default::default();
                self.presentation.navidrome_favorites = Default::default();
                self.presentation.history = vec![crate::model::Page::Home];
                self.presentation.history_index = 0;
                self.pending_play = None;
                self.albums.clear();
                self.album = None;
                self.requested_covers.clear();
                let generation = player::next_generation();
                self.playing_album = None;
                self.playing_playlist = None;
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
                    self.play_album(album.clone(), index);
                }
            }
            Action::Toggle => {
                if self.transport.waiting_for_queue || self.autoplay.waiting {
                    self.transport.paused = !self.transport.paused;
                    if self.transport.paused {
                        self.checkpoint_session(true);
                    }
                    return;
                }
                if self.transport.error.is_some()
                    || self
                        .transport
                        .song()
                        .is_some_and(|song| self.transport.position >= song.duration)
                {
                    let position = if self.transport.error.is_some() {
                        self.transport.position
                    } else {
                        0.0
                    };
                    if self.transport.error.is_none() {
                        self.scrobbles.new_play();
                    }
                    self.load(position, false);
                } else {
                    self.apply(Action::Pause(!self.transport.paused));
                }
            }
            Action::Pause(paused) => {
                if self.transport.waiting_for_queue || self.autoplay.waiting {
                    self.transport.paused = paused;
                    if paused {
                        self.checkpoint_session(true);
                    }
                    return;
                }
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
                if paused {
                    self.checkpoint_session(true);
                }
            }
            Action::Next => {
                let advance = self.transport.queue.advance();
                self.advance_queue(advance, self.transport.paused);
            }
            Action::Previous => {
                if self.transport.position <= 3.0 {
                    self.transport.queue.previous();
                }
                self.load(0.0, self.transport.paused);
            }
            Action::Seek(seconds) => {
                self.load(seconds, self.transport.paused);
                if let Some(media) = &self.media {
                    media.seeked((self.transport.position * 1000.0) as u32);
                }
            }
            Action::Volume(volume) => {
                self.preferences.volume = volume.clamp(0.0, 1.0);
                self.presentation.settings.volume =
                    (self.preferences.volume * f32::from(u16::MAX)).round() as u16;
                self.player
                    .send(player::Command::Volume(self.preferences.volume));
                self.mark_dirty();
            }
            Action::Shuffle(shuffle) => self.transport.queue.set_shuffle(shuffle),
            Action::Repeat(repeat) => {
                self.reset_autoplay();
                self.transport.repeat = repeat;
                self.transport
                    .queue
                    .set_repeat_all(repeat == crate::player::RepeatMode::Context);
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
            Action::Cover(id, size) => {
                let uri = presentation::artwork_uri(self.session, &id, size);
                if self.requested_covers.insert(uri.clone())
                    && let Some(client) = &self.client
                {
                    self.backend.send(Request::Cover {
                        session: self.session,
                        client: client.clone(),
                        size,
                        cached: None,
                        id,
                    });
                }
            }
        }
        self.prepare_next();
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty.get_or_insert_with(Instant::now);
    }

    fn save(&mut self) {
        self.preferences.recent_contexts = self.presentation.recent_contexts.clone();
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
        self.dirty = None;
    }

    fn remember_account(&mut self, server: &str, username: &str) {
        if self.preferences.server != server || self.preferences.username != username {
            self.preferences.recent_contexts.clear();
            self.presentation.recent_contexts.clear();
            self.preferences.home_history = Default::default();
            self.preferences.streaming_quality = Default::default();
        }
        self.preferences.server = server.into();
        self.preferences.username = username.into();
    }

    fn set_client(&mut self, client: Client) {
        self.reset_autoplay();
        self.browse = Default::default();
        self.stats = Default::default();
        self.ratings = Default::default();
        self.scrobbles.reset();
        self.lyrics = Default::default();
        if self
            .client
            .as_ref()
            .is_some_and(|old| old.scope != client.scope)
        {
            self.forget_session();
            self.player.send(player::Command::Stop);
            self.transport = Transport {
                paused: true,
                generation: player::next_generation(),
                ..Default::default()
            };
            self.playing_album = None;
            self.playing_playlist = None;
        }
        self.remember_account(client.base_url(), client.username());
        self.presentation.navidrome_search.session = self.session;
        self.login.password.clear();
        self.presentation.navidrome_search.recent = self
            .preferences
            .search_history
            .get(&client.scope)
            .cloned()
            .unwrap_or_default();
        self.client = Some(client);
        self.quality.active = self.preferences.streaming_quality;
        self.quality.deferred = false;
        self.refresh_streaming_capabilities();
        self.presentation.navidrome_playlists = Default::default();
        self.presentation.navidrome_favorites = Default::default();
        self.open_downloads();
        self.open_sync();
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

    fn play_album(&mut self, mut album: Album, index: usize) {
        let songs = std::mem::take(&mut album.song);
        self.play_album_songs(Some(album), songs, index);
    }

    fn play_album_songs(&mut self, album: Option<Album>, songs: Vec<Song>, index: usize) {
        if index >= songs.len() {
            return;
        }
        self.edit_session();
        self.browse.playing = None;
        self.playing_album = None;
        self.playing_playlist = None;
        if let Some(album) = album {
            self.remember_home_album(&album);
            let uri = format!("navidrome:album:{}", presentation::encode(&album.id));
            self.playing_album = Some(album);
            self.presentation.recent_contexts.retain(|old| old != &uri);
            self.presentation.recent_contexts.insert(0, uri);
            self.presentation.recent_contexts.truncate(100);
            self.mark_dirty();
        }
        self.reset_autoplay();
        self.transport.collection_rows.clear();
        self.transport.queue.start(songs, index);
        self.load(0.0, false);
    }

    fn load(&mut self, position: f64, paused: bool) {
        self.cancel_preparation();
        if position == 0.0 {
            self.quality.active = self.preferences.streaming_quality;
            self.quality.deferred = false;
        }
        if self
            .transport
            .song()
            .is_some_and(|song| self.download_ready(&song.id))
        {
            self.quality.active = crate::navidrome::quality::Quality::Original;
            self.quality.deferred = false;
        }
        self.transport.seekable = self
            .quality
            .capabilities
            .as_ref()
            .and_then(|c| c.as_ref().ok())
            .copied()
            .unwrap_or_default()
            .can_seek(self.quality.active);
        self.autoplay.waiting = false;
        if !paused {
            self.remember_home_artists();
        }
        self.sync_lyrics();
        self.dirty_session();
        self.pending_play = None;
        let Some(song) = self.transport.prepare(position, paused) else {
            return;
        };
        self.tap.clear();
        let occurrence = self.transport.queue.current().map_or(0, |entry| entry.id);
        let listening = self.scrobbles.context(
            self.session,
            occurrence,
            self.presentation.settings.scrobbling,
        );
        if let Some(client) = &self.client {
            self.player.send(player::Command::Load {
                quality: self.quality.active,
                generation: self.transport.generation,
                listening,
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

    fn restore_login(
        &mut self,
        session: u64,
        result: Result<Option<crate::credentials::NavidromePassword>, String>,
    ) {
        if session != self.session {
            return;
        }
        self.connecting = false;
        match result {
            Ok(Some(login)) => {
                self.login = Login {
                    server: login.server,
                    username: login.username,
                    password: login.password,
                };
                self.apply(Action::Login);
            }
            Ok(None) => self.restoring_login = false,
            Err(error) => {
                self.restoring_login = false;
                self.error = Some(error);
            }
        }
    }

    fn poll(&mut self, host: &impl Host) -> bool {
        let mut changed = false;
        while let Ok(event) = self.backend.events.try_recv() {
            changed = true;
            match event {
                Event::Downloads {session, revision, update} => self.download_update(session, revision, update),
                Event::Autoplay { session, request, result } => self.autoplay_result(session, request, result),
                Event::Stats { session, request, result } => self.stats_result(session, request, result),
                Event::Browse {session, request, query, offset, result} => self.browse_result(session,request,query,offset,result),
                Event::Lyrics { session, request, song, result } => self.lyrics_result(session, request, &song, result),
                Event::SessionRestored { session, token, result } => self.session_restored(session, token, result),
                Event::SessionSaveFailed => self.presentation.toast_error("Your listening session could not be saved. Check the state directory permissions."),
                Event::QueueAlbum {
                    session,
                    token,
                    result,
                } if session == self.session => {
                    self.queued_album(token, result);
                }
                event @ (Event::PlaylistLibrary { .. }
                | Event::PlaylistDestinations { .. }
                | Event::PlaylistEdited { .. }) => self.playlists_event(event),
                event @ Event::Scrobbled { .. } => self.scrobble_event(event),
                Event::Cache(summary) => {
                    if summary.revision >= self.cache_revision { self.audio_cache = summary; self.cache_pending = false; }
                },
                Event::Sync { session, revision, update } => self.sync_update(session, revision, update),
                event @ (Event::RatingWritten { .. } | Event::RatingRead { .. }) => self.rating_event(event),
                event @ (Event::Favorites { .. } | Event::FavoriteWritten { .. }) => {
                    self.favorite_event(event)
                }
                event @ (Event::Catalogue { .. }
                | Event::Playlists { .. }
                | Event::Collection { .. }
                | Event::Appearances { .. }
                | Event::ArtistInfo { .. }) => self.search_event(event),
                Event::Restored { session, result } => self.restore_login(session, result),
                Event::CredentialError { session, message } if session == self.session => {
                    if self.connected {
                        self.presentation.toast_error(message);
                    } else {
                        self.error = Some(message);
                    }
                }
                Event::Login { session, result } if session == self.session => {
                    self.connecting = false;
                    self.restoring_login = false;
                    match result {
                        Ok(client) => {
                            self.set_client(client);
                            self.connected = true;
                            self.restore_session();
                            self.mark_dirty();
                            self.list_request += 1;
                            self.fetch_albums(0);
                            self.refresh_favorites();
                            if self.presentation.page() == &crate::model::Page::Home {
                                self.fetch_playlists();
                            }
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
                            self.migrate_home_history();
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
                                self.apply(Action::Play(index));
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Event::Cover {
                    size,
                    session,
                    id,
                    result: Ok(cover),
                } if session == self.session => {
                    let uri = presentation::artwork_uri(session, &id, size);
                    self.requested_covers.remove(&uri);
                    if let Some(color) = cover.color {
                        let tint = self.palette.tint_from_art(color);
                        self.presentation.accents.insert(uri.clone(), tint);
                    }
                    host.insert_cover(self, uri, cover);
                }
                Event::Cover {
                    size,
                    session,
                    id,
                    result: Err(_),
                    ..
                } if session == self.session => {
                    let key = presentation::artwork_uri(session, &id, size);
                    if host.retry_cover(&key) {
                        self.requested_covers.remove(&key);
                    }
                }
                Event::StreamingCapabilities { session, result } if session == self.session => {
                    if let Ok(capabilities) = &result { self.transport.seekable = capabilities.can_seek(self.quality.active); }
                    self.quality.capabilities = Some(result);
                    changed = true;
                }
                Event::SaveFailed => self.error = Some(
                    "Your preferences could not be saved. Check the config directory permissions."
                        .into(),
                ),
                _ => {}
            }
        }
        while let Ok(event) = self.player_events.try_recv() {
            if let player::Event::Transition {
                information,
                from,
                generation,
                occurrence,
                listening,
                quality,
                seekable,
            } = event
            {
                if self.gapless_transition(from, generation, occurrence, listening) {
                    self.quality.active = quality;
                    self.quality.deferred = false;
                    self.transport.seekable = seekable;
                    self.transport.information = information;
                    changed = true;
                }
                continue;
            }
            if let player::Event::Scrobble { context, report } = event {
                self.listening_report(context, report);
                continue;
            }
            if let player::Event::Finished(generation) = event {
                if generation == self.transport.generation {
                    self.dirty_session();
                    let paused = self.transport.paused;
                    self.transport.event(player::Event::Finished(generation));
                    self.finish_track(paused);
                    changed = true;
                }
                continue;
            }
            let previous = (
                self.transport.queue.current().map(|entry| entry.id),
                self.transport.loading,
                self.transport.paused,
                self.transport.position as u64,
            );
            if let player::Event::Failed {
                generation,
                message,
            } = &event
                && *generation == self.transport.generation
            {
                changed = true;
                self.presentation.toast_error(message.clone());
            }
            self.transport.event(event);
            changed |= previous
                != (
                    self.transport.queue.current().map(|entry| entry.id),
                    self.transport.loading,
                    self.transport.paused,
                    self.transport.position as u64,
                );
        }
        if self
            .dirty
            .is_some_and(|since| since.elapsed() > Duration::from_millis(500))
        {
            self.save();
        }
        changed |= self.poll_search(host);
        self.poll_favorites(host);
        self.poll_ratings(host);
        self.poll_playlists(host);
        changed |= self.poll_controls(host);
        changed |= self.poll_autoplay();
        self.prepare_next();
        self.checkpoint_session(false);
        self.sync_media();
        changed
    }

    fn finish_track(&mut self, paused: bool) {
        // Completion reports mark transport paused at the end. Retain the user's
        // intention while a final random-library read is still outstanding.
        self.transport.paused = paused;
        self.poll_autoplay();
        if self.transport.repeat == crate::player::RepeatMode::Track {
            self.scrobbles.new_play();
            self.load(0.0, paused);
        } else {
            let advance = self.transport.queue.advance();
            if advance == crate::navidrome::queue::Advance::Empty {
                if !self.wait_for_autoplay(paused) {
                    self.transport.paused = true;
                    self.transport.loading = false;
                }
            } else {
                self.advance_queue(advance, paused);
            }
        }
    }

    fn poll_controls(&mut self, host: &impl Host) -> bool {
        let mut changed = false;
        use crate::media::MediaCommand as M;
        if let Some(media) = &self.media {
            for command in media.drain_commands() {
                changed = true;
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
                    M::SetRepeat(repeat) => Some(Action::Repeat(repeat)),
                    M::SetShuffle(shuffle) => Some(Action::Shuffle(shuffle)),
                    M::SetVolume(volume) => Some(Action::Volume(volume as f32)),
                    M::Raise => {
                        host.show_window();
                        None
                    }
                    M::Quit => {
                        host.close_window();
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
            changed = true;
            use crate::single_instance::ControlCommand as C;
            let action = match command {
                C::Show => {
                    host.show_window();
                    None
                }
                C::ReloadThemes => {
                    self.ui_action(crate::model::Action::ReloadThemes, host);
                    None
                }
                C::CycleRepeat => Some(Action::Repeat(self.transport.repeat.next())),
                C::SetRepeat(repeat) => Some(Action::Repeat(repeat)),
                C::ToggleShuffle => Some(Action::Shuffle(!self.transport.queue.shuffled())),
                C::SetShuffle(shuffle) => Some(Action::Shuffle(shuffle)),
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
        changed
    }

    fn sync_media(&mut self) {
        let song = self.transport.song();
        if let Some(slot) = &self.now_slot {
            let value = if let Some(song) = song {
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t\tfalse\tThis computer",
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
                    (self.preferences.volume * 100.0) as u32,
                    self.transport.queue.shuffled(),
                    self.transport.repeat.api_name()
                )
            } else {
                "stopped".into()
            };
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = value;
        }
        let Some(media) = &mut self.media else {
            return;
        };
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
            shuffle: self.transport.queue.shuffled(),
            repeat: self.transport.repeat,
            can_control: song.is_some(),
        };
        media.update(state);
    }
}

#[cfg(feature = "navidrome-demo")]
impl App {
    fn populate_demo_model(&mut self, page: Option<&str>, flags: Option<&str>, host: &impl Host) {
        let flags: Vec<_> = flags.unwrap_or_default().split(',').collect();
        self.preferences.light = flags.contains(&"light");
        self.preferences.username = "Demo listener".into();
        self.login.server = "https://music.example.com".into();
        self.login.username = "listener".into();
        self.connected = !flags.contains(&"signed-out") && !flags.contains(&"connecting");
        self.connecting = flags.contains(&"connecting");
        self.restoring_login = flags.contains(&"restoring");
        if self.restoring_login {
            self.connected = false;
            self.connecting = true;
        }
        for index in 0..crate::demo::ALBUMS.len() {
            let album = crate::demo::album(index);
            let id = album.id.clone();
            self.albums.push(album);
            let colors: [[u8; 3]; 6] = [
                [35, 85, 112],
                [128, 79, 60],
                [41, 111, 104],
                [145, 106, 47],
                [70, 64, 120],
                [110, 65, 96],
            ];
            if flags.contains(&"no-art") {
                continue;
            }
            let base = colors[index % colors.len()];
            let image = image::RgbaImage::from_fn(128, 128, |x, y| {
                let stripe = ((x as f32 * 0.04 + y as f32 * 0.03 + index as f32).sin() * 0.3 + 0.7)
                    .clamp(0.0, 1.0);
                image::Rgba([
                    (f32::from(base[0]) * stripe) as u8,
                    (f32::from(base[1]) * stripe) as u8,
                    (f32::from(base[2]) * stripe) as u8,
                    255,
                ])
            });
            host.insert_cover(
                self,
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
        self.transport.queue.start(self.albums[0].song.clone(), 0);
        self.transport.prepare(83.0, flags.contains(&"restored"));
        self.transport.loading = false;
        if flags.contains(&"shuffle") {
            self.transport.queue.demo_shuffle();
        }
        if flags.contains(&"repeat-all") {
            self.apply(Action::Repeat(crate::player::RepeatMode::Context));
        }
        if flags.contains(&"repeat-one") {
            self.apply(Action::Repeat(crate::player::RepeatMode::Track));
        }
        let favorite_page = page.and_then(|p| match p {
            "favorites" => Some(crate::model::Page::LikedSongs),
            _ => crate::model::Page::decode(p).filter(crate::model::Page::is_favorites),
        });
        self.populate_demo_favorites(favorite_page.is_some(), &flags);
        let browse_page = page
            .and_then(crate::model::Page::decode)
            .filter(crate::model::Page::is_browse);
        let library_state = browse_page.is_none()
            && favorite_page.is_none()
            && page != Some("search")
            && page != Some("playlists")
            && !page.is_some_and(|p| p.starts_with("artist:") || p.starts_with("playlist:"));
        if library_state && flags.contains(&"empty") {
            self.albums.clear();
            self.album = None;
        }
        if library_state && flags.contains(&"error") {
            self.error = Some(
                "The server did not respond in time. Check the connection and try again.".into(),
            );
        }
        if library_state && flags.contains(&"loading") {
            self.album_loading = self.album.is_some();
            self.list_loading = self.album.is_none();
        }
        self.show_eq = flags.contains(&"equalizer");
        if self.preferences.light {
            self.presentation.settings.theme = crate::settings::ThemeChoice::Light;
        }
        if let Some(page) = favorite_page {
            self.presentation.history = vec![crate::model::Page::LikedSongs];
            if page != crate::model::Page::LikedSongs {
                self.presentation.open(page);
            }
        } else if page == Some("search") {
            self.populate_demo_search(&flags, host);
        } else if page == Some("playlists") {
            self.presentation.history = vec![crate::model::Page::Playlists];
            self.populate_demo_playlists(&flags);
        } else if let Some(id) = page.and_then(|p| p.strip_prefix("artist:")) {
            self.populate_demo_artist(id, &flags);
        } else if let Some(id) = page.and_then(|p| p.strip_prefix("playlist:")) {
            self.presentation.history =
                vec![crate::model::Page::Playlist(presentation::encode(id))];
            self.presentation.navidrome_search.collection = if flags.contains(&"loading") {
                crate::model::Loadable::Loading
            } else if flags.contains(&"error") {
                crate::model::Loadable::Failed("The server did not respond in time.".into())
            } else {
                crate::model::Loadable::Loaded(crate::navidrome::api::Collection::Playlist {
                    name: "Evening drive".into(),
                    cover_art: (!flags.contains(&"no-art"))
                        .then(|| self.albums[0].cover_art.clone())
                        .flatten(),
                    owner: Some("Demo".into()),
                    writable: id != "list2",
                    songs: if flags.contains(&"empty") {
                        Vec::new()
                    } else {
                        self.albums[0].song.clone()
                    },
                })
            };
        } else if page == Some("settings") || self.show_eq {
            self.presentation.history = vec![crate::model::Page::Settings];
        } else if page.is_some_and(|p| p.starts_with("album:")) {
            self.presentation.history = vec![crate::model::Page::Album(presentation::encode(
                &self.albums[0].id,
            ))];
        }
        if flags.contains(&"appearance") || flags.contains(&"about") {
            self.presentation.history = vec![crate::model::Page::Settings];
        }
        if matches!(self.presentation.page(), crate::model::Page::Playlist(_)) {
            self.presentation.navidrome_search.collection_page =
                Some(self.presentation.page().clone());
        }
        if flags.contains(&"playlist-editor") || flags.contains(&"playlist-edits") {
            let songs = self
                .album
                .as_ref()
                .map_or_else(Vec::new, |a| a.song.iter().take(3).cloned().collect());
            self.playlist_action(playlists::Action::Open(songs));
            self.playlist_action(playlists::Action::Toggle("list0".into()));
            self.playlist_action(playlists::Action::Toggle("list1".into()));
            if flags.contains(&"playlist-edits") {
                self.playlist_action(playlists::Action::Add);
                let jobs = &mut self.presentation.navidrome_playlists.jobs;
                jobs[0].pending = false;
                jobs[0].task.sent = true;
                jobs[0].task.release_data();
                jobs[1].pending = false;
                jobs[1].error = Some("The server did not respond in time.".into());
            }
        }
        if flags.contains(&"playlist-create") {
            self.playlist_action(playlists::Action::Open(Vec::new()));
        }
        if flags.contains(&"playlist-delete") {
            self.playlist_action(playlists::Action::ConfirmDelete);
        }
        if let Some(page) = browse_page {
            self.presentation.history = vec![page];
            self.sync_browse();
            self.configure_demo_browse(&flags);
        }
        if page == Some("stats") {
            self.presentation.history = vec![crate::model::Page::Stats];
            self.configure_demo_stats(&flags);
        }
        self.configure_demo_autoplay(&flags);
        self.populate_demo_lyrics(&flags);
        if page == Some("albums") {
            self.presentation.history = vec![crate::model::Page::Albums];
        } else if page == Some("home")
            || page.is_none() && self.presentation.page() == &crate::model::Page::Home
        {
            self.populate_demo_home(&flags);
        }
        if self.restoring_login {
            self.presentation.history = vec![crate::model::Page::Home];
            self.presentation.history_index = 0;
            self.albums.clear();
            self.album = None;
            self.playing_album = None;
            self.transport = Transport {
                paused: true,
                ..Default::default()
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn with_app(test: impl FnOnce(&mut App, &host::TestHost)) {
        let root = std::env::temp_dir().join(format!(
            "navifast-presentation-{:016x}",
            rand::random::<u64>()
        ));
        let mut app = App::with_host(
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            None,
            true,
            Arc::new(|| {}),
        );
        app.connected = true;
        app.preferences.username = "listener".into();
        let album = Album {
            id: "library:/🌍/album".into(),
            name: "Original album".into(),
            artist: "Artist".into(),
            cover_art: Some("art:/🌍".into()),
            song_count: 3,
            song: vec![
                Song {
                    id: "song:/α".into(),
                    title: "First".into(),
                    duration: 120.0,
                    ..Default::default()
                },
                Song {
                    id: "song:/β".into(),
                    title: "Second".into(),
                    duration: 180.0,
                    ..Default::default()
                },
                Song {
                    id: "song:/α".into(),
                    title: "First".into(),
                    duration: 120.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        app.albums.push(album.clone());
        app.album = Some(album);
        test(&mut app, &host::TestHost);
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    fn transport() -> Transport {
        let mut transport = Transport::default();
        transport.queue.start(
            ["a", "a", "b"]
                .into_iter()
                .map(|id| Song {
                    id: id.into(),
                    duration: 60.0,
                    ..Default::default()
                })
                .collect(),
            0,
        );
        transport
    }
    #[test]
    fn duplicate_occurrences_progress_by_index_and_stale_completions_cannot_skip() {
        let mut player = transport();
        player.prepare(0.0, false);
        let old = player.generation;
        assert!(player.event(player::Event::Finished(old)));
        player.queue.advance();
        player.prepare(0.0, false);
        assert!(!player.event(player::Event::Finished(old)));
        assert_eq!(player.queue.context_index(), Some(1));
        assert!(player.event(player::Event::Finished(player.generation)));
    }
    #[test]
    fn selecting_and_seeking_are_immediate_and_late_position_reports_are_ignored() {
        let mut player = transport();
        player.prepare(0.0, false);
        let old = player.generation;
        let last = player.queue.upcoming().back().unwrap().id;
        player.queue.play(last);
        player.prepare(30.0, true);
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
        assert!(old.recent_contexts.is_empty());
    }

    #[test]
    fn recently_played_albums_survive_autosave_and_exit() {
        let root = std::env::temp_dir().join(format!(
            "navifast-recent-albums-{:016x}",
            rand::random::<u64>()
        ));
        let dirs = crate::paths::AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        let play = |app: &mut App, id: &str| {
            app.album = Some(Album {
                id: id.into(),
                name: id.into(),
                song: vec![Song {
                    id: format!("{id}/song"),
                    duration: 60.0,
                    ..Default::default()
                }],
                ..Default::default()
            });
            app.apply(Action::Play(0));
        };
        let uri = |id: &str| format!("navidrome:album:{}", presentation::encode(id));
        let mut app = App::with_host(dirs.clone(), None, true, Arc::new(|| {}));
        app.remember_account("https://music.example.com/", "listener");
        for index in 0..105 {
            play(&mut app, &format!("older-{index}"));
        }
        assert_eq!(app.presentation.recent_contexts.len(), 100);
        assert_eq!(
            app.presentation.recent_contexts.last(),
            Some(&uri("older-5"))
        );
        app.presentation.recent_contexts.clear();
        play(&mut app, "library:/🌍/first");
        play(&mut app, "second");
        play(&mut app, "library:/🌍/first");
        let expected = vec![uri("library:/🌍/first"), uri("second")];
        assert_eq!(app.presentation.recent_contexts, expected);
        assert!(app.dirty.is_some(), "playing schedules an autosave");

        // Enable only real persistence, without restoring credentials or
        // starting audio. The worker drains pending saves when dropped.
        app.demo = false;
        app.dirty = Some(Instant::now() - Duration::from_secs(1));
        app.poll(&host::TestHost);
        assert!(app.dirty.is_none());
        drop(app);

        let mut restored = App::with_host(dirs.clone(), None, true, Arc::new(|| {}));
        restored.remember_account("https://music.example.com/", "listener");
        assert_eq!(restored.presentation.recent_contexts, expected);
        assert_eq!(
            restored.preferences.home_history.collections[0].id,
            "library:/🌍/first"
        );
        play(&mut restored, "second");
        restored.demo = false;
        restored.save();
        drop(restored);

        let mut restored = App::with_host(dirs, None, true, Arc::new(|| {}));
        assert_eq!(
            restored.presentation.recent_contexts,
            vec![uri("second"), uri("library:/🌍/first")]
        );
        // The same opaque IDs can belong to unrelated albums on another
        // server or account. Only a successful account change clears them.
        restored.remember_account("https://music.example.com/", "another-listener");
        assert!(restored.presentation.recent_contexts.is_empty());
        assert!(restored.preferences.recent_contexts.is_empty());
        assert!(restored.preferences.home_history.collections.is_empty());
        play(&mut restored, "second");
        restored.remember_account("https://other.example.com/", "another-listener");
        assert!(restored.presentation.recent_contexts.is_empty());
        drop(restored);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn restored_login_is_bound_to_its_account_and_cancel_discards_late_results() {
        let root =
            std::env::temp_dir().join(format!("navifast-restore-{:016x}", rand::random::<u64>()));
        let mut app = App::with_host(
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            None,
            true,
            Arc::new(|| {}),
        );
        assert!(!app.connecting, "demo never restores a real credential");
        app.login.server = "https://edited-preferences.example.com".into();
        app.login.username = "different-account".into();
        let saved = crate::credentials::NavidromePassword {
            server: "https://music.example.com/library/".into(),
            username: "listener".into(),
            password: "dummy-password".into(),
        };
        app.restore_login(app.session, Ok(Some(saved.clone())));
        assert!(app.connected, "restoration signs in automatically");
        assert_eq!(app.login.server, saved.server);
        assert_eq!(app.login.username, saved.username);
        let mut edited = app.login.clone();
        edited.server = "https://another-server.example.com".into();
        app.apply(Action::EditLogin(edited));
        assert!(app.login.password.is_empty());
        app.login.password = saved.password.clone();
        let mut edited = app.login.clone();
        edited.username = "another-account".into();
        app.apply(Action::EditLogin(edited));
        assert!(app.login.password.is_empty());

        let old_session = app.session;
        app.apply(Action::Cancel);
        app.connected = false;
        app.restore_login(old_session, Ok(Some(saved.clone())));
        assert!(!app.connected);
        assert!(app.login.password.is_empty());
        app.restore_login(app.session, Err("Unlock the credential store.".into()));
        assert!(!app.connecting);
        assert_eq!(app.error.as_deref(), Some("Unlock the credential store."));
        let old_session = app.session;
        app.apply(Action::SignOut);
        app.restore_login(old_session, Ok(Some(saved)));
        assert!(!app.connected);
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn desktop_track_identity_changes_after_signing_in_again() {
        let mut player = transport();
        player.prepare(0.0, false);
        assert_ne!(player.media_uri(1), player.media_uri(2));
        assert_eq!(player.media_uri(2).as_deref(), Some("navidrome:track:2:a"));
    }
}
