use super::{App, player};
pub use crate::navidrome::sleep::{Mode, Timer};
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug)]
pub enum Action {
    Minutes(u32),
    Songs(u32),
    Queue,
    Cancel,
}

impl App {
    pub(super) fn sleep_action(&mut self, action: Action) {
        let mode = match action {
            Action::Minutes(minutes) if (1..=1440).contains(&minutes) => SystemTime::now()
                .checked_add(Duration::from_secs(u64::from(minutes) * 60))
                .map(Mode::Deadline),
            Action::Songs(count) if count > 0 => Some(Mode::Songs(count)),
            Action::Queue => Some(Mode::Queue),
            Action::Cancel => None,
            _ => return,
        };
        self.sleep.set(mode);
        self.sync_sleep();
    }

    fn sync_sleep(&self) {
        self.player.send(player::Command::Sleep {
            token: self.sleep.token,
            deadline: self.sleep.deadline(),
            held: self.sleep.held,
        });
    }

    pub(super) fn release_sleep_hold(&mut self) {
        if self.sleep.held {
            self.sleep.set(None);
            self.sync_sleep();
        }
    }

    pub(super) fn pause_for_sleep(&mut self) {
        self.sleep.expire();
        self.transport.paused = true;
        self.sync_sleep();
        self.player.send(player::Command::Pause(true));
        self.checkpoint_session(true);
    }

    pub(super) fn poll_sleep(&mut self, now: SystemTime) -> bool {
        if self
            .sleep
            .deadline()
            .is_some_and(|deadline| now >= deadline)
        {
            self.pause_for_sleep();
            true
        } else {
            false
        }
    }

    /// The caller has not consumed the queue head or submitted its decoder yet.
    pub(super) fn sleep_boundary(&mut self) -> bool {
        if self
            .sleep
            .boundary(SystemTime::now(), self.transport.queue.at_cycle_end())
        {
            self.pause_for_sleep();
            true
        } else {
            false
        }
    }

    #[cfg(feature = "navidrome-demo")]
    pub(super) fn configure_demo_sleep(&mut self, flags: &[&str]) {
        if flags.contains(&"sleep-active") {
            self.sleep.set(Some(Mode::Songs(2)));
        } else if flags.contains(&"sleep-time") {
            self.sleep.set(Some(Mode::Deadline(
                SystemTime::now() + Duration::from_secs(30 * 60),
            )));
        } else if flags.contains(&"sleep-songs") {
            self.sleep.set(Some(Mode::Songs(2)));
        } else if flags.contains(&"sleep-queue") {
            self.sleep.set(Some(Mode::Queue));
        } else if flags.contains(&"sleep-expired") {
            self.pause_for_sleep();
        }
        if flags.iter().any(|flag| flag.starts_with("sleep-")) && !flags.contains(&"sleep-active") {
            self.presentation.dialog = Some(crate::model::Dialog::SleepTimer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::{Action as Transport, tests::with_app};
    use crate::navidrome::api::{Album, Song};
    use crate::player::RepeatMode;

    fn finish(app: &mut App) {
        app.transport
            .event(player::Event::Finished(app.transport.generation));
        app.finish_track();
    }

    #[test]
    fn sleep_one_song_stops_before_next_or_repeat_without_consuming_the_head() {
        for repeat in [RepeatMode::Off, RepeatMode::Track, RepeatMode::Context] {
            with_app(|app, _| {
                app.apply(Transport::Play(0));
                app.apply(Transport::Repeat(repeat));
                app.sleep_action(Action::Songs(1));
                let generation = app.transport.generation;
                let next = app.transport.queue.upcoming()[0].id;
                finish(app);
                assert!(app.transport.paused);
                assert!(app.sleep.held);
                assert_eq!(app.transport.position, 120.);
                assert_eq!(
                    app.transport.generation, generation,
                    "no next decoder submitted"
                );
                assert_eq!(app.transport.queue.upcoming()[0].id, next);
                finish(app); // A late completion cannot undo the stop.
                assert_eq!(app.transport.generation, generation);
            });
        }
    }

    #[test]
    fn sleep_counts_occurrences_and_next_but_not_seeks_or_pauses() {
        with_app(|app, _| {
            app.apply(Transport::Play(0));
            app.sleep_action(Action::Songs(3));
            app.apply(Transport::Pause(true));
            app.apply(Transport::Seek(40.));
            app.apply(Transport::Seek(10.));
            assert_eq!(app.sleep.mode, Some(Mode::Songs(3)));
            app.apply(Transport::Next);
            assert_eq!(app.sleep.mode, Some(Mode::Songs(2)));
            assert!(app.transport.paused);
            app.apply(Transport::Pause(false));
            finish(app);
            assert_eq!(app.sleep.mode, Some(Mode::Songs(1)));
            assert_eq!(app.transport.queue.context_index(), Some(2)); // same song ID as first
            finish(app);
            assert!(app.sleep.held);
            assert!(app.transport.paused);
            app.apply(Transport::Toggle);
            assert!(!app.sleep.held);
            assert!(!app.transport.paused);
            assert_eq!(app.transport.position, 0.);
            app.pause_for_sleep();
            app.play_search_songs(
                vec![Song {
                    id: "chosen".into(),
                    duration: 60.,
                    ..Default::default()
                }],
                0,
            );
            assert!(!app.sleep.held);
            assert!(!app.transport.paused);
        });
        with_app(|app, _| {
            app.apply(Transport::Play(0));
            app.apply(Transport::Repeat(RepeatMode::Track));
            app.sleep_action(Action::Songs(2));
            finish(app);
            assert_eq!(app.sleep.mode, Some(Mode::Songs(1)));
            assert_eq!(app.transport.queue.context_index(), Some(0));
            finish(app);
            assert!(app.sleep.held);
        });
    }

    #[test]
    fn sleep_queue_follows_edits_and_stops_before_repeat_preview() {
        with_app(|app, _| {
            app.apply(Transport::Play(2));
            app.apply(Transport::Repeat(RepeatMode::Context));
            assert!(app.transport.queue.at_cycle_end());
            app.sleep_action(Action::Queue);
            app.queue_action(super::super::queue::Action::Songs {
                songs: vec![Song {
                    id: "manual".into(),
                    duration: 10.,
                    ..Default::default()
                }],
                next: false,
            });
            assert!(!app.transport.queue.at_cycle_end());
            finish(app);
            assert_eq!(app.transport.song().unwrap().id, "manual");
            let next = app.transport.queue.upcoming()[0].id;
            finish(app);
            assert!(app.sleep.held);
            assert_eq!(app.transport.queue.upcoming()[0].id, next);
        });
        with_app(|app, _| {
            app.apply(Transport::Play(0));
            app.apply(Transport::Repeat(RepeatMode::Track));
            app.sleep_action(Action::Queue);
            finish(app);
            assert_eq!(app.transport.queue.context_index(), Some(1));
            let last = app.transport.queue.upcoming()[0].id;
            app.queue_action(super::super::queue::Action::Remove(last));
            finish(app);
            assert!(app.sleep.held);
        });
    }

    #[test]
    fn sleep_pending_album_counts_once_and_failed_last_reservation_stops() {
        with_app(|app, _| {
            app.apply(Transport::Play(0));
            app.sleep_action(Action::Songs(2));
            let token = app
                .transport
                .queue
                .reserve("pending".into(), "Pending".into(), true);
            app.apply(Transport::Next);
            app.apply(Transport::Next);
            assert_eq!(app.sleep.mode, Some(Mode::Songs(1)));
            app.queued_album(
                token,
                Ok(Album {
                    song: vec![Song {
                        id: "next".into(),
                        duration: 60.,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            );
            assert_eq!(app.sleep.mode, Some(Mode::Songs(1)));
            finish(app);
            assert!(app.sleep.held);
        });
        with_app(|app, _| {
            app.apply(Transport::Play(2));
            app.apply(Transport::Repeat(RepeatMode::Context));
            app.sleep_action(Action::Queue);
            let token = app
                .transport
                .queue
                .reserve("pending".into(), "Pending".into(), true);
            finish(app);
            assert!(app.transport.waiting_for_queue);
            app.queued_album(token, Err("Unavailable".into()));
            assert!(app.sleep.held);
            assert!(app.transport.paused);
            assert!(!app.transport.waiting_for_queue);
            assert_eq!(app.transport.queue.context_index(), Some(2));
        });
    }

    #[test]
    fn sleep_wall_clock_expiry_preserves_position_and_replacement_cancel_signout_clear() {
        with_app(|app, host| {
            app.apply(Transport::Play(0));
            app.apply(Transport::Seek(37.));
            app.apply(Transport::Pause(true));
            app.sleep_action(Action::Minutes(15));
            let old = app.sleep.token;
            let deadline = app.sleep.deadline().unwrap();
            app.sleep_action(Action::Minutes(30));
            assert_ne!(app.sleep.token, old);
            let (events, receive) = std::sync::mpsc::channel();
            app.player_events = receive;
            events
                .send(player::Event::SleepExpired { token: old })
                .unwrap();
            app.poll(host);
            assert!(
                app.sleep.deadline().is_some(),
                "stale expiry cannot cancel a replacement"
            );
            assert!(!app.poll_sleep(deadline));
            app.sleep_action(Action::Cancel);
            assert!(!app.poll_sleep(deadline + Duration::from_secs(3600)));
            app.sleep.set(Some(Mode::Deadline(
                SystemTime::now() - Duration::from_secs(1),
            )));
            assert!(app.poll(host));
            assert!(app.sleep.held);
            assert!(app.transport.paused);
            assert_eq!(app.transport.position, 37.);
            let generation = app.transport.generation;
            finish(app);
            assert_eq!(app.transport.generation, generation);
            app.sleep_action(Action::Songs(2));
            app.apply(Transport::SignOut);
            assert_eq!(app.sleep.mode, None);
            assert!(!app.sleep.held);
        });
    }
}
