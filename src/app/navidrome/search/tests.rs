use super::*;
use crate::app::navidrome::{self, tests::with_app};
use crate::model::Action as UiAction;
use crate::navidrome::{
    api::{Client, Song},
    test_server::{Server, response},
};
use crate::settings::ProxyConfig;

#[test]
fn playlist_card_plays_after_loading_and_navigation_cancels_its_intent() {
    with_app(|app, host| {
        app.demo = false;
        let session = app.session;
        let result = |request| Event::Collection {
            session,
            request,
            result: Ok(Collection::Playlist {
                name: "Mix".into(),
                songs: vec![Song {
                    id: "first".into(),
                    ..Default::default()
                }],
                cover_art: None,
                owner: None,
                writable: false,
            }),
        };
        app.ui_action(
            UiAction::NavidromeSearch(Action::PlayPlaylist("first-list".into())),
            host,
        );
        let first = app.presentation.navidrome_search.collection_request;
        app.ui_action(UiAction::Open(Page::Albums), host);
        let generation = app.transport.generation;
        app.search_event(result(first));
        assert_eq!(app.transport.generation, generation);
        app.ui_action(
            UiAction::NavidromeSearch(Action::PlayPlaylist("second-list".into())),
            host,
        );
        app.search_event(result(app.presentation.navidrome_search.collection_request));
        assert_eq!(app.playing_playlist, Some(navidrome::encode("second-list")));
        assert_eq!(app.transport.song().unwrap().id, "first");
    });
}

#[test]
fn playlist_response_after_navigation_or_signout_cannot_replace_the_current_collection() {
    with_app(|app, host| {
        let page = |id| Page::Playlist(navidrome::encode(id));
        app.ui_action(UiAction::Open(page("first")), host);
        let first = app.presentation.navidrome_search.collection_request;
        let session = app.session;
        let event = |request, name: &str| Event::Collection {
            session,
            request,
            result: Ok(Collection::Playlist {
                name: name.into(),
                songs: Vec::new(),
                cover_art: None,
                owner: None,
                writable: false,
            }),
        };
        app.ui_action(UiAction::Open(page("second")), host);
        let second = app.presentation.navidrome_search.collection_request;
        app.search_event(event(second, "Current"));
        app.search_event(event(first, "Stale"));
        assert!(
            matches!(app.presentation.navidrome_search.collection.get(), Some(Collection::Playlist { name, .. }) if name == "Current")
        );
        app.ui_action(UiAction::Open(Page::Albums), host);
        app.search_event(event(second, "Left behind"));
        assert!(app.presentation.navidrome_search.collection.get().is_none());
        app.apply(navidrome::Action::SignOut);
        app.search_event(event(0, "Other account"));
        assert!(app.presentation.navidrome_search.collection.get().is_none());
    });
}

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

        app.ui_action(UiAction::FocusSearch, ctx);
        app.search_action(Action::Move(true), ctx);
        app.search_action(Action::Submit, ctx);
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
