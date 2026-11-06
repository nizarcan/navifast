use super::*;
use crate::{
    model::Dialog,
    settings::{LanguageChoice, PlayerBarVis},
};

fn section(v: &Paint<'_>, name: &str) -> El {
    col()
        .w_full()
        .padding(18.)
        .gap(12.)
        .radius(10.)
        .fill(color(v.p.panel))
        .child(label(name, 18., 700., v.p.text))
}
fn row_title(v: &Paint<'_>, name: &'static str) -> El {
    label(v.tr(name), 14., 550., v.p.text).grow(1.).min_w(0.)
}
pub(super) fn settings(v: &Paint<'_>) -> El {
    let a = &v.s.app;
    let settings = &a.presentation.settings;
    let query = v.s.filter.text.trim().to_lowercase();
    let matches = |keys: &str| query.is_empty() || keys.to_lowercase().contains(&query);
    let mut page = col()
        .w_full()
        .gap(16.)
        .child(label(v.tr("Settings"), 28., 700., v.p.text))
        .child(
            text_input(&v.s.filter, TextStyle::new(14., 400.))
                .single_line()
                .key("filter")
                .label(v.tr("Search settings"))
                .placeholder(v.tr("Search settings"), color(v.p.dim))
                .px(10.)
                .h(32.)
                .radius(8.)
                .fill(color(v.p.surface))
                .color(color(v.p.text))
                .on_edit(|s: &mut State| &mut s.filter),
        );
    if matches("Account Sign out") {
        page = page.child(
            section(v, &v.tr("Account")).child(
                row()
                    .items_center()
                    .child(label(&a.preferences.username, 16., 650., v.p.text).grow(1.))
                    .child(v.button(&v.tr("Sign out"), false, |s, tx| {
                        s.dispatch(Action::SignOut, tx)
                    })),
            ),
        );
    }
    if matches(
        "Appearance Theme Language Colour from album art Compact library sidebar Compact track list Interface zoom Player bar visualizer",
    ) {
        let mut group = section(v, &v.tr("Appearance"));
        let menu = v.s.menu;
        let choice = settings.custom_theme.clone().unwrap_or_else(|| {
            v.tr(match settings.theme {
                ThemeChoice::Light => "Light",
                ThemeChoice::Dark => "Dark",
                ThemeChoice::System => "Follow system",
            })
        });
        group = group.child(
            row()
                .items_center()
                .child(row_title(v, "Theme"))
                .child(v.button(&choice, false, |s, _| s.menu = Some("themes")))
                .child(v.button(&v.tr("Open themes folder"), false, |s, tx| {
                    s.dispatch(Action::OpenThemesFolder, tx)
                })),
        );
        if menu == Some("themes") {
            let mut choices = row().flex_wrap().gap(4.);
            for (name, theme) in [
                ("Follow system", ThemeChoice::System),
                ("Light", ThemeChoice::Light),
                ("Dark", ThemeChoice::Dark),
            ] {
                choices = choices.child(v.button(&v.tr(name), false, move |s, tx| {
                    s.dispatch(Action::SetTheme(theme), tx)
                }));
            }
            for item in a.presentation.custom_themes.themes() {
                let name = item.filename.clone();
                choices = choices.child(v.button(
                    fastframe_theme::display_name(&item.filename),
                    false,
                    move |s, tx| s.dispatch(Action::SetCustomTheme(name.clone()), tx),
                ));
            }
            group = group.child(choices);
        }
        group = group.child(
            row()
                .items_center()
                .child(row_title(v, "Language"))
                .child(
                    v.button(settings.language.resolve().native_name(), false, |s, _| {
                        s.menu = Some("languages")
                    }),
                ),
        );
        if menu == Some("languages") {
            let mut choices = row().flex_wrap().gap(4.).child(v.button(
                &v.tr("Follow system"),
                false,
                |s, tx| s.dispatch(Action::SetLanguage(LanguageChoice::System), tx),
            ));
            for &lang in crate::i18n::LOCALES {
                choices = choices.child(v.button(lang.native_name(), false, move |s, tx| {
                    s.dispatch(Action::SetLanguage(LanguageChoice::Locale(lang)), tx)
                }));
            }
            group = group.child(choices);
        }
        for (name, enabled) in [
            ("Colour from album art", settings.accent_from_art),
            ("Compact library sidebar", settings.sidebar_compact),
            ("Compact track list", settings.tracklist_compact),
        ] {
            group = group.child(
                row().items_center().child(row_title(v, name)).child(
                    v.button(
                        &v.tr(if enabled { "On" } else { "Off" }),
                        enabled,
                        move |s, _| {
                            let x = &mut s.app.presentation.settings;
                            let b = match name {
                                "Colour from album art" => &mut x.accent_from_art,
                                "Compact library sidebar" => &mut x.sidebar_compact,
                                _ => &mut x.tracklist_compact,
                            };
                            *b = !*b;
                            s.app.mark_dirty();
                        },
                    )
                    .role("switch")
                    .label(v.tr(name))
                    .checked(enabled),
                ),
            );
        }
        let mut vis = row()
            .items_center()
            .child(row_title(v, "Player bar visualizer"));
        for (key, mode) in [
            ("Off", PlayerBarVis::Off),
            ("Spectrum", PlayerBarVis::Spectrum),
            ("Oscilloscope", PlayerBarVis::Waveform),
        ] {
            vis = vis.child(
                v.button(&v.tr(key), settings.player_bar_vis == mode, move |s, _| {
                    s.app.presentation.settings.player_bar_vis = mode;
                    s.app.mark_dirty();
                }),
            );
        }
        group = group.child(vis).child(
            row()
                .items_center()
                .gap(8.)
                .child(row_title(v, "Interface zoom"))
                .child(v.button("−", false, |s, _| zoom(s, -0.1)))
                .child(
                    v.button(&format!("{:.0}%", settings.zoom * 100.), false, |s, _| {
                        zoom(s, 0.)
                    }),
                )
                .child(v.button("+", false, |s, _| zoom(s, 0.1))),
        );
        page = page.child(group);
    }
    if matches("Equalizer Presets Preamp Frequency") {
        let mut eq = section(v, &v.tr("Equalizer")).child(
            row().items_center().child(row_title(v, "Equalizer")).child(
                v.button(
                    &v.tr(if settings.eq_on { "On" } else { "Off" }),
                    settings.eq_on,
                    |s, _| {
                        let on = !s.app.preferences.eq_on;
                        apply_eq(s, |e| e.on = on);
                    },
                )
                .role("switch")
                .label(v.tr("Equalizer"))
                .checked(settings.eq_on),
            ),
        );
        eq = eq.child(v.button(&v.tr("Preset"), false, |s, _| s.menu = Some("presets")));
        if v.s.menu == Some("presets") {
            let mut choices = row().flex_wrap();
            for preset in crate::eq::PRESETS {
                choices = choices.child(v.button(preset.name, false, move |s, _| {
                    apply_eq(s, |e| {
                        e.on = true;
                        e.bands_db = preset.bands_db;
                    });
                    s.menu = None;
                }));
            }
            eq = eq.child(choices);
        }
        for (i, name) in std::iter::once("Pre".to_string())
            .chain(crate::eq::BANDS.iter().map(|f| format!("{f} Hz")))
            .enumerate()
        {
            let val = if i == 0 {
                settings.eq_preamp_db
            } else {
                settings.eq_bands_db[i - 1]
            };
            eq = eq.child(
                row()
                    .gap(8.)
                    .items_center()
                    .child(label(&name, 12., 500., v.p.secondary).w(60.))
                    .child(transport::slider(
                        v,
                        &name,
                        f64::from(val),
                        [-12., 12.],
                        1.,
                        move |s, x| {
                            apply_eq(s, |e| {
                                if i == 0 {
                                    e.preamp_db = x as f32
                                } else {
                                    e.bands_db[i - 1] = x as f32
                                }
                            })
                        },
                    ))
                    .child(label(format!("{val:+.0} dB"), 12., 400., v.p.secondary)),
            );
        }
        page = page.child(eq);
    }
    if matches("About Keyboard shortcuts") {
        page = page.child(
            section(v, &v.tr("About"))
                .child(label(
                    concat!("Navifast ", env!("CARGO_PKG_VERSION")),
                    16.,
                    650.,
                    v.p.text,
                ))
                .child(v.button(&v.tr("Keyboard shortcuts"), false, |s, tx| {
                    s.dispatch(Action::ShowDialog(Dialog::Shortcuts), tx)
                })),
        );
    }
    page
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
    let mut d = section(v, &v.tr("Keyboard shortcuts"))
        .w(460.)
        .role("dialog")
        .label(v.tr("Keyboard shortcuts"));
    for (key, name) in crate::ui::keys::navidrome_shortcuts(v.s.app.presentation.locale) {
        d = d.child(
            row()
                .justify_between()
                .child(label(name.as_ref(), 14., 400., v.p.text))
                .child(label(key.as_ref(), 14., 600., v.p.secondary)),
        );
    }
    col()
        .absolute(0., 0.)
        .w_full()
        .h_full()
        .center()
        .fill(Color::BLACK.alpha(0.45))
        .child(d.child(v.button(&v.tr("Close"), false, |s, tx| {
            s.dispatch(Action::CloseDialog, tx)
        })))
}
