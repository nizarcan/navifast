use super::*;
use crate::app::navidrome::{Action, tests::with_app};
use crate::model::Action as UiAction;

fn song() -> Song {
    Song {
        id: "shared:/α".into(),
        title: "The song".into(),
        ..Default::default()
    }
}

fn snapshot(starred: bool) -> Catalogue {
    Catalogue {
        song: starred.then(song).into_iter().collect(),
        // IDs are opaque and can overlap between entity types.
        artist: vec![NamedItem {
            id: song().id,
            name: "The artist".into(),
        }],
        ..Default::default()
    }
}

#[test]
fn rapid_double_toggle_serializes_writes_and_rejects_lagging_snapshots() {
    let mut state = State::default();
    state.received(0, Ok(snapshot(false)));
    let item = Item::Song(song());
    let uri = item.uri();
    assert!(state.set(item.clone(), true));
    assert!(!state.set(item, false));
    assert!(state.list.song.is_empty());
    state.received(0, Ok(snapshot(true)));
    assert!(state.list.song.is_empty());
    assert!(!state.written(&uri, true, Ok(())).unwrap().1);
    assert!(state.written(&uri, false, Ok(())).is_none());
    // This read started before the acknowledgement. Even agreement cannot retire the edit.
    state.received(0, Ok(snapshot(false)));
    assert!(state.edits.contains_key(&uri));
    state.request += 1;
    state.received(1, Ok(snapshot(true)));
    assert!(state.list.song.is_empty());
    assert!(state.verify_at.is_some());
    state.request += 1;
    state.received(2, Ok(snapshot(false)));
    assert!(state.edits.is_empty());
    assert_eq!(state.list.artist.len(), 1);
    state.request += 1;
    state.received(3, Ok(snapshot(true)));
    assert_eq!(
        state.list.song.len(),
        1,
        "later changes from another client are visible"
    );
}

#[test]
fn rejection_rolls_back_only_one_entity_and_retry_uses_the_failed_intention() {
    with_app(|app, ctx| {
        let s = song();
        let uri = FavoriteKind::Song.uri(&s.id);
        app.presentation
            .navidrome_favorites
            .received(0, Ok(snapshot(false)));
        assert!(
            app.presentation
                .navidrome_favorites
                .set(Item::Song(s.clone()), true)
        );
        app.favorite_event(Event::FavoriteWritten {
            session: app.session,
            kind: FavoriteKind::Song,
            id: s.id.clone(),
            starred: true,
            result: Err(RequestError::Rejected("Permission denied.".into())),
        });
        let state = &app.presentation.navidrome_favorites;
        assert_eq!(state.contains(&uri), Some(false));
        assert_eq!(state.list.artist.len(), 1);
        assert!(state.failures.contains_key(&uri));
        app.ui_action(UiAction::RetryFavorite(uri.clone()), ctx);
        let state = &app.presentation.navidrome_favorites;
        assert!(state.failures.is_empty());
        assert_eq!(state.list.song, [s]);
        assert_eq!(state.list.artist.len(), 1);
        app.ui_action(UiAction::Open(Page::LikedSongs), ctx);
        app.ui_action(UiAction::PlayFavorite(song().id), ctx);
        assert_eq!(app.transport.song(), Some(&song()));
    });
}

#[test]
fn a_rejected_unstar_of_a_deleted_song_is_resolved_by_the_next_fresh_list() {
    let mut state = State::default();
    state.received(0, Ok(snapshot(true)));
    let item = Item::Song(song());
    let uri = item.uri();
    assert!(state.set(item, false));
    state.written(
        &uri,
        false,
        Err(RequestError::Rejected("Item no longer available.".into())),
    );
    state.received(0, Ok(snapshot(false)));
    assert_eq!(state.contains(&uri), Some(true));
    state.request += 1;
    state.received(1, Ok(snapshot(false)));
    assert_eq!(state.contains(&uri), Some(false));
    assert!(state.edits.is_empty() && state.failures.is_empty());
    assert_eq!(state.list.artist.len(), 1);
}

#[test]
fn an_unanswered_write_is_resolved_by_a_new_read_and_can_be_retried() {
    for applied in [false, true] {
        let mut state = State::default();
        state.received(0, Ok(snapshot(false)));
        let item = Item::Song(song());
        let uri = item.uri();
        assert!(state.set(item, true));
        state.written(
            &uri,
            true,
            Err(RequestError::Unconfirmed("Timed out.".into())),
        );
        state.received(0, Ok(snapshot(false)));
        assert_eq!(state.contains(&uri), Some(true));
        state.request += 1;
        state.received(1, Ok(snapshot(applied)));
        assert_eq!(state.contains(&uri), Some(applied));
        assert_eq!(state.failures.is_empty(), applied);
        assert!(state.edits.is_empty());
    }
}

#[test]
fn signing_out_during_refresh_discards_old_account_reads_writes_and_retries() {
    with_app(|app, _| {
        let old_session = app.session;
        let old = &mut app.presentation.navidrome_favorites;
        old.received(0, Ok(snapshot(true)));
        old.set(Item::Song(song()), false);
        old.loading = true;
        old.request = 1;
        app.apply(Action::SignOut);
        app.apply(Action::Login);
        // Same request number and same opaque ID in the new account.
        let new = &mut app.presentation.navidrome_favorites;
        new.request = 1;
        new.loading = true;
        app.favorite_event(Event::Favorites {
            session: old_session,
            request: 1,
            result: Ok(snapshot(true)),
        });
        app.favorite_event(Event::FavoriteWritten {
            session: old_session,
            kind: FavoriteKind::Song,
            id: song().id,
            starred: false,
            result: Err(RequestError::Rejected("Old account".into())),
        });
        let new = &app.presentation.navidrome_favorites;
        assert!(new.loading && !new.ready);
        assert!(new.failures.is_empty() && new.list.song.is_empty());
        app.favorite_event(Event::Favorites {
            session: app.session,
            request: 1,
            result: Ok(snapshot(false)),
        });
        assert_eq!(
            app.presentation
                .navidrome_favorites
                .contains(&FavoriteKind::Song.uri(&song().id)),
            Some(false)
        );
    });
}

#[test]
fn one_click_updates_both_album_occurrences_the_player_and_favorites() {
    with_app(|app, ctx| {
        ctx.enable_accesskit();
        app.presentation
            .navidrome_favorites
            .received(0, Ok(Catalogue::default()));
        let album = app.album.as_ref().unwrap();
        let id = album.song[0].id.clone();
        assert_eq!(id, album.song[2].id);
        app.presentation.history = vec![Page::Album(encode(&album.id))];
        app.apply(Action::Play(0));
        let generation = app.transport.generation;
        let frame = |app: &mut App, events| {
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
            output.platform_output.accesskit_update.unwrap()
        };
        frame(app, vec![]);
        let controls = frame(app, vec![]);
        let bounds = controls
            .nodes
            .iter()
            .filter(|(_, n)| n.label() == Some("Add to Favorites"))
            .filter_map(|(_, n)| n.bounds())
            .max_by(|a, b| a.y1.total_cmp(&b.y1))
            .unwrap();
        let pos = egui::pos2(
            (bounds.x0 + bounds.x1) as f32 * 0.5,
            (bounds.y0 + bounds.y1) as f32 * 0.5,
        );
        for pressed in [true, false] {
            frame(
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        let controls = frame(app, vec![]);
        assert_eq!(
            controls
                .nodes
                .iter()
                .filter(|(_, n)| n.label() == Some("Remove from Favorites"))
                .count(),
            3
        );
        assert_eq!(app.presentation.navidrome_favorites.list.song[0].id, id);
        assert_eq!(app.transport.generation, generation);
    });
}
