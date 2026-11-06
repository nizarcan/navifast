use super::*;
use crate::app::navidrome::{self, tests::with_app};
use crate::model::Action as UiAction;
use crate::navidrome::{
    api::{Client, Song},
    test_server::{Server, response},
};
use crate::settings::ProxyConfig;

#[test]
fn overlapping_searches_ignore_both_old_responses_even_during_debounce() {
    with_app(|app, ctx| {
        let session = app.session;
        app.ui_action(UiAction::Search("first".into()), ctx);
        let first = app.presentation.search.serial;
        app.edit_search("second".into(), ctx);
        let second = app.presentation.search.serial;
        let catalogue = |request, id: &str| Event::Catalogue {
            session,
            request,
            result: Ok(Catalogue {
                song: vec![Song {
                    id: id.into(),
                    title: id.into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        };
        let playlists = |request, name: &str| Event::Playlists {
            session,
            request,
            result: Ok(vec![NamedItem {
                id: name.into(),
                name: name.into(),
                cover_art: None,
            }]),
        };
        app.search_event(catalogue(first, "stale"));
        app.poll_search(ctx);
        assert!(app.presentation.search.typed_at.is_some());
        assert!(app.presentation.navidrome_search.hits().next().is_none());
        assert_eq!(app.presentation.navidrome_search.recent, ["first"]);
        app.presentation.search.typed_at = Some(Instant::now() - DEBOUNCE);
        app.poll_search(ctx);
        assert!(app.presentation.search.typed_at.is_none());
        app.search_event(playlists(second, "second playlist"));
        app.presentation.navidrome_search.catalogue = Loadable::Loading;
        app.search_action(Action::Move(true), ctx);
        app.search_event(catalogue(second, "second song"));
        app.search_event(catalogue(first, "stale"));
        app.search_event(playlists(first, "stale"));
        let state = &app.presentation.navidrome_search;
        assert_eq!(state.selected, Some(Hit::Playlist(0)));
        assert_eq!(state.playlists.get().unwrap()[0].name, "second playlist");
        assert_eq!(state.catalogue.get().unwrap().song[0].title, "second song");
        app.search_action(Action::Move(false), ctx);
        app.search_action(Action::Submit, ctx);
        assert_eq!(app.transport.song().unwrap().title, "second song");
        assert_eq!(
            app.presentation.navidrome_search.recent,
            ["second", "first"]
        );
        assert!(app.playing_album.is_none());
        app.edit_search(String::new(), ctx);
        app.search_event(catalogue(second, "cleared"));
        app.search_event(playlists(second, "cleared"));
        assert!(app.presentation.navidrome_search.hits().next().is_none());
        app.apply(navidrome::Action::SignOut);
        app.search_event(catalogue(0, "old account"));
        assert!(app.presentation.navidrome_search.hits().next().is_none());
        assert!(app.presentation.navidrome_search.recent.is_empty());
    });
}

#[test]
fn playlist_only_match_survives_catalogue_failure_and_retry_then_enter_opens_it() {
    with_app(|app, ctx| {
        app.albums.clear();
        app.album = None;
        let mut searches = 0;
        let server = Server::spawn(move |request| {
            let body = if request.starts_with("GET /rest/search3.view?") {
                assert!(request.contains("query=mIx"));
                searches += 1;
                if searches == 1 {
                    r#"{"status":"failed","error":{"code":50}}"#
                } else {
                    r#"{"status":"ok","searchResult3":{}}"#
                }
            } else if request.starts_with("GET /rest/getPlaylists.view?") {
                assert!(!request.contains("query="));
                r#"{"status":"ok","playlists":{"playlist":[{"id":"p:/1","name":"Road Mix"},{"id":"other","name":"Ignored"}]}}"#
            } else if request.starts_with("GET /rest/getPlaylist.view?") {
                assert!(request.contains("id=p%3A%2F1"));
                r#"{"status":"ok","playlist":{"name":"Road Mix","entry":[{"id":"song","title":"One"},{"id":"song","title":"One"}]}}"#
            } else {
                let endpoint = request.split('?').next().unwrap();
                panic!("unexpected endpoint {endpoint}");
            };
            response(
                "200 OK",
                "",
                format!(r#"{{"subsonic-response":{body}}}"#).as_bytes(),
            )
        });
        app.set_client(
            Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap(),
        );
        app.ui_action(UiAction::Search("mIx".into()), ctx);
        let receive = |app: &mut App| {
            let event = app
                .backend
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            app.search_event(event);
        };
        receive(app);
        receive(app);
        let state = &app.presentation.navidrome_search;
        assert!(matches!(state.catalogue, Loadable::Failed(_)));
        assert_eq!(state.hits().collect::<Vec<_>>(), [Hit::Playlist(0)]);
        assert_eq!(state.playlists.get().unwrap()[0].name, "Road Mix");
        app.search_action(Action::Retry(SearchPart::Catalogue), ctx);
        assert!(app.presentation.navidrome_search.catalogue.is_loading());
        assert_eq!(
            app.presentation.navidrome_search.playlists.get().unwrap()[0].name,
            "Road Mix"
        );
        receive(app);
        assert!(
            app.presentation
                .navidrome_search
                .catalogue
                .get()
                .unwrap()
                .song
                .is_empty()
        );

        // Drive the actual search box and its keyboard handlers.
        app.ui_action(UiAction::FocusSearch, ctx);
        for key in [None, Some(egui::Key::ArrowDown), Some(egui::Key::Enter)] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100.0, 760.0),
                    )),
                    events: key
                        .into_iter()
                        .map(|key| egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        })
                        .collect(),
                    ..Default::default()
                },
                |ui| app.draw(ui),
            );
            output.textures_delta.clear();
        }
        assert_eq!(
            app.presentation.page(),
            &Page::Playlist(navidrome::encode("p:/1"))
        );
        receive(app);
        app.client = None; // Exercise the native transport without opening an audio device.
        app.search_action(Action::PlayEntry(1), ctx);
        assert_eq!(app.transport.index, Some(1));
        assert_eq!(app.transport.songs.len(), 2);
        assert_eq!(app.transport.song().unwrap().title, "One");
    });
}

#[test]
fn submitted_searches_round_trip_per_server_and_user_with_immediate_removal() {
    with_app(|app, ctx| {
        let client =
            |server, user| Client::new(server, user, "dummy".into(), &ProxyConfig::Off).unwrap();
        let first = client("http://127.0.0.1:1/music", "one");
        app.set_client(first.clone());
        for i in 0..12 {
            // The debounced search already completed before Enter.
            app.presentation.search.committed = format!("Term {i}");
            app.submit_query();
        }
        app.presentation.search.committed = "tERM 2".into();
        app.submit_query();
        let recent = &app.presentation.navidrome_search.recent;
        assert_eq!(recent.len(), 10);
        assert_eq!(&recent[..3], ["tERM 2", "Term 11", "Term 10"]);
        app.ui_action(UiAction::ForgetSearch("Term 10".into()), ctx);
        let expected = app.presentation.navidrome_search.recent.clone();
        assert_eq!(expected.len(), 9);
        let json = serde_json::to_string(&app.preferences).unwrap();
        app.preferences = serde_json::from_str(&json).unwrap();
        for other in [
            client(first.base_url(), "two"),
            client("http://127.0.0.1:1/other", "one"),
        ] {
            app.set_client(other);
            assert!(app.presentation.navidrome_search.recent.is_empty());
        }
        app.set_client(first);
        assert_eq!(app.presentation.navidrome_search.recent, expected);
    });
}

#[test]
fn artist_hit_opens_its_albums_when_playlist_lookup_fails() {
    with_app(|app, ctx| {
        let server = Server::spawn(|request| {
            let body = if request.starts_with("GET /rest/search3.view?") {
                r#"{"status":"ok","searchResult3":{"song":[{"id":"s","title":"Dawn"}],"album":[{"id":"a","name":"Dawn"}],"artist":[{"id":"r/x","name":"Dawn"}]}}"#
            } else if request.starts_with("GET /rest/getPlaylists.view?") {
                r#"{"status":"failed","error":{"code":50}}"#
            } else if request.starts_with("GET /rest/getArtistInfo2.view?") {
                assert!(request.contains("includeNotPresent=false"));
                r#"{"status":"ok","artistInfo2":{"biography":"<p>From the library</p>","similarArtist":[{"id":"r/y","name":"Dawn"},{"id":"-1","name":"External"},{"name":"Unknown"},{"id":"r/x","name":"Self"},{"id":"r/y","name":"Duplicate"}]}}"#
            } else {
                assert!(
                    request.starts_with("GET /rest/getArtist.view?")
                        && request.contains("id=r%2Fx")
                );
                r#"{"status":"ok","artist":{"name":"Dawn","album":[{"id":"a","name":"First Light"}]}}"#
            };
            response(
                "200 OK",
                "",
                format!(r#"{{"subsonic-response":{body}}}"#).as_bytes(),
            )
        });
        app.set_client(
            Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap(),
        );
        app.ui_action(UiAction::Search("Dawn".into()), ctx);
        for _ in 0..2 {
            let event = app
                .backend
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            app.search_event(event);
        }
        let state = &app.presentation.navidrome_search;
        assert!(matches!(state.playlists, Loadable::Failed(_)));
        assert_eq!(
            state.hits().collect::<Vec<_>>(),
            [Hit::Song(0), Hit::Album(0), Hit::Artist(0)]
        );
        app.search_action(Action::Activate(Hit::Artist(0)), ctx);
        assert_eq!(
            app.presentation.page(),
            &Page::Artist(navidrome::encode("r/x"))
        );
        let event = app
            .backend
            .events
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        app.search_event(event);
        let Some(Collection::Artist { albums, .. }) =
            app.presentation.navidrome_search.collection.get()
        else {
            panic!("expected artist albums")
        };
        assert_eq!(albums[0].name, "First Light");
        assert!(app.presentation.navidrome_search.artist_cover.is_none());
        // Hold the optional response until the listener leaves the artist page.
        let mut delayed = None;
        for _ in 0..2 {
            let event = app
                .backend
                .events
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            match event {
                event @ Event::Appearances { .. } => app.search_event(event),
                event @ Event::ArtistInfo { .. } => delayed = Some(event),
                _ => panic!("unexpected artist request"),
            }
        }
        let delayed = delayed.unwrap();
        let Event::ArtistInfo {
            result: Ok(info), ..
        } = &delayed
        else {
            panic!("expected metadata")
        };
        assert_eq!(info.biography.as_deref(), Some("From the library"));
        assert_eq!(info.similar_artist.len(), 1);
        assert_eq!(info.similar_artist[0].id.as_deref(), Some("r/y"));
        assert!(
            app.presentation
                .navidrome_search
                .appearances
                .get()
                .unwrap()
                .is_empty()
        );
        let generation = app.transport.generation;
        app.ui_action(UiAction::Back, ctx);
        assert_eq!(app.presentation.page(), &Page::Search);
        app.search_event(delayed);
        assert!(
            app.presentation
                .navidrome_search
                .artist_info
                .get()
                .is_none()
        );
        assert_eq!(app.transport.generation, generation);
        assert_eq!(app.presentation.search.query, "Dawn");
    });
}

fn frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.draw(ui),
    );
    output.textures_delta.clear();
    output
}

/// Click the actual painted credit. The same text can occur in a row and in
/// the player bar, so pick the occurrence by its vertical position.
fn click_credit(
    app: &mut App,
    ctx: &egui::Context,
    name: &str,
    in_player: bool,
    occurrence: usize,
) {
    frame(app, ctx, vec![]);
    let output = frame(app, ctx, vec![]);
    let positions: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| {
            let egui::epaint::Shape::Text(text) = &shape.shape else {
                return None;
            };
            let pos = text.pos + text.galley.size() * 0.5;
            (text.galley.job.text == name
                && (pos.y > 710.0) == in_player
                && shape.clip_rect.contains(pos))
            .then_some(pos)
        })
        .collect();
    let pos = positions[occurrence];
    for pressed in [true, false] {
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn same_named_credits_open_by_id_from_search_album_and_player_without_starting_a_row() {
    with_app(|app, ctx| {
        let song: Song = serde_json::from_value(serde_json::json!({
            "id":"s", "title":"Duet", "artist":"Echo", "artistId":"older-primary",
            "artists":[{"id":"r:/α","name":"Echo"},{"id":"r:/β","name":"Echo"},{"name":"Guest"}]
        }))
        .unwrap();
        let album = app.album.as_mut().unwrap();
        album.song = vec![song.clone()];
        album.artist = "Ensemble".into();
        album.credits.artist_id = Some("ensemble".into());
        let album_page = Page::Album(navidrome::encode(&album.id));
        app.albums[0] = album.clone();
        app.play_search_songs(vec![song.clone()], 0);
        app.sync_presentation();
        let now = app.presentation.now_playing().unwrap();
        assert_eq!(now.artists[2].name, "Guest");
        assert!(now.artists[2].id.is_none());
        let generation = app.transport.generation;

        app.ui_action(UiAction::Search("Duet".into()), ctx);
        app.presentation.navidrome_search.catalogue = Loadable::Loaded(Catalogue {
            song: vec![song],
            ..Default::default()
        });
        click_credit(app, ctx, "Echo", false, 1);
        assert_eq!(
            app.presentation.page(),
            &Page::Artist(navidrome::encode("r:/β"))
        );
        app.ui_action(UiAction::Back, ctx);
        assert_eq!(app.presentation.page(), &Page::Search);

        app.ui_action(UiAction::Open(album_page.clone()), ctx);
        click_credit(app, ctx, "Echo", false, 0);
        assert_eq!(
            app.presentation.page(),
            &Page::Artist(navidrome::encode("r:/α"))
        );
        app.ui_action(UiAction::Back, ctx);
        assert_eq!(app.presentation.page(), &album_page);
        click_credit(app, ctx, "Echo", true, 1);
        assert_eq!(
            app.presentation.page(),
            &Page::Artist(navidrome::encode("r:/β"))
        );
        assert_eq!(app.transport.generation, generation);
    });
}

#[test]
fn back_restores_the_visible_playlist_row_after_its_contents_reload() {
    with_app(|app, ctx| {
        let playlist = Page::Playlist(navidrome::encode("p"));
        let contents = || Collection::Playlist {
            name: "Long playlist".into(),
            songs: (0..90)
                .map(|i| Song {
                    id: i.to_string(),
                    title: format!("Track {i}"),
                    ..Default::default()
                })
                .collect(),
        };
        app.ui_action(UiAction::Open(playlist.clone()), ctx);
        app.presentation.navidrome_search.collection = Loadable::Loaded(contents());
        frame(app, ctx, vec![]);
        frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(egui::pos2(700., 400.)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: egui::vec2(0., -1250.),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let visible = |output: egui::FullOutput| {
            output
                .shapes
                .into_iter()
                .find_map(|shape| {
                    let egui::epaint::Shape::Text(text) = shape.shape else {
                        return None;
                    };
                    (text.galley.job.text.starts_with("Track ")
                        && shape.clip_rect.contains(text.pos))
                    .then(|| (text.galley.job.text.clone(), text.pos.y))
                })
                .unwrap()
        };
        // Allow egui's animated wheel movement to settle before leaving.
        for _ in 0..60 {
            frame(app, ctx, vec![]);
        }
        let original = visible(frame(app, ctx, vec![]));
        assert_ne!(original.0, "Track 0");
        app.ui_action(UiAction::Open(Page::Artist(navidrome::encode("r"))), ctx);
        frame(app, ctx, vec![]);
        app.ui_action(UiAction::Back, ctx);
        app.presentation.navidrome_search.collection = Loadable::Loading;
        frame(app, ctx, vec![]);
        app.presentation.navidrome_search.collection = Loadable::Loaded(contents());
        assert_eq!(original, visible(frame(app, ctx, vec![])));
    });
}

#[test]
fn artist_sleeves_filter_by_server_tags_and_open_the_album_without_starting_playback() {
    with_app(|app, ctx| {
        app.presentation.settings.sidebar_visible = false;
        let releases = vec![
            Album {
                id: "ep:/β".into(),
                name: "Two Ends".into(),
                song: app.albums[0].song.clone(),
                song_count: 3,
                release_types: vec!["EP".into()],
                year: 2022,
                ..Default::default()
            },
            Album {
                release_types: vec!["album".into()],
                year: 2019,
                ..app.albums[0].clone()
            },
            Album {
                id: "untagged".into(),
                name: "Archive Tape".into(),
                ..Default::default()
            },
        ];
        app.albums.push(releases[0].clone());
        app.playing_album = Some(releases[1].clone());
        app.ui_action(UiAction::Open(Page::Artist(navidrome::encode("r"))), ctx);
        app.presentation.navidrome_search.collection = Loadable::Loaded(Collection::Artist {
            name: "Library artist".into(),
            cover_art: Some("portrait:/α".into()),
            albums: releases,
        });
        app.load_artist_details();
        let state = &app.presentation.navidrome_search;
        assert_ne!(state.artist_cover, state.artwork_uri(Some("portrait:/α")));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 1800.0),
                )),
                ..Default::default()
            },
            |ui| app.draw(ui),
        );
        output.textures_delta.clear();
        let headings: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if [
                        "Discography",
                        "About",
                        "Similar artists",
                        "Appears on",
                        "Songs",
                    ]
                    .contains(&t.galley.job.text.as_str()) =>
                {
                    Some(t.galley.job.text.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            headings,
            ["Discography", "About", "Similar artists", "Appears on"]
        );
        let generation = app.transport.generation;
        click_credit(app, ctx, "Singles & EPs", false, 0);
        let output = frame(app, ctx, vec![]);
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text == "Archive Tape")));
        click_credit(app, ctx, "Two Ends", false, 0);
        assert_eq!(
            app.presentation.page(),
            &Page::Album(navidrome::encode("ep:/β"))
        );
        assert_eq!(app.transport.generation, generation);
        // Let the sleeve click leave egui's triple-click window before the song double-click.
        let later = ctx.input(|input| input.time + 1.0);
        ctx.run_ui(
            egui::RawInput {
                time: Some(later),
                ..Default::default()
            },
            |ui| app.draw(ui),
        )
        .textures_delta
        .clear();
        click_credit(app, ctx, "Second", false, 0);
        click_credit(app, ctx, "Second", false, 0);
        assert_eq!(app.transport.song().unwrap().id, "song:/β");
        assert_eq!(app.playing_album.as_ref().unwrap().id, "ep:/β");
    });
}

#[test]
fn guest_releases_open_by_id_and_collaborative_albums_remain_in_discography() {
    with_app(|app, ctx| {
        let page = Page::Artist(navidrome::encode("lead"));
        app.presentation.settings.sidebar_visible = false;
        app.ui_action(UiAction::Open(page.clone()), ctx);
        let albums = serde_json::from_value(serde_json::json!([
            {"id":"joint","name":"Joint release","artistId":"other","artists":[{"id":"other"},{"id":"lead"}]},
            {"id":"legacy","name":"Earlier release"},
            {"id":"comp:β","name":"Featured release","artistId":"another"}
        ])).unwrap();
        app.presentation.navidrome_search.collection = Loadable::Loaded(Collection::Artist {
            name: "Lead artist".into(),
            cover_art: None,
            albums,
        });
        app.load_artist_details();
        let state = &app.presentation.navidrome_search;
        let Some(Collection::Artist { albums, .. }) = state.collection.get() else {
            panic!()
        };
        assert_eq!(
            albums.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            ["joint", "legacy"]
        );
        assert_eq!(state.appearances.get().unwrap()[0].id, "comp:β");
        frame(app, ctx, vec![]);
        ctx.data_mut(|d| {
            d.insert_temp(
                egui::Id::new(("navidrome-history", &page)).with("restore"),
                1500.0f32,
            )
        });
        let generation = app.transport.generation;
        click_credit(app, ctx, "Featured release", false, 0);
        assert_eq!(
            app.presentation.page(),
            &Page::Album(navidrome::encode("comp:β"))
        );
        assert_eq!(app.transport.generation, generation);
    });
}
