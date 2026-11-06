//! Middle-click scrolling stays with the viewport under the initial press.
use super::State;
use rune::{El, Phase, PointerButton, ScrollPos, Tx};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
pub(crate) enum Pane {
    Page,
    Library,
}

impl Pane {
    fn key(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Library => "sidebar-albums",
        }
    }
}

struct Gesture {
    pane: Pane,
    anchor: f32,
    last: Instant,
}

#[derive(Default)]
pub(crate) struct Autoscroll {
    page: Rc<Cell<ScrollPos>>,
    library: Rc<Cell<ScrollPos>>,
    pointer: f32,
    gesture: Option<Gesture>,
}

impl Autoscroll {
    pub fn viewport(&self, pane: Pane) -> Rc<Cell<ScrollPos>> {
        match pane {
            Pane::Page => self.page.clone(),
            Pane::Library => self.library.clone(),
        }
    }
    pub fn cancel(&mut self) -> bool {
        self.gesture.take().is_some()
    }
    pub fn active(&self) -> bool {
        self.gesture.is_some()
    }
    fn start(&mut self, pane: Pane) {
        if self.viewport(pane).get().max[1] > 0. {
            self.gesture = Some(Gesture {
                pane,
                anchor: self.pointer,
                last: Instant::now(),
            });
        }
    }
    pub fn poll(&mut self, tx: &mut Tx) {
        let Some(g) = &mut self.gesture else {
            return;
        };
        let now = Instant::now();
        let dt = now.duration_since(g.last).as_secs_f32().min(0.05);
        g.last = now;
        let pane = g.pane;
        let distance = self.pointer - g.anchor;
        let speed = crate::autoscroll::speed(distance.abs()) * distance.signum();
        let pos = self.viewport(pane).get();
        let y = (pos.offset[1] + speed * dt).clamp(0., pos.max[1]);
        if y != pos.offset[1] {
            tx.scroll_to_y(pane.key(), y);
            tx.wake_after(Duration::from_millis(16));
        }
    }
}

pub(crate) fn pane(el: El, pane: Pane, enabled: bool) -> El {
    if !enabled {
        return el;
    }
    el.pointer_buttons([PointerButton::Middle])
        .on_drag(move |s: &mut State, d, tx| {
            if tx.button() == PointerButton::Middle && matches!(d.phase, Phase::Down) {
                s.autoscroll.start(pane);
            }
        })
}

pub(crate) fn shell(el: El, s: &State, zoom: f32) -> El {
    if !crate::autoscroll::enabled(s.app.presentation.settings.middle_click_autoscroll) {
        return el;
    }
    let mut el = el.on_pointer_move_local(move |s: &mut State, p, tx| {
        s.autoscroll.pointer = p.window_y / zoom;
        if s.autoscroll.active() {
            tx.wake_after(Duration::ZERO);
        }
        tx.live_only();
    });
    if s.autoscroll.active() {
        el = el.child(
            rune::el::el()
                .pin(Some(0.), Some(0.), Some(0.), Some(0.))
                .cursor(rune::Cursor::Move)
                .pointer_buttons([PointerButton::Middle])
                .on_press(|s: &mut State| {
                    s.autoscroll.cancel();
                })
                .on_secondary_press(|s: &mut State, _, _| {
                    s.autoscroll.cancel();
                })
                .on_wheel(|s: &mut State, _, _| s.autoscroll.cancel())
                .a11y_hidden(),
        );
    }
    el
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesture_is_bound_to_a_scrollable_pane_until_cancelled() {
        let mut a = Autoscroll::default();
        a.start(Pane::Page);
        assert!(!a.active());
        let mut visible = ScrollPos::default();
        visible.max[1] = 700.;
        a.page.set(visible);
        a.pointer = 42.;
        a.start(Pane::Page);
        assert_eq!(a.gesture.as_ref().unwrap().pane.key(), "page");
        a.pointer = 110.;
        assert_eq!(a.gesture.as_ref().unwrap().anchor, 42.);
        assert!(a.cancel());
        assert!(!a.active());
        assert_eq!(crate::autoscroll::speed(11.), 0.);
        assert_eq!(crate::autoscroll::speed(22.), 60.);
    }
}
