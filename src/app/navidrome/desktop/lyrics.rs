//! Lyrics viewport ownership and follow behavior, separate from server state.
use super::State;
use crate::{model::Loadable, navidrome::lyrics::Outcome};
use rune::{ScrollPos, Tx};
use std::{cell::Cell, rc::Rc};

pub(crate) struct View {
    pub following: Rc<Cell<bool>>,
    pub viewport: Rc<Cell<ScrollPos>>,
    pub automatic: Rc<Cell<bool>>,
    pub pending: bool,
    song: Option<String>,
    shown: Option<usize>,
    painted_following: bool,
    painted_active: Option<usize>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            following: Rc::new(Cell::new(true)),
            viewport: Default::default(),
            automatic: Default::default(),
            pending: false,
            song: None,
            shown: None,
            painted_following: true,
            painted_active: None,
        }
    }
}
impl State {
    pub(crate) fn retry_lyrics(&mut self) {
        self.app.retry_lyrics();
    }
    pub(crate) fn lyrics_scroll_observer(&self) -> impl Fn(ScrollPos) + 'static {
        let viewport = self.lyrics_view.viewport.clone();
        let following = self.lyrics_view.following.clone();
        let automatic = self.lyrics_view.automatic.clone();
        let wake = self.window.wake.clone();
        move |pos| {
            let previous = viewport.replace(pos);
            if !automatic.get()
                && previous.viewport == pos.viewport
                && (previous.offset[1] - pos.offset[1]).abs() > 1.
                && following.replace(false)
            {
                wake();
            }
        }
    }

    pub(crate) fn toggle_lyrics(&mut self, tx: &mut Tx) {
        let open = !self.app.lyrics.enabled;
        if open {
            self.queue_open = false;
        }
        self.app.enable_lyrics(open);
        self.menu = None;
        self.favorite_menu = None;
        self.queue_menu = None;
        if open {
            self.follow_lyrics(tx);
        } else if let Some(offset) = self.offsets.get(self.app.presentation.page()) {
            tx.scroll_to_y("page", offset.get());
        }
        tx.instant();
    }
    pub(crate) fn lyric_active(&self) -> Option<usize> {
        let Loadable::Loaded(Outcome::Found(lyrics)) = &self.app.lyrics.data else {
            return None;
        };
        lyrics.active_line((self.app.transport.position.max(0.0) * 1000.) as u64)
    }
    pub(crate) fn follow_lyrics(&mut self, tx: &mut Tx) {
        self.lyrics_view.following.set(true);
        self.lyrics_view.shown = None;
        self.poll_lyrics_view(tx);
    }
    pub(crate) fn pause_lyrics_follow(&mut self) {
        self.lyrics_view.following.set(false);
        self.lyrics_view.automatic.set(false);
        self.lyrics_view.pending = false;
    }
    pub(crate) fn poll_lyrics_view(&mut self, tx: &mut Tx) -> bool {
        let view = &mut self.lyrics_view;
        let mut changed = false;
        if view.pending {
            view.automatic.set(false);
            view.pending = false;
        }
        if view.song != self.app.lyrics.song {
            *view = View {
                song: self.app.lyrics.song.clone(),
                ..Default::default()
            };
            changed = true;
            tx.scroll_to_y("lyrics-scroll", 0.);
        }
        if !self.app.lyrics.enabled {
            return changed;
        }
        if view.painted_following != view.following.get() {
            view.painted_following = view.following.get();
            changed = true;
        }
        let active = self.lyric_active();
        let view = &mut self.lyrics_view;
        if view.painted_active != active {
            view.painted_active = active;
            changed = true;
        }
        if view.following.get() && view.shown != active {
            view.automatic.set(true);
            view.pending = true;
            view.shown = active;
            if let Some(active) = active {
                tx.scroll_center_in_instant("lyrics-scroll", format!("lyric-{active}"));
            } else {
                tx.scroll_to_y("lyrics-scroll", 0.);
            }
            // The offset callback in this commit belongs to the follow request.
            // Clear suppression on the next poll, even with paused playback.
            tx.wake_after(std::time::Duration::ZERO);
            changed = true;
        }
        changed
    }
}
