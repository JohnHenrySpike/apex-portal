//! Window layout, overview and device settings.

use std::time::{Duration, Instant};

use apex5::device::{self, NICKNAME_MAX};
use apex5::worker::Cmd;
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, pos2, vec2};

use crate::{App, Tab};

pub mod copies;
mod input;
pub mod lighting;
mod pad;
mod profile;
pub mod theme;
mod widgets;

use theme::{BAD, BORDER, DIM, FAINT, OK, SIDEBAR, TEXT, WARN, accent};
use widgets::{Icon, card, card_titled, hint, page_title, pill, row, toggle_row};

const NAV: [(Tab, Icon, &str); 5] = [
    (Tab::Overview, Icon::Home, "Геймпад"),
    (Tab::Lighting, Icon::Light, "Подсветка"),
    (Tab::Profile, Icon::Profiles, "Профили"),
    (Tab::Settings, Icon::Settings, "Настройки"),
    (Tab::Input, Icon::Input, "Тест ввода"),
];

pub fn shell(app: &mut App, ui: &mut egui::Ui) {
    sidebar(app, ui);
    if app.tab == Tab::Profile && app.info.is_some() {
        profile::action_bar(app, ui);
    }
    toast(app, ui);

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(theme::BG)
                .inner_margin(egui::Margin {
                    left: 28,
                    right: 18,
                    top: 22,
                    bottom: 0,
                }),
        )
        .show(ui, |ui| {
            if app.info.is_none() {
                disconnected(app, ui);
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.set_max_width((ui.available_width() - 14.0).min(1100.0));
                    match app.tab {
                        Tab::Overview => overview(app, ui),
                        Tab::Lighting => lighting::show(app, ui),
                        Tab::Profile => profile::show(app, ui),
                        Tab::Settings => settings(app, ui),
                        Tab::Input => input::show(app, ui),
                    }
                    ui.add_space(24.0);
                });
        });
}

fn sidebar(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::left("nav")
        .resizable(false)
        .exact_size(232.0)
        .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin::symmetric(16, 20)))
        .show(ui, |ui| {
            // Wordmark.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new("APEX").font(theme::bold(24.0)).color(theme::SILVER));
                ui.label(RichText::new("5").font(theme::bold(24.0)).color(accent()));
            });
            ui.label(RichText::new("C O N T R O L").font(theme::medium(10.5)).color(FAINT));
            ui.add_space(18.0);

            device_badge(app, ui);
            ui.add_space(18.0);

            for (tab, icon, label) in NAV {
                nav_item(app, ui, tab, icon, label);
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    widgets::icon(ui.painter(), rect, Icon::Shield, FAINT);
                    ui.label(RichText::new("Вибрация заблокирована").size(12.0).color(FAINT));
                })
                .response
                .on_hover_text(
                    "Приложение никогда не отправляет команды вибрации и вибрационные режимы курков. \
                     Опасные команды (прошивка, сброс, смена режима) тоже заблокированы.",
                );
            });
        });
}

fn device_badge(app: &App, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            match &app.info {
                Some(i) => {
                    ui.horizontal(|ui| {
                        status_dot(ui, OK);
                        ui.label(
                            RichText::new(format!("Flydigi {}", i.model()))
                                .font(theme::medium(14.0))
                                .color(TEXT),
                        );
                    });
                    ui.horizontal(|ui| {
                        widgets::battery(ui, i.battery_level, i.charging);
                        let b = format!("{}%", i.battery_level as u32 * 20);
                        ui.label(RichText::new(b).size(12.5).color(DIM));
                        ui.label(RichText::new("·").color(FAINT));
                        ui.label(
                            RichText::new(if i.wired {
                                "кабель"
                            } else {
                                "донгл"
                            })
                            .size(12.5)
                            .color(DIM),
                        );
                    });
                    if let Some(p) = &app.pad_profile {
                        ui.label(
                            RichText::new(format!("Профиль {} · {}", app.active + 1, p.title()))
                                .size(12.5)
                                .color(DIM),
                        );
                    }
                }
                None => {
                    ui.horizontal(|ui| {
                        status_dot(ui, BAD);
                        ui.label(
                            RichText::new("Не подключён")
                                .font(theme::medium(14.0))
                                .color(TEXT),
                        );
                    });
                    ui.label(RichText::new("ожидание геймпада…").size(12.5).color(DIM));
                }
            }
        });
}

fn status_dot(ui: &mut egui::Ui, c: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 16.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 6.0, c.gamma_multiply(0.2));
    ui.painter().circle_filled(rect.center(), 3.5, c);
}

fn nav_item(app: &mut App, ui: &mut egui::Ui, tab: Tab, icon: Icon, label: &str) {
    let sel = app.tab == tab;
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), egui::Sense::click());
    let t = ui.ctx().animate_bool_responsive(r.id, sel);
    let p = ui.painter();
    if sel || r.hovered() {
        let bg = if sel { theme::CARD_HI } else { theme::CARD };
        p.rect_filled(rect, CornerRadius::same(10), bg);
    }
    if t > 0.0 {
        let bar = egui::Rect::from_min_size(
            pos2(rect.left(), rect.center().y - 10.0 * t),
            vec2(3.0, 20.0 * t),
        );
        p.rect_filled(bar, CornerRadius::same(2), accent());
    }
    let colour = if sel {
        accent()
    } else if r.hovered() {
        TEXT
    } else {
        DIM
    };
    let ir =
        egui::Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(18.0, 18.0));
    widgets::icon(p, ir, icon, colour);
    p.text(
        pos2(rect.left() + 48.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        theme::medium(14.5),
        if sel { TEXT } else { colour },
    );
    if r.clicked() {
        app.tab = tab;
    }
    ui.add_space(2.0);
}

/// Page header: title on the left, refresh and activity on the right.
pub fn header(app: &App, ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            page_title(ui, title, subtitle);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::icon_button(ui, Icon::Refresh, "Перечитать всё с геймпада").clicked()
            {
                app.send(Cmd::Refresh);
            }
            if app.busy {
                ui.add(egui::Spinner::new().size(18.0).color(accent()));
            }
        });
    });
    ui.add_space(14.0);
}

fn toast(app: &App, ui: &mut egui::Ui) {
    let Some((msg, err, t)) = &app.toast else {
        return;
    };
    if t.elapsed() > Duration::from_secs(5) {
        return;
    }
    let colour = if *err { BAD } else { OK };
    egui::Area::new(egui::Id::new("toast"))
        .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-24.0, -24.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(theme::CARD_HI)
                .stroke(Stroke::new(1.0, colour.gamma_multiply(0.6)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(16, 12))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(150),
                })
                .show(ui, |ui| {
                    ui.set_max_width(380.0);
                    ui.horizontal(|ui| {
                        status_dot(ui, colour);
                        ui.label(RichText::new(msg).color(TEXT));
                    });
                });
        });
    ui.ctx().request_repaint_after(Duration::from_millis(250));
}

fn disconnected(app: &App, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        let w = ui.available_width().min(560.0);
        pad::show(
            ui,
            vec2(w, w * 0.62),
            &pad::Opts {
                brightness: 0.0,
                ..Default::default()
            },
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("Геймпад не найден")
                .font(theme::bold(24.0))
                .color(TEXT),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Подключите Apex 5 кабелем или через донгл — приложение найдёт его само.",
            )
            .color(DIM),
        );
        ui.label(RichText::new(&app.status).size(12.5).color(FAINT));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Если геймпад подключён, но нет доступа, установите правило udev (см. README).",
            )
            .size(12.5)
            .color(FAINT),
        );
        ui.add_space(12.0);
        ui.add(egui::Spinner::new().size(22.0).color(accent()));
    });
}

// ------------------------------------------------------------------ overview

fn overview(app: &mut App, ui: &mut egui::Ui) {
    let Some(info) = app.info.clone() else { return };
    header(
        app,
        ui,
        &format!("Flydigi {}", info.model()),
        "Состояние геймпада и быстрые действия",
    );

    let wide = ui.available_width() > 860.0;
    let hero = |app: &mut App, ui: &mut egui::Ui| {
        card(ui, |ui| {
            let w = ui.available_width();
            let leds = app
                .led
                .as_ref()
                .map(|l| lighting::current_frame(&l.cfg, ui.input(|i| i.time)));
            let bright = app
                .led
                .as_ref()
                .map(|l| l.cfg.brightness() as f32 / 100.0)
                .unwrap_or(0.0);
            let screen = format!("P{}", app.active + 1);
            pad::show(
                ui,
                vec2(w, (w * 0.66).min(440.0)),
                &pad::Opts {
                    leds: leds.as_deref(),
                    brightness: bright,
                    screen: Some(&screen),
                    ..Default::default()
                },
            );
            if app.led.as_ref().is_some_and(|l| l.effect.animated()) {
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        });
    };

    if wide {
        widgets::split(ui, 0.6, app, hero, |app, ui| status_cards(app, ui, &info));
    } else {
        hero(app, ui);
        ui.add_space(12.0);
        status_cards(app, ui, &info);
    }

    ui.add_space(12.0);
    card_titled(ui, "Профиль", |ui| {
        hint(
            ui,
            "Переключение профиля сразу меняет его на геймпаде. Название известно только у активного.",
        );
        ui.add_space(6.0);
        let busy = app.busy;
        ui.horizontal(|ui| {
            let w = ((ui.available_width() - 30.0) / 4.0).max(90.0);
            for slot in 0..apex5::profile::SLOTS {
                let active = slot == app.active;
                let sub = if active {
                    app.pad_profile
                        .as_ref()
                        .map(|p| p.title())
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                let r = widgets::tile(
                    ui,
                    vec2(w, 58.0),
                    active,
                    &format!("Профиль {}", slot + 1),
                    &sub,
                );
                if r.clicked() && !active && !busy {
                    switch_profile(app, slot);
                }
            }
        });
    });
}

pub fn switch_profile(app: &mut App, slot: u8) {
    if app.profile_dirty() {
        app.toast = Some((
            "Сначала примените или отмените изменения профиля".into(),
            true,
            std::time::Instant::now(),
        ));
    } else {
        app.send(Cmd::SwitchProfile(slot));
    }
}

fn status_cards(app: &mut App, ui: &mut egui::Ui, info: &device::Info) {
    card_titled(ui, "Питание и связь", |ui| {
        ui.horizontal(|ui| {
            widgets::battery(ui, info.battery_level, info.charging);
            ui.label(
                RichText::new(format!("{}%", info.battery_level as u32 * 20))
                    .font(theme::bold(20.0))
                    .color(TEXT),
            );
            if info.charging {
                pill(ui, "заряжается", Some(accent()));
            }
        });
        ui.horizontal(|ui| {
            pill(
                ui,
                if info.wired {
                    "USB-кабель"
                } else {
                    "Беспроводной донгл"
                },
                Some(OK),
            );
        });
    });
    ui.add_space(12.0);
    card_titled(ui, "Подсветка", |ui| {
        if let Some(led) = app.led.as_mut() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(lighting::effect_name(led.effect))
                        .font(theme::medium(14.5))
                        .color(TEXT),
                );
                for c in led.colours.iter().take(5) {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    ui.painter().circle(
                        rect.center(),
                        7.0,
                        Color32::from_rgb(c[0], c[1], c[2]),
                        Stroke::new(1.0, BORDER),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Яркость").color(DIM));
                let mut b = led.cfg.brightness();
                ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(80.0);
                if ui
                    .add(egui::Slider::new(&mut b, 0..=100).suffix("%"))
                    .changed()
                {
                    led.cfg.set_brightness(b);
                    led.dirty_since = Some(std::time::Instant::now());
                }
            });
        }
        if ui.link("Настроить подсветку →").clicked() {
            app.tab = Tab::Lighting;
        }
    });
    ui.add_space(12.0);
    card_titled(ui, "Вибрация от игр", |ui| {
        if let Some(p) = &app.pad_profile {
            let g = p.grip_vibration();
            let on = g.master && (g.left || g.right);
            ui.horizontal(|ui| {
                pill(
                    ui,
                    if on {
                        "включена в профиле"
                    } else {
                        "выключена в профиле"
                    },
                    Some(if on { WARN } else { FAINT }),
                );
            });
        }
        hint(
            ui,
            "Включается и выключается в профиле, на вкладке «Курки».",
        );
    });
}

// ------------------------------------------------------------------ settings

const FEATURES: [(u8, &str, &str); 5] = [
    (
        1,
        "Быстрая смена профиля",
        "Fn + A/B/X/Y переключает профиль прямо с геймпада",
    ),
    (
        5,
        "Фильтр дребезга стиков",
        "Сглаживает дрожание; без него не работает автокалибровка",
    ),
    (
        6,
        "Автокалибровка стиков",
        "Подстраивает центр стиков во время игры",
    ),
    (
        7,
        "Фильтр отскока стика",
        "Гасит обратный выброс при резком отпускании стика",
    ),
    (4, "Кнопка Turbo", "Разрешает турбо-режимы кнопок"),
];

fn settings(app: &mut App, ui: &mut egui::Ui) {
    header(
        app,
        ui,
        "Настройки",
        "Применяются сразу и хранятся в самом геймпаде",
    );
    let Some(s) = app.settings.clone() else {
        ui.add(egui::Spinner::new().color(accent()));
        return;
    };

    card_titled(ui, "Стики", |ui| {
        let mut first = true;
        for (sub, name, hint_text) in FEATURES {
            if !s.is_supported(sub) {
                continue;
            }
            if !first {
                widgets::separator(ui);
            }
            first = false;
            let mut on = s.is_on(sub);
            let enabled = !(sub == 6 && !s.is_on(5));
            ui.add_enabled_ui(enabled, |ui| {
                if toggle_row(ui, name, hint_text, &mut on) {
                    app.send(Cmd::SetFeature(sub, on));
                }
            });
        }
        widgets::separator(ui);
        row(
            ui,
            "Точность стиков",
            "Разрешение обычного (Xbox) ввода",
            |ui| {
                combo(ui, "prec", s.precision, &device::PRECISION, |v| {
                    app.send(Cmd::SetNumber(21, v))
                });
            },
        );
        widgets::separator(ui);
        row(
            ui,
            "Чувствительность центра",
            "Насколько рано стик реагирует на отклонение",
            |ui| {
                combo(ui, "sens", s.sensitivity, &device::SENSITIVITY, |v| {
                    app.send(Cmd::SetNumber(22, v))
                });
            },
        );
    });

    ui.add_space(12.0);
    card_titled(ui, "Питание", |ui| {
        row(
            ui,
            "Автоотключение",
            "Через сколько минут бездействия геймпад засыпает",
            |ui| {
                // a value set elsewhere stays pickable alongside the presets
                let custom = format!("{} мин", s.sleep_minutes);
                let mut items = device::SLEEP.to_vec();
                if !items.iter().any(|(v, _)| *v == s.sleep_minutes) {
                    items.push((s.sleep_minutes, &custom));
                }
                combo(ui, "sleep", s.sleep_minutes, &items, |v| {
                    app.send(Cmd::SetNumber(23, v))
                });
            },
        );
    });

    ui.add_space(12.0);
    card_titled(ui, "Экран", |ui| {
        let mut first = true;
        for (sub, name, hint_text) in [
            (9u8, "Экран всегда включён", "Не гасить экран во время игры"),
            (
                8,
                "Строка состояния всегда видна",
                "Батарея и профиль поверх картинки",
            ),
        ] {
            if s.is_supported(sub) {
                if !first {
                    widgets::separator(ui);
                }
                first = false;
                let mut on = s.is_on(sub);
                if toggle_row(ui, name, hint_text, &mut on) {
                    app.send(Cmd::SetFeature(sub, on));
                }
            }
        }
        ui.add_space(4.0);
        hint(
            ui,
            "Загрузка своей картинки не поддерживается: для неё чип экрана нужно переводить в режим прошивки.",
        );
    });

    ui.add_space(12.0);
    let Some(info) = app.info.clone() else { return };
    card_titled(ui, "Имя устройства", |ui| {
        hint(
            ui,
            "Служебное имя, которое геймпад хранит в себе. Его показывают Flydigi Space Station и другие программы, \
             чтобы различать несколько геймпадов. На экране геймпада и в играх оно не отображается.",
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut app.nickname)
                    .desired_width(240.0)
                    .hint_text("без имени")
                    .margin(vec2(10.0, 7.0)),
            );
            let n = app.nickname.len();
            ui.label(
                RichText::new(format!("{n}/{NICKNAME_MAX}"))
                    .size(12.0)
                    .color(FAINT),
            );
            let ok = n > 0 && n <= NICKNAME_MAX && app.nickname != info.nickname;
            if widgets::secondary(ui, "Сохранить имя", ok).clicked() {
                app.send(Cmd::SetNickname(app.nickname.clone()));
            }
        });
    });

    ui.add_space(12.0);
    reset_card(app, ui);

    ui.add_space(12.0);
    card_titled(ui, "Об устройстве", |ui| {
        egui::Grid::new("about")
            .num_columns(2)
            .spacing([28.0, 6.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Модель").color(DIM));
                ui.label(format!(
                    "Flydigi {} (тип {})",
                    info.model(),
                    info.device_type
                ));
                ui.end_row();
                ui.label(RichText::new("UID").color(DIM));
                ui.monospace(&info.uid);
                ui.end_row();
                for (name, v) in &info.firmware {
                    ui.label(RichText::new(format!("Прошивка: {name}")).color(DIM));
                    ui.monospace(v);
                    ui.end_row();
                }
            });
    });
}

/// Factory profiles in all four slots, confirmed by a second press.
fn reset_card(app: &mut App, ui: &mut egui::Ui) {
    const WAIT: Duration = Duration::from_secs(5);
    card_titled(ui, "Сброс", |ui| {
        hint(
            ui,
            "Вернуть заводские значения во всех четырёх профилях и сохранить их в геймпаде: \
             кнопки, макросы, стики, курки, гироскоп и названия. Вибрация рукояток не меняется. \
             Несохранённые изменения пропадут.",
        );
        ui.add_space(4.0);
        ui.checkbox(&mut app.reset_lighting, "И подсветку тоже");
        ui.add_space(4.0);
        let armed = app.reset_armed.is_some_and(|t| t.elapsed() < WAIT);
        ui.horizontal(|ui| {
            let text = if armed {
                "Точно сбросить? Нажмите ещё раз"
            } else {
                "Сбросить все профили"
            };
            if widgets::secondary(ui, text, !app.busy).clicked() {
                if armed {
                    app.reset_armed = None;
                    app.send(Cmd::ResetAll(app.reset_lighting));
                } else {
                    app.reset_armed = Some(Instant::now());
                }
            }
            if armed {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
                if ui.button("Отмена").clicked() {
                    app.reset_armed = None;
                }
            }
        });
    });
}

pub fn combo(
    ui: &mut egui::Ui,
    id: &str,
    current: u8,
    items: &[(u8, &str)],
    mut on_pick: impl FnMut(u8),
) {
    let text = items
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, t)| t.to_string())
        .unwrap_or(format!("({current})"));
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(200.0)
        .show_ui(ui, |ui| {
            for (v, t) in items {
                if ui.selectable_label(*v == current, *t).clicked() && *v != current {
                    on_pick(*v);
                }
            }
        });
}
