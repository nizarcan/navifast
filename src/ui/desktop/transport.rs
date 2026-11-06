use super::*;
use crate::{app::navidrome::Action as Service, settings::PlayerBarVis};

fn slider(
    v: &Paint<'_>,
    name: &str,
    value: f64,
    range: [f64; 2],
    step: f64,
    apply: impl Fn(&mut State, f64, Option<rune::Phase>) + Copy + 'static,
) -> El {
    let [low, high] = range;
    let extent = (high - low).max(f64::EPSILON);
    el().key(name)
        .label(name)
        .role("slider")
        .focusable(true)
        .h(22.)
        .grow(1.)
        .min_w(0.)
        .center()
        .range_value(value, low, high)
        .range_step(step)
        .on_step(move |s: &mut State, dir, _| {
            apply(
                s,
                (value + f64::from(dir) * step).clamp(low, high),
                Some(rune::Phase::Up),
            )
        })
        .on_step_limit(move |s: &mut State, end, _| {
            apply(s, if end { high } else { low }, Some(rune::Phase::Up))
        })
        .on_drag(move |s: &mut State, d, _| {
            if d.phase == rune::Phase::Down {
                d.data = value as f32;
            }
            let position = if d.cancelled() {
                f64::from(d.data)
            } else {
                low + extent * f64::from((d.x / d.w.max(1.)).clamp(0., 1.))
            };
            apply(s, position, (!d.cancelled()).then_some(d.phase));
        })
        .child(
            el().w_full().h(4.).radius(2.).fill(color(v.p.dim)).child(
                el().h(4.)
                    .radius(2.)
                    .w_frac(((value - low) / extent).clamp(0., 1.) as f32)
                    .fill(color(v.p.text)),
            ),
        )
}
pub(super) fn player(v: &Paint<'_>) -> El {
    let a = &v.s.app;
    let song = a.transport.song();
    let mut meta = row()
        .w(v.width * 0.30)
        .min_w(0.)
        .h_full()
        .items_center()
        .gap(12.);
    if let Some(t) = song {
        let mut title = label(&t.title, 14., 550., v.p.text).truncate();
        if let Some(album) = &a.playing_album {
            let id = navidrome::encode(&album.id);
            title = title
                .role("button")
                .label(&t.title)
                .on_press_tx(move |s: &mut State, tx| {
                    s.dispatch(Action::Open(Page::Album(id.clone())), tx)
                });
        }
        meta = meta
            .child(v.cover(a.playing_cover(), CoverSize::Sleeve, 56., 6.))
            .child(col().min_w(0.).gap(2.).child(title).child(library::credit(
                v,
                &t.artist,
                t.credits.artist_id.as_deref(),
            )))
            .child(library::star(
                v,
                crate::navidrome::api::FavoriteKind::Song,
                &t.id,
            ));
    }
    let play = a.transport.paused || song.is_none();
    let controls = row()
        .gap(6.)
        .items_center()
        .child(
            v.icon_button(&v.tr("Shuffle"), Icon::Shuffle, 16., |_, _| {})
                .disabled(true),
        )
        .child(
            v.icon_button(&v.tr("Previous"), Icon::SkipBackFilled, 20., |s, _| {
                s.app.apply(Service::Previous)
            })
            .disabled(song.is_none()),
        )
        .child(
            el().key("toggle")
                .role("button")
                .focusable(true)
                .label(v.tr(if play { "Play" } else { "Pause" }))
                .size(36., 36.)
                .center()
                .radius(18.)
                .fill(color(v.p.text))
                .child(v.icon(
                    if play {
                        Icon::PlayFilled
                    } else {
                        Icon::PauseFilled
                    },
                    18.,
                    v.p.window,
                ))
                .disabled(song.is_none())
                .on_press(|s: &mut State| s.app.apply(Service::Toggle)),
        )
        .child(
            v.icon_button(&v.tr("Next"), Icon::SkipForwardFilled, 20., |s, _| {
                s.app.apply(Service::Next)
            })
            .disabled(a.transport.next_index().is_none()),
        )
        .child(
            v.icon_button(&v.tr("Repeat"), Icon::Repeat, 16., |_, _| {})
                .disabled(true),
        );
    let duration = song.map_or(0., |s| s.duration);
    let position =
        v.s.seek_preview
            .filter(|(generation, _)| *generation == a.transport.generation)
            .map_or(a.transport.position, |(_, s)| s);
    let elapsed = |t: f64| crate::util::format_duration_ms((t * 1000.) as u32);
    let center = col()
        .grow(1.)
        .min_w(0.)
        .center()
        .gap(3.)
        .child(controls)
        .child(
            row()
                .w_full()
                .items_center()
                .gap(8.)
                .child(label(elapsed(position), 11., 400., v.p.secondary))
                .child(
                    slider(
                        v,
                        &v.tr("Playback position (%)"),
                        if duration > 0. {
                            100. * position / duration
                        } else {
                            0.
                        },
                        [0., 100.],
                        1.,
                        move |s, p, phase| {
                            let t = p * duration / 100.;
                            let generation = s.app.transport.generation;
                            match phase {
                                None => s.seek_preview = None,
                                Some(rune::Phase::Up) => {
                                    if s.seek_preview.take().is_none_or(|(g, _)| g == generation) {
                                        s.app.apply(Service::Seek(t));
                                    }
                                }
                                Some(rune::Phase::Down) => s.seek_preview = Some((generation, t)),
                                Some(rune::Phase::Move) => {
                                    if let Some((_, seconds)) = &mut s.seek_preview {
                                        *seconds = t;
                                    }
                                }
                            }
                        },
                    )
                    .disabled(duration <= 0.),
                )
                .child(label(elapsed(duration), 11., 400., v.p.secondary)),
        );
    let volume = row()
        .w(v.width * 0.30)
        .items_center()
        .justify_end()
        .gap(4.)
        .children(
            [
                ("Lyrics", Icon::Mic),
                ("Queue", Icon::ListVideo),
                ("Connect to a device", Icon::Speaker),
            ]
            .into_iter()
            .map(|(name, icon)| {
                v.icon_button(&v.tr(name), icon, 18., |_, _| {})
                    .disabled(true)
            }),
        )
        .child(v.icon_button(
            &v.tr(if a.preferences.volume == 0. {
                "Unmute"
            } else {
                "Mute"
            }),
            match (a.preferences.volume * 100.).round() as u32 {
                0 => Icon::VolumeX,
                1..=33 => Icon::Volume,
                34..=66 => Icon::Volume1,
                _ => Icon::Volume2,
            },
            18.,
            |s, tx| s.dispatch(Action::ToggleMute, tx),
        ))
        .child(row().w(92.).child(slider(
            v,
            &v.tr("Volume"),
            f64::from(a.preferences.volume) * 100.,
            [0., 100.],
            5.,
            |s, x, _| s.app.apply(Service::Volume((x / 100.) as f32)),
        )));
    let height = crate::theme::PLAYER_BAR_HEIGHT;
    let tint = v.s.playing_tint(v.p);
    let fill = tint.map_or(v.p.panel, |t| crate::ui::blend(v.p.panel, t, 0.12));
    let (low, high) = crate::ui::player_bar::vis_colours(tint.unwrap_or(v.p.accent), v.p.dark);
    let strength = if v.p.dark { 1. } else { 1.5 };
    let mut panel = col()
        .key("player")
        .h(height)
        .w_full()
        .fill(color(fill))
        .px(16.)
        .clip_soft(0.);
    let mut plot = el()
        .absolute(0., 0.)
        .w_full()
        .h(height)
        .role("button")
        .label(v.tr("Player bar visualizer"))
        .on_press_tx(|s: &mut State, tx| s.dispatch(Action::CyclePlayerBarVis, tx))
        .on_layout(|s: &mut State, r| s.plot_width = r[2]);
    match a.presentation.settings.player_bar_vis {
        PlayerBarVis::Spectrum => {
            plot = plot.child(rune::effect(super::spectrum::MARK).w_full().h_full());
        }
        PlayerBarVis::Waveform if v.s.trace.len() >= 2 => {
            let trace = &v.s.trace;
            let width = v.s.plot_width.max(1.);
            let dx = width / (trace.len() - 1) as f32;
            let point =
                |i: usize, gain: f32| [i as f32 * dx, height * (0.5 - 0.46 * trace[i] * gain)];
            let mut shape = rune::Shape::new();
            for i in 0..trace.len() - 1 {
                let ink = low.lerp_to_gamma(high, i as f32 / (trace.len() - 1) as f32);
                shape = shape.fill(
                    vec![
                        point(i, 1.),
                        point(i + 1, 1.),
                        point(i + 1, 0.),
                        point(i, 0.),
                    ],
                    color(ink.gamma_multiply(0.14 * strength)),
                );
            }
            for start in (0..trace.len() - 1).step_by(8) {
                let end = (start + 8).min(trace.len() - 1);
                let ink =
                    low.lerp_to_gamma(high, (start + end) as f32 / (2 * (trace.len() - 1)) as f32);
                let line = |gain| (start..=end).map(|i| point(i, gain)).collect::<Vec<_>>();
                shape = shape.stroke(line(-0.6), 1., color(ink.gamma_multiply(0.18)));
                for (width, alpha) in crate::ui::player_bar::WAVE_LAYERS {
                    shape = shape.stroke(line(1.), width, color(ink.gamma_multiply(alpha)));
                }
            }
            plot = plot.shape(shape.view_box(width, height));
        }
        _ => {}
    }
    panel = panel.child(plot);
    panel
        .child(el().w_full().h(1.).fill(color(v.p.outline)))
        .child(
            row()
                .w_full()
                .grow(1.)
                .items_center()
                .child(meta)
                .child(center)
                .child(volume),
        )
}
