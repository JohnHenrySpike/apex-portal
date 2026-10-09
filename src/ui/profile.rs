//! Mapping profiles: buttons, macros, sticks, triggers, gyro.

use apex5::device::InputState;
use apex5::profile::{
    self, ForceEffect, Macro, MacroStep, PAD_KEYS, Profile, STICK_PRESETS, TARGET_IDENTITY,
    TARGET_KEYBOARD, TARGET_MACRO, XINPUT_TARGETS, key_name,
};
use apex5::worker::Cmd;
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, StrokeKind, pos2, vec2};

use super::theme::{self, BORDER, CARD_HI, DIM, FAINT, TEXT, WARN, accent};
use super::widgets::{self, card, card_titled, hint, segmented};
use super::{header, pad};
use crate::{App, ProfileTab};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    header(
        app,
        ui,
        "Профили",
        "Кнопки, макросы, стики, курки и гироскоп — для каждого из 4 профилей",
    );
    if app.edit.is_none() {
        ui.add(egui::Spinner::new().color(accent()));
        return;
    }

    // Slot picker and title.
    card(ui, |ui| {
        ui.horizontal(|ui| {
            let busy = app.busy;
            for slot in 0..profile::SLOTS {
                let cur = app.active == slot;
                let r = widgets::tile(
                    ui,
                    vec2(118.0, 46.0),
                    cur,
                    &format!("Профиль {}", slot + 1),
                    "",
                );
                if r.on_hover_text("Сделать активным на геймпаде").clicked()
                    && !cur
                    && !busy
                {
                    super::switch_profile(app, slot);
                }
            }
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(RichText::new("Название").size(12.0).color(DIM));
                ui.add(
                    egui::TextEdit::singleline(&mut app.title)
                        .desired_width(180.0)
                        .char_limit(10)
                        .margin(vec2(10.0, 6.0))
                        .hint_text("без названия"),
                );
            });
        });
    });
    ui.add_space(14.0);

    segmented(
        ui,
        &mut app.profile_tab,
        &[
            (ProfileTab::Buttons, "Кнопки"),
            (ProfileTab::Macros, "Макросы"),
            (ProfileTab::Sticks, "Стики"),
            (ProfileTab::Triggers, "Курки"),
            (ProfileTab::Gyro, "Гироскоп"),
        ],
    );
    ui.add_space(14.0);

    let tab = app.profile_tab;
    let input = app.input();
    let screen = format!("P{}", app.active + 1);
    let App {
        edit,
        macros,
        selected_key,
        ..
    } = app;
    let e = edit.as_mut().unwrap();
    match tab {
        ProfileTab::Buttons => buttons(e, macros, selected_key, &screen, ui),
        ProfileTab::Macros => macros_ui(macros, ui),
        ProfileTab::Sticks => sticks(e, input, ui),
        ProfileTab::Triggers => triggers(e, ui),
        ProfileTab::Gyro => gyro(e, ui),
    }
}

/// Bottom bar with the apply/save workflow. Shown on the profile page.
pub fn action_bar(app: &mut App, ui: &mut egui::Ui) {
    if app.edit.is_none() {
        return;
    }
    let dirty = app.profile_dirty();
    let flash = app.flash_dirty;
    egui::Panel::bottom("profile_actions")
        .frame(
            egui::Frame::new()
                .fill(theme::SIDEBAR)
                .stroke(Stroke::new(1.0, BORDER))
                .inner_margin(egui::Margin::symmetric(28, 12)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (dot, title, sub) = if dirty {
                    (WARN, "Есть изменения", "Применить — попробовать, Сохранить — запомнить в геймпаде")
                } else if flash {
                    (WARN, "Применено, но не сохранено", "Пропадёт, когда геймпад уснёт")
                } else {
                    (theme::OK, "Всё сохранено", "Геймпад совпадает с экраном")
                };
                let (rect, _) = ui.allocate_exact_size(vec2(12.0, 36.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(title).font(theme::medium(14.0)).color(TEXT));
                    ui.label(RichText::new(sub).size(12.0).color(DIM));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let busy = app.busy;
                    if widgets::primary(ui, "Сохранить", (dirty || flash) && !busy)
                        .on_hover_text("Записать в геймпад и сохранить во флеш-память")
                        .clicked()
                    {
                        if dirty {
                            app.write_profile();
                        }
                        app.send(Cmd::SaveProfile);
                    }
                    if widgets::secondary(ui, "Применить", dirty && !busy)
                        .on_hover_text("Записать в рабочую память геймпада, не сохраняя")
                        .clicked()
                    {
                        app.write_profile();
                    }
                    if widgets::secondary(ui, "Отменить", dirty).clicked()
                        && let Some(p) = app.pad_profile.clone()
                    {
                        app.macros = p.macros();
                        app.title = p.title();
                        app.edit = Some(p);
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("Заводские").color(DIM)).frame(false))
                        .on_hover_text("Вернуть заводские значения этого профиля (вибрация не меняется). Нужно будет применить.")
                        .clicked()
                        && let Some(e) = app.edit.as_mut()
                    {
                        e.factory_reset();
                        app.macros = e.macros();
                        app.title = e.title();
                    }
                });
            });
        });
}

// ------------------------------------------------------------------ buttons

fn target_label(id: u8, t: u8) -> String {
    match t {
        TARGET_IDENTITY => "своя функция".into(),
        TARGET_MACRO => "макрос".into(),
        TARGET_KEYBOARD => "клавиатура".into(),
        x if x == id || x > 32 => "своя функция".into(),
        x => key_name(x),
    }
}

fn is_remapped(p: &Profile, id: u8) -> bool {
    let k = p.key(id);
    let own = k.target == TARGET_IDENTITY
        || k.target == id
        || (k.target > 32 && k.target != TARGET_MACRO);
    !own || k.turbo_freq != 0
}

fn key_title(id: u8) -> String {
    match id {
        0 => "Крестовина вверх".into(),
        1 => "Крестовина вправо".into(),
        2 => "Крестовина вниз".into(),
        3 => "Крестовина влево".into(),
        14 => "Нажатие левого стика (L3)".into(),
        15 => "Нажатие правого стика (R3)".into(),
        18..=21 => format!("Задняя кнопка {}", key_name(id)),
        22 => "Левая доп. кнопка (LM)".into(),
        23 => "Правая доп. кнопка (RM)".into(),
        27 => "Кнопка Home".into(),
        _ => format!("Кнопка {}", key_name(id)),
    }
}

fn buttons(p: &mut Profile, macros: &[Macro], selected: &mut u8, screen: &str, ui: &mut egui::Ui) {
    let marked: Vec<u8> = PAD_KEYS
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| is_remapped(p, *id))
        .collect();
    let wide = ui.available_width() > 860.0;
    let mut state = (p, selected);
    let left = |(p, selected): &mut (&mut Profile, &mut u8), ui: &mut egui::Ui| {
        card(ui, |ui| {
            let w = ui.available_width();
            let r = pad::show(
                ui,
                vec2(w, w * 0.75),
                &pad::Opts {
                    interactive: true,
                    selected: Some(**selected),
                    marked: &marked,
                    show_tiles: true,
                    screen: Some(screen),
                    ..Default::default()
                },
            );
            if let Some(id) = r.clicked {
                **selected = id;
            }
            if let Some(id) = r.hovered {
                let k = p.key(id);
                let mut tip = format!("{} → {}", key_title(id), target_label(id, k.target));
                if k.turbo_freq != 0 {
                    tip += &format!(", турбо {} нажатий/с", k.turbo_freq);
                }
                egui::Tooltip::always_open(
                    ui.ctx().clone(),
                    ui.layer_id(),
                    egui::Id::new("pad_tip"),
                    egui::PopupAnchor::Pointer,
                )
                .show(|ui| ui.label(tip));
            }
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(10.0, 16.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, accent());
                hint(
                    ui,
                    "— переназначена или с турбо. Нажмите на кнопку, чтобы настроить.",
                );
            });
        });
    };
    let right = |(p, selected): &mut (&mut Profile, &mut u8), ui: &mut egui::Ui| {
        key_editor(p, **selected, macros, ui)
    };
    if wide {
        widgets::split(ui, 0.56, &mut state, left, right);
    } else {
        left(&mut state, ui);
        ui.add_space(12.0);
        right(&mut state, ui);
    }
}

fn key_editor(p: &mut Profile, id: u8, macros: &[Macro], ui: &mut egui::Ui) {
    let mut k = p.key(id);
    let before = k;
    card(ui, |ui| {
        ui.label(
            RichText::new(key_title(id))
                .font(theme::bold(20.0))
                .color(TEXT),
        );
        ui.label(RichText::new(format!("Сейчас: {}", target_label(id, k.target))).color(DIM));
        ui.add_space(10.0);

        widgets::caption(ui, "Работает как");
        if k.target == TARGET_MACRO {
            ui.label(RichText::new("Назначен макрос").color(accent()));
            hint(ui, "Изменить или удалить его можно на вкладке «Макросы».");
        } else {
            let own = k.target == TARGET_IDENTITY || k.target == id || k.target > 32;
            let gap = 6.0;
            let cols = 4;
            let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
            let mut options: Vec<(u8, String)> = vec![(TARGET_IDENTITY, "своя".into())];
            options.extend(
                XINPUT_TARGETS
                    .iter()
                    .filter(|t| **t != id)
                    .map(|t| (*t, key_name(*t))),
            );
            for chunk in options.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (t, name) in chunk {
                        let sel = if *t == TARGET_IDENTITY {
                            own
                        } else {
                            k.target == *t
                        };
                        if chip(ui, vec2(w, 32.0), sel, name).clicked() {
                            k.target = *t;
                        }
                    }
                });
            }
            if k.target == TARGET_KEYBOARD {
                ui.label(
                    RichText::new("Назначена клавиша клавиатуры — в Linux это не работает.")
                        .color(WARN),
                );
            }
            if macros.iter().all(|m| m.key != id) {
                hint(
                    ui,
                    "Макрос на эту кнопку можно добавить на вкладке «Макросы».",
                );
            }
        }

        ui.add_space(12.0);
        widgets::caption(ui, "Турбо");
        let mut mode = if k.turbo_freq == 0 {
            0u8
        } else {
            k.turbo_mode.clamp(1, 2)
        };
        ui.add_enabled_ui(k.target != TARGET_MACRO, |ui| {
            segmented(
                ui,
                &mut mode,
                &[(0, "Выкл"), (1, "При удержании"), (2, "Переключатель")],
            );
        });
        if mode == 0 {
            k.turbo_freq = 0;
            k.turbo_mode = 0;
        } else {
            k.turbo_mode = mode;
            if k.turbo_freq == 0 {
                k.turbo_freq = 10;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Частота").color(DIM));
                ui.add(egui::Slider::new(&mut k.turbo_freq, 1..=40).suffix(" нажатий/с"));
            });
            hint(
                ui,
                if mode == 1 {
                    "Пока кнопка зажата, геймпад повторяет нажатие."
                } else {
                    "Одно нажатие включает автоповтор, второе — выключает."
                },
            );
        }
    });
    if k != before {
        p.set_key(id, k);
    }
}

fn chip(ui: &mut egui::Ui, size: egui::Vec2, selected: bool, text: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
    let t = ui.ctx().animate_bool_responsive(r.id, selected);
    let bg = if r.hovered() && !selected {
        theme::HOVER
    } else {
        CARD_HI
    };
    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius::same(8),
        widgets::lerp_colour(bg, accent(), t),
        Stroke::new(1.0, widgets::lerp_colour(BORDER, accent(), t)),
        StrokeKind::Inside,
    );
    let colour = if selected { theme::on_accent() } else { TEXT };
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        theme::medium(13.5),
        colour,
    );
    r
}

// ------------------------------------------------------------------ macros

fn macros_ui(macros: &mut Vec<Macro>, ui: &mut egui::Ui) {
    let total: usize = macros.iter().map(|m| m.steps.len()).sum();
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Макросы выполняет сам геймпад").font(theme::medium(15.0)).color(TEXT));
                hint(
                    ui,
                    &format!(
                        "До {} макросов и {} шагов на профиль (занято {total}). Время — с шагом 10 мс.",
                        profile::MACRO_MAX,
                        profile::MACRO_STEPS_MAX
                    ),
                );
            });
            if macros.len() < profile::MACRO_MAX {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let used: Vec<u8> = macros.iter().map(|m| m.key).collect();
                    egui::ComboBox::from_id_salt("newmacro").selected_text("+ Новый макрос").width(170.0).show_ui(ui, |ui| {
                        for (id, name) in PAD_KEYS {
                            if !used.contains(&id) && ui.selectable_label(false, format!("на кнопку {name}")).clicked() {
                                macros.push(Macro { key: id, kind: 1, interval_ms: None, steps: vec![] });
                            }
                        }
                    });
                });
            }
        });
    });

    let kinds = [
        (1u8, "Один раз"),
        (2, "Пока удерживается"),
        (3, "Вкл/выкл нажатием"),
    ];
    let mut remove = None;
    for (mi, m) in macros.iter_mut().enumerate() {
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(10),
                    accent().gamma_multiply(0.18),
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    key_name(m.key),
                    theme::bold(15.0),
                    accent(),
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(
                        RichText::new(format!("Макрос на кнопке {}", key_name(m.key)))
                            .font(theme::medium(15.0))
                            .color(TEXT),
                    );
                    ui.label(
                        RichText::new(format!("шагов: {}", m.steps.len()))
                            .size(12.0)
                            .color(DIM),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Удалить").color(theme::BAD))
                                .frame(false),
                        )
                        .clicked()
                    {
                        remove = Some(mi);
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                segmented(ui, &mut m.kind, &kinds);
                if m.kind != 1 {
                    let mut iv = m.interval_ms.unwrap_or(30);
                    ui.label(RichText::new("пауза между повторами").color(DIM));
                    if ui
                        .add(
                            egui::DragValue::new(&mut iv)
                                .range(0..=2540)
                                .speed(10)
                                .suffix(" мс"),
                        )
                        .changed()
                    {
                        m.interval_ms = Some(iv / 10 * 10);
                    }
                }
            });
            ui.add_space(6.0);

            let mut del = None;
            for (si, st) in m.steps.iter_mut().enumerate() {
                egui::Frame::new()
                    .fill(CARD_HI)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{:>2}", si + 1))
                                    .monospace()
                                    .color(FAINT),
                            );
                            ui.add(
                                egui::DragValue::new(&mut st.delay_ms)
                                    .range(0..=8000)
                                    .speed(10)
                                    .prefix("через ")
                                    .suffix(" мс"),
                            );
                            segmented(ui, &mut st.press, &[(true, "нажать"), (false, "отпустить")]);
                            egui::ComboBox::from_id_salt(("sk", mi, si))
                                .width(90.0)
                                .selected_text(key_name(st.key))
                                .show_ui(ui, |ui| {
                                    for t in XINPUT_TARGETS {
                                        ui.selectable_value(&mut st.key, t, key_name(t));
                                    }
                                });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(RichText::new("✕").color(DIM))
                                                .frame(false),
                                        )
                                        .on_hover_text("Удалить шаг")
                                        .clicked()
                                    {
                                        del = Some(si);
                                    }
                                },
                            );
                        });
                    });
                ui.add_space(2.0);
            }
            if let Some(si) = del {
                m.steps.remove(si);
            }
            ui.horizontal(|ui| {
                let key = m.steps.last().map(|s| s.key).unwrap_or(4);
                let d = if m.steps.is_empty() { 0 } else { 50 };
                if widgets::secondary(
                    ui,
                    "+ Нажатие кнопки",
                    total + 2 <= profile::MACRO_STEPS_MAX,
                )
                .clicked()
                {
                    m.steps.push(MacroStep {
                        delay_ms: d,
                        key,
                        press: true,
                    });
                    m.steps.push(MacroStep {
                        delay_ms: 50,
                        key,
                        press: false,
                    });
                }
                if widgets::secondary(ui, "+ Шаг", total < profile::MACRO_STEPS_MAX).clicked() {
                    m.steps.push(MacroStep {
                        delay_ms: d,
                        key,
                        press: true,
                    });
                }
                let presses = m.steps.iter().filter(|s| s.press).count();
                if presses * 2 != m.steps.len() {
                    ui.label(RichText::new("нажатия и отпускания не совпадают").color(WARN));
                }
            });
        });
    }
    if let Some(i) = remove {
        macros.remove(i);
    }
}

// ------------------------------------------------------------------ sticks

fn sticks(p: &mut Profile, input: Option<InputState>, ui: &mut egui::Ui) {
    let axis = |v: i16| v as f32 / 32767.0;
    let live = input.map(|s| [(axis(s.lx), axis(s.ly)), (axis(s.rx), axis(s.ry))]);
    let wide = ui.available_width() > 860.0;
    if wide {
        widgets::split(
            ui,
            0.5,
            p,
            |p, ui| stick_card(p, 0, live.map(|l| l[0]), ui),
            |p, ui| stick_card(p, 1, live.map(|l| l[1]), ui),
        );
    } else {
        stick_card(p, 0, live.map(|l| l[0]), ui);
        ui.add_space(12.0);
        stick_card(p, 1, live.map(|l| l[1]), ui);
    }
    // The live preview needs a steady stream of frames.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(16));
    ui.add_space(8.0);
    hint(
        ui,
        "Геймпад играет 9-точечную кривую; она пересчитывается из этих настроек. Точки на графике можно перетаскивать.",
    );
}

fn stick_card(p: &mut Profile, side: usize, live: Option<(f32, f32)>, ui: &mut egui::Ui) {
    let mut s = p.stick(side);
    let before = s;
    card_titled(
        ui,
        if side == 0 {
            "Левый стик"
        } else {
            "Правый стик"
        },
        |ui| {
            let mut kind = s.kind;
            let mut items: Vec<(u8, &str)> =
                STICK_PRESETS.iter().map(|(k, n, _, _)| (*k, *n)).collect();
            items.push((3, "Своя"));
            if segmented(ui, &mut kind, &items) {
                if let Some((_, _, p1, p2)) = STICK_PRESETS.iter().find(|(k, ..)| *k == kind) {
                    s.p1 = *p1;
                    s.p2 = *p2;
                    s.center = 0;
                    s.edge = 0;
                }
                s.kind = kind;
            }
            ui.add_space(6.0);
            let shown = compiled(&s);
            let mag = live.map(|(x, y)| (x.hypot(y) * 100.0).min(100.0));
            let mut custom = false;
            let pw = (ui.available_width() * 0.36).clamp(110.0, 170.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - pw - 10.0);
                    custom = curve(ui, &mut s, side, mag);
                });
                stick_preview(ui, pw, &shown, live);
            });
            ui.add_space(6.0);
            ui.spacing_mut().slider_width = (ui.available_width() - 170.0).max(100.0);
            egui::Grid::new(("st", side))
                .num_columns(2)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Мёртвая зона").color(DIM));
                    custom |= ui
                        .add(egui::Slider::new(&mut s.center, 0..=100).suffix("%"))
                        .changed();
                    ui.end_row();
                    ui.label(RichText::new("Внешняя зона").color(DIM));
                    custom |= ui
                        .add(egui::Slider::new(&mut s.edge, 0..=100).suffix("%"))
                        .changed();
                    ui.end_row();
                    ui.label(RichText::new("Форма хода").color(DIM));
                    segmented(ui, &mut s.round, &[(true, "Круг"), (false, "Квадрат")]);
                    ui.end_row();
                });
            if custom {
                s.kind = 3;
            }
        },
    );
    if s != before {
        p.set_stick(side, s);
    }
}

/// Response curve with two draggable control points. Returns true if dragged.
/// The stick settings with the zones clamped and the bank recompiled, as the
/// pad would play them.
fn compiled(s: &profile::Stick) -> profile::Stick {
    let mut c = *s;
    c.center = c.center.min(100);
    c.edge = c.edge.min(100 - c.center);
    c.compile();
    c
}

/// Live stick position: the raw deflection (hollow) and where the curve puts
/// it (filled), with the dead zone and outer zone drawn as rings.
fn stick_preview(ui: &mut egui::Ui, w: f32, s: &profile::Stick, live: Option<(f32, f32)>) {
    ui.vertical(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(w, w), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let c = rect.center();
        let r = w / 2.0 - 4.0;
        if s.round {
            p.circle(
                c,
                r,
                Color32::from_rgb(12, 14, 18),
                Stroke::new(1.0, BORDER),
            );
        } else {
            p.rect(
                egui::Rect::from_center_size(c, vec2(2.0 * r, 2.0 * r)),
                CornerRadius::same(10),
                Color32::from_rgb(12, 14, 18),
                Stroke::new(1.0, BORDER),
                StrokeKind::Inside,
            );
        }
        let dz = s.center as f32 / 100.0;
        if dz > 0.0 {
            p.circle_filled(c, r * dz, accent().gamma_multiply(0.08));
            p.circle_stroke(c, r * dz, Stroke::new(1.0, accent().gamma_multiply(0.4)));
        }
        let ez = s.edge as f32 / 100.0;
        if ez > 0.0 {
            p.circle_stroke(
                c,
                r * (1.0 - ez),
                Stroke::new(1.0, accent().gamma_multiply(0.4)),
            );
        }
        p.line_segment(
            [pos2(c.x - r, c.y), pos2(c.x + r, c.y)],
            Stroke::new(1.0, Color32::from_rgb(30, 34, 41)),
        );
        p.line_segment(
            [pos2(c.x, c.y - r), pos2(c.x, c.y + r)],
            Stroke::new(1.0, Color32::from_rgb(30, 34, 41)),
        );
        match live {
            Some((x, y)) => {
                let mag = x.hypot(y).min(1.0);
                let raw = pos2(c.x + x.clamp(-1.0, 1.0) * r, c.y - y.clamp(-1.0, 1.0) * r);
                let k = if mag > 0.001 {
                    (s.output(mag as f64 * 100.0).min(100.0) as f32 / 100.0) / mag
                } else {
                    0.0
                };
                let out = pos2(c.x + x * k * r, c.y - y * k * r);
                p.circle_stroke(raw, 6.0, Stroke::new(1.5, DIM));
                p.circle_filled(out, 11.0, accent().gamma_multiply(0.2));
                p.circle_filled(out, 6.0, accent());
            }
            None => {
                p.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    "подвигайте\nстиком",
                    egui::FontId::proportional(12.0),
                    FAINT,
                );
            }
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (rr, _) = ui.allocate_exact_size(vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter()
                .circle_stroke(rr.center(), 4.0, Stroke::new(1.5, DIM));
            ui.label(RichText::new("стик").size(11.5).color(DIM));
            let (rr, _) = ui.allocate_exact_size(vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(rr.center(), 4.0, accent());
            ui.label(RichText::new("в игре").size(11.5).color(DIM));
        });
    });
}

fn curve(ui: &mut egui::Ui, s: &mut profile::Stick, side: usize, live: Option<f32>) -> bool {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, (w * 0.75).min(240.0)), egui::Sense::hover());
    let plot = rect.shrink(14.0);
    let map = |x: f32, y: f32| {
        pos2(
            plot.left() + x / 100.0 * plot.width(),
            plot.bottom() - y / 100.0 * plot.height(),
        )
    };
    let unmap = |p: egui::Pos2| {
        (
            (p.x - plot.left()) / plot.width() * 100.0,
            (plot.bottom() - p.y) / plot.height() * 100.0,
        )
    };

    let pt = ui.painter_at(rect);
    pt.rect_filled(rect, CornerRadius::same(10), Color32::from_rgb(12, 14, 18));
    for i in 1..4 {
        let f = i as f32 * 25.0;
        pt.line_segment(
            [map(f, 0.0), map(f, 100.0)],
            Stroke::new(1.0, Color32::from_rgb(28, 32, 39)),
        );
        pt.line_segment(
            [map(0.0, f), map(100.0, f)],
            Stroke::new(1.0, Color32::from_rgb(28, 32, 39)),
        );
    }
    pt.line_segment(
        [map(0.0, 0.0), map(100.0, 100.0)],
        Stroke::new(1.0, Color32::from_rgb(48, 54, 64)),
    );
    pt.text(
        map(1.0, 99.0),
        egui::Align2::LEFT_TOP,
        "выход",
        egui::FontId::proportional(11.0),
        FAINT,
    );
    pt.text(
        map(99.0, 1.0),
        egui::Align2::RIGHT_BOTTOM,
        "отклонение стика",
        egui::FontId::proportional(11.0),
        FAINT,
    );

    let shown = compiled(s);
    // Dead zone and outer zone shading.
    let dz = shown.center as f32;
    if dz > 0.0 {
        pt.rect_filled(
            egui::Rect::from_min_max(map(0.0, 100.0), map(dz, 0.0)),
            0.0,
            accent().gamma_multiply(0.06),
        );
    }
    let ez = shown.edge as f32;
    if ez > 0.0 {
        pt.rect_filled(
            egui::Rect::from_min_max(map(100.0 - ez, 100.0), map(100.0, 0.0)),
            0.0,
            accent().gamma_multiply(0.06),
        );
    }
    let pts: Vec<_> = (0..=100)
        .map(|x| map(x as f32, shown.output(x as f64).min(100.0) as f32))
        .collect();
    pt.add(egui::Shape::line(
        pts.clone(),
        Stroke::new(6.0, accent().gamma_multiply(0.15)),
    ));
    pt.add(egui::Shape::line(pts, Stroke::new(2.5, accent())));

    // Where the stick is right now.
    if let Some(m) = live {
        let out = shown.output(m as f64).min(100.0) as f32;
        pt.line_segment(
            [map(m, 0.0), map(m, out)],
            Stroke::new(1.0, Color32::from_white_alpha(60)),
        );
        pt.line_segment(
            [map(0.0, out), map(m, out)],
            Stroke::new(1.0, Color32::from_white_alpha(60)),
        );
        pt.circle_filled(map(m, out), 5.0, Color32::WHITE);
    }

    // Control points (0..127 scale on the pad).
    let mut dragged = false;
    for (i, (px, py)) in [s.p1, s.p2].into_iter().enumerate() {
        let at = map(px as f32 / 127.0 * 100.0, py as f32 / 127.0 * 100.0);
        let id = ui.id().with(("curve", side, i));
        let r = ui.interact(
            egui::Rect::from_center_size(at, vec2(22.0, 22.0)),
            id,
            egui::Sense::drag(),
        );
        let hot = r.hovered() || r.dragged();
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        pt.circle(
            at,
            if hot { 8.0 } else { 6.5 },
            theme::BG,
            Stroke::new(2.5, accent()),
        );
        if r.dragged()
            && let Some(pos) = r.interact_pointer_pos()
        {
            let (x, y) = unmap(pos);
            let v = (
                (x.clamp(0.0, 100.0) / 100.0 * 127.0) as u8,
                (y.clamp(0.0, 100.0) / 100.0 * 127.0) as u8,
            );
            if i == 0 {
                s.p1 = v;
            } else {
                s.p2 = v;
            }
            dragged = true;
        }
    }
    dragged
}

// ------------------------------------------------------------------ triggers

const MODES: [(u8, &str, &str); 4] = [
    (0, "Обычный", "Курок свободный"),
    (1, "Гоночный", "Постоянное сопротивление"),
    (3, "Прорыв", "Упор, который продавливается"),
    (4, "Блокировка", "Жёсткий упор, короткий ход"),
];

fn triggers(p: &mut Profile, ui: &mut egui::Ui) {
    let wide = ui.available_width() > 860.0;
    if wide {
        widgets::split(
            ui,
            0.5,
            p,
            |p, ui| trigger_card(p, 0, ui),
            |p, ui| trigger_card(p, 1, ui),
        );
    } else {
        trigger_card(p, 0, ui);
        ui.add_space(12.0);
        trigger_card(p, 1, ui);
    }
    ui.add_space(8.0);
    hint(
        ui,
        "Сопротивление начинает работать после «Применить». Вибрационные режимы курков приложение не включает.",
    );
}

fn trigger_card(p: &mut Profile, side: usize, ui: &mut egui::Ui) {
    card_titled(
        ui,
        if side == 0 {
            "Левый курок · LT"
        } else {
            "Правый курок · RT"
        },
        |ui| {
            ui.label(
                RichText::new("Рабочий ход")
                    .font(theme::medium(14.5))
                    .color(TEXT),
            );
            hint(
                ui,
                "Сдвиньте конец влево, чтобы курок срабатывал полностью при коротком нажатии.",
            );
            let (mut z, mut e) = p.travel(side);
            if travel_bar(ui, &mut z, &mut e, side) {
                p.set_travel(side, z, e);
            }
            ui.add_space(12.0);

            ui.label(
                RichText::new("Сопротивление")
                    .font(theme::medium(14.5))
                    .color(TEXT),
            );
            let mut f = p.force(side);
            if !apex5::safety::RESISTANCE_MODES.contains(&f.mode) {
                ui.label(
                RichText::new(format!(
                    "В профиле записан вибрационный режим ({}). Приложение его не включает; можно заменить на один из режимов ниже.",
                    f.mode
                ))
                .color(WARN),
            );
            }
            let before = f;
            let gap = 6.0;
            let w = ((ui.available_width() - gap) / 2.0).floor();
            for pair in MODES.chunks(2) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (m, name, desc) in pair {
                        if widgets::tile(ui, vec2(w, 56.0), f.mode == *m, name, desc).clicked()
                            && f.mode != *m
                        {
                            f = defaults(*m, f);
                        }
                    }
                });
            }
            ui.add_space(6.0);
            ui.spacing_mut().slider_width = (ui.available_width() - 170.0).max(100.0);
            let pr = &mut f.params;
            egui::Grid::new(("force", side))
                .num_columns(2)
                .spacing([14.0, 8.0])
                .show(ui, |ui| match f.mode {
                    1 => {
                        ui.label(RichText::new("Начало").color(DIM));
                        ui.add(egui::Slider::new(&mut pr[0], 0..=192));
                        ui.end_row();
                        ui.label(RichText::new("Сила").color(DIM));
                        ui.add(egui::Slider::new(&mut pr[1], 1..=255));
                        ui.end_row();
                    }
                    3 => {
                        ui.label(RichText::new("Начало").color(DIM));
                        ui.add(egui::Slider::new(&mut pr[0], 0..=192));
                        ui.end_row();
                        ui.label(RichText::new("Длина упора").color(DIM));
                        ui.add(egui::Slider::new(&mut pr[1], 1..=255));
                        ui.end_row();
                        ui.label(RichText::new("Сила").color(DIM));
                        ui.add(egui::Slider::new(&mut pr[2], 1..=255));
                        ui.end_row();
                        ui.label(RichText::new("Учитывать нажатие").color(DIM));
                        let mut m = pr[4] == 1;
                        if widgets::toggle(ui, &mut m).changed() {
                            pr[4] = m as u8;
                        }
                        ui.end_row();
                    }
                    4 => {
                        ui.label(RichText::new("Точка упора").color(DIM));
                        ui.add(egui::Slider::new(&mut pr[0], 20..=200));
                        ui.end_row();
                    }
                    _ => {}
                });
            if f != before {
                p.set_force(side, f);
            }
        },
    );
}

/// Range bar with two handles for the trigger's active travel.
fn travel_bar(ui: &mut egui::Ui, start: &mut u8, end: &mut u8, side: usize) -> bool {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 44.0), egui::Sense::hover());
    let rail = egui::Rect::from_center_size(
        pos2(rect.center().x, rect.top() + 16.0),
        vec2(w - 24.0, 8.0),
    );
    let x_of = |v: u8| rail.left() + v as f32 / 255.0 * rail.width();
    let p = ui.painter();
    p.rect_filled(rail, CornerRadius::same(4), CARD_HI);
    let active = egui::Rect::from_min_max(
        pos2(x_of(*start), rail.top()),
        pos2(x_of(*end), rail.bottom()),
    );
    p.rect_filled(active, CornerRadius::same(4), accent());
    let mut changed = false;
    for i in 0..2 {
        let v = if i == 0 { *start } else { *end };
        let c = pos2(x_of(v), rail.center().y);
        let r = ui.interact(
            egui::Rect::from_center_size(c, vec2(24.0, 28.0)),
            ui.id().with(("travel", side, i)),
            egui::Sense::drag(),
        );
        let hot = r.hovered() || r.dragged();
        ui.painter().circle(
            c,
            if hot { 10.0 } else { 9.0 },
            TEXT,
            Stroke::new(3.0, accent()),
        );
        if r.dragged()
            && let Some(pos) = r.interact_pointer_pos()
        {
            let nv = (((pos.x - rail.left()) / rail.width()).clamp(0.0, 1.0) * 255.0).round() as u8;
            if i == 0 {
                *start = nv.min(end.saturating_sub(10));
            } else {
                *end = nv.max(start.saturating_add(10));
            }
            changed = true;
        }
    }
    let pct = |v: u8| (v as f32 / 255.0 * 100.0).round() as u32;
    ui.painter().text(
        pos2(rail.left(), rect.bottom() - 6.0),
        egui::Align2::LEFT_BOTTOM,
        format!("начало {}%", pct(*start)),
        egui::FontId::proportional(12.0),
        DIM,
    );
    ui.painter().text(
        pos2(rail.right(), rect.bottom() - 6.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("конец {}%", pct(*end)),
        egui::FontId::proportional(12.0),
        DIM,
    );
    changed
}

fn defaults(mode: u8, cur: ForceEffect) -> ForceEffect {
    let mut params = [0u8; 10];
    match mode {
        1 => params[..2].copy_from_slice(&[64, 128]),
        3 => {
            params[..3].copy_from_slice(&[64, 64, 160]);
            params[4] = 1;
        }
        4 => params[..3].copy_from_slice(&[100, 255, 1]),
        _ => params = cur.params,
    }
    ForceEffect { mode, params }
}

// ------------------------------------------------------------------ gyro

fn gyro(p: &mut Profile, ui: &mut egui::Ui) {
    let mut g = p.gyro();
    let before = g;
    card(ui, |ui| {
        ui.label(
            RichText::new("Гироскоп как стик")
                .font(theme::bold(18.0))
                .color(TEXT),
        );
        hint(
            ui,
            "Наклон геймпада двигает выбранный стик — удобно для точного прицеливания.",
        );
        ui.add_space(8.0);
        segmented(
            ui,
            &mut g.kind,
            &[
                (0, "Выключен"),
                (2, "Правый стик (прицел)"),
                (1, "Левый стик (руль)"),
            ],
        );
        ui.add_space(10.0);
        ui.add_enabled_ui(g.kind != 0, |ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 230.0).clamp(100.0, 360.0);
            egui::Grid::new("gyro")
                .num_columns(2)
                .spacing([20.0, 10.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Кнопка включения").color(DIM));
                    key_combo(ui, "gk", &mut g.key);
                    ui.end_row();
                    ui.label(RichText::new("Как включать").color(DIM));
                    segmented(
                        ui,
                        &mut g.hold,
                        &[(true, "Пока удерживается"), (false, "Нажатие переключает")],
                    );
                    ui.end_row();
                    if g.hold {
                        ui.label(RichText::new("Вторая кнопка").color(DIM));
                        key_combo(ui, "gk2", &mut g.key2);
                        ui.end_row();
                    }
                    ui.label(RichText::new("Мёртвая зона").color(DIM));
                    ui.add(egui::Slider::new(&mut g.deadzone, 0..=100));
                    ui.end_row();
                    ui.label(RichText::new("Чувствительность по X").color(DIM));
                    ui.add(egui::Slider::new(&mut g.sens_x, 0..=100));
                    ui.end_row();
                    ui.label(RichText::new("Чувствительность по Y").color(DIM));
                    ui.add(egui::Slider::new(&mut g.sens_y, 0..=100));
                    ui.end_row();
                });
        });
        if g.hold && g.key2 == 0 && g.kind != 0 {
            ui.add_space(4.0);
            ui.label(RichText::new("Вторая кнопка сейчас «крестовина вверх» (заводское значение) — она тоже включает гироскоп.").color(WARN));
        }
    });
    if g != before {
        p.set_gyro(g);
    }
}

fn key_combo(ui: &mut egui::Ui, id: &str, key: &mut u8) {
    let text = if *key == 255 {
        "нет".to_string()
    } else {
        key_name(*key)
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .show_ui(ui, |ui| {
            ui.selectable_value(key, 255, "нет");
            for (k, n) in PAD_KEYS {
                ui.selectable_value(key, k, n);
            }
        });
}
