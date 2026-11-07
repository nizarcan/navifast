use crate::color::Srgba;
use crate::i18n::mark;
use crate::{
    app::navidrome::{self, desktop::State},
    model::{Action, Page},
    navidrome::api::CoverSize,
    settings::ThemeChoice,
    theme::Palette,
};
use crate::{i18n::gettext, theme::Icon};
use rune::{Color, ScrollSpec, TextStyle, col, el, row, text, text_input};
use rune::{Cx, El, Tx};

mod albums;
pub(crate) mod artist;
mod backdrop;
mod browse;
mod home;
mod library;
mod lyrics;
pub(crate) mod lyrics_card;
mod playlists;
mod preferences;
mod queue;
mod ratings;
mod scrobble;
mod shares;
mod sleep;
pub(crate) mod spectrum;
mod stats;
mod transport;

fn color(c: Srgba) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied().map(|x| f32::from(x) / 255.);
    Color {
        r: rune::color::srgb_decode(r),
        g: rune::color::srgb_decode(g),
        b: rune::color::srgb_decode(b),
        a,
    }
}
fn label(value: impl Into<String>, size: f32, weight: f32, ink: Srgba) -> El {
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
        (self.width - sidebar - self.queue_width() - 48.).max(1.)
    }
    fn queue_width(&self) -> f32 {
        let settings = &self.s.app.presentation.settings;
        let sidebar = if settings.sidebar_visible {
            settings
                .sidebar_width
                .clamp(210., (self.width - 340.).max(210.))
        } else {
            0.
        };
        if (self.s.queue_open || self.s.app.lyrics.enabled)
            && self.width >= 1060.
            && self.width - sidebar >= 660.
        {
            320.
        } else {
            0.
        }
    }
    fn tr(&self, value: &'static str) -> String {
        gettext(self.s.app.presentation.locale, value).into_owned()
    }
    fn spinner(&self, size: f32, ink: Srgba) -> El {
        let points =
            crate::theme::spinner_points(size, 0.).map(|[x, y]| [size / 2. + x, size / 2. + y]);
        let arc = rune::Shape::new().stroke(points, 2., color(ink));
        rune::canvas(arc)
            .size(size, size)
            .looping(rune::LoopSpec::rotation(
                1. / crate::theme::SPINNER_TURNS_PER_SECOND,
            ))
            .a11y_hidden()
    }
    fn loading(&self) -> El {
        row()
            .pl(8.)
            .gap(8.)
            .items_center()
            .role("status")
            .child(self.spinner(18., self.p.accent))
            .child(label(
                self.tr(mark("Loading…")),
                13.,
                400.,
                self.p.secondary,
            ))
    }
    fn empty(&self, icon: Icon, title: &str, detail: &str) -> El {
        col()
            .w_full()
            .pt(48.)
            .items_center()
            .child(self.icon(icon, 40., self.p.dim))
            .child(label(title, 16., 600., self.p.text).mt(14.))
            .child(label(detail, 13.5, 400., self.p.secondary).mt(8.).wrap())
    }
    fn icon(&self, icon: Icon, size: f32, ink: Srgba) -> El {
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
    /// A search pill has a full-height hit area and one centered line of type.
    /// Giving the editor the height of the pill puts its glyphs at the top.
    fn search_field(
        &self,
        key: &'static str,
        hint: &str,
        edit: &rune::TextEdit,
        input: El,
        clear: impl Fn(&mut State, &mut Tx) + 'static,
    ) -> El {
        let input = input
            .single_line()
            .key(key)
            .label(hint)
            .w_full()
            .min_w(0.)
            .h(18.)
            .caret(
                color(self.p.accent),
                color(self.p.accent.gamma_multiply(0.28)),
            )
            .color(color(self.p.text));
        let mut editor = el().grow(1.).min_w(0.).h(18.);
        if edit.text.is_empty() {
            editor = editor.child(
                label(hint, 14., 400., self.p.secondary)
                    .absolute(0., 0.)
                    .w_full()
                    .pr(4.)
                    .truncate()
                    .a11y_hidden(),
            );
        }
        let mut pill = row()
            .h(34.)
            .pl(10.)
            .pr(30.)
            .gap(8.)
            .items_center()
            .radius(17.)
            .fill(color(if edit.focused {
                self.p.surface_hover
            } else {
                self.p.surface
            }))
            .child(self.icon(Icon::Search, 16., self.p.secondary))
            .child(editor.child(input));
        if edit.focused {
            pill = pill.border(1.5, color(self.p.text.gamma_multiply(0.6)));
        }
        if !edit.text.is_empty() {
            pill = pill.child(
                self.icon_button(&self.tr(mark("Clear")), Icon::X, 15., clear)
                    .key(format!("clear-{key}"))
                    .size(24., 24.)
                    .pin(None, Some(5.), Some(5.), None),
            );
        }
        pill
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
        ink: Srgba,
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
            tile = tile.pixels_fit(&p, rune::SurfaceFit::Fill)
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
        // The reference reserves the requested diameter but paints the four
        // extra backing pixels around the mark at their native scale.
        let side = pixels as f32 / (self.scale * self.zoom);
        let inset = (edge - side) / 2.;
        el().size(edge, edge)
            .child(el().absolute(inset, inset).size(side, side).pixels(logo))
            .a11y_hidden()
    }
    fn menu_item(
        &self,
        name: &str,
        icon: Option<Icon>,
        action: impl Fn(&mut State, &mut Tx) + 'static,
    ) -> El {
        row()
            .w_full()
            .h(30.)
            .px(8.)
            .gap(8.)
            .items_center()
            .radius(4.)
            .hover(color(self.p.surface_hover))
            .press(color(self.p.surface_active))
            .role("menuitem")
            .label(name)
            .focusable(true)
            .child(icon.map_or_else(
                || el().size(16., 16.),
                |icon| self.icon(icon, 16., self.p.secondary),
            ))
            .child(label(name, 13.5, 400., self.p.text))
            .on_press_tx(action)
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
            .focus_return(trigger)
            .anchored_fit(trigger, rune::Side::Bottom, rune::Align::End, 4.)
            .modal(true, |s: &mut State| s.menu = None)
    }
    fn signin(&self) -> El {
        let app = &self.s.app;
        let width = (self.width - 40.).clamp(240., 430.);
        let mut form = col()
            .w(width + 2.)
            .offset(1., 0.)
            .padding(37.)
            .radius(16.)
            .border(1., color(self.p.outline))
            .fill(color(self.p.panel))
            .child(
                col()
                    .w_full()
                    .items_center()
                    .gap(16.)
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
                .mt(22.),
            );
        for (key, name, field) in [
            ("server", "Server URL", &self.s.server),
            ("user", mark("Username"), &self.s.user),
            ("password", mark("Password"), &self.s.password),
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
                    25.5
                } else {
                    13.5
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
                .child(self.spinner(18., self.p.accent))
                .child(label(mark("Connecting…"), 14., 400., self.p.text))
                .child(self.button(mark("Cancel"), false, |s, _| {
                    s.app.apply(navidrome::Action::Cancel)
                }))
        } else {
            row().child(self.button("Connect", true, |s, _| s.connect()))
        };
        form=form.child(buttons.mt(22.)).child(label("Your login is saved in the system credential store. Use HTTPS for a remote server.",12.,400.,self.p.secondary).wrap().mt(16.));
        let top = ((self.height - 430.) * 0.35).max(12.) + 14.;
        let scroll = col()
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
            .child(form);
        col()
            .w_full()
            .h_full()
            .child(
                rune::effect(backdrop::MARK)
                    .absolute(0., 0.)
                    .w_full()
                    .h_full()
                    .a11y_hidden(),
            )
            .child(scroll)
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
    if k.key == Key::Escape && s.autoscroll.cancel() {
        return true;
    }
    if !s.app.connected {
        return false;
    }
    if s.lyrics_share.draft.is_some() && k.key == Key::Escape {
        if s.lyrics_share
            .draft
            .as_ref()
            .is_some_and(|d| d.preview_open)
        {
            s.edit_lyric_selection();
        } else {
            s.cancel_lyric_selection();
        }
        return true;
    }
    let editing = matches!(
        tx.focused(),
        Some(
            "query"
                | "filter"
                | "library-filter"
                | "server"
                | "user"
                | "password"
                | "playlist-name"
        )
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
        Key::Escape if s.queue_menu.is_some() => {
            s.queue_menu = None;
            return true;
        }
        Key::Escape if s.app.lyrics.enabled => {
            s.toggle_lyrics(tx);
            return true;
        }
        Key::Escape if s.queue_open => {
            s.toggle_queue(tx);
            return true;
        }
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
        _ if tx.focused().is_none() && s.queue_open && s.queue_selected.is_some() => {
            return queue::keys(s, k, tx);
        }
        _ if tx.focused().is_none()
            && s.menu.is_none()
            && s.app.presentation.dialog.is_none()
            && matches!(
                s.app.presentation.page(),
                Page::Album(_) | Page::Playlist(_) | Page::Songs | Page::Genre(_)
            )
            && s.table.cursor.is_some() =>
        {
            return library::track_key(s, k, tx);
        }
        _ => return false,
    };
    s.dispatch(action, tx);
    true
}
