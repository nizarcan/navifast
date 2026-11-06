use crate::{
    app::navidrome::{self, desktop::State},
    model::{Action, Page},
    navidrome::api::CoverSize,
    settings::ThemeChoice,
    theme::Palette,
};
use crate::{i18n::gettext, theme::Icon};
use egui::Color32;
use rune::{Color, ScrollSpec, TextStyle, col, el, row, text, text_input};
use rune::{Cx, El, Tx};

pub(crate) mod artist;
mod library;
mod preferences;
pub(crate) mod spectrum;
mod transport;

fn color(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied().map(|x| f32::from(x) / 255.);
    Color {
        r: rune::color::srgb_decode(r),
        g: rune::color::srgb_decode(g),
        b: rune::color::srgb_decode(b),
        a,
    }
}
fn label(value: impl Into<String>, size: f32, weight: f32, ink: Color32) -> El {
    text(value.into())
        .font(TextStyle::new(size, weight))
        .color(color(ink))
}

struct Paint<'a> {
    s: &'a State,
    p: Palette,
    scale: f32,
    zoom: f32,
    width: f32,
    height: f32,
}

impl Paint<'_> {
    fn content_width(&self) -> f32 {
        let settings = &self.s.app.presentation.settings;
        let sidebar = if settings.sidebar_visible {
            settings
                .sidebar_width
                .clamp(210., (self.width - 340.).max(210.))
        } else {
            0.
        };
        (self.width - sidebar - 48.).max(1.)
    }
    fn tr(&self, value: &'static str) -> String {
        gettext(self.s.app.presentation.locale, value).into_owned()
    }
    fn icon(&self, icon: Icon, size: f32, ink: Color32) -> El {
        let px = (size * self.scale * self.zoom).round().max(1.) as u32;
        let mut cache = self.s.icons.borrow_mut();
        let img = cache.entry((icon, ink, px)).or_insert_with(|| {
            let [r, g, b, _] = ink.to_srgba_unmultiplied();
            let tint = format!("#{r:02x}{g:02x}{b:02x}");
            let svg = std::str::from_utf8(icon.bytes())
                .expect("bundled SVG")
                .replace("#ffffff", &tint)
                .replace("currentColor", &tint);
            self.s.images.svg(svg.into_bytes(), px as f32, 1.)
        });
        el().size(size, size).pixels(img.pixels()).a11y_hidden()
    }
    fn icon_button(
        &self,
        name: &str,
        icon: Icon,
        size: f32,
        action: impl Fn(&mut State, &mut Tx) + 'static,
    ) -> El {
        self.tinted_button(name, icon, size, self.p.secondary, action)
    }
    fn tinted_button(
        &self,
        name: &str,
        icon: Icon,
        size: f32,
        ink: Color32,
        action: impl Fn(&mut State, &mut Tx) + 'static,
    ) -> El {
        el().size(32., 32.)
            .center()
            .radius(16.)
            .hover(color(self.p.surface_hover))
            .press(color(self.p.surface_active))
            .role("button")
            .label(name)
            .focusable(true)
            .child(self.icon(icon, size, ink))
            .on_press_tx(action)
    }
    fn button(
        &self,
        name: &str,
        primary: bool,
        action: impl Fn(&mut State, &mut Tx) + 'static,
    ) -> El {
        el().h(32.)
            .px(18.)
            .center()
            .radius(16.)
            .fill(color(if primary {
                self.p.accent
            } else {
                self.p.surface
            }))
            .hover(color(if primary {
                self.p.accent_hover
            } else {
                self.p.surface_hover
            }))
            .role("button")
            .label(name)
            .focusable(true)
            .child(label(
                name,
                13.,
                600.,
                if primary {
                    self.p.on_accent
                } else {
                    self.p.text
                },
            ))
            .on_press_tx(action)
    }
    fn cover(&self, id: Option<&str>, size: CoverSize, edge: f32, radius: f32) -> El {
        let mut tile = el()
            .size(edge, edge)
            .center()
            .radius(radius)
            .clip_rounded(radius)
            .fill(color(self.p.surface));
        let Some(id) = id else {
            return tile.child(self.icon(Icon::Music, edge * 0.35, self.p.dim));
        };
        let (view_key, pixels) = self.s.cover_image(id, size);
        if let Some(p) = pixels {
            tile = tile.pixels(&p)
        } else {
            tile = tile.child(self.icon(Icon::Music, edge * 0.35, self.p.dim));
        }
        let id = id.to_owned();
        tile.key(view_key).on_visible(move |s: &mut State, on, tx| {
            if on {
                s.request_cover(&id, size);
            }
            tx.live_only();
        })
    }
    fn logo(&self, edge: f32) -> El {
        let pixels = (edge * self.scale * self.zoom).round() as u32 + 4;
        let mut logos = self.s.logos.borrow_mut();
        let logo = logos.entry(pixels).or_insert_with(|| {
            let p = rune::Pixels::new();
            p.set(pixels, pixels, crate::util::app_icon_rgba(pixels as usize));
            p
        });
        el().size(edge, edge).pixels(logo).a11y_hidden()
    }
    fn menu(&self, trigger: &'static str) -> El {
        col()
            .key(format!("{trigger}-menu"))
            .role("menu")
            .w(208.)
            .min_h(0.)
            .padding(6.)
            .gap(2.)
            .radius(8.)
            .fill(color(self.p.overlay))
            .border(1., color(self.p.outline))
            .anchored_fit(trigger, rune::Side::Bottom, rune::Align::End, 4.)
            .modal(true, |s: &mut State| s.menu = None)
    }
    fn signin(&self) -> El {
        let app = &self.s.app;
        let width = (self.width - 40.).clamp(240., 430.);
        let mut form = col()
            .w(width)
            .padding(37.)
            .radius(16.)
            .border(1., color(self.p.outline))
            .fill(color(self.p.panel))
            .child(
                col()
                    .w_full()
                    .items_center()
                    .gap(14.)
                    .child(self.logo(64.))
                    .child(label("Navifast", 34., 700., self.p.text)),
            )
            .child(
                label(
                    "Connect Navifast to Navidrome.",
                    14.,
                    400.,
                    self.p.secondary,
                )
                .mt(20.),
            );
        for (key, name, field) in [
            ("server", "Server URL", &self.s.server),
            ("user", "Username", &self.s.user),
            ("password", "Password", &self.s.password),
        ] {
            let mut input = text_input(field, TextStyle::new(14., 400.))
                .single_line()
                .key(key)
                .label(name)
                .h(21.)
                .w_full()
                .px(4.)
                .radius(6.)
                .fill(color(self.p.surface))
                .color(color(self.p.text))
                .disabled(app.connecting)
                .on_edit(move |s: &mut State| match key {
                    "server" => &mut s.server,
                    "user" => &mut s.user,
                    _ => &mut s.password,
                });
            if key == "server" {
                input = input.placeholder("https://music.example.com", color(self.p.dim));
            }
            if key == "password" {
                input = input.secure().on_submit(|s: &mut State, _| s.connect());
            } else {
                input = input.on_input_change(|s: &mut State, _| {
                    s.password.clear();
                    s.app.login.password.clear();
                });
            }
            form = form
                .child(label(name, 14., 400., self.p.text).mt(if key == "server" {
                    24.
                } else {
                    14.
                }))
                .child(input.mt(6.));
        }
        if let Some(error) = &app.error {
            form = form.child(label(error, 14., 400., self.p.danger).mt(14.).wrap())
        }
        let buttons = if app.connecting {
            row()
                .items_center()
                .gap(10.)
                .child(label("Connecting…", 14., 400., self.p.text))
                .child(self.button("Cancel", false, |s, _| {
                    s.app.apply(navidrome::Action::Cancel)
                }))
        } else {
            row().child(self.button("Connect", true, |s, _| s.connect()))
        };
        form=form.child(buttons.mt(22.)).child(label("Your login is saved in the system credential store. Use HTTPS for a remote server.",12.,400.,self.p.secondary).wrap().mt(16.));
        let top = ((self.height - 430.) * 0.35).max(12.) + 14.;
        col()
            .key("login")
            .window_drag()
            .w_full()
            .h_full()
            .items_center()
            .pt(top)
            .pb(16.)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .gradient(
                color(crate::ui::blend(self.p.window, self.p.accent, 0.1)),
                color(self.p.window),
            )
            .child(form)
    }
}

impl State {
    fn connect(&mut self) {
        if !self.app.connecting {
            self.app.login = navidrome::Login {
                server: self.server.text.clone(),
                username: self.user.text.clone(),
                password: self.password.text.clone(),
            };
            self.app.apply(navidrome::Action::Login);
        }
    }
}

pub(crate) fn view(s: &State, cx: &mut Cx<State>) -> El {
    let [width, height] = cx.layout_size("window").unwrap_or([1100., 760.]);
    let zoom = s.app.presentation.settings.zoom.clamp(0.5, 2.5);
    let p = Paint {
        s,
        p: s.palette(cx.system_dark()),
        scale: cx.env().scale,
        zoom,
        width: width / zoom,
        height: height / zoom,
    };
    let screen = if s.app.connected || s.app.restoring_login {
        library::shell(&p)
    } else {
        p.signin()
    };
    col()
        .key("window")
        .on_layout(|s: &mut State, r| s.resized(r[2], r[3]))
        .role("window")
        .w_full()
        .h_full()
        .fill(color(p.p.window))
        .child(
            col()
                .w(width / zoom)
                .h(height / zoom)
                .scale(zoom)
                .origin(0., 0.)
                .child(screen),
        )
}

pub(crate) fn keys(s: &mut State, k: &rune::KeyEvent, tx: &mut Tx) -> bool {
    use rune::Key;
    if !s.app.connected {
        return false;
    }
    let editing = matches!(
        tx.focused(),
        Some("query" | "filter" | "library-filter" | "server" | "user" | "password")
    );
    let action = match &k.key {
        Key::Command('+' | '=') => {
            preferences::zoom(s, 0.1);
            return true;
        }
        Key::Command('-') => {
            preferences::zoom(s, -0.1);
            return true;
        }
        Key::Command('0') => {
            preferences::zoom(s, 0.);
            return true;
        }
        Key::Command(',') => Action::Open(Page::Settings),
        Key::Command('b') if k.mods.shift => {
            let Some(album) = &s.app.playing_album else {
                return false;
            };
            Action::Open(Page::Album(navidrome::encode(&album.id)))
        }
        Key::Command('a') if k.mods.shift => {
            let Some(id) = s
                .app
                .transport
                .song()
                .and_then(|t| t.credits.artist_id.as_deref())
            else {
                return false;
            };
            Action::Open(Page::Artist(navidrome::encode(id)))
        }
        Key::Command('h') if k.mods.shift == cfg!(target_os = "macos") => {
            Action::Open(Page::Albums)
        }
        Key::Command('l') => Action::Open(Page::LikedSongs),
        Key::Command('b') if !editing => Action::ToggleSidebar,
        Key::Command('f') => {
            s.query.focused = true;
            Action::FocusSearch
        }
        Key::Command('/') => Action::ShowDialog(crate::model::Dialog::Shortcuts),
        Key::Text(v) if !editing && v == "/" => {
            s.query.focused = true;
            Action::FocusSearch
        }
        Key::Text(v) if !editing && v == "?" => Action::ShowDialog(crate::model::Dialog::Shortcuts),
        Key::Left if !editing && k.mods.cmd => Action::Previous,
        Key::Right if !editing && k.mods.cmd => Action::Next,
        Key::Up if !editing && k.mods.cmd => Action::VolumeBy(5),
        Key::Down if !editing && k.mods.cmd => Action::VolumeBy(-5),
        Key::Escape if s.app.presentation.dialog.is_some() => Action::CloseDialog,
        Key::Escape if s.menu.is_some() => {
            s.menu = None;
            return true;
        }
        Key::Command('w') => Action::CloseWindow,
        Key::Command('q') => Action::Quit,
        Key::Left if !editing && k.mods.alt => Action::Back,
        Key::Right if !editing && k.mods.alt => Action::Forward,
        Key::Left if !editing && k.mods.shift => Action::SeekBy(-10000),
        Key::Right if !editing && k.mods.shift => Action::SeekBy(10000),
        Key::Text(v) if !editing && v == " " => Action::TogglePlay,
        Key::Text(v) if !editing && v.eq_ignore_ascii_case("m") => Action::ToggleMute,
        _ => return false,
    };
    s.dispatch(action, tx);
    true
}
