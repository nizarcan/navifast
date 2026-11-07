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
    let index = rt.state.app.transport.index;
    let paused = rt.state.app.transport.paused;
    assert!(rt.accessibility_action("album-play", rune::accesskit::Action::Click, None));
    rt.frame(0.);
    assert_eq!(rt.state.app.transport.generation, generation);
    assert_eq!(rt.state.app.transport.index, index);
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
