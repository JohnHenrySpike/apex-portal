//! DualSense mode: the switch, what the virtual pad is doing, and how to
//! get Steam to hand it to games.

use std::time::Duration;

use apex5::ds5;
use apex5::worker::Cmd;
use eframe::egui::{self, RichText};

use super::theme::{DIM, FAINT, TEXT, WARN, accent};
use super::widgets::{self, card_titled, hint, pill};
use crate::App;

/// Steam opens the Apex through its own driver; this keeps it off it, so
/// games see only the virtual DualSense.
const STEAM: &str = "env SDL_GAMECONTROLLER_IGNORE_DEVICES=0x37d7/0x2501 \
                     SDL_JOYSTICK_IGNORE_DEVICES=0x37d7/0x2501 steam";

pub fn card(app: &mut App, ui: &mut egui::Ui) {
    let st = app.ds5();
    card_titled(ui, "Режим DualSense", |ui| {
        let ready = apex5::udev::uhid_ready();
        let mut on = st.on;
        let changed = ui
            .add_enabled_ui(ready || st.on, |ui| {
                widgets::toggle_row(
                    ui,
                    "Показывать играм DualSense",
                    "Игры, которые умеют адаптивные курки DualSense (например, No Man's Sky), \
                     управляют курками Apex сами: нагрев, отдача, вибрация",
                    &mut on,
                )
            })
            .inner;
        if changed {
            app.send(Cmd::DualSense(on));
        }
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
        if !st.on {
            ui.add_space(6.0);
            steam_hint(ui);
            return;
        }

        ui.add_space(8.0);
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
                        Some((e, a)) => ui.label(
                            RichText::new(format!(
                                "{} → {}",
                                ds5::describe(e),
                                a.as_ref()
                                    .map(ds5::describe_apex)
                                    .unwrap_or_else(|| "не переводится, курок как был".into())
                            ))
                            .color(TEXT),
                        ),
                    };
                    ui.end_row();
                }
            });
        ui.add_space(6.0);
        hint(
            ui,
            "Пока режим включён, эффекты курков из профиля не действуют: курками управляет игра. \
             Вибрация рукояток идёт от игры, если она включена в профиле.",
        );
        ui.add_space(6.0);
        steam_hint(ui);
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    });
}

fn steam_hint(ui: &mut egui::Ui) {
    hint(
        ui,
        "Steam сам открывает Apex и отдаёт играм его, а не DualSense. Перед игрой закройте Steam \
         и запустите его так (Steam Input у игры оставьте по умолчанию):",
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(STEAM).monospace().size(12.0).color(TEXT)).wrap());
        if widgets::secondary(ui, "Копировать", true).clicked() {
            ui.ctx().copy_text(STEAM.into());
        }
    });
}
