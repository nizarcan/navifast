//! Interaction coverage moved from the retired renderer onto the shipping Rune views.
use super::tests::runtime;
use super::*;
use crate::{
    model::Loadable,
    navidrome::api::{Album, Collection, Song},
};
use rune::Runtime;

fn click_label(rt: &mut Runtime<State>, name: &str, occurrence: usize) {
    rt.frame(0.);
    rt.settle_all();
    let element = rt
        .elements()
        .into_iter()
        .filter(|e| e.label.as_deref() == Some(name))
        .nth(occurrence)
        .unwrap_or_else(|| panic!("missing {name} occurrence {occurrence}"));
    let [x, y, w, h] = element.rect;
    rt.pointer_down(x + w / 2., y + h / 2.);
    rt.pointer_up(x + w / 2., y + h / 2.);
    rt.frame(0.);
    rt.settle_all();
}

#[test]
fn collection_shuffle_and_repeat_controls_keep_the_playing_context_and_position() {
    use crate::player::RepeatMode;
    for theme in ["", "light"] {
        let mut rt = runtime("playlist:list0", theme);
        let context = rt.state.app.playing_album.as_ref().unwrap().id.clone();
        let occurrence = rt.state.app.transport.queue.current().unwrap().id;
        let position = rt.state.app.transport.position;
        let generation = rt.state.app.transport.generation;
        assert!(rt.accessibility_action(
            "collection-shuffle",
            rune::accesskit::Action::Click,
            None
        ));
        rt.frame(0.);
        assert!(rt.state.app.transport.queue.shuffled());
        let order: Vec<_> = rt
            .state
            .app
            .transport
            .queue
            .upcoming()
            .iter()
            .map(|e| e.id)
            .collect();
        for mode in [RepeatMode::Context, RepeatMode::Track, RepeatMode::Off] {
            assert!(rt.accessibility_action("player-repeat", rune::accesskit::Action::Click, None));
            rt.frame(0.);
            assert_eq!(rt.state.app.transport.repeat, mode);
            assert_eq!(rt.state.app.playing_album.as_ref().unwrap().id, context);
            assert_eq!(
                rt.state.app.transport.queue.current().unwrap().id,
                occurrence
            );
            assert_eq!(rt.state.app.transport.position, position);
            assert_eq!(rt.state.app.transport.generation, generation);
            assert_eq!(
                rt.state
                    .app
                    .transport
                    .queue
                    .upcoming()
                    .iter()
                    .map(|e| e.id)
                    .collect::<Vec<_>>(),
                order
            );
        }
    }
}

#[test]
fn queue_reflows_only_while_open_and_preserves_browsing_on_close() {
    for theme in ["", "light"] {
        for width in [760., 1280.] {
            let mut rt = runtime("albums", theme);
            rt.set_viewport((width, 800.), 2.);
            rt.frame(0.);
            rt.settle_all();
            let album = |rt: &Runtime<State>| {
                rt.elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some("album-alb0"))
                    .map(|e| e.rect)
            };
            let before = album(&rt).unwrap();
            let generation = rt.state.app.transport.generation;
            assert!(rt.accessibility_action("queue-toggle", rune::accesskit::Action::Click, None));
            rt.frame(0.);
            rt.settle_all();
            let panel = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("queue-panel"))
                .unwrap()
                .rect;
            assert!(panel[0] >= 0. && panel[0] + panel[2] <= width + 1.);
            assert_eq!(album(&rt).is_none(), width < 1060.);
            assert_eq!(rt.state.app.transport.generation, generation);
            assert!(rt.accessibility_action("queue-close", rune::accesskit::Action::Click, None));
            rt.frame(0.);
            rt.settle_all();
            assert_eq!(album(&rt).unwrap(), before);
            assert_eq!(rt.state.app.transport.generation, generation);
        }
    }
}

#[test]
fn queue_selection_follows_duplicate_occurrences_through_menu_moves_and_play() {
    use crate::app::navidrome::queue::Action as Q;
    let mut rt = runtime("albums", "queue");
    rt.update(|s, _| {
        s.app.queue_action(Q::Clear);
        let song = s.app.transport.song().unwrap().clone();
        s.app.queue_action(Q::Songs {
            songs: vec![song.clone(), song],
            next: false,
        });
        s.app.apply(super::super::Action::Pause(true));
    });
    rt.frame(0.);
    rt.settle_all();
    let id = rt.state.app.transport.queue.upcoming()[1].id;
    assert!(rt.accessibility_action(
        &format!("queue-more-{id}"),
        rune::accesskit::Action::Click,
        None
    ));
    rt.frame(0.);
    assert!(rt.accessibility_action("queue-move--1", rune::accesskit::Action::Click, None));
    rt.frame(0.);
    assert_eq!(rt.state.queue_selected, Some(id));
    assert_eq!(rt.state.app.transport.queue.upcoming()[0].id, id);
    assert!(rt.state.app.transport.paused);
    rt.update(|s, _| s.app.apply(super::super::Action::Next));
    rt.frame(0.);
    assert_eq!(rt.state.app.transport.queue.current().unwrap().id, id);
    assert!(rt.state.app.transport.paused);
    let remaining = rt.state.app.transport.queue.upcoming()[0].id;
    assert!(rt.accessibility_action(
        &format!("queue-play-{remaining}"),
        rune::accesskit::Action::Click,
        None
    ));
    rt.frame(0.);
    assert_eq!(
        rt.state.app.transport.queue.current().unwrap().id,
        remaining
    );
    assert!(!rt.state.app.transport.paused);
}

#[test]
fn queue_virtualizes_long_lists_and_resolves_selected_pending_albums_in_place() {
    let mut rt = runtime("albums", "queue-loading");
    let token = rt.state.app.transport.queue.upcoming()[0].id;
    rt.update(|s, _| {
        s.queue_selected = Some(token);
        let song = s.app.transport.song().unwrap().clone();
        s.app
            .queue_action(crate::app::navidrome::queue::Action::Songs {
                songs: vec![song.clone(); 10_000],
                next: false,
            });
        s.app.queued_album(
            token,
            Ok(Album {
                song: vec![song],
                ..Default::default()
            }),
        );
    });
    rt.frame(0.);
    rt.settle_all();
    assert_eq!(rt.state.queue_selected, Some(token));
    assert_eq!(rt.state.app.transport.queue.upcoming()[0].id, token);
    assert!(
        rt.elements()
            .iter()
            .filter(|e| e
                .key
                .as_deref()
                .is_some_and(|key| key.starts_with("queue-play-")))
            .count()
            < 30
    );
    let [x, y, _, h] = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(&format!("queue-{token}")))
        .unwrap()
        .rect;
    rt.pointer_down(x + 80., y + h / 2.);
    rt.pointer_up(x + 80., y + h / 2.);
    rt.frame(0.);
    rt.key(rune::KeyEvent {
        key: rune::Key::Down,
        mods: rune::Mods::default(),
    });
    rt.frame(0.);
    assert_eq!(
        rt.state.queue_selected,
        Some(rt.state.app.transport.queue.upcoming()[1].id)
    );
}

#[test]
fn playlist_editor_uses_displayed_selection_order_and_marks_readonly_destinations() {
    for flags in ["", "light"] {
        for width in [760., 1280.] {
            let mut rt = runtime("playlist:list0", flags);
            rt.set_viewport((width, 800.), 2.);
            rt.update(|s, tx| {
                if let Some(Collection::Playlist { songs, .. }) =
                    s.app.presentation.navidrome_search.collection.get_mut()
                {
                    *songs = [("dup", "Beta"), ("last", "Alpha"), ("dup", "Beta")]
                        .into_iter()
                        .map(|(id, title)| Song {
                            id: id.into(),
                            title: title.into(),
                            ..Default::default()
                        })
                        .collect();
                }
                s.sync_tracks();
                s.sort_collection(crate::model::SortColumn::Title);
                s.table.selection.rows = [0, 1, 2].into();
                let key = s.table.key(1).to_owned();
                s.add_to_playlists(Some(&key), tx);
            });
            rt.frame(0.);
            rt.settle_all();
            assert_eq!(
                rt.state
                    .app
                    .presentation
                    .navidrome_playlists
                    .songs
                    .iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>(),
                ["last", "dup", "dup"]
            );
            assert!(rt.accessibility_json().to_string().contains("Read-only"));
            let destinations = rt
                .state
                .app
                .presentation
                .navidrome_playlists
                .destinations
                .get()
                .unwrap();
            assert!(!rt.state.app.playlist_destination_writable(&destinations[2]));
            assert!(rt.accessibility_action(
                "destination-list0",
                rune::accesskit::Action::Click,
                None
            ));
            rt.frame(0.);
            assert!(
                rt.state
                    .app
                    .presentation
                    .navidrome_playlists
                    .selected
                    .contains("list0")
            );
            assert!(rt.accessibility_action("playlist-add", rune::accesskit::Action::Click, None));
            rt.frame(0.);
            assert_eq!(rt.state.app.presentation.navidrome_playlists.jobs.len(), 1);
        }
    }
}

#[test]
fn playlist_browser_opens_same_named_items_by_id_and_play_starts_at_the_top() {
    for flags in ["", "light"] {
        for width in [760., 1280.] {
            let mut rt = runtime("playlists", flags);
            rt.set_viewport((width, 800.), 2.);
            let generation = rt.state.app.transport.generation;
            click_label(&mut rt, "Evening drive", 1);
            assert_eq!(
                rt.state.app.presentation.page(),
                &Page::Playlist(super::super::encode("list2"))
            );
            assert_eq!(rt.state.app.transport.generation, generation);
            assert!(rt.accessibility_action("playlist-play", rune::accesskit::Action::Click, None));
            rt.frame(0.);
            assert_eq!(rt.state.app.transport.queue.context_index(), Some(0));
            assert!(!rt.state.app.transport.paused);
        }
    }
}

#[test]
fn playlist_browser_reuses_album_card_geometry_and_virtualizes_large_grids() {
    for theme in ["", "light"] {
        for width in [760., 1280.] {
            let mut albums = runtime("albums", theme);
            let mut playlists = runtime("playlists", theme);
            playlists.update(|s, _| {
                let list = s
                    .app
                    .presentation
                    .navidrome_playlists
                    .list
                    .get_mut()
                    .unwrap();
                list.extend((3..1000).map(|i| crate::navidrome::api::NamedItem {
                    id: format!("list{i}"),
                    name: format!("Playlist {i}"),
                    cover_art: None,
                }));
            });
            for rt in [&mut albums, &mut playlists] {
                rt.set_viewport((width, 800.), 2.);
                rt.frame(0.);
                rt.settle_all();
            }
            let rect = |rt: &Runtime<State>, key: &str| {
                rt.elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some(key))
                    .unwrap()
                    .rect
            };
            assert_eq!(
                rect(&albums, "album-alb0"),
                rect(&playlists, "playlist-list0")
            );
            assert_eq!(
                rect(&albums, "album-alb1"),
                rect(&playlists, "playlist-list1")
            );
            assert_eq!(
                rect(&albums, "album-alb2"),
                rect(&playlists, "playlist-list2")
            );
            assert!(
                playlists
                    .elements()
                    .iter()
                    .filter(|e| e
                        .key
                        .as_deref()
                        .is_some_and(|key| key.starts_with("playlist-list")))
                    .count()
                    < 50
            );
            assert!(playlists.accessibility_action(
                "playlist-card-play-list1",
                rune::accesskit::Action::Click,
                None
            ));
            playlists.frame(0.);
            assert_eq!(
                playlists.state.app.playing_playlist,
                Some(super::super::encode("list1"))
            );
            assert_eq!(playlists.state.app.transport.queue.context_index(), Some(0));
        }
    }
}

#[test]
fn playlist_browser_distinguishes_loading_empty_and_failed_lists() {
    for (state, expected) in [
        ("loading", "Loading…"),
        ("empty", "No playlists"),
        ("error", "Retry"),
    ] {
        for theme in ["", ",light"] {
            let mut rt = runtime("playlists", &format!("{state}{theme}"));
            for width in [760., 1280.] {
                rt.set_viewport((width, 800.), 2.);
                rt.frame(0.);
                rt.settle_all();
                assert!(rt.accessibility_json().to_string().contains(expected));
            }
        }
    }
}

#[test]
fn playlist_uses_album_selection_sorting_and_pause_without_losing_duplicate_occurrences() {
    let mut rt = runtime("playlist:list0", "");
    rt.update(|s, _| {
        if let Some(Collection::Playlist { songs, .. }) =
            s.app.presentation.navidrome_search.collection.get_mut()
        {
            *songs = [
                ("dup", "Beta"),
                ("other", "Alpha"),
                ("dup", "Beta"),
                ("tail", "Zulu"),
            ]
            .into_iter()
            .map(|(id, title)| Song {
                id: id.into(),
                title: title.into(),
                duration: 60.,
                ..Default::default()
            })
            .collect();
        }
        s.sync_tracks();
        s.sort_collection(crate::model::SortColumn::Title);
    });
    rt.frame(0.);
    rt.settle_all();
    let generation = rt.state.app.transport.generation;
    let key = rt.state.table.key(2).to_owned();
    let element = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(&key))
        .unwrap();
    let [x, y, _, h] = element.rect;
    rt.pointer_down(x + 90., y + h / 3.);
    rt.pointer_up(x + 90., y + h / 3.);
    rt.frame(0.);
    assert_eq!(rt.state.table.cursor, Some(2));
    assert_eq!(
        rt.state.app.transport.generation, generation,
        "a single click only selects"
    );
    rt.pointer_down(x + 90., y + h / 3.);
    rt.pointer_up(x + 90., y + h / 3.);
    rt.frame(0.);
    assert_eq!(rt.state.app.transport.queue.context_index(), Some(2));
    assert_eq!(rt.state.playing_row().as_deref(), Some(key.as_str()));
    assert_eq!(
        rt.state
            .app
            .transport
            .queue
            .context()
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["other", "dup", "dup", "tail"]
    );
    let generation = rt.state.app.transport.generation;
    assert!(rt.accessibility_action("playlist-play", rune::accesskit::Action::Click, None));
    rt.frame(0.);
    assert!(rt.state.app.transport.paused);
    assert_eq!(rt.state.app.transport.generation, generation);
    rt.update(|s, tx| s.dispatch(Action::Next, tx));
    assert_eq!(rt.state.app.transport.song().unwrap().id, "tail");
    assert!(rt.state.app.transport.paused);
}

#[test]
fn album_and_playlist_share_header_table_and_play_control_geometry() {
    for theme in ["", "light"] {
        for width in [760., 1280.] {
            let mut album = runtime("album:alb0", theme);
            let mut playlist = runtime("playlist:list0", theme);
            album.update(|s, _| s.app.album.as_mut().unwrap().name = "Collection".into());
            playlist.update(|s, _| {
                if let Some(Collection::Playlist { name, .. }) =
                    s.app.presentation.navidrome_search.collection.get_mut()
                {
                    *name = "Collection".into();
                }
            });
            for rt in [&mut album, &mut playlist] {
                rt.set_viewport((width, 800.), 2.);
                rt.frame(0.);
                rt.settle_all();
            }
            let rect = |rt: &Runtime<State>, key: &str| {
                rt.elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some(key))
                    .unwrap()
                    .rect
            };
            for key in [
                "collection-art",
                "collection-title",
                "sort-Title",
                "sort-Duration",
            ] {
                assert_eq!(rect(&album, key), rect(&playlist, key), "{key} at {width}");
            }
            assert_eq!(rect(&album, "album-play"), rect(&playlist, "playlist-play"));
        }
    }
}

#[test]
fn login_fields_remain_vertical_at_both_window_sizes_and_themes() {
    for flags in ["signed-out", "signed-out,light"] {
        let mut rt = runtime("albums", flags);
        for width in [760., 1280.] {
            rt.set_viewport((width, 800.), 2.);
            rt.frame(0.);
            rt.settle_all();
            let elements = rt.elements();
            let fields: Vec<_> = ["server", "user", "password"]
                .into_iter()
                .map(|key| {
                    elements
                        .iter()
                        .find(|e| e.key.as_deref() == Some(key))
                        .unwrap()
                })
                .collect();
            for field in &fields {
                assert!(field.rect[0] >= 0. && field.rect[0] + field.rect[2] <= width);
            }
            assert!(fields[0].rect[1] + fields[0].rect[3] <= fields[1].rect[1]);
            assert!(fields[1].rect[1] + fields[1].rect[3] <= fields[2].rect[1]);
        }
    }
}

#[test]
fn credential_lookup_shows_the_library_without_a_login_or_empty_state_flash() {
    for flags in ["restoring", "restoring,light"] {
        let mut rt = runtime("albums", flags);
        for width in [760., 1280.] {
            rt.set_viewport((width, 800.), 2.);
            rt.frame(0.);
            rt.settle_all();
            let tree = rt.accessibility_json().to_string();
            assert!(tree.contains("Albums"));
            assert!(!tree.contains("Password"));
            assert!(!tree.contains("No albums"));
        }
    }
}

#[test]
fn album_play_button_pauses_without_restarting_the_selected_song() {
    let mut rt = runtime("album:alb0", "");
    let generation = rt.state.app.transport.generation;
    let index = rt.state.app.transport.queue.context_index();
    let paused = rt.state.app.transport.paused;
    assert!(rt.accessibility_action("album-play", rune::accesskit::Action::Click, None));
    rt.frame(0.);
    assert_eq!(rt.state.app.transport.generation, generation);
    assert_eq!(rt.state.app.transport.queue.context_index(), index);
    assert_ne!(rt.state.app.transport.paused, paused);
}

#[test]
fn same_named_artist_credits_navigate_by_id_without_starting_the_row() {
    let mut rt = runtime("album:alb0", "");
    rt.update(|s,_| {
        let song:Song=serde_json::from_value(serde_json::json!({"id":"duet","title":"Duet","artist":"Echo","artistId":"primary","artists":[{"id":"r:/α","name":"Echo"},{"id":"r:/β","name":"Echo"},{"name":"Guest"}]})).unwrap();
        let album=s.app.album.as_mut().unwrap();
        album.song=vec![song];
        s.sync_tracks();
    });
    let generation = rt.state.app.transport.generation;
    click_label(&mut rt, "Echo", 1);
    assert_eq!(
        rt.state.app.presentation.page(),
        &Page::Artist(super::super::encode("r:/β"))
    );
    assert_eq!(rt.state.app.transport.generation, generation);
    rt.update(|s, tx| s.dispatch(Action::Back, tx));
    click_label(&mut rt, "Echo", 0);
    assert_eq!(
        rt.state.app.presentation.page(),
        &Page::Artist(super::super::encode("r:/α"))
    );
    assert_eq!(rt.state.app.transport.generation, generation);
}

#[test]
fn playlist_scroll_and_selection_survive_back_navigation_and_reloading() {
    let mut rt = runtime("playlist:pl0", "");
    let page = rt.state.app.presentation.page().clone();
    let contents = || Collection::Playlist {
        name: "Long playlist".into(),
        cover_art: None,
        owner: None,
        writable: false,
        songs: (0..90)
            .map(|i| Song {
                id: i.to_string(),
                title: format!("Track {i}"),
                ..Default::default()
            })
            .collect(),
    };
    rt.update(|s, _| {
        s.app.demo = false;
        s.app.presentation.navidrome_search.collection = Loadable::Loaded(contents());
        s.sync_tracks();
    });
    rt.update(|s, tx| tx.scroll_into_view(s.table.key(70)));
    rt.frame(0.);
    rt.settle_all();
    let original = rt.state.offsets[&page].get();
    assert!(original > 1000.);
    rt.update(|s, tx| s.dispatch(Action::Open(Page::Settings), tx));
    rt.frame(0.);
    rt.update(|s, tx| {
        s.app.presentation.navidrome_search.collection = Loadable::Loading;
        s.dispatch(Action::Back, tx);
    });
    rt.frame(0.);
    rt.update(|s, tx| {
        s.app.presentation.navidrome_search.collection = Loadable::Loaded(contents());
        s.sync_tracks();
        if let Some(y) = s.restore_scroll.take() {
            tx.scroll_to_y("page", y);
        }
    });
    rt.frame(0.);
    rt.settle_all();
    assert!((rt.state.offsets[&page].get() - original).abs() < 1.);
}

#[test]
fn artist_filters_use_release_tags_and_sleeves_open_without_playing() {
    let mut rt = runtime("artist:art0", "details");
    rt.set_viewport((1280., 1600.), 2.);
    rt.update(|s, _| {
        let songs = s.app.albums[0].song.clone();
        let ep = Album {
            id: "ep:/β".into(),
            name: "Two Ends".into(),
            song: songs,
            song_count: 12,
            release_types: vec!["EP".into()],
            year: 2022,
            ..Default::default()
        };
        s.app.albums.push(ep.clone());
        s.app.presentation.navidrome_search.collection = Loadable::Loaded(Collection::Artist {
            name: "Library artist".into(),
            cover_art: None,
            albums: vec![
                ep,
                Album {
                    release_types: vec!["album".into()],
                    ..s.app.albums[0].clone()
                },
                Album {
                    id: "untagged".into(),
                    name: "Archive Tape".into(),
                    ..Default::default()
                },
            ],
        });
    });
    let generation = rt.state.app.transport.generation;
    click_label(&mut rt, "Singles & EPs", 0);
    assert!(
        !rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Archive Tape"))
    );
    click_label(&mut rt, "Two Ends", 0);
    assert_eq!(
        rt.state.app.presentation.page(),
        &Page::Album(super::super::encode("ep:/β"))
    );
    assert_eq!(rt.state.app.transport.generation, generation);
}
