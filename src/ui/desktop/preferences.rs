use super::*;
use crate::{
    model::Dialog,
    settings::{LanguageChoice, PlayerBarVis},
};

fn panel(v: &Paint<'_>) -> El {
    col()
        .w_full()
        .px(20.)
        .py(16.)
        .gap(14.)
        .radius(f32::from(crate::theme::RADIUS) + 2.)
        .border(1., color(v.p.outline))
        .fill(color(v.p.surface.gamma_multiply(if v.p.dark {
            0.7
        } else {
            1.
        })))
}

fn section(v: &Paint<'_>, title: &str, rows: Vec<El>) -> El {
    col()
        .w_full()
        .gap(8.)
        .child(label(title, 18., 700., v.p.text))
        .child(panel(v).children(rows))
}

fn switch(v: &Paint<'_>, name: &str, checked: bool, change: impl Fn(&mut State) + 'static) -> El {
    el().size(40., 22.)
        .radius(11.)
        .role("switch")
        .focusable(true)
        .label(name)
        .checked(checked)
        .fill(color(if checked {
            v.p.accent
        } else {
            v.p.surface_active
        }))
        .child(
            el().size(16., 16.)
                .radius(8.)
                .absolute(if checked { 21. } else { 3. }, 3.)
                .fill(color(v.p.text)),
        )
        .on_press(change)
}

fn setting(v: &Paint<'_>, name: &str, detail: &str, control: El, control_width: f32) -> El {
    let copy = col()
        .min_w(0.)
        .gap(4.)
        .child(label(name, 15., 600., v.p.text).wrap())
        .child(label(detail, 13., 400., v.p.secondary).wrap());
    if v.content_width().min(800.) - 42. - control_width < 155. {
        col().w_full().gap(8.).child(copy).child(control)
    } else {
        row()
            .w_full()
            .items_center()
            .gap(12.)
            .child(copy.grow(1.))
            .child(control)
    }
}

fn choice(v: &Paint<'_>, key: &'static str, name: &str, current: &str) -> El {
    row()
        .key(key)
        .w(200.)
        .h(30.)
        .px(10.)
        .items_center()
        .radius(5.)
        .role("combobox")
        .focusable(true)
        .label(format!("{name}: {current}"))
        .expanded(v.s.menu == Some(key))
        .fill(color(v.p.surface))
        .child(label(current, 14., 500., v.p.text).grow(1.).truncate())
        .child(v.icon(Icon::ChevronDown, 14., v.p.secondary))
        .on_press(move |s: &mut State| s.menu = Some(key))
}

pub(super) fn choice_menu(v: &Paint<'_>, key: &'static str) -> El {
    let p = &v.s.app.presentation;
    let options = if key == "themes" {
        let mut opts = vec![
            (v.tr("Follow system"), Action::SetTheme(ThemeChoice::System)),
            (v.tr("Light"), Action::SetTheme(ThemeChoice::Light)),
            (v.tr("Dark"), Action::SetTheme(ThemeChoice::Dark)),
        ];
        opts.extend(p.custom_themes.themes().iter().map(|t| {
            (
                fastframe_theme::display_name(&t.filename).to_owned(),
                Action::SetCustomTheme(t.filename.clone()),
            )
        }));
        opts
    } else {
        let system = crate::i18n::pgettext(p.locale, "language", "System");
        let mut opts = vec![(
            system.into_owned(),
            Action::SetLanguage(LanguageChoice::System),
        )];
        opts.extend(crate::i18n::LOCALES.iter().map(|l| {
            (
                l.native_name().to_owned(),
                Action::SetLanguage(LanguageChoice::Locale(*l)),
            )
        }));
        opts
    };
    v.menu(key).child(
        col()
            .min_h(0.)
            .shrink()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .children(options.into_iter().map(|(name, action)| {
                v.button(&name, false, move |s, tx| s.dispatch(action.clone(), tx))
                    .role("menuitem")
            })),
    )
}

fn chip(v: &Paint<'_>, name: &str, selected: bool, change: impl Fn(&mut State) + 'static) -> El {
    el().h(30.)
        .px(12.)
        .center()
        .radius(15.)
        .focusable(true)
        .label(name)
        .role("button")
        .checked(selected)
        .fill(color(if selected { v.p.text } else { v.p.surface }))
        .child(label(
            name,
            13.,
            550.,
            if selected { v.p.window } else { v.p.text },
        ))
        .on_press(change)
}

pub(super) fn settings(v: &Paint<'_>) -> El {
    let a = &v.s.app;
    let p = &a.presentation;
    let settings = &p.settings;
    let q = v.s.filter.text.trim().to_lowercase();
    let matches = |values: &[&str]| values.iter().any(|v| v.to_lowercase().contains(&q));
    let mut filter = row()
        .w(v.content_width().min(400.))
        .h(34.)
        .px(10.)
        .gap(6.)
        .items_center()
        .radius(17.)
        .fill(color(v.p.surface))
        .child(v.icon(Icon::Search, 16., v.p.secondary))
        .child(
            text_input(&v.s.filter, TextStyle::new(14., 400.))
                .single_line()
                .key("filter")
                .label(v.tr("Search settings"))
                .placeholder(v.tr("Search settings"), color(v.p.secondary))
                .grow(1.)
                .min_w(0.)
                .color(color(v.p.text))
                .on_edit(|s: &mut State| &mut s.filter),
        );
    if !q.is_empty() {
        filter = filter.child(v.icon_button(&v.tr("Clear"), Icon::X, 14., |s, _| s.filter.clear()));
    }
    let mut page = col()
        .w_full()
        .max_w(802.)
        .gap(14.)
        .child(label(v.tr("Settings"), 28., 700., v.p.text))
        .child(filter);
    let mut visible = false;
    let account = v.tr("Account");
    let signout = v.tr("Sign out");
    if matches(&[&account, &signout, &a.preferences.username, "Navidrome"]) {
        visible = true;
        let name = &a.preferences.username;
        let account_row = row()
            .w_full()
            .items_center()
            .gap(14.)
            .child(
                el().size(56., 56.)
                    .radius(28.)
                    .center()
                    .fill(color(v.p.surface_active))
                    .child(v.icon(Icon::User, 24., v.p.secondary)),
            )
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(4.)
                    .child(label(name, 16., 600., v.p.text))
                    .child(label("Navidrome", 13., 400., v.p.secondary))
                    .child(label(
                        v.tr("Connected as {username}").replace("{username}", name),
                        12.,
                        400.,
                        v.p.dim,
                    )),
            )
            .child(
                v.button(&signout, false, |s, tx| s.dispatch(Action::SignOut, tx))
                    .border(1., color(v.p.dim)),
            );
        page = page.child(section(v, &account, vec![account_row]));
    }
    let appearance = v.tr("Appearance");
    let mut rows = vec![];
    let detail =
        crate::theme::catalog_detail(&p.custom_themes, p.locale, settings.custom_theme.as_deref());
    let description = if !detail.is_empty() {
        detail.into_owned()
    } else if p.custom_themes.follows_omarchy() {
        v.tr("Follow system uses your Omarchy colours.")
    } else {
        v.tr("Follow system uses your desktop's light or dark appearance.")
    };
    let theme = v.tr("Theme");
    if matches(&[
        &appearance,
        &theme,
        &description,
        &v.tr("Open themes folder"),
        &v.tr("How to make a theme"),
    ]) {
        let current = settings
            .custom_theme
            .as_ref()
            .map(|name| fastframe_theme::display_name(name).to_owned())
            .unwrap_or_else(|| {
                v.tr(match settings.theme {
                    ThemeChoice::Light => "Light",
                    ThemeChoice::Dark => "Dark",
                    ThemeChoice::System => "Follow system",
                })
            });
        let picker = col()
            .items_end()
            .gap(6.)
            .child(choice(v, "themes", &theme, &current))
            .child(
                row()
                    .flex_wrap()
                    .gap(6.)
                    .child(
                        v.button(&v.tr("Open themes folder"), false, |s, tx| {
                            s.dispatch(Action::OpenThemesFolder, tx)
                        })
                        .px(9.),
                    )
                    .child(
                        v.button(&v.tr("How to make a theme"), false, |s, tx| {
                            s.dispatch(
                                Action::OpenUrl(crate::ui::settings::THEMES_GUIDE_URL.into()),
                                tx,
                            )
                        })
                        .px(9.),
                    ),
            );
        rows.push(setting(v, &theme, &description, picker, 340.));
    }
    let lang = v.tr("Language");
    let description =
        v.tr("System follows your computer's language. Untranslated text stays in English.");
    if matches(&[&appearance, &lang, &description]) {
        let system = crate::i18n::pgettext(p.locale, "language", "System");
        let current = match settings.language {
            LanguageChoice::System => system.as_ref(),
            LanguageChoice::Locale(l) => l.native_name(),
        };
        let picker = choice(v, "languages", &lang, current);
        rows.push(setting(v, &lang, &description, picker, 200.));
    }
    let toggle = |name: &'static str,
                  desc: &'static str,
                  current: bool,
                  field: fn(&mut crate::settings::Settings) -> &mut bool| {
        let name = v.tr(name);
        let desc = v.tr(desc);
        matches(&[&appearance, &name, &desc]).then(|| {
            setting(
                v,
                &name,
                &desc,
                switch(v, &name, current, move |s| {
                    let x = field(&mut s.app.presentation.settings);
                    *x = !*x;
                    s.app.mark_dirty();
                }),
                40.,
            )
        })
    };
    rows.extend(toggle(
        "Colour from album art",
        "Use the current cover's colour on pages and the player bar.",
        settings.accent_from_art,
        |s| &mut s.accent_from_art,
    ));
    let name = v.tr("Player bar visualizer");
    let desc = v.tr("Show the song moving behind the player bar's controls while it plays here.");
    if matches(&[&appearance, &name, &desc]) {
        let choices = row().flex_wrap().gap(6.).children(
            [
                ("Off", PlayerBarVis::Off),
                ("Spectrum", PlayerBarVis::Spectrum),
                ("Waveform", PlayerBarVis::Waveform),
            ]
            .into_iter()
            .map(|(key, mode)| {
                chip(v, &v.tr(key), settings.player_bar_vis == mode, move |s| {
                    s.app.presentation.settings.player_bar_vis = mode;
                    s.app.mark_dirty();
                })
            }),
        );
        rows.push(setting(v, &name, &desc, choices, 250.));
    }
    rows.extend(toggle(
        "Compact library sidebar",
        "Show names without covers in the sidebar.",
        settings.sidebar_compact,
        |s| &mut s.sidebar_compact,
    ));
    rows.extend(toggle(
        "Compact track list",
        "Show each track on one line without a cover.",
        settings.tracklist_compact,
        |s| &mut s.tracklist_compact,
    ));
    let name = v.tr("Interface zoom");
    let desc = v.tr(crate::ui::keys::platform_shortcut(
        "Ctrl+Plus and Ctrl+Minus work anywhere; Ctrl+0 resets.",
        "Cmd+Plus and Cmd+Minus work anywhere; Cmd+0 resets.",
    ));
    if matches(&[&appearance, &name, &desc]) {
        let controls = row()
            .items_center()
            .gap(6.)
            .child(v.icon_button(&v.tr("Zoom out"), Icon::Minus, 16., |s, _| zoom(s, -0.1)))
            .child(
                v.button(&format!("{:.0}%", settings.zoom * 100.), false, |s, _| {
                    zoom(s, 0.)
                })
                .px(7.),
            )
            .child(v.icon_button(&v.tr("Zoom in"), Icon::Plus, 16., |s, _| zoom(s, 0.1)));
        rows.push(setting(v, &name, &desc, controls, 138.));
    }
    #[cfg(target_os="linux")]
    rows.extend(toggle("Middle-click autoscroll","Middle-click a list, then move the pointer to scroll it. Off by default, because a middle click usually pastes on Linux.",settings.middle_click_autoscroll,|s|&mut s.middle_click_autoscroll));
    if !rows.is_empty() {
        visible = true;
        page = page.child(section(v, &appearance, rows));
    }
    let equalizer = v.tr("Equalizer");
    let desc = v.tr(
        "A ten-band equalizer for playback on this computer. It does not affect other devices.",
    );
    let mut eq = vec![];
    if matches(&[&equalizer, &desc]) {
        eq.push(setting(
            v,
            &equalizer,
            &desc,
            switch(v, &equalizer, settings.eq_on, |s| {
                apply_eq(s, |e| e.on = !e.on)
            }),
            40.,
        ));
    }
    if matches(&[
        &equalizer,
        &v.tr("Presets"),
        &v.tr("Preamp"),
        &v.tr("Bands"),
        "dB",
    ]) || crate::eq::PRESETS.iter().any(|p| matches(&[p.name]))
    {
        eq.push(
            row()
                .flex_wrap()
                .gap(6.)
                .children(crate::eq::PRESETS.iter().map(|preset| {
                    chip(
                        v,
                        preset.name,
                        settings.eq_bands_db == preset.bands_db,
                        move |s| {
                            apply_eq(s, |e| {
                                e.on = true;
                                e.bands_db = preset.bands_db;
                            })
                        },
                    )
                })),
        );
        eq.push(eq_curve(v));
        let mut bands = row().w_full().gap(14.);
        for (i, db) in std::iter::once(settings.eq_preamp_db)
            .chain(settings.eq_bands_db)
            .enumerate()
        {
            let label = if i == 0 {
                crate::i18n::pgettext(p.locale, "equalizer", "Pre").into_owned()
            } else {
                crate::ui::settings::hertz(crate::eq::BANDS[i - 1])
            };
            bands = bands.child(eq_slider(v, i, &label, db));
        }
        eq.push(bands);
    }
    if !eq.is_empty() {
        visible = true;
        page = page.child(section(v, &equalizer, eq));
    }
    let about = v.tr("About");
    let keys = v.tr("Keyboard shortcuts");
    let source = v.tr("Source code");
    let version = concat!("Navifast ", env!("CARGO_PKG_VERSION"));
    let built = "Built with Rust and Rune. Playback from your Navidrome library.";
    if matches(&[&about, &keys, &source, version, built]) {
        visible = true;
        page = page.child(section(
            v,
            &about,
            vec![
                row().items_center().gap(8.).child(v.logo(40.)).child(
                    col()
                        .gap(4.)
                        .child(label(version, 15., 600., v.p.text))
                        .child(label(built, 13., 400., v.p.secondary).wrap()),
                ),
                row()
                    .gap(8.)
                    .child(v.button(&keys, false, |s, tx| {
                        s.dispatch(Action::ShowDialog(Dialog::Shortcuts), tx)
                    }))
                    .child(v.button(&source, false, |s, tx| {
                        s.dispatch(Action::OpenUrl(env!("CARGO_PKG_REPOSITORY").into()), tx)
                    })),
            ],
        ));
    }
    if !visible {
        page = page
            .child(label(
                v.tr("No settings for “{query}”").replace("{query}", &q),
                18.,
                600.,
                v.p.text,
            ))
            .child(label(
                v.tr("Try fewer words, or check the spelling."),
                14.,
                400.,
                v.p.secondary,
            ));
    }
    page
}

fn eq_slider(v: &Paint<'_>, index: usize, name: &str, value: f32) -> El {
    let apply = move |s: &mut State, db: f32| {
        apply_eq(s, |e| {
            if index == 0 {
                e.preamp_db = db
            } else {
                e.bands_db[index - 1] = db
            }
        })
    };
    let range = crate::eq::RANGE_DB;
    let y = 59. - value * 49. / range;
    let ink = if v.s.app.preferences.eq_on {
        v.p.accent
    } else {
        v.p.dim
    };
    col()
        .min_w(0.)
        .w(30.)
        .items_center()
        .gap(4.)
        .child(
            el().w(30.)
                .h(118.)
                .key(format!("eq-{index}"))
                .focusable(true)
                .label(name)
                .role("slider")
                .range_value(value.into(), (-range).into(), range.into())
                .range_step(0.1)
                .on_step(move |s, dir, _| apply(s, (value + dir as f32 * 0.1).clamp(-range, range)))
                .on_step_limit(move |s, end, _| apply(s, if end { range } else { -range }))
                .on_double_press(move |s| apply(s, 0.))
                .on_drag(move |s, d, _| {
                    if !d.cancelled() {
                        let db = ((d.h - 10. - d.y) / (d.h - 20.).max(1.) * 2. * range - range)
                            .clamp(-range, range);
                        apply(s, (db * 10.).round() / 10.);
                    }
                })
                .child(
                    el().absolute(13., 10.)
                        .size(4., 98.)
                        .radius(2.)
                        .fill(color(v.p.surface_active)),
                )
                .child(
                    el().absolute(13., y.min(59.))
                        .size(4., (y - 59.).abs())
                        .radius(2.)
                        .fill(color(ink)),
                )
                .child(el().absolute(10., 59.).size(10., 1.).fill(color(v.p.dim)))
                .child(
                    el().absolute(8., y - 7.)
                        .size(14., 14.)
                        .radius(7.)
                        .fill(color(v.p.text)),
                ),
        )
        .child(label(name, 11.5, 400., v.p.secondary))
}

fn eq_curve(v: &Paint<'_>) -> El {
    let w = (v.content_width() - 42.).clamp(24., 720.);
    let range = crate::eq::RANGE_DB;
    let eq = crate::app::eq_settings(&v.s.app.presentation.settings);
    let x = |hz: f32| 10. + (hz / 20.).log10() / 3. * (w - 20.);
    let y = |db: f32| 60. - db.clamp(-range, range) / range * 48.;
    let mut shape = rune::Shape::new();
    for db in [-12., -6., 0., 6., 12.] {
        shape = shape.stroke(
            vec![[10., y(db)], [w - 10., y(db)]],
            1.,
            color(if db == 0. { v.p.dim } else { v.p.outline }),
        );
    }
    for hz in crate::eq::BANDS {
        shape = shape.stroke(vec![[x(hz), 12.], [x(hz), 108.]], 1., color(v.p.outline));
    }
    let curve = eq.curve();
    let ink = if eq.on { v.p.accent } else { v.p.dim };
    let points = (0..=240)
        .map(|i| {
            let t = i as f32 / 240.;
            [10. + t * (w - 20.), y(curve.db_at(20. * 1000f32.powf(t)))]
        })
        .collect::<Vec<_>>();
    let mut plot = el()
        .w(w)
        .h(120.)
        .radius(8.)
        .fill(color(v.p.surface))
        .shape(shape.stroke(points, 2., color(ink)).view_box(w, 120.));
    for (hz, db) in crate::eq::BANDS.iter().zip(eq.bands_db) {
        plot = plot.child(
            el().absolute(x(*hz) - 3., y(db + eq.preamp_db) - 3.)
                .size(6., 6.)
                .radius(3.)
                .fill(color(ink)),
        );
    }
    plot
}
pub(super) fn zoom(s: &mut State, delta: f32) {
    let factor = &mut s.app.presentation.settings.zoom;
    *factor = if delta == 0. {
        1.
    } else {
        (*factor + delta).clamp(0.5, 2.5)
    };
    s.app.mark_dirty();
}
fn apply_eq(s: &mut State, edit: impl FnOnce(&mut crate::eq::EqSettings)) {
    let mut eq = crate::app::eq_settings(&s.app.presentation.settings);
    edit(&mut eq);
    let eq = eq.clamped();
    let x = &mut s.app.presentation.settings;
    x.eq_on = eq.on;
    x.eq_preamp_db = eq.preamp_db;
    x.eq_bands_db = eq.bands_db;
    s.app.apply(navidrome::Action::SetEq {
        on: eq.on,
        bands: eq.bands_db,
        preamp: eq.preamp_db,
    });
}
pub(super) fn shortcuts(v: &Paint<'_>) -> El {
    let mut keys = col().gap(8.);
    let mut descriptions = col().gap(8.);
    for (key, description) in crate::ui::keys::navidrome_shortcuts(v.s.app.presentation.locale) {
        keys = keys.child(label(key.as_ref(), 13., 600., v.p.text).h(28.));
        descriptions =
            descriptions.child(label(description.as_ref(), 13.5, 400., v.p.secondary).h(28.));
    }
    let list = col()
        .w_full()
        .max_h((v.height - 190.).max(120.))
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(row().gap(24.).child(keys).child(descriptions));
    let done = v
        .button(&v.tr("Done"), true, |s, tx| {
            s.dispatch(Action::CloseDialog, tx)
        })
        .px(18.);
    let d = col()
        .w(470_f32.min(v.width - 16.))
        .padding(24.)
        .radius(12.)
        .border(1., color(v.p.outline))
        .fill(color(v.p.overlay))
        .shadow(10., 40., color(v.p.shadow))
        .modal(true, |s: &mut State| s.app.presentation.dialog = None)
        .role("dialog")
        .label(v.tr("Keyboard shortcuts"))
        .child(label(v.tr("Keyboard shortcuts"), 20., 700., v.p.text).mb(18.))
        .child(list)
        .child(row().w_full().justify_end().mt(22.).child(done));
    col()
        .absolute(0., 0.)
        .w_full()
        .h_full()
        .center()
        .fill(color(Color32::from_black_alpha(if v.p.dark {
            150
        } else {
            80
        })))
        .child(d)
}
