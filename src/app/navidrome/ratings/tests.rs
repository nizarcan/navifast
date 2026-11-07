use super::*;
use crate::app::navidrome::tests::with_app;
use crate::navidrome::api::{Album, FavoriteKind, Song};

fn state(kind: Kind) -> (State, String) {
    let mut state = State::default();
    let uri = kind.uri("opaque:/α & 夜");
    state.entries.insert(
        uri.clone(),
        Entry {
            kind,
            id: "opaque:/α & 夜".into(),
            name: "Test item".into(),
            confirmed: 2,
            shown: 2,
            desired: None,
            flight: None,
            attempts: 0,
        },
    );
    (state, uri)
}
fn token(op: Option<Operation>) -> u64 {
    match op.unwrap().flight {
        Flight::Write { token, .. } | Flight::Read(token) => token,
    }
}

#[test]
fn rapid_edits_serialize_and_old_reads_cannot_undo_the_latest_value() {
    for kind in [Kind::Song, Kind::Album] {
        let (mut state, uri) = state(kind);
        let old_read = token(state.begin(&uri, None));
        let first = token(state.set(&uri, 5));
        state.received(&uri, old_read, Ok(2));
        assert_eq!(state.value(&uri), Some(5));
        assert!(state.set(&uri, 1).is_none());
        assert!(state.set(&uri, 0).is_none());
        let second = state.written(&uri, first, Ok(())).unwrap();
        assert!(matches!(second.flight, Flight::Write { value: 0, .. }));
        let second = token(Some(second));
        assert!(state.written(&uri, first, Ok(())).is_none());
        let read = token(state.written(&uri, second, Ok(())));
        state.received(&uri, read, Ok(5));
        assert_eq!(state.value(&uri), Some(0));
        assert!(!state.due.is_empty());
        let read = token(state.begin(&uri, None));
        state.received(&uri, read, Ok(0));
        assert!(state.entries[&uri].desired.is_none());
        assert!(state.due.is_empty());
        let read = token(state.begin(&uri, None));
        state.received(&uri, read, Ok(4));
        assert_eq!(
            state.value(&uri),
            Some(4),
            "later edits by another client are visible"
        );
    }
}

#[test]
fn rejection_reconciles_and_retry_keeps_the_failed_intention() {
    for kind in [Kind::Song, Kind::Album] {
        let (mut state, uri) = state(kind);
        let write = token(state.set(&uri, 5));
        let read = token(state.written(
            &uri,
            write,
            Err(RequestError::Rejected("Permission denied".into())),
        ));
        assert_eq!(state.value(&uri), Some(2));
        assert_eq!(state.failures[&uri].value, 5);
        state.received(&uri, read, Ok(3));
        assert_eq!(state.value(&uri), Some(3));
        let write = token(state.set(&uri, state.failures[&uri].value));
        let read = token(state.written(&uri, write, Ok(())));
        state.received(&uri, read, Ok(5));
        assert_eq!(state.value(&uri), Some(5));
        assert!(state.failures.is_empty());
    }
}

#[test]
fn uncertain_writes_hold_the_value_and_stop_automatic_work_after_bounded_reads() {
    let (mut state, uri) = state(Kind::Song);
    let write = token(state.set(&uri, 4));
    let mut read = token(state.written(
        &uri,
        write,
        Err(RequestError::Unconfirmed("Timeout".into())),
    ));
    for attempt in 0..4 {
        state.received(&uri, read, Ok(2));
        assert_eq!(state.value(&uri), Some(4));
        if attempt < 3 {
            read = token(state.begin(&uri, None));
        }
    }
    assert!(state.due.is_empty());
    assert!(!state.failures.is_empty());
    let read = token(state.begin(&uri, None));
    state.received(&uri, read, Ok(4));
    assert!(state.failures.is_empty());
}

#[test]
fn ratings_are_shared_but_independent_of_favorites_other_kinds_and_accounts() {
    with_app(|app, _| {
        let album = Album {
            id: "shared".into(),
            name: "Album".into(),
            user_rating: Some(3),
            song: vec![Song {
                id: "shared".into(),
                title: "Song".into(),
                user_rating: None,
                ..Default::default()
            }],
            ..Default::default()
        };
        app.album = Some(album);
        let song = Kind::Song.uri("shared");
        let album = Kind::Album.uri("shared");
        app.prepare_rating(&song);
        app.prepare_rating(&album);
        assert_eq!(app.ratings.value(&song), Some(0));
        assert_eq!(app.ratings.value(&album), Some(3));
        app.rating_action(Action::Set {
            uri: song.clone(),
            value: 5,
        });
        assert_eq!(app.ratings.value(&album), Some(3));
        assert!(app.presentation.navidrome_favorites.list.song.is_empty());
        assert_eq!(FavoriteKind::Song.uri("shared"), song);
        app.demo = false; // Leave the next write pending.
        let request = token(app.ratings.set(&song, 1));
        let session = app.session;
        app.apply(super::super::Action::SignOut);
        app.rating_event(Event::RatingWritten {
            session,
            request,
            kind: Kind::Song,
            id: "shared".into(),
            result: Ok(()),
        });
        app.rating_event(Event::RatingRead {
            session,
            request,
            kind: Kind::Song,
            id: "shared".into(),
            result: Ok(1),
        });
        assert!(app.ratings.entries.is_empty());
    });
}
