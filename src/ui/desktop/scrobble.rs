use super::*;
use crate::app::navidrome::scrobble::Action as Reporting;

pub(super) fn errors(v: &Paint<'_>) -> El {
    let mut errors = col()
        .w_full()
        .max_h(120.)
        .px(crate::ui::widgets::PAGE_PADDING)
        .py(4.)
        .gap(6.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    for (&request, failure) in &v.s.app.scrobbles.failures {
        let message = v
            .tr(if failure.report.submission {
                mark("Could not report the listen for {name}. {error}")
            } else {
                mark("Could not report now playing for {name}. {error}")
            })
            .replace("{name}", &failure.report.title)
            .replace("{error}", &failure.message);
        let mut copy = col()
            .min_w(0.)
            .grow(1.)
            .gap(3.)
            .child(label(message, 13., 400., v.p.text).wrap());
        if !failure.retryable {
            copy = copy.child(label(v.tr(if failure.report.submission {
                mark("The server may have counted this listen. It will not be sent again automatically.")
            } else { mark("A new song will send a fresh update.") }),12.,400.,v.p.secondary).wrap());
        }
        let mut actions = row().gap(6.).items_center();
        if failure.retryable {
            actions = actions.child(
                v.button(&v.tr(mark("Retry")), false, move |s, tx| {
                    s.dispatch(Action::Scrobble(Reporting::Retry(request)), tx)
                })
                .key(format!("scrobble-retry-{request}")),
            );
        }
        actions = actions.child(
            v.icon_button(&v.tr(mark("Dismiss")), Icon::X, 16., move |s, tx| {
                s.dispatch(Action::Scrobble(Reporting::Dismiss(request)), tx)
            })
            .key(format!("scrobble-dismiss-{request}")),
        );
        errors = errors.child(
            row()
                .w_full()
                .gap(8.)
                .items_center()
                .child(copy)
                .child(actions),
        );
    }
    errors
}
