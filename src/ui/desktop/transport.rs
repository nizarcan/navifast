use super::*;
use crate::i18n::mark;
use crate::{app::navidrome::Action as Service, settings::PlayerBarVis};

fn slider(
    v: &Paint<'_>,
    name: &str,
    value: f64,
    range: [f64; 2],
    step: f64,
    width: f32,
    apply: impl Fn(&mut State, f64, Option<rune::Phase>) + Copy + 'static,
) -> El {
    let [low, high] = range;
    let extent = (high - low).max(f64::EPSILON);
    el().key(name)
        .label(name)
        .role("slider")
        .focusable(true)
        .h(16.)
        .w(width)
        .hover_group()
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
            el().w_full()
                .h(4.)
                .radius(2.)
                .fill(color(if v.p.dark {
                    Srgba::from_white_alpha(50)
                } else {
                    Srgba::from_black_alpha(40)
                }))
                .child(
                    el().h(4.)
                        .radius(2.)
                        .w_frac(((value - low) / extent).clamp(0., 1.) as f32)
                        .fill(color(v.p.text))
                        .child(
                            el().w_full()
                                .h_full()
                                .radius(2.)
                                .fill(color(v.p.accent))
                                .group_opacity(0., 1.),
                        )
                        .child(
                            el().size(12., 12.)
                                .pin(None, Some(-4.), Some(-6.), None)
                                .radius(6.)
                                .fill(color(v.p.text))
                                .group_opacity(0., 1.),
                        ),
                ),
        )
}
pub(super) fn artwork(v: &Paint<'_>, edge: f32, expanded: bool) -> El {
    let a = &v.s.app;
    let mut sleeve = v.cover(
        a.playing_cover(),
        CoverSize::Sleeve,
        edge,
        if expanded { 8. } else { 6. },
    );
    if let Some(album) = &a.playing_album {
        let page = Page::Album(navidrome::encode(&album.id));
        sleeve = sleeve
            .role("button")
            .label(&album.name)
            .focusable(true)
            .on_press_tx(move |s: &mut State, tx| s.dispatch(Action::Open(page.clone()), tx));
    }
    let mut art = el().size(edge, edge).hover_group().child(sleeve);
    if a.playing_cover().is_some()
        && a.presentation.settings.sidebar_visible
        && (expanded || !a.presentation.settings.art_expanded)
    {
        let (diameter, inset, icon, name) = if expanded {
            (20., 6., Icon::ChevronDown, "Collapse artwork")
        } else {
            (18., 1., Icon::ChevronUp, "Expand artwork")
        };
        art = art.child(
            el().key(if expanded {
                "collapse-art"
            } else {
                "expand-art"
            })
            .size(diameter, diameter)
            .radius(diameter / 2.)
            .center()
            .pin(None, Some(inset), Some(inset), None)
            .fill(color(v.p.panel.gamma_multiply(0.9)))
            .role("button")
            .label(v.tr(name))
            .focusable(true)
            .group_opacity(0., 1.)
            .child(v.icon(icon, diameter - 6., v.p.text))
            .on_press_tx(move |s: &mut State, tx| {
                s.app.presentation.settings.art_expanded = !expanded;
                s.dispatch(Action::SettingsChanged, tx);
            }),
        );
    }
    art
}
pub(super) fn player(v: &Paint<'_>) -> El {
    let a = &v.s.app;
    let song = a.transport.song();
    let inner_width = (v.width - 32.).max(0.);
    let side = (inner_width * 0.30).clamp(200., 420.);
    let seek_width = (inner_width - side * 2. - 120.).clamp(120., 620.);
    let mut meta = row()
        .w(side)
        .min_w(0.)
        .h_full()
        .items_center()
        .pl(4.)
        .gap(12.);
    if let Some(t) = song {
        let mut title = label(&t.title, 14., 550., v.p.text).truncate();
        if let Some(album) = &a.playing_album {
            let id = navidrome::encode(&album.id);
            title = title
                .role("button")
                .focusable(true)
                .label(&t.title)
                .on_press_tx(move |s: &mut State, tx| {
                    s.dispatch(Action::Open(Page::Album(id.clone())), tx)
                });
        }
        meta = meta
            .child(artwork(v, 56., false))
            .child(col().min_w(0.).gap(2.).child(title).child(library::credit(
                v,
                &t.artist,
                t.credits.artist_id.as_deref(),
            )))
            .child(library::star(
                v,
                crate::navidrome::api::FavoriteKind::Song,
                &t.id,
                18.,
            ));
    } else {
        meta = meta.child(v.cover(None, CoverSize::Sleeve, 56., 6.)).child(
            col()
                .min_w(0.)
                .gap(2.)
                .child(label(
                    v.tr(mark("Nothing playing")),
                    14.,
                    500.,
                    v.p.secondary,
                ))
                .child(
                    label(
                        v.tr(mark("Pick a song, album, or playlist")),
                        12.,
                        400.,
                        v.p.dim,
                    )
                    .truncate(),
                ),
        );
    }
    let play = a.transport.paused || song.is_none();
    let controls = row()
        .gap(10.)
        .items_center()
        .child(
            v.icon_button(&v.tr(mark("Shuffle")), Icon::Shuffle, 17., |_, _| {})
                .size(29., 36.)
                .disabled(true),
        )
        .child(
            v.icon_button(
                &v.tr(mark("Previous")),
                Icon::SkipBackFilled,
                18.,
                |s, _| s.app.apply(Service::Previous),
            )
            .size(30., 36.)
            .disabled(song.is_none()),
        )
        .child(
            el().key("toggle")
                .role("button")
                .focusable(true)
                .label(v.tr(if play { mark("Play") } else { mark("Pause") }))
                .size(36., 36.)
                .center()
                .radius(18.)
                .fill(color(v.p.text))
                .hover(color(if v.p.dark { Srgba::WHITE } else { v.p.text }))
                .child(if a.playback_loading() {
                    v.spinner(22., v.p.window)
                } else {
                    let glyph = if play {
                        Icon::PlayFilled
                    } else {
                        Icon::PauseFilled
                    };
                    let size = 36. * 0.46;
                    let offset = crate::theme::play_glyph_offset(glyph, size);
                    v.icon(glyph, size, v.p.window).offset(offset[0], offset[1])
                })
                .disabled(song.is_none())
                .on_press(|s: &mut State| s.app.apply(Service::Toggle)),
        )
        .child(
            v.icon_button(&v.tr(mark("Next")), Icon::SkipForwardFilled, 18., |s, _| {
                s.app.apply(Service::Next)
            })
            .size(30., 36.)
            .disabled(a.transport.next_index().is_none()),
        )
        .child(
            v.icon_button(&v.tr(mark("Repeat")), Icon::Repeat, 17., |_, _| {})
                .size(29., 36.)
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
        .h(88.)
        .items_center()
        .pt(18.)
        .gap(5.)
        .child(controls)
        .child(
            row()
                .w_full()
                .justify_center()
                .items_center()
                .gap(8.)
                .child(
                    label(elapsed(position), 11.5, 400., v.p.secondary)
                        .w(42.)
                        .align(rune::Align::End),
                )
                .child(
                    slider(
                        v,
                        &v.tr(mark("Playback position (%)")),
                        if duration > 0. {
                            100. * position / duration
                        } else {
                            0.
                        },
                        [0., 100.],
                        1.,
                        seek_width,
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
                .child(label(elapsed(duration), 11.5, 400., v.p.secondary).w(42.)),
        );
    let volume = row()
        .w(side)
        .items_center()
        .justify_end()
        .gap(6.)
        .children(
            [
                (mark("Lyrics"), Icon::Mic),
                (mark("Queue"), Icon::ListVideo),
                (mark("Connect to a device"), Icon::Speaker),
            ]
            .into_iter()
            .map(|(name, icon)| {
                v.icon_button(&v.tr(name), icon, 18., |_, _| {})
                    .disabled(true)
            }),
        )
        .child(v.icon_button(
            &v.tr(if a.preferences.volume == 0. {
                mark("Unmute")
            } else {
                mark("Mute")
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
        .child(
            row().w(92.).child(
                slider(
                    v,
                    &v.tr(mark("Volume (%)")),
                    f64::from(a.preferences.volume) * 100.,
                    [0., 100.],
                    5.,
                    92.,
                    |s, x, _| s.app.apply(Service::Volume((x / 100.) as f32)),
                )
                .on_wheel(|s: &mut State, e, tx| {
                    if e.dy == 0. {
                        return false;
                    }
                    let steps = if e.precise {
                        s.volume_wheel_points += e.dy;
                        let whole = (s.volume_wheel_points / 50.).trunc();
                        s.volume_wheel_points -= whole * 50.;
                        whole
                    } else {
                        e.dy.signum()
                    };
                    if steps != 0. {
                        s.app.apply(Service::Volume(
                            (s.app.preferences.volume + 0.05 * steps).clamp(0., 1.),
                        ));
                    } else {
                        tx.live_only();
                    }
                    true
                }),
            ),
        );
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
        .label(v.tr(mark("Player bar visualizer")))
        .help(v.tr(mark("Click to change the visualizer")))
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
