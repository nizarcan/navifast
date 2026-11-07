//! Native navigation, artwork identities and control actions.
use super::*;
use crate::model::{Action as UiAction, Page, RowContext};

pub(super) fn artwork_uri(session: u64, id: &str, size: CoverSize) -> String {
    let variant = match size {
        CoverSize::Portrait => "portrait:",
        CoverSize::Original => "original:",
        CoverSize::Sleeve => "",
    };
    format!("navifast-art:{session}:{variant}{}", encode(id))
}

#[cfg(test)]
fn artwork_request(session: u64, uri: &str) -> Option<(String, CoverSize)> {
    let key = uri.strip_prefix(&format!("navifast-art:{session}:"))?;
    let (id, size) = if let Some(id) = key.strip_prefix("original:") {
        (id, CoverSize::Original)
    } else {
        key.strip_prefix("portrait:")
            .map_or((key, CoverSize::Sleeve), |id| (id, CoverSize::Portrait))
    };
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
pub(crate) fn decode(id: &str) -> Option<String> {
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
impl App {
    pub(crate) fn cover_uri(&self, id: &str) -> String {
        artwork_uri(self.session, id, CoverSize::Sleeve)
    }
    fn play_view(&mut self, uris: &[String], index: usize) {
        let ids: Option<Vec<_>> = uris.iter().map(|uri| raw_uri(uri, "track")).collect();
        let Some(ids) = ids else {
            return;
        };
        let source = self
            .album
            .as_ref()
            .map(|album| album.song.as_slice())
            .unwrap_or(self.transport.queue.context());
        let songs: Option<Vec<_>> = ids
            .iter()
            .map(|id| source.iter().find(|song| &song.id == id).cloned())
            .collect();
        if let Some(songs) = songs {
            self.play_album_songs(self.album.clone(), songs, index);
        }
    }
    pub(super) fn ui_action(&mut self, action: UiAction, host: &impl Host) {
        let was_favorites = self.presentation.page().is_favorites();
        let was_playlists = matches!(self.presentation.page(), Page::Home | Page::Playlists);
        match action {
            UiAction::Browse(action) if self.connected => self.browse_action(action),
            UiAction::Open(ref page) if self.connected && page.is_browse() => {
                self.presentation.open(page.clone())
            }
            UiAction::Reload(ref page) if self.connected && page.is_browse() => {
                self.browse_action(super::browse::Action::Refresh)
            }
            UiAction::Queue(action) if self.connected => self.queue_action(action),
            UiAction::EditPlaylists(action) if self.connected => self.playlist_action(action),
            UiAction::Scrobble(action) => self.scrobble_action(action),
            UiAction::Rating(action) if self.connected => self.rating_action(action),
            UiAction::ToggleSaved(uri) if self.connected => self.toggle_favorite(&uri),
            UiAction::RetryFavorite(uri) if self.connected => self.retry_favorite(&uri),
            UiAction::PlayFavorite(id) if self.connected => {
                let songs = &self.presentation.navidrome_favorites.list.song;
                if let Some(index) = songs.iter().position(|s| s.id == id) {
                    self.play_search_songs(songs.clone(), index);
                }
            }
            UiAction::Open(page) if self.connected && page.is_favorites() => {
                self.presentation.open(page)
            }
            UiAction::Reload(page) if self.connected && page.is_favorites() => {
                self.refresh_favorites()
            }
            UiAction::Open(Page::Playlists) if self.connected => {
                self.presentation.open(Page::Playlists);
            }
            UiAction::Reload(Page::Playlists) if self.connected => self.fetch_playlists(),
            UiAction::NavidromeSearch(action) if self.connected => self.search_action(action, host),
            UiAction::Search(query) if self.connected => {
                self.presentation.search.focus_requested = true;
                self.presentation.open(Page::Search);
                self.edit_search(query, host);
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
                host.local_action(self, action);
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
            UiAction::Open(page @ (Page::Albums | Page::Home)) => {
                self.presentation.open(page);
            }
            UiAction::Reload(Page::Home) => {
                self.fetch_playlists();
                self.apply(Action::Browse(self.newest));
            }
            UiAction::Open(Page::Stats) if self.connected => self.presentation.open(Page::Stats),
            UiAction::Reload(Page::Stats) if self.connected => self.refresh_stats(),
            UiAction::Open(page @ (Page::Settings | Page::Downloads | Page::Download(_))) => {
                self.presentation.open(page)
            }
            UiAction::Back | UiAction::Forward => {
                host.local_action(self, action);
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
                    self.ui_action(UiAction::Open(Page::Album(encode(&id))), host);
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
                        self.edit_session();
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
                    host,
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
            UiAction::OpenUrl(url) => crate::opener::open_url(url),
            // Local interface/settings actions can keep their existing implementation.
            action @ (UiAction::SetTheme(_)
            | UiAction::SetCustomTheme(_)
            | UiAction::ReloadThemes
            | UiAction::SetLanguage(_)
            | UiAction::SettingsChanged
            | UiAction::ToggleSidebar
            | UiAction::SetLibraryGrid(_)
            | UiAction::ArrangeLibrary { .. }
            | UiAction::CyclePlayerBarVis
            | UiAction::ToggleEq
            | UiAction::SetEqBand(..)
            | UiAction::SetEqPreamp(_)
            | UiAction::ApplyEqPreset(_)
            | UiAction::ShowDialog(_)
            | UiAction::CloseDialog
            | UiAction::ShowWindow
            | UiAction::CloseWindow
            | UiAction::Quit) => {
                host.local_action(self, action);
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
        if !was_favorites && self.presentation.page().is_favorites() {
            self.refresh_favorites();
        }
        let is_playlists = matches!(self.presentation.page(), Page::Home | Page::Playlists);
        if is_playlists
            && (!was_playlists
                || matches!(
                    self.presentation.navidrome_playlists.list,
                    crate::model::Loadable::NotLoaded
                ))
        {
            self.fetch_playlists();
        } else if was_playlists && !is_playlists {
            self.cancel_playlists();
        }
        let collection = match self.presentation.page() {
            page @ (Page::Artist(_) | Page::Playlist(_)) => Some(page),
            _ => None,
        };
        if collection != self.presentation.navidrome_search.collection_page.as_ref() {
            self.fetch_collection(&self.presentation.page().clone());
        }
        self.sync_browse();
        self.sync_stats();
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
                .map(|i| format!("navidrome:track:{}", encode(&tracks[i].id)))
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
            assert_eq!(app.transport.song().unwrap().title, "First");
            app.ui_action(UiAction::Next, ctx);
            assert_eq!(app.transport.song().unwrap().title, "Second");
            app.ui_action(UiAction::Next, ctx);
            assert_eq!(app.transport.song().unwrap().title, "First");
            assert_eq!(app.transport.queue.context_index(), Some(2));
        });
    }
    #[test]
    fn startup_shows_the_main_window_through_lookup_and_ping_then_requires_login_only_on_failure() {
        for accepted in [true, false] {
            with_app(|app, ctx| {
                let server = crate::navidrome::test_server::Server::spawn(move |request| {
                    let body = if !accepted {
                        br#"{"subsonic-response":{"status":"failed","error":{"code":40}}}"#
                            .as_slice()
                    } else if request.starts_with("GET /rest/getOpenSubsonicExtensions.view?") {
                        br#"{"subsonic-response":{"status":"ok","openSubsonicExtensions":[]}}"#
                            .as_slice()
                    } else if request.starts_with("GET /rest/ping.view?") {
                        br#"{"subsonic-response":{"status":"ok"}}"#.as_slice()
                    } else if request.starts_with("GET /rest/getStarred2.view?") {
                        br#"{"subsonic-response":{"status":"ok","starred2":{}}}"#.as_slice()
                    } else if request.starts_with("GET /rest/getPlaylists.view?") {
                        br#"{"subsonic-response":{"status":"ok","playlists":{}}}"#.as_slice()
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
                    assert!(app.restoring_login);
                    assert!(!app.connected);
                    assert_eq!(app.presentation.page(), &Page::Home);
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
                let completed_session = app.session;
                app.apply(Action::SignOut);
                app.restore_login(completed_session, Ok(None));
                assert!(!app.restoring_login);
                assert!(!app.connected);
            });
        }
    }
    #[test]
    fn missing_or_unreadable_saved_login_returns_to_the_form_and_cancel_stays_signed_out() {
        with_app(|app, _| {
            for result in [Ok(None), Err("Unlock the credential store.".into())] {
                app.connected = false;
                app.connecting = true;
                app.restoring_login = true;
                app.restore_login(app.session, result);
                assert!(!app.connecting);
                assert!(!app.restoring_login);
                assert!(!app.connected);
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
            let album_uri = format!(
                "navidrome:album:{}",
                encode(&app.album.as_ref().unwrap().id)
            );
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
            assert_eq!(app.transport.song().unwrap().title, "Second");
            assert_eq!(app.playing_album.as_ref().unwrap().name, "Original album");
            assert_eq!(app.cover_uri(app.playing_cover().unwrap()), cover);
            assert_eq!(
                format!(
                    "navidrome:album:{}",
                    encode(&app.playing_album.as_ref().unwrap().id)
                ),
                album_uri
            );
        });
    }
    #[test]
    fn existing_settings_control_the_active_theme_and_player_equalizer() {
        with_app(|app, ctx| {
            app.ui_action(UiAction::SetTheme(crate::settings::ThemeChoice::Light), ctx);
            assert_eq!(
                app.presentation.settings.theme,
                crate::settings::ThemeChoice::Light
            );
            let mut bands = [0.; 10];
            bands[3] = 6.;
            app.presentation.settings.eq_on = true;
            app.presentation.settings.eq_bands_db = bands;
            app.apply(Action::SetEq {
                on: true,
                bands,
                preamp: 0.,
            });
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
    fn opaque_ids_round_trip_and_artwork_does_not_contain_authenticated_urls() {
        with_app(|app, _| {
            let id = encode(&app.album.as_ref().unwrap().id);
            assert_eq!(
                raw_uri(&format!("navidrome:album:{id}"), "album").as_deref(),
                Some("library:/🌍/album")
            );
            let artwork = app.cover_uri("art:/🌍");
            assert!(artwork.starts_with("navifast-art:0:"));
            assert!(!artwork.contains("http"));
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
            let old_request = app.album_request;
            app.ui_action(
                UiAction::SetLibrarySort {
                    shelf: crate::settings::LibraryShelf::Albums,
                    sort: crate::settings::LibrarySort::Name,
                },
                ctx,
            );
            assert_eq!(app.albums.len(), 1);
            assert_eq!(app.album_request, old_request);
            assert_eq!(app.album.as_ref().unwrap().name, "Original album");
            assert_eq!(app.presentation.page(), &Page::Album(id));
            assert!(!app.newest);
        });
    }
    #[test]
    fn artwork_sizes_and_sessions_round_trip_opaque_service_ids_separately() {
        for size in [CoverSize::Sleeve, CoverSize::Portrait, CoverSize::Original] {
            let uri = artwork_uri(7, "portrait:/λ", size);
            assert_eq!(artwork_request(7, &uri), Some(("portrait:/λ".into(), size)));
            assert_eq!(artwork_request(8, &uri), None);
        }
    }
}
