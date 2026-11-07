use super::*;
use crate::app::navidrome::{Action as Transport, tests::with_app};

fn report() -> Report {
    Report {
        id: "song".into(),
        title: "The song".into(),
        occurrence: 1,
        time: 1234,
        submission: true,
    }
}

#[test]
fn disabled_and_old_account_reports_never_reach_the_backend() {
    with_app(|app, _| {
        let context = app.scrobbles.context(app.session, 1, true);
        app.listening_report(context, report());
        assert_eq!(app.scrobbles.request, 1);
        app.scrobble_action(Action::Enabled(false));
        app.listening_report(context, report());
        assert_eq!(app.scrobbles.request, 1);
        app.scrobble_action(Action::Enabled(true));
        app.listening_report(context, report());
        assert_eq!(
            app.scrobbles.request, 1,
            "late reports from before disabling stay cancelled"
        );
        let context = app.scrobbles.context(app.session, 1, true);
        app.apply(Transport::SignOut);
        app.listening_report(context, report());
        app.scrobble_event(Event::Scrobbled {
            session: context.session,
            epoch: context.epoch,
            request: 1,
            report: report(),
            result: Err(RequestError::Rejected("Old account".into())),
        });
        assert_eq!(app.scrobbles.request, 1);
        assert!(app.scrobbles.failures.is_empty());
    });
}

#[test]
fn only_definitely_rejected_completed_reports_offer_retry() {
    with_app(|app, _| {
        for (request, submission, error, retryable) in [
            (1, true, RequestError::Rejected("Rejected".into()), true),
            (
                2,
                true,
                RequestError::Unconfirmed("Timed out".into()),
                false,
            ),
            (3, false, RequestError::Rejected("Rejected".into()), false),
        ] {
            let mut report = report();
            report.submission = submission;
            app.scrobble_event(Event::Scrobbled {
                session: app.session,
                epoch: app.scrobbles.epoch,
                request,
                report,
                result: Err(error),
            });
            assert_eq!(app.scrobbles.failures[&request].retryable, retryable);
        }
        app.scrobble_action(Action::Retry(2));
        assert_eq!(app.scrobbles.request, 0);
        app.scrobble_action(Action::Retry(1));
        assert_eq!(app.scrobbles.request, 1);
        assert!(!app.scrobbles.failures.contains_key(&1));
        app.scrobble_action(Action::Dismiss(2));
        assert!(!app.scrobbles.failures.contains_key(&2));
    });
}

#[test]
fn transport_seeks_preserve_identity_but_repeat_and_play_after_end_renew_it() {
    with_app(|app, _| {
        app.apply(Transport::Play(0));
        let first = app.scrobbles.play.unwrap().1;
        app.apply(Transport::Seek(1.));
        app.apply(Transport::Pause(true));
        app.apply(Transport::Pause(false));
        assert_eq!(app.scrobbles.play.unwrap().1, first);
        app.transport.repeat = crate::player::RepeatMode::Track;
        app.finish_track();
        let second = app.scrobbles.play.unwrap().1;
        assert_ne!(first, second);
        app.transport.position = app.transport.song().unwrap().duration;
        app.apply(Transport::Toggle);
        assert_ne!(app.scrobbles.play.unwrap().1, second);
    });
}
