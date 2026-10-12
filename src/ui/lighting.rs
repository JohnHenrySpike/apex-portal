//! Lighting: effect, colours, brightness, speed and a live preview.

use std::time::{Duration, Instant};

use apex5::lighting::{Effect, LedConfig, hue, left_to_right};
use eframe::egui::{self, Color32, CornerRadius, RichText, Stroke, StrokeKind, pos2, vec2};

use super::theme::{self, BORDER, CARD_HI, DIM, HOVER, TEXT, WARN, accent};
use super::widgets::{self, card, card_titled, hint};
use super::{header, pad};
use crate::App;

/// Seconds per unit of the stored cycle time. The pad's unit is not documented;
/// this is the value that matches what the hardware shows.
const TICK: f64 = 0.1;

pub fn effect_name(e: Effect) -> &'static str {
    match e {
        Effect::Off => "Выключена",
        Effect::Solid => "Один цвет",
        Effect::Breath => "Дыхание",
        Effect::Flow => "Перетекание",
        Effect::Rotation => "Вращение",
        Effect::Wave => "Волна",
        Effect::Rainbow => "Радуга",
        Effect::Flash => "Мигание",
        Effect::StaticMulti => "Градиент",
    }
}

const PRESETS: [[u8; 3]; 10] = [
    [255, 0, 0],
    [255, 70, 0],
    [255, 170, 0],
    [255, 240, 0],
    [0, 255, 60],
    [0, 255, 200],
    [0, 200, 255],
    [0, 70, 255],
    [150, 0, 255],
    [255, 0, 160],
];

/// The colours the pad shows at time `t`, cross-fading between stored frames.
pub fn current_frame(cfg: &LedConfig, t: f64) -> Vec<[u8; 3]> {
    let (start, end) = cfg.loop_range();
    let last = cfg.frames().saturating_sub(1);
    let (start, end) = (start.min(last), end.min(last).max(start.min(last)));
    let frames = end - start + 1;
    let step = cfg.cycle().max(1) as f64 * TICK;
    let pos = t / step;
    let i = pos.floor() as usize % frames;
    let f = pos.fract() as f32;
    let (a, b) = (start + i, start + (i + 1) % frames);
    (0..cfg.leds())
        .map(|l| {
            let (x, y) = (cfg.led(a, l), cfg.led(b, l));
            std::array::from_fn(|k| (x[k] as f32 + (y[k] as f32 - x[k] as f32) * f) as u8)
        })
        .collect()
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    header(app, ui, "Подсветка", "Изменения сразу видны на геймпаде");
    if app.led.is_none() {
        ui.add(egui::Spinner::new().color(accent()));
        return;
    }

    let wide = ui.available_width() > 860.0;
    if wide {
        widgets::split(ui, 0.52, app, preview_card, controls);
    } else {
        preview_card(app, ui);
        ui.add_space(12.0);
        controls(app, ui);
    }
}

fn preview_card(app: &mut App, ui: &mut egui::Ui) {
    let Some(led) = app.led.as_ref() else { return };
    let t = ui.input(|i| i.time);
    let frame = current_frame(&led.cfg, t);
    let bright = led.cfg.brightness() as f32 / 100.0;
    card(ui, |ui| {
        let w = ui.available_width();
        let screen = format!("P{}", app.active + 1);
        pad::show(
            ui,
            vec2(w, w * 0.67),
            &pad::Opts {
                leds: Some(&frame),
                brightness: bright,
                screen: Some(&screen),
                ..Default::default()
            },
        );
        strip(ui, &frame, bright);
    });
    if led.effect.animated() {
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

/// The bar under the page: whether the pad keeps this lighting, Save and
/// Undo.
pub fn action_bar(app: &mut App, ui: &mut egui::Ui) {
    if app.led.is_none() {
        return;
    }
    let dirty = app.led_dirty();
    let (dot, title, sub) = if dirty {
        (
            WARN,
            "Подсветка изменена",
            "Видна на геймпаде, но сбросится, когда он уснёт",
        )
    } else {
        (theme::OK, "Всё сохранено", "Подсветка сохранена в геймпаде")
    };
    widgets::save_bar(ui, "lighting_actions", dot, title, sub, |ui| {
        let busy = app.busy;
        if widgets::primary(ui, "Сохранить", dirty && !busy)
            .on_hover_text("Сохранить подсветку во флеш-память геймпада")
            .clicked()
        {
            app.save_led();
        }
        if widgets::secondary(ui, "Отменить", dirty && !busy && app.led_saved.is_some())
            .on_hover_text("Вернуть подсветку, какой она была до изменений")
            .clicked()
        {
            app.revert_led();
        }
    });
}

/// The 12 LEDs unrolled into one bar.
fn strip(ui: &mut egui::Ui, frame: &[[u8; 3]], bright: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(7), Color32::from_rgb(10, 12, 15));
    let n = frame.len().max(1);
    let w = rect.width() / n as f32;
    let k = 0.3 + 0.7 * bright;
    for (i, c) in left_to_right(frame.len())
        .iter()
        .map(|&s| &frame[s])
        .enumerate()
    {
        let r = egui::Rect::from_min_size(
            pos2(rect.left() + i as f32 * w, rect.top()),
            vec2(w, rect.height()),
        )
        .shrink2(vec2(2.0, 3.0));
        let col = Color32::from_rgb(
            (c[0] as f32 * k) as u8,
            (c[1] as f32 * k) as u8,
            (c[2] as f32 * k) as u8,
        );
        p.rect_filled(r, CornerRadius::same(3), col);
    }
}

fn controls(app: &mut App, ui: &mut egui::Ui) {
    let Some(led) = app.led.as_mut() else { return };
    let mut changed = false;

    card_titled(ui, "Эффект", |ui| {
        let cols = 3;
        let gap = 8.0;
        let w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
        for chunk in Effect::ALL.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &e in chunk {
                    if effect_tile(ui, w, e, led.effect == e, &led.colours) && led.effect != e {
                        led.effect = e;
                        if matches!(e, Effect::Solid | Effect::Breath | Effect::Flash) {
                            led.colours.truncate(1);
                        }
                        changed = true;
                    }
                }
            });
        }
    });

    if led.effect.uses_colours() {
        ui.add_space(12.0);
        card_titled(ui, "Цвета", |ui| {
            let multi = !matches!(led.effect, Effect::Solid);
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size = vec2(46.0, 34.0);
                let mut remove = None;
                let can_remove = multi && led.colours.len() > 1;
                for (i, c) in led.colours.iter_mut().enumerate() {
                    let r = ui.color_edit_button_srgb(c);
                    changed |= r.changed();
                    let r = r.on_hover_text(if can_remove {
                        "Нажмите, чтобы изменить. ПКМ — удалить"
                    } else {
                        "Нажмите, чтобы изменить"
                    });
                    if can_remove && r.secondary_clicked() {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    led.colours.remove(i);
                    changed = true;
                }
                if multi && led.colours.len() < 5 {
                    let r = ui.add(
                        egui::Button::new(RichText::new("+").size(18.0)).min_size(vec2(46.0, 34.0)),
                    );
                    if r.on_hover_text("Добавить цвет").clicked() {
                        led.colours.push(hue(led.colours.len() as f32 / 5.0 + 0.33));
                        changed = true;
                    }
                }
            });
            if multi {
                hint(ui, "До пяти цветов. Правый клик по цвету — удалить.");
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for p in PRESETS {
                    let (rect, r) = ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::click());
                    let c = Color32::from_rgb(p[0], p[1], p[2]);
                    if r.hovered() {
                        ui.painter()
                            .circle_filled(rect.center(), 13.0, c.gamma_multiply(0.3));
                    }
                    ui.painter().circle(
                        rect.center(),
                        10.0,
                        c,
                        Stroke::new(1.0, Color32::from_black_alpha(80)),
                    );
                    if r.on_hover_text("Сделать первым цветом").clicked() {
                        if let Some(first) = led.colours.first_mut() {
                            *first = p;
                        }
                        changed = true;
                    }
                }
            });
        });
    }

    ui.add_space(12.0);
    card_titled(ui, "Яркость и скорость", |ui| {
        ui.spacing_mut().slider_width = (ui.available_width() - 200.0).max(100.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [90.0, 24.0],
                egui::Label::new(RichText::new("Яркость").color(DIM)),
            );
            let mut b = led.cfg.brightness();
            if ui
                .add(widgets::slider(&mut b, 0..=100).suffix("%"))
                .changed()
            {
                led.cfg.set_brightness(b);
                led.dirty_since = Some(Instant::now());
            }
        });
        if led.effect.animated() {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [90.0, 24.0],
                    egui::Label::new(RichText::new("Скорость").color(DIM)),
                );
                // Stored as a cycle time (bigger = slower); shown as speed.
                let mut speed = 31 - led.cfg.cycle().clamp(1, 30);
                if ui.add(widgets::slider(&mut speed, 1..=30)).changed() {
                    led.cfg.set_cycle(31 - speed);
                    led.dirty_since = Some(Instant::now());
                }
            });
        }
    });

    ui.add_space(12.0);
    card(ui, |ui| {
        let mut on = led.cfg.grip_sync();
        if widgets::toggle_row(
            ui,
            "Вспышка при вибрации",
            "Подсветка ярко вспыхивает, пока работают моторы рукояток. Саму вибрацию не включает.",
            &mut on,
        ) {
            led.cfg.set_grip_sync(on);
            led.dirty_since = Some(Instant::now());
        }
    });

    if changed {
        let (effect, colours) = (led.effect, led.colours.clone());
        led.cfg.apply(effect, &colours);
        led.dirty_since = Some(Instant::now());
    }
}

/// Effect picker tile with a tiny preview of the effect's first frames.
fn effect_tile(ui: &mut egui::Ui, w: f32, e: Effect, selected: bool, colours: &[[u8; 3]]) -> bool {
    let (rect, r) = ui.allocate_exact_size(vec2(w, 62.0), egui::Sense::click());
    let t = ui.ctx().animate_bool_responsive(r.id, selected);
    let p = ui.painter();
    let bg = if r.hovered() && !selected {
        HOVER
    } else {
        CARD_HI
    };
    p.rect(
        rect,
        CornerRadius::same(10),
        widgets::lerp_colour(bg, accent().gamma_multiply(0.14), t),
        Stroke::new(
            if selected { 1.5 } else { 1.0 },
            widgets::lerp_colour(BORDER, accent(), t),
        ),
        StrokeKind::Inside,
    );
    p.text(
        pos2(rect.left() + 12.0, rect.top() + 18.0),
        egui::Align2::LEFT_CENTER,
        effect_name(e),
        theme::medium(13.5),
        TEXT,
    );

    // Mini preview: the effect's frames sampled over time.
    let mut cfg = LedConfig {
        slot: 0,
        blob: preview_blob(),
    };
    cfg.apply(e, colours);
    let time = ui.input(|i| i.time);
    let frame = if e.animated() {
        current_frame(&cfg, time)
    } else {
        (0..cfg.leds()).map(|l| cfg.led(0, l)).collect()
    };
    let bar = egui::Rect::from_min_max(
        pos2(rect.left() + 12.0, rect.bottom() - 20.0),
        pos2(rect.right() - 12.0, rect.bottom() - 12.0),
    );
    p.rect_filled(bar, CornerRadius::same(4), Color32::from_rgb(10, 12, 15));
    let n = frame.len().max(1);
    let cw = bar.width() / n as f32;
    for (i, c) in left_to_right(frame.len())
        .iter()
        .map(|&s| &frame[s])
        .enumerate()
    {
        let cr = egui::Rect::from_min_size(
            pos2(bar.left() + i as f32 * cw, bar.top()),
            vec2(cw, bar.height()),
        )
        .shrink(1.0);
        p.rect_filled(
            cr,
            CornerRadius::same(2),
            Color32::from_rgb(c[0], c[1], c[2]),
        );
    }
    if e.animated() {
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
    r.clicked()
}

fn preview_blob() -> Vec<u8> {
    let mut b = vec![0u8; 20 + 10 * 12 * 3];
    b[5] = 4; // cycle
    b[6] = 100;
    b[7] = 12;
    b
}
