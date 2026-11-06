//! Adapts service data to the existing native interface. No Spotify requests.

use super::*;
use crate::api::models as view;
use crate::model::{Action as UiAction, Loadable, Page, RowContext};
use egui::load::{ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};

const FULL_ART_BYTES: usize = 32 * 1024 * 1024;
const CACHED_ART_BYTES: usize = 64 * 1024 * 1024;

struct CachedCover {
    image: Option<Arc<egui::ColorImage>>,
    preview: Arc<egui::ColorImage>,
    bytes: Arc<[u8]>,
    used: Instant,
}

impl CachedCover {
    fn full_bytes(&self) -> usize {
        self.image
            .as_ref()
            .map_or(0, |image| image.pixels.len() * 4)
    }

    fn cached_bytes(&self) -> usize {
        self.preview.pixels.len() * 4 + self.bytes.len()
    }
}

#[derive(Default)]
pub(super) struct Artwork {
    images: Mutex<HashMap<String, CachedCover>>,
    requests: Mutex<HashSet<String>>,
}

impl Artwork {
    pub fn insert(
        &self,
        ctx: &egui::Context,
        uri: String,
        cover: crate::navidrome::backend::Cover,
    ) {
        // A returning row may already have a preview texture. Replace it on
        // the next paint, without discarding the downloaded bytes or preview.
        Self::forget_textures(ctx, &uri);
        self.images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(
                uri,
                CachedCover {
                    image: Some(Arc::new(cover.image)),
                    preview: Arc::new(cover.preview),
                    bytes: cover.bytes,
                    used: Instant::now(),
                },
            );
    }

    pub fn cached_bytes(&self, uri: &str) -> Option<Arc<[u8]>> {
        self.images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(uri)
            .map(|cover| cover.bytes.clone())
    }

    pub fn evict(&self, ctx: &egui::Context) -> Vec<String> {
        self.evict_to(ctx, FULL_ART_BYTES, CACHED_ART_BYTES)
    }

    fn evict_to(
        &self,
        ctx: &egui::Context,
        full_budget: usize,
        cached_budget: usize,
    ) -> Vec<String> {
        let mut images = self.images.lock().unwrap_or_else(|p| p.into_inner());
        let mut full_bytes: usize = images.values().map(CachedCover::full_bytes).sum();
        let mut cached_bytes: usize = images.values().map(CachedCover::cached_bytes).sum();
        if full_bytes <= full_budget && cached_bytes <= cached_budget {
            return Vec::new();
        }
        let mut oldest: Vec<_> = images
            .iter()
            .map(|(uri, cover)| (uri.clone(), cover.used))
            .collect();
        oldest.sort_by_key(|(_, used)| *used);
        let mut demoted = Vec::new();
        let mut forgotten = Vec::new();
        for (uri, _) in oldest {
            if full_bytes <= full_budget && cached_bytes <= cached_budget {
                break;
            }
            if cached_bytes > cached_budget {
                if let Some(cover) = images.remove(&uri) {
                    full_bytes -= cover.full_bytes();
                    cached_bytes -= cover.cached_bytes();
                }
                forgotten.push(uri);
            } else if full_bytes > full_budget {
                let cover = images.get_mut(&uri).unwrap();
                full_bytes -= cover.full_bytes();
                if cover.image.take().is_some() {
                    demoted.push(uri);
                }
            }
        }
        drop(images);
        // GPU eviction must not also erase the small, immediately paintable
        // preview and source bytes. Decoding a full cover happens on the worker.
        for uri in demoted {
            Self::forget_textures(ctx, &uri);
        }
        for uri in &forgotten {
            ctx.forget_image(uri);
        }
        forgotten
    }

    fn forget_textures(ctx: &egui::Context, uri: &str) {
        for loader in ctx.loaders().texture.lock().iter() {
            loader.forget(uri);
        }
    }
    pub fn clear(&self, ctx: &egui::Context) {
        let uris: Vec<_> = self
            .images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect();
        self.forget_all();
        for uri in uris {
            ctx.forget_image(&uri);
        }
    }
    pub fn drain(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .collect()
    }
}
impl ImageLoader for Artwork {
    fn id(&self) -> &str {
        concat!(module_path!(), "::Artwork")
    }
    fn load(&self, _: &egui::Context, uri: &str, _: SizeHint) -> ImageLoadResult {
        if !uri.starts_with("navifast-art:") {
            return Err(LoadError::NotSupported);
        }
        if let Some(cover) = self
            .images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(uri)
        {
            cover.used = Instant::now();
            if cover.image.is_none() {
                self.requests
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .insert(uri.into());
            }
            return Ok(ImagePoll::Ready {
                image: cover.image.as_ref().unwrap_or(&cover.preview).clone(),
            });
        }
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(uri.into());
        Ok(ImagePoll::Pending { size: None })
    }
    fn forget(&self, uri: &str) {
        self.images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(uri);
    }
    fn forget_all(&self) {
        self.images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
    fn byte_size(&self) -> usize {
        self.images
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .map(|cover| cover.full_bytes() + cover.cached_bytes())
            .sum()
    }
}

pub(super) fn artwork_uri(session: u64, id: &str, size: CoverSize) -> String {
    let variant = if size == CoverSize::Portrait {
        "portrait:"
    } else {
        ""
    };
    format!("navifast-art:{session}:{variant}{}", encode(id))
}

fn artwork_request(session: u64, uri: &str) -> Option<(String, CoverSize)> {
    let key = uri.strip_prefix(&format!("navifast-art:{session}:"))?;
    let (id, size) = key
        .strip_prefix("portrait:")
        .map_or((key, CoverSize::Sleeve), |id| (id, CoverSize::Portrait));
    Some((decode(id)?, size))
}

pub(crate) fn encode(id: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in id.bytes() {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}
pub(super) fn decode(id: &str) -> Option<String> {
    let (pairs, tail) = id.as_bytes().as_chunks::<2>();
    if !tail.is_empty() {
        return None;
    }
    String::from_utf8(
        pairs
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
            .collect::<Option<Vec<_>>>()?,
    )
    .ok()
}
fn raw_uri(uri: &str, kind: &str) -> Option<String> {
    decode(uri.strip_prefix(&format!("navidrome:{kind}:"))?)
}
pub(crate) fn view_artists(
    credits: &crate::navidrome::api::ArtistCredits,
    name: &str,
) -> Vec<view::ArtistRef> {
    let artist = |name: &str, id: Option<&str>| {
        let id = id.filter(|id| !id.trim().is_empty()).map(encode);
        view::ArtistRef {
            name: name.into(),
            uri: id.as_ref().map(|id| format!("navidrome:artist:{id}")),
            id,
        }
    };
    let artists: Vec<_> = credits
        .artists
        .iter()
        .filter(|a| !a.name.trim().is_empty())
        .map(|a| artist(&a.name, a.id.as_deref()))
        .collect();
    if artists.is_empty() && !name.trim().is_empty() {
        vec![artist(name, credits.artist_id.as_deref())]
    } else {
        artists
    }
}
impl App {
    pub(super) fn draw(&mut self, ui: &mut egui::Ui) {
        if self
            .presentation_revision
            .as_ref()
            .is_some_and(|old| old.0 != self.session)
        {
            self.artwork.clear(ui.ctx());
        }
        self.sync_presentation();
        if self.connected || self.restoring_login {
            self.presentation.frame_ui(ui);
            let actions = std::mem::take(&mut self.presentation.actions);
            for action in actions {
                self.ui_action(action, ui.ctx());
            }
        } else {
            self.presentation.apply_theme(ui.ctx());
            self.palette = self.presentation.palette;
            for action in crate::ui::navidrome::login(self, ui) {
                self.apply(action);
            }
        }
        for uri in self.artwork.drain() {
            if let Some((id, size)) = artwork_request(self.session, &uri) {
                self.apply(Action::Cover(id, size));
            }
        }
        for uri in self.artwork.evict(ui.ctx()) {
            self.requested_covers.remove(&uri);
            self.presentation.accents.remove(&uri);
        }
        self.sync_presentation();
    }
    pub(super) fn cover_uri(&self, id: &str) -> String {
        artwork_uri(self.session, id, CoverSize::Sleeve)
    }
    fn view_album(&self, album: &Album) -> view::Album {
        let id = encode(&album.id);
        view::Album {
            uri: format!("navidrome:album:{id}"),
            id,
            name: album.name.clone(),
            artists: view_artists(&album.credits, &album.artist),
            album_type: Some("album".into()),
            total_tracks: Some(album.song_count),
            release_date: (album.year > 0).then(|| album.year.to_string()),
            images: album
                .cover_art
                .as_ref()
                .map(|id| view::Image {
                    url: self.cover_uri(id),
                    width: Some(256),
                    height: Some(256),
                })
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }
    fn view_track(&self, song: &Song, album: Option<&Album>) -> view::Track {
        let id = encode(&song.id);
        view::Track {
            uri: format!("navidrome:track:{id}"),
            id: Some(id),
            name: song.title.clone(),
            artists: view_artists(&song.credits, &song.artist),
            duration_ms: (song.duration * 1000.0) as u32,
            track_number: Some(song.track),
            disc_number: Some(song.disc_number),
            album: album.map(|album| self.view_album(album)),
            is_playable: Some(true),
            ..Default::default()
        }
    }
    pub(super) fn sync_presentation(&mut self) {
        let signature = (
            self.session,
            self.list_request,
            self.album_request,
            self.albums.len(),
            self.list_loading,
            self.album_loading,
            self.connected,
            self.restoring_login,
            self.error.clone(),
        );
        if self.presentation_revision.as_ref() != Some(&signature) {
            let albums: Vec<_> = self
                .albums
                .iter()
                .map(|album| view::SavedAlbum {
                    added_at: None,
                    album: self.view_album(album),
                })
                .collect();
            let album_page = self.album.as_ref().map(|album| {
                let tracks: Vec<_> = album
                    .song
                    .iter()
                    .map(|song| self.view_track(song, Some(album)))
                    .collect();
                let mut page = crate::model::AlbumPage {
                    generation: self.album_request,
                    album: Loadable::Loaded(self.view_album(album)),
                    ..Default::default()
                };
                page.tracks.absorb(
                    0,
                    view::Page {
                        total: tracks.len() as u32,
                        limit: tracks.len() as u32,
                        offset: 0,
                        next: None,
                        items: tracks,
                    },
                );
                (encode(&album.id), page)
            });
            let ui = &mut self.presentation;
            if self
                .presentation_revision
                .as_ref()
                .is_some_and(|old| old.0 != self.session)
            {
                ui.album_pages.clear();
                ui.track_cache.clear();
                ui.table_rows.clear();
                ui.accents.clear();
                self.artwork.forget_all();
            }
            ui.auth = if self.connected {
                crate::backend::AuthStatus::Connected {
                    username: self.preferences.username.clone(),
                }
            } else if self.restoring_login {
                crate::backend::AuthStatus::Starting
            } else {
                crate::backend::AuthStatus::SignedOut
            };
            ui.user = self.connected.then(|| view::User {
                id: self.preferences.username.clone(),
                display_name: Some(self.preferences.username.clone()),
                product: Some("Navidrome".into()),
                ..Default::default()
            });
            ui.library.albums.items = albums;
            ui.library.albums.loaded_once =
                !(self.list_loading || self.restoring_login) || !self.albums.is_empty();
            ui.library.albums.loading = self.list_loading || self.restoring_login;
            ui.library.albums.error = self.error.clone();
            ui.library.albums.next_offset = self.has_more.then_some(self.albums.len() as u32);
            ui.library.albums.total = Some(self.albums.len() as u32 + u32::from(self.has_more));
            ui.library.albums.revision = ui.library.albums.revision.wrapping_add(1);
            if let Some((id, mut page)) = album_page {
                if self.album_loading {
                    page.album = Loadable::Loading;
                } else if let Some(error) = &self.error {
                    page.album = Loadable::Failed(error.clone());
                }
                for track in &page.tracks.items {
                    if let Some(id) = &track.id {
                        ui.track_cache.insert(id.clone(), track.clone());
                    }
                }
                ui.album_pages.insert(id, page);
            } else if let Page::Album(id) = ui.page().clone() {
                ui.album_pages.insert(
                    id,
                    crate::model::AlbumPage {
                        generation: self.album_request,
                        album: if let Some(error) = &self.error {
                            Loadable::Failed(error.clone())
                        } else {
                            Loadable::Loading
                        },
                        ..Default::default()
                    },
                );
            }
            self.presentation_revision = Some(signature);
        }
        let track = self.transport.song().map(|song| {
            let album = self.playing_album.as_ref();
            crate::player::LocalTrack {
                uri: format!("navidrome:track:{}", encode(&song.id)),
                title: song.title.clone(),
                artists: view_artists(&song.credits, &song.artist),
                album: album.map_or(&song.album, |a| &a.name).clone(),
                art_url: album
                    .and_then(|a| a.cover_art.as_ref())
                    .or(song.cover_art.as_ref())
                    .map(|id| self.cover_uri(id)),
                duration_ms: (song.duration * 1000.0) as u32,
                ..Default::default()
            }
        });
        let local = &mut self.presentation.local;
        local.connected = self.connected;
        local.username = self.preferences.username.clone();
        local.volume = (self.preferences.volume * f32::from(u16::MAX)).round() as u16;
        local.position_ms = (self.transport.position * 1000.0) as u32;
        local.position_at = None;
        local.track_sequence = self.transport.generation;
        local.seek_sequence = self.transport.generation;
        local.playback = if track.is_none() {
            crate::player::Playback::Stopped
        } else if self.transport.paused {
            crate::player::Playback::Paused
        } else if self.transport.loading {
            crate::player::Playback::Loading
        } else {
            crate::player::Playback::Playing
        };
        local.track = track;
        local.error = self.transport.error.clone();
        self.presentation.assumed_context =
            self.playing_album
                .as_ref()
                .map(|album| super::super::AssumedContext {
                    uri: format!("navidrome:album:{}", encode(&album.id)),
                    shuffle: Some(false),
                    at: Instant::now(),
                });
        self.presentation.local_ready = self.connected;
        self.presentation.local_playback = crate::backend::LocalPlayback::Ready {
            device_id: "navidrome-local".into(),
        };
        self.presentation.settings.volume =
            (self.preferences.volume * f32::from(u16::MAX)).round() as u16;
    }
    fn play_view(&mut self, uris: &[String], index: usize) {
        let ids: Option<Vec<_>> = uris.iter().map(|uri| raw_uri(uri, "track")).collect();
        let Some(ids) = ids else {
            return;
        };
        let source = self
            .album
            .as_ref()
            .map(|album| &album.song)
            .unwrap_or(&self.transport.songs);
        let songs: Option<Vec<_>> = ids
            .iter()
            .map(|id| source.iter().find(|song| &song.id == id).cloned())
            .collect();
        if let Some(songs) = songs {
            self.play_album_songs(self.album.clone(), songs, index);
        }
    }
    pub(super) fn ui_action(&mut self, action: UiAction, ctx: &egui::Context) {
        let was_favorites = self.presentation.page() == &Page::LikedSongs;
        match action {
            UiAction::ToggleSaved(uri) if self.connected => self.toggle_favorite(&uri),
            UiAction::RetryFavorite(uri) if self.connected => self.retry_favorite(&uri),
            UiAction::PlayFavorite(id) if self.connected => {
                let songs = &self.presentation.navidrome_favorites.list.song;
                if let Some(index) = songs.iter().position(|s| s.id == id) {
                    self.play_search_songs(songs.clone(), index);
                }
            }
            UiAction::Open(Page::LikedSongs) if self.connected => {
                self.presentation.open(Page::LikedSongs)
            }
            UiAction::Reload(Page::LikedSongs) if self.connected => self.refresh_favorites(),
            UiAction::NavidromeSearch(action) if self.connected => self.search_action(action, ctx),
            UiAction::Search(query) if self.connected => {
                self.presentation.search.focus_requested = true;
                self.presentation.open(Page::Search);
                self.edit_search(query, ctx);
                self.submit_query();
            }
            UiAction::ForgetSearch(query) if self.connected => {
                self.presentation
                    .navidrome_search
                    .recent
                    .retain(|term| term != &query);
                self.save_recent_searches();
            }
            UiAction::Open(Page::Search) | UiAction::FocusSearch if self.connected => {
                self.presentation.apply(action, ctx);
            }
            UiAction::Open(page @ (Page::Artist(_) | Page::Playlist(_))) if self.connected => {
                self.presentation.open(page);
            }
            UiAction::Reload(page @ (Page::Artist(_) | Page::Playlist(_))) => {
                self.fetch_collection(&page)
            }
            UiAction::Open(Page::Album(id)) => {
                if let Some(id) = decode(&id) {
                    self.apply(Action::Open(id));
                }
                self.presentation.open(Page::Album(id));
            }
            UiAction::Open(Page::Albums | Page::Home) => {
                self.presentation.open(Page::Albums);
            }
            UiAction::Open(Page::Settings) => self.presentation.open(Page::Settings),
            UiAction::Back | UiAction::Forward => {
                self.presentation.apply(action, ctx);
                crate::ui::restore_navidrome_scroll(ctx, self.presentation.page());
                if let Page::Album(id) = self.presentation.page()
                    && let Some(id) = decode(id)
                    && !(self.album.as_ref().is_some_and(|album| album.id == id)
                        && !self.album_loading
                        && self.error.is_none())
                {
                    self.apply(Action::Open(id));
                }
            }
            UiAction::OpenUri(uri) => {
                if let Some(id) = raw_uri(&uri, "album") {
                    self.ui_action(UiAction::Open(Page::Album(encode(&id))), ctx);
                }
            }
            UiAction::PlayContext {
                uri,
                offset_index,
                offset_uri,
            } => {
                if let Some(id) = raw_uri(&uri, "album") {
                    let index = offset_index
                        .map(|i| i as usize)
                        .or_else(|| {
                            offset_uri
                                .and_then(|uri| raw_uri(&uri, "track"))
                                .and_then(|id| {
                                    self.album
                                        .as_ref()?
                                        .song
                                        .iter()
                                        .position(|song| song.id == id)
                                })
                        })
                        .unwrap_or(0);
                    if self.album.as_ref().is_some_and(|album| album.id == id)
                        && !self.album_loading
                    {
                        self.apply(Action::Play(index));
                    } else {
                        self.apply(Action::Open(id.clone()));
                        self.pending_play = Some((id, index));
                    }
                }
            }
            UiAction::PlayFromRow {
                context,
                uri,
                index,
            } => match context {
                RowContext::Uris(uris) | RowContext::View { uris, .. } => {
                    self.play_view(&uris, index as usize)
                }
                RowContext::Context { uri: album, .. } => self.ui_action(
                    UiAction::PlayContext {
                        uri: album,
                        offset_uri: Some(uri),
                        offset_index: Some(index),
                    },
                    ctx,
                ),
                RowContext::Queue => {}
            },
            UiAction::PlayUris { uris, index } => self.play_view(&uris, index as usize),
            UiAction::TogglePlay => self.apply(Action::Toggle),
            UiAction::Next => self.apply(Action::Next),
            UiAction::Previous => self.apply(Action::Previous),
            UiAction::Seek(ms) => self.apply(Action::Seek(f64::from(ms) / 1000.0)),
            UiAction::SeekBy(ms) => {
                self.apply(Action::Seek(self.transport.position + ms as f64 / 1000.0))
            }
            UiAction::SetVolume(value) | UiAction::PreviewVolume(value) => {
                self.apply(Action::Volume(f32::from(value) / 100.0))
            }
            UiAction::VolumeBy(value) => self.apply(Action::Volume(
                self.preferences.volume + f32::from(value) / 100.0,
            )),
            UiAction::ToggleMute => self.apply(Action::Volume(if self.preferences.volume == 0.0 {
                0.7
            } else {
                0.0
            })),
            UiAction::SignOut => {
                self.pending_play = None;
                self.apply(Action::SignOut);
            }
            UiAction::LoadMore(Page::Albums) => {
                if !self.list_loading && self.has_more {
                    self.apply(Action::More);
                }
            }
            UiAction::Reload(Page::Albums) => self.apply(Action::Browse(self.newest)),
            UiAction::Reload(Page::Album(id)) => {
                if let Some(id) = decode(&id) {
                    self.apply(Action::Open(id));
                }
            }
            UiAction::SetLibrarySort {
                shelf: crate::settings::LibraryShelf::Albums,
                sort,
            } => {
                self.presentation
                    .settings
                    .library_sort
                    .insert(crate::settings::LibraryShelf::Albums, sort);
                self.mark_dirty();
                if matches!(
                    sort,
                    crate::settings::LibrarySort::RecentlyAdded
                        | crate::settings::LibrarySort::Name
                ) {
                    self.apply(Action::Browse(
                        sort == crate::settings::LibrarySort::RecentlyAdded,
                    ));
                }
            }
            // Local interface/settings actions can keep their existing implementation.
            action @ (UiAction::SetTheme(_)
            | UiAction::SetCustomTheme(_)
            | UiAction::ReloadThemes
            | UiAction::SetLanguage(_)
            | UiAction::OpenUrl(_)
            | UiAction::SettingsChanged
            | UiAction::ToggleSidebar
            | UiAction::SetLibraryGrid(_)
            | UiAction::ArrangeLibrary { .. }
            | UiAction::CyclePlayerBarVis
            | UiAction::ToggleEq
            | UiAction::SetEqBand(..)
            | UiAction::SetEqPreamp(_)
            | UiAction::ApplyEqPreset(_)
            | UiAction::ShowDialog(crate::model::Dialog::Shortcuts)
            | UiAction::CloseDialog
            | UiAction::ShowWindow
            | UiAction::CloseWindow
            | UiAction::Quit) => {
                self.presentation.apply(action, ctx);
                self.preferences.eq_on = self.presentation.settings.eq_on;
                self.preferences.bands = self.presentation.settings.eq_bands_db;
                self.preferences.preamp = self.presentation.settings.eq_preamp_db;
                self.mark_dirty();
            }
            UiAction::OpenThemesFolder => {
                self.backend
                    .send(Request::OpenThemesFolder(self.dirs.config.join("themes")));
            }
            _ => {}
        }
        if !was_favorites && self.presentation.page() == &Page::LikedSongs {
            self.refresh_favorites();
        }
        let collection = match self.presentation.page() {
            page @ (Page::Artist(_) | Page::Playlist(_)) => Some(page),
            _ => None,
        };
        if collection != self.presentation.navidrome_search.collection_page.as_ref() {
            self.fetch_collection(&self.presentation.page().clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::app::navidrome::tests::with_app;

    #[test]
    fn native_rows_play_the_displayed_order_and_keep_duplicate_occurrences() {
        with_app(|app, ctx| {
            let tracks = app.album.as_ref().unwrap().song.clone();
            let uris: Vec<_> = [2, 1, 0]
                .map(|i| app.view_track(&tracks[i], app.album.as_ref()).uri)
                .into();
            app.ui_action(
                UiAction::PlayFromRow {
                    context: RowContext::View {
                        uris: uris.into(),
                        context_uri: String::new(),
                        editable_playlist: None,
                    },
                    uri: String::new(),
                    index: 0,
                },
                ctx,
            );
            app.sync_presentation();
            assert_eq!(app.presentation.now_playing().unwrap().title, "First");
            app.ui_action(UiAction::Next, ctx);
            app.sync_presentation();
            assert_eq!(app.presentation.now_playing().unwrap().title, "Second");
            app.ui_action(UiAction::Next, ctx);
            app.sync_presentation();
            assert_eq!(app.presentation.now_playing().unwrap().title, "First");
            assert_eq!(app.transport.index, Some(2));
        });
    }

    fn drawn_text(app: &mut App, ctx: &egui::Context, width: f32) -> Vec<String> {
        fn collect(shape: &egui::epaint::Shape, text: &mut Vec<String>) {
            match shape {
                egui::epaint::Shape::Text(value) => text.push(value.galley.job.text.clone()),
                egui::epaint::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, text);
                    }
                }
                _ => {}
            }
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
                ..Default::default()
            },
            |ui| app.draw(ui),
        );
        output.textures_delta.clear();
        let mut text = Vec::new();
        for shape in output.shapes {
            collect(&shape.shape, &mut text);
        }
        text
    }

    #[test]
    fn startup_shows_the_main_window_through_lookup_and_ping_then_requires_login_only_on_failure() {
        for accepted in [true, false] {
            with_app(|app, ctx| {
                let server = crate::navidrome::test_server::Server::spawn(move |request| {
                    let body = if !accepted {
                        br#"{"subsonic-response":{"status":"failed","error":{"code":40}}}"#
                            .as_slice()
                    } else if request.starts_with("GET /rest/ping.view?") {
                        br#"{"subsonic-response":{"status":"ok"}}"#.as_slice()
                    } else if request.starts_with("GET /rest/getStarred2.view?") {
                        br#"{"subsonic-response":{"status":"ok","starred2":{}}}"#.as_slice()
                    } else {
                        assert!(request.starts_with("GET /rest/getAlbumList2.view?"));
                        br#"{"subsonic-response":{"status":"ok","albumList2":{"album":[]}}}"#
                            .as_slice()
                    };
                    crate::navidrome::test_server::response("200 OK", "", body)
                });
                app.connected = false;
                app.connecting = true;
                app.restoring_login = true;
                app.albums.clear();
                app.album = None;
                app.proxy = crate::settings::ProxyConfig::Off;
                app.demo = false;
                for phase in ["lookup", "ping"] {
                    for theme in [
                        crate::settings::ThemeChoice::Light,
                        crate::settings::ThemeChoice::Dark,
                    ] {
                        app.presentation.settings.theme = theme;
                        for width in [760.0, 1280.0] {
                            let text = drawn_text(app, ctx, width);
                            assert!(
                                text.iter().any(|value| value == "Albums"),
                                "{phase}: main page is visible"
                            );
                            assert!(
                                !text.iter().any(|value| value == "Password"
                                    || value == "Server URL"
                                    || value == "No albums"),
                                "{phase}: no login or empty-state flash"
                            );
                        }
                    }
                    assert!(app.presentation.library.albums.loading);
                    assert!(!app.presentation.library.albums.loaded_once);
                    assert!(!app.presentation.is_connected());
                    assert!(app.presentation.user.is_none());
                    assert!(!app.presentation.local_ready);
                    if phase == "lookup" {
                        app.restore_login(
                            app.session,
                            Ok(Some(crate::credentials::NavidromePassword {
                                server: server.url.clone(),
                                username: "listener".into(),
                                password: "dummy-password".into(),
                            })),
                        );
                        assert!(app.restoring_login);
                        assert!(app.connecting);
                    }
                }
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while app.connecting || app.presentation.navidrome_favorites.loading {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "login request timed out"
                    );
                    app.poll(ctx);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                assert!(!app.restoring_login);
                assert_eq!(app.connected, accepted);
                assert_eq!(app.presentation.navidrome_favorites.ready, accepted);
                let text = drawn_text(app, ctx, 1280.0);
                assert_eq!(text.iter().any(|value| value == "Password"), !accepted);
                let completed_session = app.session;
                app.apply(Action::SignOut);
                app.restore_login(completed_session, Ok(None));
                assert!(!app.restoring_login);
                assert!(
                    drawn_text(app, ctx, 760.0)
                        .iter()
                        .any(|value| value == "Password")
                );
            });
        }
    }

    #[test]
    fn missing_or_unreadable_saved_login_returns_to_the_form_and_cancel_stays_signed_out() {
        with_app(|app, ctx| {
            for result in [Ok(None), Err("Unlock the credential store.".into())] {
                app.connected = false;
                app.connecting = true;
                app.restoring_login = true;
                app.restore_login(app.session, result);
                assert!(!app.connecting);
                assert!(!app.restoring_login);
                assert!(
                    drawn_text(app, ctx, 760.0)
                        .iter()
                        .any(|value| value == "Password")
                );
            }
            app.connecting = true;
            app.restoring_login = true;
            let old_session = app.session;
            app.apply(Action::Cancel);
            app.restore_login(
                old_session,
                Ok(Some(crate::credentials::NavidromePassword {
                    server: "https://music.example.com".into(),
                    username: "listener".into(),
                    password: "dummy-password".into(),
                })),
            );
            assert!(!app.connected);
            assert!(!app.restoring_login);
        });
    }

    #[test]
    fn browsing_another_album_does_not_replace_the_playing_album_or_artwork() {
        with_app(|app, ctx| {
            let album_uri = app.view_album(app.album.as_ref().unwrap()).uri;
            app.ui_action(
                UiAction::PlayContext {
                    uri: album_uri.clone(),
                    offset_index: Some(1),
                    offset_uri: None,
                },
                ctx,
            );
            let cover = app.cover_uri("art:/🌍");
            app.album = Some(Album {
                id: "other".into(),
                name: "Browsing".into(),
                cover_art: Some("other-art".into()),
                ..Default::default()
            });
            app.album_request += 1;
            app.sync_presentation();
            let now = app.presentation.now_playing().unwrap();
            assert_eq!(now.title, "Second");
            assert_eq!(now.album_name, "Original album");
            assert_eq!(now.art_url.as_deref(), Some(cover.as_str()));
            assert_eq!(
                app.presentation.playing_context_uri().as_deref(),
                Some(album_uri.as_str())
            );
        });
    }

    #[test]
    fn existing_settings_control_the_active_theme_and_player_equalizer() {
        with_app(|app, ctx| {
            app.ui_action(UiAction::SetTheme(crate::settings::ThemeChoice::Light), ctx);
            assert_eq!(
                app.presentation.palette.window,
                crate::theme::Palette::light().window
            );
            app.ui_action(UiAction::ToggleEq, ctx);
            app.ui_action(UiAction::SetEqBand(3, 6.0), ctx);
            let eq = *app.eq.lock().unwrap();
            assert!(eq.on);
            assert_eq!(eq.bands_db[3], 6.0);
            let restored: crate::settings::Settings =
                serde_json::from_str(&serde_json::to_string(&app.presentation.settings).unwrap())
                    .unwrap();
            assert_eq!(restored.theme, crate::settings::ThemeChoice::Light);
            assert_eq!(restored.eq_bands_db[3], eq.bands_db[3]);
            assert_eq!(app.preferences.bands[3], eq.bands_db[3]);
        });
    }

    #[test]
    fn album_header_pauses_without_restarting_and_unsupported_shuffle_is_disabled() {
        with_app(|app, ctx| {
            ctx.enable_accesskit();
            let id = encode(&app.album.as_ref().unwrap().id);
            app.presentation.history = vec![Page::Album(id)];
            app.apply(Action::Play(1));
            app.apply(Action::Seek(45.0));
            let generation = app.transport.generation;
            let mut controls = None;
            for _ in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.draw(ui),
                );
                output.textures_delta.clear();
                controls = output.platform_output.accesskit_update;
            }
            let controls = controls.unwrap();
            let pause = controls
                .nodes
                .iter()
                .filter(|(_, node)| node.label() == Some("Pause"))
                .filter_map(|(_, node)| node.bounds())
                .max_by(|a, b| a.height().total_cmp(&b.height()))
                .unwrap();
            let shuffle = controls
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some("Shuffle"))
                .unwrap();
            assert!(shuffle.1.is_disabled());
            let pos = egui::pos2(
                (pause.x0 + pause.x1) as f32 / 2.0,
                (pause.y0 + pause.y1) as f32 / 2.0,
            );
            for pressed in [true, false] {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 800.0),
                        )),
                        events: vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: Default::default(),
                            },
                        ],
                        ..Default::default()
                    },
                    |ui| app.draw(ui),
                );
                output.textures_delta.clear();
            }
            assert!(app.transport.paused);
            assert_eq!(app.transport.position, 45.0);
            assert_eq!(app.transport.generation, generation);
        });
    }

    #[test]
    fn opaque_ids_round_trip_and_artwork_does_not_contain_authenticated_urls() {
        with_app(|app, _| {
            let album = app.view_album(app.album.as_ref().unwrap());
            assert_eq!(
                raw_uri(&album.uri, "album").as_deref(),
                Some("library:/🌍/album")
            );
            assert_eq!(decode(&album.id).as_deref(), Some("library:/🌍/album"));
            assert!(album.images[0].url.starts_with("navifast-art:0:"));
            assert!(!album.images[0].url.contains("http"));
            assert!(raw_uri("spotify:album:123", "album").is_none());
            assert!(decode("abc").is_none());
            assert!(decode("zz").is_none());
        });
    }

    #[test]
    fn library_sort_keeps_existing_rows_and_the_open_album_visible() {
        with_app(|app, ctx| {
            let id = encode(&app.album.as_ref().unwrap().id);
            app.presentation.history = vec![Page::Album(id.clone())];
            app.sync_presentation();
            let old_request = app.album_request;
            app.ui_action(
                UiAction::SetLibrarySort {
                    shelf: crate::settings::LibraryShelf::Albums,
                    sort: crate::settings::LibrarySort::Name,
                },
                ctx,
            );
            app.sync_presentation();
            assert_eq!(app.presentation.library.albums.items.len(), 1);
            assert_eq!(app.album_request, old_request);
            assert_eq!(app.album.as_ref().unwrap().name, "Original album");
            assert!(matches!(
                app.presentation.album_pages[&id].album,
                Loadable::Loaded(_)
            ));
            assert!(!app.newest);
        });
    }

    fn cover_fixture() -> crate::navidrome::backend::Cover {
        crate::navidrome::backend::Cover {
            image: egui::ColorImage::filled([300, 300], egui::Color32::RED),
            preview: egui::ColorImage::filled([64, 64], egui::Color32::RED),
            bytes: Arc::from([1, 2, 3]),
            color: None,
        }
    }

    fn texture(ctx: &egui::Context, uri: &str) -> egui::load::SizedTexture {
        let egui::load::TexturePoll::Ready { texture } = ctx
            .try_load_texture(uri, Default::default(), Default::default())
            .unwrap()
        else {
            panic!("cached artwork must be immediately paintable");
        };
        texture
    }

    #[test]
    fn scrolling_past_the_full_art_budget_keeps_returning_covers_paintable() {
        let ctx = egui::Context::default();
        let artwork = Arc::new(Artwork::default());
        ctx.add_image_loader(artwork.clone());
        // Three library pages exceed both the old 120-cover cap and the
        // full-image memory budget. Each cover has really reached a texture.
        for index in 0..180 {
            let uri = format!("navifast-art:1:{index}");
            artwork.insert(&ctx, uri.clone(), cover_fixture());
            assert_eq!(texture(&ctx, &uri).size, egui::vec2(300.0, 300.0));
            assert!(artwork.evict(&ctx).is_empty());
        }
        assert!(artwork.byte_size() <= FULL_ART_BYTES + CACHED_ART_BYTES);
        for index in 0..180 {
            let uri = format!("navifast-art:1:{index}");
            let shown = texture(&ctx, &uri);
            assert!(shown.size.x == 64.0 || shown.size.x == 300.0);
            assert_eq!(&*artwork.cached_bytes(&uri).unwrap(), &[1, 2, 3]);
        }
        assert!(
            !artwork.drain().is_empty(),
            "previews request local decoding"
        );
    }

    #[test]
    fn returning_cover_keeps_a_preview_until_the_full_texture_is_restored() {
        let ctx = egui::Context::default();
        let artwork = Arc::new(Artwork::default());
        ctx.add_image_loader(artwork.clone());
        let uri = "navifast-art:1:0";
        artwork.insert(&ctx, uri.into(), cover_fixture());
        let original = texture(&ctx, uri);
        assert!(artwork.evict_to(&ctx, 0, CACHED_ART_BYTES).is_empty());
        let preview = texture(&ctx, uri);
        assert_eq!(preview.size, egui::vec2(64.0, 64.0));
        assert_ne!(original.id, preview.id);
        assert_eq!(artwork.drain(), vec![uri]);

        artwork.insert(&ctx, uri.into(), cover_fixture());
        let restored = texture(&ctx, uri);
        assert_eq!(restored.size, original.size);
        assert_ne!(restored.id, preview.id);
        assert!(artwork.drain().is_empty());
    }

    #[cfg(feature = "demo")]
    #[test]
    fn demo_library_repaints_covers_after_scrolling_in_both_themes_and_sizes() {
        fn painted_artwork(
            app: &mut App,
            ctx: &egui::Context,
            uris: &[String],
            width: f32,
        ) -> usize {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| app.draw(ui),
            );
            output.textures_delta.clear();
            let ids: HashSet<_> = uris.iter().map(|uri| texture(ctx, uri).id).collect();
            ctx.tessellate(output.shapes, output.pixels_per_point)
                .iter()
                .filter_map(|primitive| match &primitive.primitive {
                    egui::epaint::Primitive::Mesh(mesh) if ids.contains(&mesh.texture_id) => {
                        Some(mesh.indices.len())
                    }
                    _ => None,
                })
                .sum()
        }

        for light in [false, true] {
            for width in [760.0, 1280.0] {
                with_app(|app, ctx| {
                    app.albums.clear();
                    app.album = None;
                    app.populate_demo(Some("albums"), light.then_some("light"), ctx);
                    app.presentation.history = vec![Page::Albums];
                    let uris: Vec<_> = app
                        .albums
                        .iter()
                        .filter_map(|album| album.cover_art.as_ref())
                        .map(|id| app.cover_uri(id))
                        .collect();
                    painted_artwork(app, ctx, &uris, width); // Settle the initial layout.
                    let before = painted_artwork(app, ctx, &uris, width);
                    assert!(before > 0);
                    // Loading covers further down the collection evicts the
                    // first page's full images before the user scrolls back.
                    for index in 0..180 {
                        app.artwork.insert(
                            ctx,
                            format!("navifast-art:0:offscreen-{index}"),
                            cover_fixture(),
                        );
                    }
                    assert!(app.artwork.evict(ctx).is_empty());
                    assert!(
                        uris.iter()
                            .all(|uri| { app.artwork.images.lock().unwrap()[uri].image.is_none() })
                    );
                    // Draw again while the local restore is still pending.
                    let after = painted_artwork(app, ctx, &uris, width);
                    assert_eq!(after, before, "light={light}, width={width}");
                });
            }
        }
    }

    #[test]
    fn artwork_budget_keeps_visible_textures_and_sign_out_clears_every_tier() {
        let ctx = egui::Context::default();
        let artwork = Arc::new(Artwork::default());
        ctx.add_image_loader(artwork.clone());
        let visible = "navifast-art:1:0";
        let older = "navifast-art:1:1";
        for uri in [visible, older] {
            artwork.insert(&ctx, uri.into(), cover_fixture());
        }
        let shown = texture(&ctx, visible);
        // Paint through the actual widget after the texture exists, which
        // must refresh recency even though egui skips its image loader.
        let mut output = ctx.run_ui(Default::default(), |ui| {
            crate::ui::widgets::cover(
                ui,
                &crate::theme::Palette::dark(),
                Some(visible),
                64.0,
                0.0,
                crate::theme::Icon::Music,
            );
        });
        output.textures_delta.clear();
        let cache_size = 64 * 64 * 4 + 3;
        let forgotten = artwork.evict_to(&ctx, 300 * 300 * 4, cache_size);
        assert_eq!(forgotten, vec![older]);
        assert_eq!(texture(&ctx, visible).id, shown.id);
        assert_eq!(artwork.byte_size(), 300 * 300 * 4 + cache_size);
        assert!(matches!(
            artwork.load(&ctx, older, Default::default()).unwrap(),
            ImagePoll::Pending { .. }
        ));
        assert_eq!(artwork.drain(), forgotten);
        assert!(artwork.evict_to(&ctx, 0, cache_size).is_empty());
        texture(&ctx, visible);
        artwork.clear(&ctx);
        assert_eq!(artwork.byte_size(), 0);
        assert!(artwork.drain().is_empty());
        assert!(matches!(
            ctx.try_load_texture(visible, Default::default(), Default::default()),
            Ok(egui::load::TexturePoll::Pending { .. })
        ));
    }
    #[test]
    fn artwork_sizes_and_sessions_round_trip_opaque_service_ids_separately() {
        for size in [CoverSize::Sleeve, CoverSize::Portrait] {
            let uri = artwork_uri(7, "portrait:/λ", size);
            assert_eq!(artwork_request(7, &uri), Some(("portrait:/λ".into(), size)));
            assert_eq!(artwork_request(8, &uri), None);
        }
    }
}
