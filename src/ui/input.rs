//! Live input tester fed by the pad's vendor input stream.

use std::cell::Cell;
use std::time::Duration;

use apex5::device::{BUTTONS, InputState};
use apex5::tester::{self, Gate, SECTORS};
use apex5::worker::Cmd;
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, pos2, vec2};

use super::theme::{self, CARD_HI, DIM, FAINT, TEXT, accent};
use super::widgets::{self, card, card_titled, hint};
use super::{header, pad};
use crate::App;

/// Input state, pressed key ids, stick axes, triggers in 0..255, LED
/// frame and brightness.
type View = (
    InputState,
    Vec<u8>,
    [(f32, f32); 2],
    (u8, u8),
    Option<Vec<[u8; 3]>>,
    f32,
);

/// Report label → profile key id, for highlighting the drawing.
fn key_id(label: &str) -> Option<u8> {
    Some(match label {
        "↑" => 0,
        "→" => 1,
        "↓" => 2,
        "←" => 3,
        "A" => 4,
        "B" => 5,
        "View" => 6,
        "X" => 7,
        "Y" => 8,
        "Menu" => 9,
        "LB" => 10,
        "RB" => 11,
        "LT" => 12,
        "RT" => 13,
        "L3" => 14,
        "R3" => 15,
        "M1" => 18,
        "M2" => 19,
        "M3" => 20,
        "M4" => 21,
        "LM" => 22,
        "RM" => 23,
        "Home" => 27,
        _ => return None,
    })
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    header(
        app,
        ui,
        "Тест ввода",
        "Нажимайте кнопки и двигайте стики — всё отображается в реальном времени",
    );
    ui.ctx().request_repaint_after(Duration::from_millis(16));
    let Some(s) = app.input() else {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().color(accent()));
                ui.label(
                    RichText::new("Ожидание данных от геймпада… пошевелите стиком").color(DIM),
                );
            });
        });
        ui.add_space(12.0);
        rumble(app, ui);
        return;
    };

    let (rate, gates, out) = app.tester();
    source(app, ui, out.is_some());
    let (pressed, sticks, triggers) = match out.filter(|_| app.tester_output) {
        Some((_, o)) => {
            let byte = |v: f32| (v * 255.0).round() as u8;
            (
                o.pressed(),
                o.sticks,
                (byte(o.triggers.0), byte(o.triggers.1)),
            )
        }
        None => {
            let pressed = BUTTONS
                .iter()
                .filter(|(l, b, m)| *l != "?" && s.pressed(*b, *m))
                .filter_map(|(l, ..)| key_id(l))
                .collect();
            let axis = tester::axis;
            let sticks = [(axis(s.lx), axis(s.ly)), (axis(s.rx), axis(s.ry))];
            (pressed, sticks, (s.lt, s.rt))
        }
    };
    let out = out.map(|(g, _)| g);
    let leds = app
        .led
        .as_ref()
        .map(|l| super::lighting::current_frame(&l.cfg, ui.input(|i| i.time)));
    let bright = app
        .led
        .as_ref()
        .map(|l| l.cfg.brightness() as f32 / 100.0)
        .unwrap_or(0.0);

    let reset = Cell::new(false);
    let wide = ui.available_width() > 860.0;
    let screen = format!("P{}", app.active + 1);
    let st = (s, pressed, sticks, triggers, leds, bright);
    let left = |(s, pressed, sticks, (lt, rt), leds, bright): &mut View, ui: &mut egui::Ui| {
        card(ui, |ui| {
            let w = ui.available_width();
            pad::show(
                ui,
                vec2(w, w * 0.75),
                &pad::Opts {
                    leds: leds.as_deref(),
                    brightness: *bright,
                    pressed: pressed.as_slice(),
                    sticks: Some(*sticks),
                    triggers: Some((*lt as f32 / 255.0, *rt as f32 / 255.0)),
                    show_tiles: true,
                    screen: Some(&screen),
                    ..Default::default()
                },
            );
            let fn_keys: Vec<&str> = BUTTONS
                .iter()
                .filter(|(l, b, m)| matches!(*l, "Fn" | "Turbo" | "Back") && s.pressed(*b, *m))
                .map(|(l, ..)| *l)
                .collect();
            ui.horizontal(|ui| {
                hint(ui, "Служебные кнопки:");
                for name in ["Fn", "Turbo", "Back"] {
                    let on = fn_keys.contains(&name);
                    let text = RichText::new(format!(" {name} ")).font(theme::medium(12.5));
                    ui.label(if on {
                        text.color(theme::on_accent()).background_color(accent())
                    } else {
                        text.color(FAINT)
                    });
                }
                if let Some(hz) = rate {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{hz:.0} Гц")).monospace().color(TEXT));
                        hint(ui, "Частота опроса:");
                    });
                }
            });
        });
    };
    let right = |(s, _, sticks, (lt, rt), _, _): &mut View, ui: &mut egui::Ui| {
        card_titled(ui, "Стики", |ui| {
            ui.horizontal(|ui| {
                let w = ((ui.available_width() - 12.0) / 2.0).min(170.0);
                for (i, name) in ["Левый", "Правый"].into_iter().enumerate() {
                    stick(
                        ui,
                        w,
                        name,
                        sticks[i],
                        &gates[i],
                        out.as_ref().map(|o| &o[i]),
                    );
                }
            });
            ui.add_space(4.0);
            hint(
                ui,
                "Обведите стики по краю: цифра — среднее отклонение от круга.",
            );
            ui.add_space(2.0);
            if widgets::secondary(
                ui,
                "Сбросить след",
                gates.iter().any(|g| g.coverage() > 0.0),
            )
            .on_hover_text("«Датчик» — до обработки профилем, «Для игр» — то, что получают игры.")
            .clicked()
            {
                reset.set(true);
            }
        });
        ui.add_space(12.0);
        card_titled(ui, "Курки", |ui| {
            trigger(ui, "LT", *lt);
            trigger(ui, "RT", *rt);
        });
        ui.add_space(12.0);
        card_titled(ui, "Движение", |ui| {
            for (name, v, k, unit) in [
                ("Гироскоп", s.gyro, 1.0f32 / 32768.0, ""),
                ("Ускорение", s.accel, 1.0 / 16384.0, ""),
            ] {
                ui.label(RichText::new(name).color(DIM));
                for (axis, x) in ["X", "Y", "Z"].iter().zip(v) {
                    bar(ui, axis, x as f32 * k, &format!("{x:>6}{unit}"));
                }
                ui.add_space(4.0);
            }
        });
    };
    // the motor test fills the room under the drawing
    let mut test = (st, app);
    if wide {
        widgets::split(
            ui,
            0.58,
            &mut test,
            |(st, app), ui| {
                left(st, ui);
                ui.add_space(12.0);
                rumble(app, ui);
            },
            |(st, _), ui| right(st, ui),
        );
    } else {
        left(&mut test.0, ui);
        ui.add_space(12.0);
        right(&mut test.0, ui);
        ui.add_space(12.0);
        rumble(test.1, ui);
    }
    if reset.get() {
        test.1.reset_gates();
    }
}

/// Sensor or what games get: picks what the drawing, sticks and triggers
/// show.
fn source(app: &mut App, ui: &mut egui::Ui, have_output: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(have_output, |ui| {
            widgets::segmented(
                ui,
                &mut app.tester_output,
                &[(false, "Датчик"), (true, "Для игр")],
            );
        });
        hint(
            ui,
            match (have_output, app.tester_output) {
                (false, _) => {
                    "Выход для игр недоступен: геймпад не виден как контроллер \
                     (например, его захватил Steam)."
                }
                (true, false) => "Кнопки как нажаты, до профиля.",
                (true, true) => {
                    "Что получают игры: с назначениями, турбо и макросами. \
                     M1–M4, Fn, Turbo и Back игры не видят."
                }
            },
        );
    });
    ui.add_space(8.0);
}

/// Runs the grip motors for under a second at the chosen strengths.
fn rumble(app: &mut App, ui: &mut egui::Ui) {
    card_titled(ui, "Проверка моторов", |ui| {
        ui.spacing_mut().slider_width = (ui.available_width() - 260.0).clamp(80.0, 320.0);
        let (mut l, mut r) = app.rumble;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Левый мотор").color(DIM));
            ui.add(widgets::slider(&mut l, 0..=100).suffix(" %"));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Правый мотор").color(DIM));
            ui.add(widgets::slider(&mut r, 0..=100).suffix(" %"));
        });
        app.rumble = (l, r);
        ui.add_space(4.0);
        let level = |v: u8| (v as u32 * 255 / 100) as u8;
        ui.horizontal(|ui| {
            let idle = !app.busy;
            for (text, left, right) in [("Левый", l, 0), ("Правый", 0, r), ("Оба", l, r)]
            {
                if widgets::secondary(ui, text, idle && left.max(right) > 0).clicked() {
                    app.send(Cmd::TestRumble(level(left), level(right)));
                }
            }
        });
        ui.add_space(4.0);
        hint(ui, "Моторы работают меньше секунды и останавливаются сами.");
    });
}

/// `gate` is the sensor, `out` what games get after the profile.
fn stick(
    ui: &mut egui::Ui,
    w: f32,
    name: &str,
    (x, y): (f32, f32),
    gate: &Gate,
    out: Option<&Gate>,
) {
    ui.vertical(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(w, w), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let c = rect.center();
        let r = w / 2.0 - 6.0;
        p.circle(
            c,
            r,
            Color32::from_rgb(12, 14, 18),
            Stroke::new(1.0, theme::BORDER),
        );
        p.circle_stroke(c, r * 0.5, Stroke::new(1.0, Color32::from_rgb(30, 34, 41)));
        p.line_segment(
            [pos2(c.x - r, c.y), pos2(c.x + r, c.y)],
            Stroke::new(1.0, Color32::from_rgb(30, 34, 41)),
        );
        p.line_segment(
            [pos2(c.x, c.y - r), pos2(c.x, c.y + r)],
            Stroke::new(1.0, Color32::from_rgb(30, 34, 41)),
        );
        // Positive Y is up on the pad, screen Y grows downwards.
        let trace = |g: &Gate, colour: Color32| {
            let at = |i: usize| {
                let a = i as f32 / SECTORS as f32 * std::f32::consts::TAU;
                let k = (g.reach[i] * r).min(w / 2.0 - 1.0);
                pos2(c.x + a.cos() * k, c.y - a.sin() * k)
            };
            for i in 0..SECTORS {
                let j = (i + 1) % SECTORS;
                if g.reach[i] > 0.0 && g.reach[j] > 0.0 {
                    p.line_segment([at(i), at(j)], Stroke::new(1.5, colour));
                }
            }
        };
        trace(gate, DIM);
        if let Some(o) = out {
            trace(o, theme::OK);
        }
        let dot = pos2(c.x + x.clamp(-1.0, 1.0) * r, c.y - y.clamp(-1.0, 1.0) * r);
        p.line_segment([c, dot], Stroke::new(1.5, accent().gamma_multiply(0.5)));
        p.circle_filled(dot, 12.0, accent().gamma_multiply(0.2));
        p.circle_filled(dot, 6.0, accent());
        ui.label(
            RichText::new(format!("{name}  {x:+.2}  {y:+.2}"))
                .monospace()
                .size(12.5)
                .color(DIM),
        );
        roundness(ui, "Датчик", gate, DIM);
        if let Some(o) = out {
            roundness(ui, "Для игр", o, theme::OK);
        }
    });
}

/// How far from a circle the edge of the trace is.
fn roundness(ui: &mut egui::Ui, what: &str, g: &Gate, colour: Color32) {
    let value = match g.error() {
        Some(e) => format!("{e:.1} %"),
        None if g.coverage() > 0.0 => format!("обведено {:.0} %", g.coverage() * 100.0),
        None => "не обведён".into(),
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("━").size(12.5).color(colour));
        ui.label(
            RichText::new(format!("{what}: {value}"))
                .size(12.5)
                .color(DIM),
        );
    });
}

fn trigger(ui: &mut egui::Ui, name: &str, v: u8) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [30.0, 20.0],
            egui::Label::new(RichText::new(name).font(theme::medium(13.5)).color(TEXT)),
        );
        let w = ui.available_width() - 50.0;
        let (rect, _) = ui.allocate_exact_size(vec2(w, 12.0), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(6), CARD_HI);
        let f = egui::Rect::from_min_size(
            rect.min,
            vec2(rect.width() * v as f32 / 255.0, rect.height()),
        );
        p.rect_filled(f, CornerRadius::same(6), accent());
        ui.label(RichText::new(format!("{v:>3}")).monospace().color(DIM));
    });
}

/// Centered bar for a signed value in -1..1.
fn bar(ui: &mut egui::Ui, axis: &str, v: f32, text: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [16.0, 18.0],
            egui::Label::new(RichText::new(axis).color(FAINT)),
        );
        let w = ui.available_width() - 70.0;
        let (rect, _) = ui.allocate_exact_size(vec2(w, 8.0), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(4), CARD_HI);
        let c = rect.center().x;
        let x = c + v.clamp(-1.0, 1.0) * rect.width() / 2.0;
        let f = egui::Rect::from_x_y_ranges(c.min(x)..=c.max(x), rect.y_range());
        p.rect_filled(f, CornerRadius::same(4), accent());
        p.line_segment(
            [pos2(c, rect.top() - 2.0), pos2(c, rect.bottom() + 2.0)],
            Stroke::new(1.0, FAINT),
        );
        ui.label(RichText::new(text).monospace().size(12.0).color(DIM));
    });
}
