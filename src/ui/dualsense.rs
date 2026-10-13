//! DualSense mode: the switch, what the virtual pad is doing, and how to
//! get Steam to hand it to games.

use std::time::Duration;

use apex5::ds5;
use apex5::lang::t;
use apex5::worker::Cmd;
use eframe::egui::{self, RichText, vec2};

use super::theme::{self, DIM, FAINT, TEXT, WARN, accent};
use super::widgets::{self, card, card_titled, hint, pill};
use crate::App;

/// Steam opens the Apex through its own driver; this keeps it off it, so
/// games see only the virtual DualSense.
const STEAM: &str = "env SDL_GAMECONTROLLER_IGNORE_DEVICES=0x37d7/0x2501 \
                     SDL_JOYSTICK_IGNORE_DEVICES=0x37d7/0x2501 steam";

/// The "DualSense mode" page.
pub fn page(app: &mut App, ui: &mut egui::Ui) {
    super::header(
        app,
        ui,
        t("DualSense mode"),
        t("Games with adaptive triggers drive the Apex triggers"),
    );
    let st = app.ds5();
    let ready = apex5::udev::uhid_ready();
    card(ui, |ui| {
        let mut on = st.on;
        let changed = ui
            .add_enabled_ui(ready || st.on, |ui| {
                widgets::toggle_row(
                    ui,
                    t("Show games a DualSense"),
                    t("Trigger build-up, kick and vibration come from the game itself (e.g. No Man's Sky)"),
                    &mut on,
                )
            })
            .inner;
        if changed {
            app.send(Cmd::DualSense(on));
        }
        if st.on {
            hint(
                ui,
                t(
                    "While the mode is on, closing the window sends it to the tray: games keep the DualSense",
                ),
            );
        }
        widgets::separator(ui);
        resistance(app, ui);
        if !ready && !st.on {
            ui.add_space(4.0);
            ui.label(
                RichText::new(t(
                    "No access to /dev/uhid: it needs a udev rule and the uhid module. \
                     The button below sets up both.",
                ))
                .color(WARN),
            );
            ui.add_space(4.0);
            super::screen::rule_offer(app, ui);
        }
    });

    if st.on {
        ui.add_space(12.0);
        card_titled(ui, t("Now"), |ui| {
            ui.horizontal_wrapped(|ui| {
                pill(
                    ui,
                    if st.opened {
                        t("a game or Steam is connected")
                    } else {
                        t("nobody has connected yet")
                    },
                    Some(if st.opened { accent() } else { FAINT }),
                );
                pill(
                    ui,
                    if st.pad {
                        t("Apex passes input")
                    } else {
                        t("Apex not responding: buttons released")
                    },
                    Some(if st.pad { accent() } else { WARN }),
                );
            });
            ui.add_space(8.0);
            egui::Grid::new("ds5_effects")
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    for (side, name) in [(0, "LT"), (1, "RT")] {
                        ui.label(RichText::new(name).color(DIM));
                        match &st.effects[side] {
                            None => ui.label(RichText::new(t("the game set nothing")).color(FAINT)),
                            Some((e, a)) => ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "{} → {}",
                                        ds5::describe(e),
                                        a.as_ref().map(ds5::describe_apex).unwrap_or_else(|| {
                                            t("not translated, the trigger stays as it was").into()
                                        })
                                    ))
                                    .color(TEXT),
                                )
                                .wrap(),
                            ),
                        };
                        ui.end_row();
                    }
                });
            ui.add_space(6.0);
            hint(
                ui,
                t(
                    "The profile's trigger effects are off for now. Grip rumble comes from the game \
                     if it's on in the profile.",
                ),
            );
        });
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    ui.add_space(12.0);
    card_titled(ui, t("Starting Steam"), steam_hint);
}

/// How much of the games' resistance reaches the Apex, kept between runs.
fn resistance(app: &mut App, ui: &mut egui::Ui) {
    widgets::row(
        ui,
        t("Game resistance strength"),
        t(
            "If a trigger has to be pushed all the way (the mining beam in No Man's Sky), turn it \
             down; the build-up stays",
        ),
        |ui| {
            // left to right inside the row's right-aligned control
            let r = ui
                .allocate_ui_with_layout(
                    vec2(250.0, 30.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().slider_width = 170.0;
                        ui.add(
                            widgets::slider(&mut app.ds5_resistance, 0..=100)
                                .suffix("%")
                                .step_by(5.0),
                        )
                    },
                )
                .inner;
            // on release, not every frame of a drag
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                app.send(Cmd::DualSenseResistance(app.ds5_resistance));
            }
            r.changed()
        },
    );
}

fn steam_hint(ui: &mut egui::Ui) {
    hint(
        ui,
        t(
            "Steam opens the Apex itself and gives games it, not the DualSense. Before playing, close \
             Steam and start it like this (leave the game's Steam Input at its default):",
        ),
    );
    ui.add_space(6.0);
    egui::Frame::new()
        .fill(theme::BG)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(RichText::new(STEAM).monospace().size(12.0).color(TEXT)).wrap(),
            );
        });
    ui.add_space(6.0);
    if widgets::secondary(ui, t("Copy command"), true).clicked() {
        ui.ctx().copy_text(STEAM.into());
    }
}
