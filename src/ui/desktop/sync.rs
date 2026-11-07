//! Sync status stays with account settings, using the existing panel and buttons.
use super::*;
use crate::navidrome::sync::Status;

pub(super) fn error(v: &Paint<'_>) -> El {
    row()
        .w_full()
        .px(crate::ui::widgets::PAGE_PADDING)
        .py(4.)
        .gap(8.)
        .items_center()
        .child(
            label(
                v.tr(mark("Changes need attention. Open Sync in Settings.")),
                13.,
                400.,
                v.p.text,
            )
            .wrap()
            .min_w(0.)
            .grow(1.),
        )
        .child(v.button(&v.tr(mark("Settings")), false, |s, tx| {
            s.dispatch(Action::Open(Page::Settings), tx);
            s.filter = rune::TextEdit::new("Sync");
        }))
}

pub(super) fn rows(v: &Paint<'_>) -> Vec<El> {
    let sync = &v.s.app.sync;
    let mut rows = Vec::new();
    if sync.loading {
        rows.push(v.loading());
    }
    if let Some(error) = &sync.error {
        rows.push(label(error, 13., 400., v.p.secondary).wrap());
    }
    let pending = sync.pending();
    rows.push(label(
        v.tr(mark("{count} pending changes"))
            .replace("{count}", &pending.to_string()),
        15.,
        600.,
        v.p.text,
    ));
    let last = sync
        .journal
        .as_ref()
        .and_then(|j| j.last_sync)
        .map(|seconds| {
            jiff::Timestamp::from_second(seconds.min(i64::MAX as u64) as i64)
                .map(|t| t.strftime("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_default()
        })
        .unwrap_or_else(|| v.tr(mark("Not yet synced")));
    rows.push(
        label(
            format!("{}: {last}", v.tr(mark("Last sync"))),
            13.,
            400.,
            v.p.secondary,
        )
        .wrap(),
    );
    rows.push(label(v.tr(mark("Favorites, ratings and completed listens are saved on this device and synced to this account.")), 13., 400., v.p.secondary).wrap());
    if v.s.app.offline() {
        rows.push(
            label(
                v.tr(mark("Working offline. Reconnect to sync.")),
                13.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
        rows.push(v.button(&v.tr(mark("Reconnect")), false, |s, _| s.app.reconnect()));
    } else if pending > 0 || sync.error.is_some() {
        rows.push(v.button(&v.tr(mark("Retry sync")), false, |s, _| {
            s.app.retry_sync(None)
        }));
    }
    if !v.s.app.presentation.settings.scrobbling && sync.listens > 0 {
        rows.push(
            label(
                v.tr(mark(
                    "Listening reports are paused while scrobbling is off.",
                )),
                13.,
                400.,
                v.p.secondary,
            )
            .wrap(),
        );
    }
    if let Some(journal) = &sync.journal {
        // Settings renders only a bounded page. Dismissing or retrying reveals later failures.
        for key in &sync.failures {
            let Some(entry) = journal.pending.get(key) else {
                continue;
            };
            let (message, uncertain) = match &entry.status {
                Status::Failed(message) | Status::Waiting(message) => (message, false),
                Status::Uncertain(message) => (message, true),
                _ => unreachable!(),
            };
            let mut row = col()
                .w_full()
                .gap(6.)
                .child(label(entry.work.name(), 14., 600., v.p.text).wrap())
                .child(label(message, 13., 400., v.p.secondary).wrap());
            if uncertain {
                row = row.child(
                    label(
                        v.tr(mark(
                            "This listen may already be counted. It will not be sent again.",
                        )),
                        13.,
                        400.,
                        v.p.secondary,
                    )
                    .wrap(),
                );
            }
            let dismiss = key.clone();
            let mut controls = super::row().w_full().gap(8.).flex_wrap();
            if !uncertain && !v.s.app.offline() {
                let retry = key.clone();
                controls = controls.child(v.button(&v.tr(mark("Retry")), false, move |s, _| {
                    s.app.retry_sync(Some(retry.clone()))
                }));
            }
            controls = controls.child(v.button(&v.tr(mark("Dismiss")), false, move |s, _| {
                s.app.dismiss_sync(dismiss.clone())
            }));
            rows.push(row.child(controls));
        }
    }
    rows
}
