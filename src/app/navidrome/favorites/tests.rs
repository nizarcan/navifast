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
            cover_art: None,
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
