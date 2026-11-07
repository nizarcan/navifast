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
    painted_words: Vec<usize>,
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
            painted_words: Vec::new(),
        }
    }
}
impl State {
    pub(crate) fn set_lrclib_lyrics(&mut self, enabled: bool) {
        self.app.set_lrclib_lyrics(enabled);
    }
    pub(crate) fn lyric_words(&self) -> impl Iterator<Item = usize> + '_ {
        let line = match (&self.app.lyrics.data, self.lyric_active()) {
            (Loadable::Loaded(Outcome::Found(lyrics)), Some(index)) => lyrics.lines.get(index),
            _ => None,
        };
        let position = (self.app.transport.position.max(0.) * 1000.) as u64;
        line.into_iter()
            .flat_map(move |line| line.active_words(position))
    }
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
        let reset_to_top =
            self.lyrics_view.song != self.app.lyrics.song && self.lyric_active().is_none();
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
            if reset_to_top {
                tx.scroll_to_y("lyrics-scroll", 0.);
            }
        }
        if !self.app.lyrics.enabled {
            return changed;
        }
        if view.painted_following != view.following.get() {
            view.painted_following = view.following.get();
            changed = true;
        }
        let active = self.lyric_active();
        let words = self
            .lyric_words()
            .ne(self.lyrics_view.painted_words.iter().copied())
            .then(|| self.lyric_words().collect());
        let view = &mut self.lyrics_view;
        if let Some(words) = words {
            view.painted_words = words;
            changed = true;
        }
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
