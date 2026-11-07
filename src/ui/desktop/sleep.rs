use super::*;
use crate::app::navidrome::sleep::{Action as Sleep, Mode};

pub(super) fn button(v: &Paint<'_>) -> El {
    v.tinted_button(
        &v.tr(mark("Sleep timer")),
        Icon::Clock,
        18.,
        if v.s.app.sleep.mode.is_some() {
            v.p.accent
        } else {
            v.p.secondary
        },
        |s, tx| s.dispatch(Action::ShowDialog(crate::model::Dialog::SleepTimer), tx),
    )
    .key("sleep-open")
}

pub(super) fn status(v: &Paint<'_>) -> String {
    match v.s.app.sleep.mode {
        Some(Mode::Deadline(deadline)) => {
            let time = jiff::Timestamp::try_from(deadline)
                .expect("bounded sleep deadline")
                .to_zoned(jiff::tz::TimeZone::system());
            format!("{} · {}", v.tr(mark("Pause at")), time.strftime("%H:%M %Z"))
        }
        Some(Mode::Songs(count)) => format!("{} · {count}", v.tr(mark("Songs remaining"))),
        Some(Mode::Queue) => v.tr(mark("Pause at end of queue")),
        None if v.s.app.sleep.held => v.tr(mark("Sleep timer finished. Playback is paused.")),
        None => v.tr(mark("No sleep timer set")),
    }
}

pub(super) fn dialog(v: &Paint<'_>) -> El {
    let mut presets = row().gap(8.).flex_wrap();
    for (minutes, text) in [
        (15, mark("15 min")),
        (30, mark("30 min")),
        (60, mark("60 min")),
        (120, mark("120 min")),
    ] {
        presets = presets.child(
            v.button(&v.tr(text), false, move |s, tx| {
                s.dispatch(Action::Sleep(Sleep::Minutes(minutes)), tx);
            })
            .key(format!("sleep-minutes-{minutes}")),
        );
    }
    let mut contents = col().w_full().gap(14.)
        .child(label(status(v), 15., 600., v.p.text).wrap())
        .child(label(v.tr(mark("Pause after")), 13., 500., v.p.secondary))
        .child(presets)
        .child(label(v.tr(mark("Number of songs, including the current song")), 13., 500., v.p.secondary).wrap())
        .child(row().gap(10.).items_center().child(
            text_input(&v.s.sleep_songs, TextStyle::new(14., 400.))
                .on_edit(|s: &mut State| &mut s.sleep_songs)
                .on_submit(|s: &mut State, tx| s.set_sleep_songs(tx))
                .key("sleep-songs").label(v.tr(mark("Number of songs")))
                .single_line().w(96.).h(34.).px(10.).radius(5.)
                .fill(color(v.p.surface)).color(color(v.p.text))
                .caret(color(v.p.accent), color(v.p.accent.gamma_multiply(0.28)))
        ).child(v.button(&v.tr(mark("Apply")), false, |s, tx| s.set_sleep_songs(tx)).key("sleep-songs-apply")))
        .child(v.button(&v.tr(mark("Pause at end of queue")), v.s.app.sleep.mode == Some(Mode::Queue), |s, tx| {
            s.dispatch(Action::Sleep(Sleep::Queue), tx);
        }).key("sleep-queue"))
        .child(label(v.tr(mark("Next counts as one song. Seeking does not. The queue timer follows your edits and pauses before repeat.")), 13., 400., v.p.secondary).wrap());
    if v.s.sleep_invalid {
        contents = contents.child(
            label(
                v.tr(mark("Enter a whole number greater than zero.")),
                13.,
                400.,
                v.p.danger,
            )
            .wrap(),
        );
    }
    let mut actions = row().gap(8.);
    if v.s.app.sleep.mode.is_some() {
        actions = actions.child(
            v.button(&v.tr(mark("Cancel timer")), false, |s, tx| {
                s.dispatch(Action::Sleep(Sleep::Cancel), tx);
            })
            .key("sleep-cancel"),
        );
    }
    actions = actions.child(v.button(&v.tr(mark("Close")), false, |s, tx| {
        s.dispatch(Action::CloseDialog, tx)
    }));
    playlists::dialog(v, &v.tr(mark("Sleep timer")), contents, actions)
}
