//! Navidrome authentication form in the existing native login style.
use crate::app::navidrome::{Action, App};
use crate::theme;
use egui::{Align, Frame, Layout, Margin, Vec2, vec2};

pub fn login(app: &App, ui: &mut egui::Ui) -> Vec<Action> {
    let mut actions = Vec::new();
    let palette = app.palette;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(palette.window))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            super::titlebar_drag(ui, egui::Rect::from_min_size(rect.min, vec2(rect.width(), theme::TOP_BAR_HEIGHT + theme::titlebar_inset(ui.ctx()))));
            let tint = super::blend(palette.window, palette.accent, 0.10);
            super::widgets::paint_vertical_gradient(ui, rect, tint, palette.window);
            let width = (ui.available_width() - 40.0).clamp(240.0, 430.0);
            let top = ((ui.available_height() - 430.0) * 0.35).max(12.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(top);
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::LEFT), |ui| {
                        Frame::new().fill(palette.panel)
                            .stroke(egui::Stroke::new(1.0, palette.outline))
                            .corner_radius(theme::RADIUS + 8).inner_margin(Margin::same(36))
                            .show(ui, |ui| {
                                ui.set_width((width - 72.0).max(180.0));
                                ui.vertical_centered(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), egui::Sense::hover());
                                    theme::logo(ui, rect.center(), 64.0);
                                    ui.add_space(12.0);
                                    theme::text(ui, "Navifast", theme::bold(34.0), palette.text);
                                });
                                ui.add_space(16.0);
                                theme::text(ui, "Connect Navifast to Navidrome.", theme::regular(14.0), palette.secondary);
                                ui.add_space(20.0);
                                let mut form = app.login.clone();
                                let mut changed = false;
                                let mut submit = false;
                                ui.add_enabled_ui(!app.connecting, |ui| {
                                    ui.label("Server URL");
                                    changed |= ui.add(egui::TextEdit::singleline(&mut form.server)
                                        .hint_text("https://music.example.com").desired_width(f32::INFINITY)).changed();
                                    ui.add_space(8.0);
                                    ui.label("Username");
                                    changed |= ui.add(egui::TextEdit::singleline(&mut form.username)
                                        .desired_width(f32::INFINITY)).changed();
                                    ui.add_space(8.0);
                                    ui.label("Password");
                                    let response = ui.add(egui::TextEdit::singleline(&mut form.password)
                                        .password(true).desired_width(f32::INFINITY));
                                    changed |= response.changed();
                                    submit = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                                });
                                if changed { actions.push(Action::EditLogin(form)); }
                                ui.add_space(16.0);
                                if let Some(error) = &app.error {
                                    ui.colored_label(palette.danger, error);
                                    ui.add_space(8.0);
                                }
                                if app.connecting {
                                    ui.horizontal(|ui| {
                                        theme::spinner(ui, 18.0, palette.accent);
                                        ui.label("Connecting…");
                                        if ui.button("Cancel").clicked() { actions.push(Action::Cancel); }
                                    });
                                } else if theme::pill_button(ui, &palette, "Connect", true).clicked() || submit {
                                    actions.push(Action::Login);
                                }
                                ui.add_space(12.0);
                                ui.add(egui::Label::new(egui::RichText::new("Your password stays in memory for this session. Use HTTPS for a remote server.")
                                    .size(12.0).color(palette.secondary)).wrap());
                            });
                    });
                });
            });
        });
    super::window_controls(ui, &palette, crate::i18n::Locale::English);
    super::window_resize(ui);
    actions
}
