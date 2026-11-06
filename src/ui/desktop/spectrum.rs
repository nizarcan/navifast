//! The spectrum occupies one slot behind the transport controls. Reuse Rune's
//! native gradient renderer for all the bands in that slot's painter order.
use super::{State, color};
use rune::{
    Compositor,
    compose::Frame,
    render::{Instance, KIND_EFFECT, KIND_GRADIENT},
};

pub(super) const MARK: u32 = 1;
#[derive(Default)]
pub(crate) struct Spectrum {
    instances: Vec<Instance>,
    next_frame: Option<std::time::Instant>,
}

impl Compositor<State> for Spectrum {
    fn render(&mut self, f: &mut Frame<'_, State>) {
        self.next_frame = None;
        let Some(index) =
            f.rt.instances
                .iter()
                .position(|i| i.params[1] == KIND_EFFECT && i.border[0] == MARK as f32)
        else {
            f.draw_ui();
            return;
        };
        let slot = f.rt.instances[index];
        let s = &f.rt.state;
        let moving =
            (!s.app.transport.paused && s.app.transport.song().is_some()) || !s.analyser.settled();
        if !moving {
            f.draw_ui();
            return;
        }
        // While this frame is still current Rune can rest. At the analyser's
        // deadline, its timer asks the compositor for the next sampled frame.
        self.next_frame = s.spectrum_deadline();
        let p = s.palette(f.rt.env.system_dark);
        let tint = s
            .app
            .transport
            .song()
            .and_then(|t| t.cover_art.as_ref())
            .filter(|_| s.app.presentation.settings.accent_from_art)
            .and_then(|id| s.app.presentation.accents.get(&s.app.cover_uri(id)))
            .copied()
            .unwrap_or(p.accent);
        let palette = crate::ui::player_bar::vis_colours(tint, p.dark);
        let scale = slot.params[3];
        let [x, y, w, h] = slot.rect;
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w / scale, h / scale));
        let srgb = f.srgb_blend();
        let ink = |c: egui::Color32| {
            if srgb {
                // Color32 already stores premultiplied sRGB, the native surface format.
                c.to_array().map(|channel| f32::from(channel) / 255.)
            } else {
                color(c).premul(1.)
            }
        };
        self.instances.clear();
        self.instances.extend_from_slice(&f.rt.instances[..index]);
        crate::ui::player_bar::spectrum_quads(
            bounds,
            s.analyser.levels(),
            &s.analyser.peaks(),
            palette,
            if p.dark { 1. } else { 1.5 },
            |rect, top, bottom| {
                self.instances.push(Instance {
                    rect: [
                        x + rect.left() * scale,
                        y + rect.top() * scale,
                        rect.width() * scale,
                        rect.height() * scale,
                    ],
                    radii: [0.; 4],
                    fill: ink(top),
                    border: [0.; 4],
                    uv: ink(bottom),
                    params: [0., KIND_GRADIENT, 0., 1.],
                    clip: slot.clip,
                    fade: slot.fade,
                });
            },
        );
        self.instances
            .extend_from_slice(&f.rt.instances[index + 1..]);
        f.ui.prepare(
            f.device,
            f.queue,
            &mut f.rt.text.atlas,
            &self.instances,
            f.size,
        );
        f.ui.draw(f.encoder, f.target, f.rt.clear_color());
    }
    fn animating(&self) -> bool {
        self.next_frame
            .is_some_and(|deadline| std::time::Instant::now() >= deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn scene_rests_between_analyser_frames() {
        let mut spectrum = Spectrum::default();
        assert!(!spectrum.animating());
        spectrum.next_frame = Some(Instant::now() + Duration::from_secs(1));
        assert!(!spectrum.animating());
        spectrum.next_frame = Some(Instant::now());
        assert!(spectrum.animating());
    }
}
