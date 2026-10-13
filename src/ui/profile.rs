//! Mapping profiles: buttons, macros, sticks, triggers, gyro.

use apex5::device::InputState;
use apex5::lang::t;
use apex5::profile::{
    self, FORCE_MODES, ForceEffect, GripMotor, MODE_SNIPER, MODE_VIBRATION, Macro, MacroStep,
    PAD_KEYS, Profile, STICK_PRESETS, TARGET_IDENTITY, TARGET_KEYBOARD, TARGET_MACRO,
    XINPUT_TARGETS, key_name,
};
use apex5::worker::Cmd;
use eframe::egui::{self, CornerRadius, RichText, Stroke, StrokeKind, pos2, vec2};

use super::theme::{self, accent};
use super::widgets::{self, card, card_titled, hint, segmented};
use super::{header, pad};
use crate::{App, ProfileTab};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    header(
        app,
        ui,
        &apex5::tr!("Profile {slot}", slot = app.active + 1),
        t("Buttons, sticks, triggers and gyro of the active profile"),
    );
    if app.edit.is_none() {
        ui.add(egui::Spinner::new().color(accent()));
        return;
    }
    if let Some(by) = app
        .flags
        .as_ref()
        .map(|f| &f.control_by)
        .filter(|b| !b.is_empty())
    {
        card(ui, |ui| {
            ui.label(
                RichText::new(apex5::tr!(
                    "The controller is taken by {by}: while it holds it, the profile does nothing \
                     and the controller's normal input is off.",
                    by
                ))
                .color(theme::warn()),
            );
        });
        ui.add_space(12.0);
    }

    // Sub-pages on the left; the title and the menu on the right, or under
    // them when the page is narrow.
    let tabs = |app: &mut App, ui: &mut egui::Ui| {
        segmented(
            ui,
            &mut app.profile_tab,
            &[
                (ProfileTab::Buttons, t("Buttons")),
                (ProfileTab::Macros, t("Macros")),
                (ProfileTab::Sticks, t("Sticks")),
                (ProfileTab::Triggers, t("Triggers")),
                (ProfileTab::Vibration, t("Vibration")),
                (ProfileTab::Gyro, t("Gyro")),
            ],
        );
    };
    let title = |app: &mut App, ui: &mut egui::Ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            super::copies::menu(app, ui);
            title_edit(app, ui);
            ui.label(RichText::new(t("Name#profile")).color(theme::dim()));
        });
    };
    if ui.available_width() > 860.0 {
        ui.horizontal(|ui| {
            tabs(app, ui);
            title(app, ui);
        });
    } else {
        tabs(app, ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| title(app, ui));
    }
    ui.add_space(14.0);

    let tab = app.profile_tab;
    let input = app.input();
    let screen = format!("P{}", app.active + 1);
    let App {
        edit,
        macros,
        selected_key,
        cloud,
        ..
    } = app;
    let e = edit.as_mut().unwrap();
    match tab {
        ProfileTab::Buttons => buttons(e, macros, selected_key, &screen, ui),
        ProfileTab::Macros => macros_ui(macros, ui),
        ProfileTab::Sticks => sticks(e, input, ui),
        ProfileTab::Triggers => {
            triggers(e, ui);
            ui.add_space(12.0);
            super::cloud::game_presets(cloud, e, ui);
        }
        ProfileTab::Vibration => grip_vibration_card(e, ui),
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
    let (dot, title, sub) = if dirty {
        (
            theme::warn(),
            t("Unsaved changes"),
            t("Not yet written to the controller"),
        )
    } else if flash {
        (
            theme::warn(),
            t("Applied, not saved"),
            t("Lost when the controller sleeps"),
        )
    } else {
        (
            theme::ok(),
            t("All saved"),
            t("The controller matches the screen"),
        )
    };
    widgets::save_bar(ui, "profile_actions", dot, title, sub, |ui| {
        let busy = app.busy;
        if widgets::primary(ui, t("Save"), (dirty || flash) && !busy)
            .on_hover_text(t("Write to the controller and save to flash"))
            .clicked()
        {
            if dirty {
                app.write_profile();
            }
            app.send(Cmd::SaveProfile);
        }
        if widgets::secondary(ui, t("Apply"), dirty && !busy)
            .on_hover_text(t("Write to the controller's working memory without saving"))
            .clicked()
        {
            app.write_profile();
        }
        if widgets::secondary(ui, t("Revert"), dirty).clicked()
            && let Some(p) = app.pad_profile.clone()
        {
            app.macros = p.macros();
            app.title = p.title();
            app.edit = Some(p);
        }
    });
}

// ------------------------------------------------------------------ titles

/// Factory profiles are titled 配置1…配置4 ("config N"): the number, if `t`
/// is one of them.
fn factory_title(t: &str) -> Option<&str> {
    t.strip_prefix("配置")
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// A profile title as shown: the factory ones in the app's language. The pad keeps
/// what it has.
pub fn shown_title(t: &str) -> String {
    match factory_title(t) {
        Some(n) => apex5::tr!("Profile {n}", n),
        None => t.to_string(),
    }
}

/// The title box. A factory title is shown as a hint and stays on the pad
/// until something else is typed.
fn title_edit(app: &mut App, ui: &mut egui::Ui) {
    let pad = app
        .pad_profile
        .as_ref()
        .map(|p| p.title())
        .unwrap_or_default();
    let factory = factory_title(&app.title).is_some();
    let mut buf = if factory {
        String::new()
    } else {
        app.title.clone()
    };
    let hint = if factory {
        shown_title(&app.title)
    } else {
        t("untitled").into()
    };
    let r = ui
        .add(
            egui::TextEdit::singleline(&mut buf)
                .desired_width(150.0)
                .char_limit(10)
                .margin(vec2(10.0, 7.0))
                .hint_text(hint),
        )
        .on_hover_text(t("Profile name, up to 10 characters"));
    if r.changed() {
        // emptied again: back to the factory title rather than none
        app.title = if buf.is_empty() && factory_title(&pad).is_some() {
            pad
        } else {
            buf
        };
    }
}

// ------------------------------------------------------------------ summary

/// One line per sub-page: what the profile does there, and whether it
/// differs from a plain pad.
pub fn summary(p: &Profile) -> Vec<(ProfileTab, &'static str, String, bool)> {
    let remapped = PAD_KEYS
        .iter()
        .filter(|(id, _)| is_remapped(p, *id))
        .count();
    let macros = p.macros().len();
    let stick = |side| {
        let k = p.stick(side).kind;
        STICK_PRESETS
            .iter()
            .find(|(kind, ..)| *kind == k)
            .map_or(t("custom"), |(_, n, ..)| t(n))
    };
    let mode = |side| {
        let m = p.force(side).mode;
        modes()
            .into_iter()
            .find(|(mode, ..)| *mode == m)
            .map_or(t("unknown"), |(_, n, _)| n)
    };
    let (l, r) = (mode(0), mode(1));
    let g = p.grip_vibration();
    let grip = g.master && (g.left.on || g.right.on);
    let gyro = match p.gyro().kind {
        0 => t("off#gyro"),
        1 => t("left stick"),
        _ => t("right stick"),
    };
    vec![
        (
            ProfileTab::Buttons,
            t("Buttons"),
            match remapped {
                0 => t("as usual").into(),
                n => apex5::tr!("remapped: {n}", n),
            },
            remapped > 0,
        ),
        (
            ProfileTab::Macros,
            t("Macros"),
            match macros {
                0 => t("none").into(),
                n => n.to_string(),
            },
            macros > 0,
        ),
        (
            ProfileTab::Sticks,
            t("Sticks"),
            if stick(0) == stick(1) {
                stick(0).to_string()
            } else {
                format!("{} / {}", stick(0), stick(1))
            },
            p.stick(0).kind != 0 || p.stick(1).kind != 0,
        ),
        (
            ProfileTab::Triggers,
            t("Triggers"),
            if l == r {
                l.to_string()
            } else {
                format!("{l} / {r}")
            },
            p.force(0).mode != 0 || p.force(1).mode != 0,
        ),
        (
            ProfileTab::Vibration,
            t("Grip rumble"),
            if grip { t("on") } else { t("off#grip rumble") }.into(),
            grip,
        ),
        (ProfileTab::Gyro, t("Gyro"), gyro.into(), p.gyro().kind != 0),
    ]
}

// ------------------------------------------------------------------ buttons

fn target_label(id: u8, target: u8) -> String {
    match target {
        TARGET_IDENTITY => t("its own").into(),
        TARGET_MACRO => t("macro").into(),
        TARGET_KEYBOARD => t("keyboard").into(),
        x if x == id || x > 32 => t("its own").into(),
        x => key_name(x),
    }
}

fn is_remapped(p: &Profile, id: u8) -> bool {
    let k = p.key(id);
    let own = k.target == TARGET_IDENTITY
        || k.target == id
        || (k.target > 32 && k.target != TARGET_MACRO);
    !own || k.turbo()
}

fn key_title(id: u8) -> String {
    match id {
        0 => t("D-pad up").into(),
        1 => t("D-pad right").into(),
        2 => t("D-pad down").into(),
        3 => t("D-pad left").into(),
        14 => t("Left stick press (L3)").into(),
        15 => t("Right stick press (R3)").into(),
        18..=21 => apex5::tr!("Back paddle {key}", key = key_name(id)),
        22 => t("Left extra button (LM)").into(),
        23 => t("Right extra button (RM)").into(),
        27 => t("Home button").into(),
        _ => apex5::tr!("Button {key}", key = key_name(id)),
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
                if k.turbo() {
                    tip += &apex5::tr!(", turbo {rate} presses/s", rate = k.turbo_freq);
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
                    t("— remapped or with turbo. Click a button to set it up."),
                );
            });
        });
    };
    let right = |(p, selected): &mut (&mut Profile, &mut u8), ui: &mut egui::Ui| {
        key_editor(p, **selected, macros, ui);
        ui.add_space(12.0);
        assignments(p, selected, &marked, ui);
    };
    if wide {
        widgets::split(ui, 0.56, &mut state, left, right);
    } else {
        left(&mut state, ui);
        ui.add_space(12.0);
        right(&mut state, ui);
    }
}

/// Every key that does something else than its own job, at a glance; a
/// click opens it in the editor.
fn assignments(p: &Profile, selected: &mut u8, marked: &[u8], ui: &mut egui::Ui) {
    card_titled(ui, t("Assignments"), |ui| {
        if marked.is_empty() {
            hint(ui, t("All buttons work as usual."));
            return;
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        for &id in marked {
            let k = p.key(id);
            let mut value = target_label(id, k.target);
            if k.turbo() {
                value += &apex5::tr!(" · turbo {rate}/s", rate = k.turbo_freq);
            }
            let colour = if id == *selected {
                accent()
            } else {
                theme::text()
            };
            if widgets::link_row(ui, &key_title(id), &value, colour)
                .on_hover_text(t("Set up this button"))
                .clicked()
            {
                *selected = id;
            }
        }
        ui.add_space(6.0);
        hint(
            ui,
            &apex5::tr!(
                "The other buttons ({count}) work as usual.",
                count = PAD_KEYS.len() - marked.len()
            ),
        );
    });
}

fn key_editor(p: &mut Profile, id: u8, macros: &[Macro], ui: &mut egui::Ui) {
    let mut k = p.key(id);
    let before = k;
    card(ui, |ui| {
        ui.label(
            RichText::new(key_title(id))
                .font(theme::bold(20.0))
                .color(theme::text()),
        );
        ui.add_space(6.0);

        widgets::caption(ui, t("Works as"));
        if k.target == TARGET_MACRO {
            ui.label(RichText::new(t("A macro is assigned")).color(accent()));
            hint(ui, t("You can change or delete it on the \"Macros\" tab."));
        } else {
            let own = k.target == TARGET_IDENTITY || k.target == id || k.target > 32;
            let gap = 6.0;
            let cols = 4;
            let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
            let mut options: Vec<(u8, String)> = vec![(TARGET_IDENTITY, t("own").into())];
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
                    RichText::new(t(
                        "A keyboard key is assigned — this doesn't work on Linux.",
                    ))
                    .color(theme::warn()),
                );
            }
            if macros.iter().all(|m| m.key != id) {
                hint(
                    ui,
                    t("A macro for this button can be added on the \"Macros\" tab."),
                );
            }
        }

        ui.add_space(12.0);
        widgets::caption(ui, t("Turbo"));
        let mut mode = if k.turbo() {
            k.turbo_mode.clamp(1, 2)
        } else {
            0u8
        };
        let picked = ui
            .add_enabled_ui(k.target != TARGET_MACRO, |ui| {
                segmented(
                    ui,
                    &mut mode,
                    &[
                        (0, t("Off#turbo")),
                        (1, t("While held#turbo")),
                        (2, t("Toggle")),
                    ],
                )
            })
            .inner;
        // only what is picked here is written: looking at a key must not edit it
        if picked {
            if mode == 0 {
                k.turbo_freq = 0;
                k.turbo_mode = 0;
            } else {
                k.turbo_mode = mode;
                if k.turbo_freq == 0 {
                    k.turbo_freq = 10;
                }
            }
        }
        if mode != 0 {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Frequency")).color(theme::dim()));
                ui.spacing_mut().slider_width = (ui.available_width() - 150.0).max(80.0);
                ui.add(widgets::slider(&mut k.turbo_freq, 1..=40).suffix(t(" /s")))
                    .on_hover_text(t("Presses per second"));
            });
            hint(
                ui,
                if mode == 1 {
                    t("While the button is held, the controller repeats the press.")
                } else {
                    t("One press starts auto-repeat, the next one stops it.")
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
        theme::hover()
    } else {
        theme::card_hi()
    };
    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius::same(8),
        widgets::lerp_colour(bg, accent(), t),
        Stroke::new(1.0, widgets::lerp_colour(theme::border(), accent(), t)),
        StrokeKind::Inside,
    );
    let colour = if selected {
        theme::on_accent()
    } else {
        theme::text()
    };
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
        let what = apex5::tr!(
            "Up to {macros} macros and {steps} steps per profile ({total} used). Times go in 10 ms steps.",
            macros = profile::MACRO_MAX,
            steps = profile::MACRO_STEPS_MAX,
            total
        );
        widgets::row(ui, t("The controller runs macros itself"), &what, |ui| {
            if macros.len() >= profile::MACRO_MAX {
                return;
            }
            let used: Vec<u8> = macros.iter().map(|m| m.key).collect();
            egui::ComboBox::from_id_salt("newmacro")
                .selected_text(t("+ New macro"))
                .width(170.0)
                .show_ui(ui, |ui| {
                    for (id, name) in PAD_KEYS {
                        if !used.contains(&id)
                            && ui
                                .selectable_label(false, apex5::tr!("on button {name}", name))
                                .clicked()
                        {
                            macros.push(Macro {
                                key: id,
                                kind: 1,
                                interval_ms: None,
                                steps: vec![],
                            });
                        }
                    }
                });
        });
    });

    let kinds = [
        (1u8, t("Once")),
        (2, t("While held")),
        (3, t("On/off by press")),
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
                        RichText::new(apex5::tr!("Macro on button {key}", key = key_name(m.key)))
                            .font(theme::medium(15.0))
                            .color(theme::text()),
                    );
                    ui.label(
                        RichText::new(apex5::tr!("steps: {count}", count = m.steps.len()))
                            .size(12.0)
                            .color(theme::dim()),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(t("Delete")).color(theme::bad()))
                                .frame(false),
                        )
                        .clicked()
                    {
                        remove = Some(mi);
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                segmented(ui, &mut m.kind, &kinds);
                if m.kind != 1 {
                    let mut iv = m.interval_ms.unwrap_or(30);
                    ui.label(RichText::new(t("pause between repeats")).color(theme::dim()));
                    if ui
                        .add(
                            egui::DragValue::new(&mut iv)
                                .range(0..=2540)
                                .speed(10)
                                .suffix(t(" ms")),
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
                    .fill(theme::card_hi())
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{:>2}", si + 1))
                                    .monospace()
                                    .color(theme::faint()),
                            );
                            ui.add(
                                egui::DragValue::new(&mut st.delay_ms)
                                    .range(0..=8000)
                                    .speed(10)
                                    .prefix(t("after "))
                                    .suffix(t(" ms")),
                            );
                            segmented(
                                ui,
                                &mut st.press,
                                &[(true, t("press")), (false, t("release"))],
                            );
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
                                            egui::Button::new(
                                                RichText::new("✕").color(theme::dim()),
                                            )
                                            .frame(false),
                                        )
                                        .on_hover_text(t("Delete step"))
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
                    t("+ Button press"),
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
                if widgets::secondary(ui, t("+ Step"), total < profile::MACRO_STEPS_MAX).clicked() {
                    m.steps.push(MacroStep {
                        delay_ms: d,
                        key,
                        press: true,
                    });
                }
                let presses = m.steps.iter().filter(|s| s.press).count();
                if presses * 2 != m.steps.len() {
                    ui.label(
                        RichText::new(t("presses and releases don't match")).color(theme::warn()),
                    );
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
    ui.label(
        RichText::new(t("You can drag the points on the graph."))
            .size(12.5)
            .color(theme::dim()),
    )
    .on_hover_text(t(
        "The controller plays a 9-point curve; it is recomputed from these settings.",
    ));
}

fn stick_card(p: &mut Profile, side: usize, live: Option<(f32, f32)>, ui: &mut egui::Ui) {
    let mut s = p.stick(side);
    let before = s;
    card_titled(
        ui,
        if side == 0 {
            t("Left stick")
        } else {
            t("Right stick")
        },
        |ui| {
            let mut kind = s.kind;
            let mut items: Vec<(u8, &str)> = STICK_PRESETS
                .iter()
                .map(|(k, n, _, _)| (*k, t(n)))
                .collect();
            items.push((3, t("Custom")));
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
            ui.spacing_mut().slider_width = (ui.available_width() - 270.0).max(90.0);
            egui::Grid::new(("st", side))
                .num_columns(2)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    ui.label(RichText::new(t("Center")).color(theme::dim()))
                        .on_hover_text(t(
                            "Right — dead zone: the stick is silent near the center.\n\
                         Left — offset: the output starts right at this value, \
                         to cover the game's built-in dead zone.",
                        ));
                    custom |= zone_slider(ui, &mut s.center, |v| match v {
                        v if v < 0 => apex5::tr!("offset {percent} %", percent = -v),
                        v => apex5::tr!("dead zone {v} %", v),
                    });
                    ui.end_row();
                    ui.label(RichText::new(t("Edge")).color(theme::dim()))
                        .on_hover_text(t("Right — outer zone: full output before the stop.\n\
                         Left — ceiling: even at the stop the output stays below 100 %, \
                         for precise aiming."));
                    custom |= zone_slider(ui, &mut s.edge, |v| match v {
                        v if v < 0 => apex5::tr!("ceiling {percent} %", percent = 100 + v),
                        v => apex5::tr!("outer zone {v} %", v),
                    });
                    ui.end_row();
                    ui.label(RichText::new(t("Gate shape")).color(theme::dim()));
                    segmented(
                        ui,
                        &mut s.round,
                        &[(true, t("Circle")), (false, t("Square"))],
                    );
                    ui.end_row();
                });
            if custom {
                s.kind = 3;
            }
            if s.center < 0 {
                ui.add_space(4.0);
                hint(
                    ui,
                    t(
                        "With a center offset, stick auto-calibration doesn't work (so Flydigi says).",
                    ),
                );
            }
        },
    );
    if s != before {
        p.set_stick(side, s);
    }
}

/// A two-sided -100..100 % zone slider; `name` words a non-zero value.
fn zone_slider(ui: &mut egui::Ui, v: &mut i8, name: impl Fn(i32) -> String + 'static) -> bool {
    ui.add(
        widgets::slider(v, -100..=100).custom_formatter(move |v, _| match v as i32 {
            0 => "0".into(),
            v => name(v),
        }),
    )
    .changed()
}

/// The stick settings with the zones clamped and the bank recompiled, as the
/// pad would play them.
fn compiled(s: &profile::Stick) -> profile::Stick {
    let mut c = *s;
    c.clamp_zones();
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
            p.circle(c, r, theme::well(), Stroke::new(1.0, theme::border()));
        } else {
            p.rect(
                egui::Rect::from_center_size(c, vec2(2.0 * r, 2.0 * r)),
                CornerRadius::same(10),
                theme::well(),
                Stroke::new(1.0, theme::border()),
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
            Stroke::new(1.0, theme::grid()),
        );
        p.line_segment(
            [pos2(c.x, c.y - r), pos2(c.x, c.y + r)],
            Stroke::new(1.0, theme::grid()),
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
                p.circle_stroke(raw, 6.0, Stroke::new(1.5, theme::dim()));
                p.circle_filled(out, 11.0, accent().gamma_multiply(0.2));
                p.circle_filled(out, 6.0, accent());
            }
            None => {
                p.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    t("move the\nstick"),
                    egui::FontId::proportional(12.0),
                    theme::faint(),
                );
            }
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (rr, _) = ui.allocate_exact_size(vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter()
                .circle_stroke(rr.center(), 4.0, Stroke::new(1.5, theme::dim()));
            ui.label(RichText::new(t("stick")).size(11.5).color(theme::dim()));
            let (rr, _) = ui.allocate_exact_size(vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(rr.center(), 4.0, accent());
            ui.label(RichText::new(t("in game")).size(11.5).color(theme::dim()));
        });
    });
}

/// Response curve with two draggable control points. Returns true if dragged.
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
    pt.rect_filled(rect, CornerRadius::same(10), theme::well());
    for i in 1..4 {
        let f = i as f32 * 25.0;
        pt.line_segment(
            [map(f, 0.0), map(f, 100.0)],
            Stroke::new(1.0, theme::grid()),
        );
        pt.line_segment(
            [map(0.0, f), map(100.0, f)],
            Stroke::new(1.0, theme::grid()),
        );
    }
    pt.line_segment(
        [map(0.0, 0.0), map(100.0, 100.0)],
        Stroke::new(1.0, theme::grid_hi()),
    );
    pt.text(
        map(1.0, 99.0),
        egui::Align2::LEFT_TOP,
        t("output"),
        egui::FontId::proportional(11.0),
        theme::faint(),
    );
    pt.text(
        map(99.0, 1.0),
        egui::Align2::RIGHT_BOTTOM,
        t("stick deflection"),
        egui::FontId::proportional(11.0),
        theme::faint(),
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
            Stroke::new(1.0, theme::strong().gamma_multiply(0.24)),
        );
        pt.line_segment(
            [map(0.0, out), map(m, out)],
            Stroke::new(1.0, theme::strong().gamma_multiply(0.24)),
        );
        pt.circle_filled(map(m, out), 5.0, theme::strong());
    }

    // Control points, drawn where their nodes sit between start and end.
    let mut dragged = false;
    let nodes = shown.nodes();
    if nodes.len() < 4 {
        return false;
    }
    for i in 0..2 {
        let (nx, ny) = nodes[i + 1];
        let at = map(nx as f32, ny as f32);
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
            theme::bg(),
            Stroke::new(2.5, accent()),
        );
        if r.dragged()
            && let Some(pos) = r.interact_pointer_pos()
        {
            let (x, y) = unmap(pos);
            let v = shown.point_at(x as f64, y as f64);
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

// Space Station's own labels for 2 and 3 are swapped against the modes.
fn modes() -> [(u8, &'static str, &'static str); 6] {
    [
        (0, t("Normal"), t("Trigger is free")),
        (1, t("Racing"), t("Constant resistance")),
        (3, t("Breakpoint"), t("A stop that gives way")),
        (4, t("Lock"), t("Hard stop, short travel")),
        (MODE_SNIPER, t("Burst"), t("Shakes past a point of travel")),
        (
            MODE_VIBRATION,
            t("Game vibration"),
            t("Grip rumble in the trigger"),
        ),
    ]
}

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
    ui.label(
        RichText::new(t("Trigger modes take effect after \"Apply\"."))
            .size(12.5)
            .color(theme::dim()),
    )
    .on_hover_text(t(
        "\"Game vibration\" stays while the controller is connected: if you switch profiles with \
         the controller's button, it stays until reconnecting or switching profiles in the app.",
    ));
}

/// Grip rumble from games: on or off, and each motor's window and strength.
fn grip_vibration_card(p: &mut Profile, ui: &mut egui::Ui) {
    let g = p.grip_vibration();
    let mut on = g.master && (g.left.on || g.right.on);
    card_titled(ui, t("Grip rumble"), |ui| {
        if widgets::toggle_row(
            ui,
            t("Rumble from games"),
            t("The grip motors, driven by the game"),
            &mut on,
        ) {
            p.set_grip_vibration(on);
        }
        ui.add_space(8.0);
        ui.add_enabled_ui(on, |ui| {
            ui.spacing_mut().slider_width = ((ui.available_width() - 150.0) / 2.0 - 70.0).max(80.0);
            let mut motors = [g.left, g.right];
            egui::Grid::new("grip_motors")
                .num_columns(3)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    ui.label("");
                    ui.label(RichText::new(t("Left motor")).color(theme::dim()));
                    ui.label(RichText::new(t("Right motor")).color(theme::dim()));
                    ui.end_row();
                    let row = |ui: &mut egui::Ui,
                               name: &str,
                               tip: &str,
                               motors: &mut [GripMotor; 2],
                               field: fn(&mut GripMotor) -> &mut u8,
                               slider: fn(&mut u8) -> egui::Slider<'_>| {
                        ui.label(RichText::new(name).color(theme::dim())).on_hover_text(tip);
                        for m in motors.iter_mut() {
                            ui.add(slider(field(m)));
                        }
                        ui.end_row();
                    };
                    row(
                        ui,
                        t("Strength"),
                        t("How much of the game's vibration reaches the motor. In Space Station this is the \"grip vibration strength\", 50% from the factory."),
                        &mut motors,
                        |m| &mut m.scale,
                        |v| widgets::slider(v, 1..=100).suffix("%"),
                    );
                    row(
                        ui,
                        t("Minimum#motor"),
                        t("If the game rumbles at all, the motor runs no weaker than this."),
                        &mut motors,
                        |m| &mut m.min,
                        level_slider,
                    );
                    row(
                        ui,
                        t("Maximum#motor"),
                        t("The motor never runs stronger than this."),
                        &mut motors,
                        |m| &mut m.max,
                        level_slider,
                    );
                });
            for (side, m) in motors.into_iter().enumerate() {
                if m != [g.left, g.right][side] {
                    p.set_grip_motor(side, m);
                }
            }
        });
        ui.add_space(4.0);
        hint(
            ui,
            t("Reaches the controller after \"Apply\". You can test the motors in \"Input test\"."),
        );
    });
}

/// A 0..255 rumble level shown in percent.
fn level_slider(v: &mut u8) -> egui::Slider<'_> {
    widgets::slider(v, 0..=255)
        .custom_formatter(|v, _| format!("{:.0}%", v / 2.55))
        .custom_parser(|t| {
            t.trim_end_matches('%')
                .trim()
                .parse::<f64>()
                .ok()
                .map(|p| p * 2.55)
        })
}

fn trigger_card(p: &mut Profile, side: usize, ui: &mut egui::Ui) {
    card_titled(
        ui,
        if side == 0 {
            t("Left trigger · LT")
        } else {
            t("Right trigger · RT")
        },
        |ui| {
            ui.label(
                RichText::new(t("Travel"))
                    .font(theme::medium(14.5))
                    .color(theme::text()),
            )
            .on_hover_text(t(
                "Move the end left so a short press fires the trigger fully.",
            ));
            let (mut z, mut e) = p.travel(side);
            if travel_bar(ui, &mut z, &mut e, side) {
                p.set_travel(side, z, e);
            }
            ui.add_space(12.0);

            ui.label(
                RichText::new(t("Mode"))
                    .font(theme::medium(14.5))
                    .color(theme::text()),
            );
            let mut f = p.force(side);
            if f.mode >= FORCE_MODES {
                ui.label(
                    RichText::new(apex5::tr!("The profile holds an unknown mode ({mode}). You can replace it with one of the modes below.", mode = f.mode))
                    .color(theme::warn()),
                );
            }
            let before = f;
            let gap = 6.0;
            let w = ((ui.available_width() - gap) / 2.0).floor();
            for pair in modes().chunks(2) {
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
            ui.spacing_mut().slider_width = (ui.available_width() - 230.0).max(90.0);
            let mode = f.mode;
            let (pr, bind) = (&mut f.params, &mut f.bind);
            let row = |ui: &mut egui::Ui, name: &str, tip: &str, slider: egui::Slider<'_>| {
                ui.label(RichText::new(name).color(theme::dim()))
                    .on_hover_text(tip);
                ui.add(slider);
                ui.end_row();
            };
            egui::Grid::new(("force", side))
                .num_columns(2)
                .spacing([14.0, 8.0])
                .show(ui, |ui| match mode {
                    1 => {
                        ui.label(RichText::new(t("Start")).color(theme::dim()));
                        ui.add(widgets::slider(&mut pr[0], 0..=192));
                        ui.end_row();
                        ui.label(RichText::new(t("Strength")).color(theme::dim()));
                        ui.add(widgets::slider(&mut pr[1], 1..=255));
                        ui.end_row();
                    }
                    MODE_SNIPER => {
                        let [start, press, strength, freq, ..] = pr;
                        row(
                            ui,
                            t("Start"),
                            t("Where in the travel the trigger starts to shake."),
                            widgets::slider(start, 0..=192),
                        );
                        row(
                            ui,
                            t("Press force"),
                            t("How hard to press at this point for the shaking to start."),
                            widgets::slider(press, 1..=255),
                        );
                        row(
                            ui,
                            t("Strength"),
                            t("Shake strength."),
                            widgets::slider(strength, 1..=255),
                        );
                        row(
                            ui,
                            t("Frequency"),
                            t("Shake frequency."),
                            widgets::slider(freq, 1..=255),
                        );
                        ui.label(RichText::new(t("Respond to pressure")).color(theme::dim()));
                        let mut m = pr[4] == 1;
                        if widgets::toggle(ui, &mut m).changed() {
                            pr[4] = m as u8;
                        }
                        ui.end_row();
                    }
                    3 => {
                        ui.label(RichText::new(t("Start")).color(theme::dim()));
                        ui.add(widgets::slider(&mut pr[0], 0..=192));
                        ui.end_row();
                        ui.label(RichText::new(t("Stop length")).color(theme::dim()));
                        ui.add(widgets::slider(&mut pr[1], 1..=255));
                        ui.end_row();
                        ui.label(RichText::new(t("Strength")).color(theme::dim()));
                        ui.add(widgets::slider(&mut pr[2], 1..=255));
                        ui.end_row();
                        ui.label(RichText::new(t("Respond to pressure")).color(theme::dim()));
                        let mut m = pr[4] == 1;
                        if widgets::toggle(ui, &mut m).changed() {
                            pr[4] = m as u8;
                        }
                        ui.end_row();
                    }
                    4 => {
                        ui.label(RichText::new(t("Stop point")).color(theme::dim()));
                        ui.add(widgets::slider(&mut pr[0], 20..=200));
                        ui.end_row();
                    }
                    MODE_VIBRATION => {
                        let [stroke, freq, ..] = pr;
                        row(
                            ui,
                            t("Travel#vibration"),
                            t("Which part of the travel vibrates."),
                            widgets::slider(stroke, 1..=200),
                        );
                        row(
                            ui,
                            t("Frequency"),
                            t("Trigger vibration frequency."),
                            widgets::slider(freq, 1..=255),
                        );
                        row(
                            ui,
                            t("Threshold"),
                            t("Grip rumble below the threshold doesn't reach the trigger."),
                            widgets::slider(&mut bind.block, 1..=255),
                        );
                        row(
                            ui,
                            t("Strength"),
                            t("Trigger vibration strength factor."),
                            widgets::slider(&mut bind.scale, 0..=200),
                        );
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
    p.rect_filled(rail, CornerRadius::same(4), theme::card_hi());
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
            theme::palette().knob,
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
        apex5::tr!("start {percent}%", percent = pct(*start)),
        egui::FontId::proportional(12.0),
        theme::dim(),
    );
    ui.painter().text(
        pos2(rail.right(), rect.bottom() - 6.0),
        egui::Align2::RIGHT_BOTTOM,
        apex5::tr!("end {percent}%", percent = pct(*end)),
        egui::FontId::proportional(12.0),
        theme::dim(),
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
        MODE_SNIPER => {
            params[..4].copy_from_slice(&[64, 1, 160, 60]);
            params[4] = 1;
        }
        // the bind bytes keep the last Vibration settings (factory: stroke
        // 100, frequency 70, block 10, scale 10)
        MODE_VIBRATION => {
            params[0] = cur.bind.params[0].max(1);
            params[1] = cur.bind.params[3].max(1);
        }
        _ => params = cur.params,
    }
    let mut bind = cur.bind;
    if mode == MODE_VIBRATION {
        // Space Station's editor always writes 1, 1 here; only a game
        // preset sets other values
        bind.params[1..3].copy_from_slice(&[1, 1]);
    }
    ForceEffect { mode, params, bind }
}

// ------------------------------------------------------------------ gyro

fn gyro(p: &mut Profile, ui: &mut egui::Ui) {
    let mut g = p.gyro();
    let before = g;
    card(ui, |ui| {
        ui.label(
            RichText::new(t("Gyro as a stick"))
                .font(theme::bold(18.0))
                .color(theme::text()),
        );
        hint(
            ui,
            t("Tilting the controller moves the chosen stick — handy for precise aiming."),
        );
        ui.add_space(8.0);
        segmented(
            ui,
            &mut g.kind,
            &[
                (0, t("Off#gyro")),
                (2, t("Right stick (aim)")),
                (1, t("Left stick (steering)")),
            ],
        );
        ui.add_space(10.0);
        ui.add_enabled_ui(g.kind != 0, |ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 230.0).clamp(100.0, 360.0);
            egui::Grid::new("gyro")
                .num_columns(2)
                .spacing([20.0, 10.0])
                .show(ui, |ui| {
                    ui.label(RichText::new(t("Enable button")).color(theme::dim()));
                    key_combo(ui, "gk", &mut g.key);
                    ui.end_row();
                    ui.label(RichText::new(t("How to enable")).color(theme::dim()));
                    segmented(
                        ui,
                        &mut g.hold,
                        &[(true, t("While held")), (false, t("Press toggles"))],
                    );
                    ui.end_row();
                    if g.hold {
                        ui.label(RichText::new(t("Second button")).color(theme::dim()));
                        key_combo(ui, "gk2", &mut g.key2);
                        ui.end_row();
                    }
                    ui.label(RichText::new(t("Dead zone")).color(theme::dim()));
                    ui.add(widgets::slider(&mut g.deadzone, 0..=100));
                    ui.end_row();
                    ui.label(RichText::new(t("X sensitivity")).color(theme::dim()));
                    ui.add(widgets::slider(&mut g.sens_x, 0..=100));
                    ui.end_row();
                    ui.label(RichText::new(t("Y sensitivity")).color(theme::dim()));
                    ui.add(widgets::slider(&mut g.sens_y, 0..=100));
                    ui.end_row();
                });
        });
        if g.hold && g.key2 == 0 && g.kind != 0 {
            ui.add_space(4.0);
            ui.label(RichText::new(t("The second button is now \"D-pad up\" (the factory value) — it also turns the gyro on.")).color(theme::warn()));
        }
    });
    if g != before {
        p.set_gyro(g);
    }
}

fn key_combo(ui: &mut egui::Ui, id: &str, key: &mut u8) {
    let text = if *key == 255 {
        t("none").to_string()
    } else {
        key_name(*key)
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .show_ui(ui, |ui| {
            ui.selectable_value(key, 255, t("none"));
            for (k, n) in PAD_KEYS {
                ui.selectable_value(key, k, n);
            }
        });
}
