//! Session-only sleep policy. Boundaries are checked before selecting/feeding audio.
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Deadline(SystemTime),
    Songs(u32),
    Queue,
}

#[derive(Default)]
pub struct Timer {
    pub mode: Option<Mode>,
    pub held: bool,
    pub token: u64,
}

impl Timer {
    pub fn set(&mut self, mode: Option<Mode>) {
        self.token = self.token.wrapping_add(1);
        self.mode = mode.filter(|mode| *mode != Mode::Songs(0));
        self.held = false;
    }

    pub fn deadline(&self) -> Option<SystemTime> {
        match self.mode {
            Some(Mode::Deadline(deadline)) => Some(deadline),
            _ => None,
        }
    }

    pub fn expire(&mut self) {
        self.mode = None;
        self.held = true;
    }

    /// Seek and queue edits never call this. Repeat One is a new song play.
    pub fn boundary(&mut self, now: SystemTime, queue_end: bool) -> bool {
        let stop = match self.mode {
            Some(Mode::Songs(remaining)) if remaining > 1 => {
                self.mode = Some(Mode::Songs(remaining - 1));
                false
            }
            Some(Mode::Songs(_)) => true,
            Some(Mode::Queue) => queue_end,
            Some(Mode::Deadline(deadline)) => now >= deadline,
            None => self.held,
        };
        if stop {
            self.expire();
        }
        stop
    }
}

/// Output-side deadline also runs while the window is hidden or playback paused.
#[derive(Default)]
pub(super) struct OutputTimer {
    pub token: u64,
    pub deadline: Option<SystemTime>,
    pub held: bool,
}

impl OutputTimer {
    pub fn expired(&mut self, now: SystemTime) -> bool {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.deadline = None;
            self.held = true;
            true
        } else {
            false
        }
    }
}
