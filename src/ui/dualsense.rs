//! DualSense mode: the switch, what the virtual pad is doing, and how to
//! get Steam to hand it to games.

use std::time::Duration;

use apex5::ds5;
use apex5::worker::Cmd;
use eframe::egui::{self, RichText, vec2};

use super::theme::{self, DIM, FAINT, TEXT, WARN, accent};
use super::widgets::{self, card, card_titled, hint, pill};
use crate::App;

/// Steam opens the Apex through its own driver; this keeps it off it, so
/// games see only the virtual DualSense.
const STEAM: &str = "env SDL_GAMECONTROLLER_IGNORE_DEVICES=0x37d7/0x2501 \
                     SDL_JOYSTICK_IGNORE_DEVICES=0x37d7/0x2501 steam";

/// The "Режим DualSense" page.
pub fn page(app: &mut App, ui: &mut egui::Ui) {
    super::header(
        app,
        ui,
        "Режим DualSense",
        "Игры с адаптивными курками управляют курками Apex",
    );
    let st = app.ds5();
    let ready = apex5::udev::uhid_ready();
    card(ui, |ui| {
        let mut on = st.on;
        let changed = ui
            .add_enabled_ui(ready || st.on, |ui| {
                widgets::toggle_row(
                    ui,
                    "Показывать играм DualSense",
                    "Нагрев, отдача и вибрация курков — от самой игры (например, No Man's Sky)",
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
                "Пока режим включён, закрытое окно уходит в трей: игры не теряют DualSense",
            );
        }
        widgets::separator(ui);
        resistance(app, ui);
        if !ready && !st.on {
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Нет доступа к /dev/uhid: нужно правило udev и модуль uhid. \
                     Кнопка ниже поставит и то и другое.",
                )
                .color(WARN),
            );
            ui.add_space(4.0);
            super::screen::rule_offer(app, ui);
        }
    });

    if st.on {
        ui.add_space(12.0);
        card_titled(ui, "Сейчас", |ui| {
            ui.horizontal_wrapped(|ui| {
                pill(
                    ui,
                    if st.opened {
                        "игра или Steam подключены"
                    } else {
                        "пока никто не подключился"
                    },
                    Some(if st.opened { accent() } else { FAINT }),
                );
                pill(
                    ui,
                    if st.pad {
                        "Apex передаёт ввод"
                    } else {
                        "Apex не на связи: кнопки отпущены"
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
                            None => ui.label(RichText::new("игра ничего не задавала").color(FAINT)),
                            Some((e, a)) => ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "{} → {}",
                                        ds5::describe(e),
                                        a.as_ref().map(ds5::describe_apex).unwrap_or_else(|| {
                                            "не переводится, курок как был".into()
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
                "Эффекты курков из профиля сейчас не действуют. Вибрация рукояток идёт от игры, \
                 если она включена в профиле.",
            );
        });
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }

    ui.add_space(12.0);
    card_titled(ui, "Запуск Steam", steam_hint);
}

/// How much of the games' resistance reaches the Apex, kept between runs.
fn resistance(app: &mut App, ui: &mut egui::Ui) {
    widgets::row(
        ui,
        "Сила сопротивления от игры",
        "Если курок приходится дожимать до конца (луч добычи в No Man's Sky), убавьте; \
         нарастание нагрева останется",
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
        "Steam сам открывает Apex и отдаёт играм его, а не DualSense. Перед игрой закройте Steam \
         и запустите его так (Steam Input у игры оставьте по умолчанию):",
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
    if widgets::secondary(ui, "Копировать команду", true).clicked() {
        ui.ctx().copy_text(STEAM.into());
    }
}
