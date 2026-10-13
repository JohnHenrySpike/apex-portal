//! Small building blocks shared by every page: cards, switches, segmented
//! controls, buttons and line icons.

use eframe::egui::{
    self, Color32, CornerRadius, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, pos2,
    vec2,
};

use super::theme::{self, accent, on_accent};

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::card())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A card with a small caption at the top.
pub fn card_titled<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    card(ui, |ui| {
        caption(ui, title);
        ui.add_space(4.0);
        add(ui)
    })
}

/// Upper-case, letter-spaced section caption.
pub fn caption(ui: &mut Ui, text: &str) {
    let spaced: String = text
        .to_uppercase()
        .chars()
        .flat_map(|c| [c, '\u{200A}'])
        .collect();
    ui.label(
        RichText::new(spaced)
            .font(theme::medium(11.5))
            .color(theme::dim()),
    );
}

pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(theme::dim()));
}

pub fn page_title(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(
        RichText::new(title)
            .font(theme::bold(26.0))
            .color(theme::text()),
    );
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).color(theme::dim()));
    }
}

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(42.0, 24.0);
    let (rect, mut r) = ui.allocate_exact_size(size, Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(r.id, *on);
        let off_bg = if r.hovered() {
            theme::hover()
        } else {
            theme::card_hi()
        };
        let bg = lerp_colour(off_bg, accent(), t);
        let p = ui.painter();
        p.rect(
            rect,
            CornerRadius::same(12),
            bg,
            Stroke::new(1.0, lerp_colour(theme::border(), accent(), t)),
            StrokeKind::Inside,
        );
        let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
        p.circle_filled(
            pos2(x, rect.center().y),
            8.5,
            if *on { on_accent() } else { theme::dim() },
        );
    }
    r
}

/// A settings row: title and hint on the left, a control on the right. The
/// text wraps in the room the control leaves, measured on the last frame.
pub fn row<R>(ui: &mut Ui, title: &str, hint_text: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    let id = ui.next_auto_id().with("row_control");
    let cw: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(120.0);
    let gap = 16.0;
    ui.horizontal(|ui| {
        let tw = (ui.available_width() - cw - gap).max(120.0);
        // as tall as the row starts, so the centred row puts it at the top
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            vec2(tw, h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(tw);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(theme::medium(14.5))
                            .color(theme::text()),
                    )
                    .wrap(),
                );
                if !hint_text.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(hint_text).size(12.5).color(theme::dim()))
                            .wrap(),
                    );
                }
            },
        );
        let r = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
        // the child's rect hugs what the control drew
        let used = r.response.rect.width();
        if (used - cw).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(id, used));
            ui.ctx().request_discard("row control width");
        }
        r.inner
    })
    .inner
}

pub fn toggle_row(ui: &mut Ui, title: &str, hint_text: &str, on: &mut bool) -> bool {
    row(ui, title, hint_text, |ui| toggle(ui, on).changed())
}

pub fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::border());
}

/// Row of mutually exclusive options. Returns true when the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, items: &[(T, &str)]) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .fill(theme::well())
        .stroke(Stroke::new(1.0, theme::border()))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (v, label) in items {
                    let sel = *value == *v;
                    let font = theme::medium(13.5);
                    let galley =
                        ui.painter()
                            .layout_no_wrap(label.to_string(), font, theme::text());
                    let size = vec2(galley.size().x + 26.0, 28.0);
                    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
                    let t = ui.ctx().animate_bool_responsive(r.id, sel);
                    let bg = if sel {
                        accent().gamma_multiply(0.25 + 0.75 * t)
                    } else if r.hovered() {
                        theme::hover()
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
                    let colour = if sel {
                        on_accent()
                    } else if r.hovered() {
                        theme::text()
                    } else {
                        theme::dim()
                    };
                    ui.painter()
                        .galley(rect.center() - galley.size() / 2.0, galley, colour);
                    if r.clicked() && !sel {
                        *value = *v;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// A slider that only clamps what the user enters. egui's default clamps the
/// value on every frame, so a byte out of range would be written back just
/// by looking at a page.
pub fn slider<Num: egui::emath::Numeric>(
    value: &mut Num,
    range: std::ops::RangeInclusive<Num>,
) -> egui::Slider<'_> {
    egui::Slider::new(value, range).clamping(egui::SliderClamping::Edits)
}

/// Accent-filled call-to-action.
pub fn primary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let b = egui::Button::new(RichText::new(text).color(if enabled {
        on_accent()
    } else {
        theme::faint()
    }))
    .fill(if enabled { accent() } else { theme::card_hi() })
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(10))
    .min_size(vec2(0.0, 34.0));
    ui.add_enabled(enabled, b)
}

pub fn secondary(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let b = egui::Button::new(text)
        .corner_radius(CornerRadius::same(10))
        .min_size(vec2(0.0, 34.0));
    ui.add_enabled(enabled, b)
}

/// The bar under a page that edits what the pad keeps: a status dot, what
/// state the edits are in, and the page's buttons on the right.
pub fn save_bar(
    ui: &mut Ui,
    id: &str,
    dot: Color32,
    title: &str,
    sub: &str,
    buttons: impl FnOnce(&mut Ui),
) {
    egui::Panel::bottom(id.to_owned())
        .frame(
            egui::Frame::new()
                .fill(theme::sidebar())
                .stroke(Stroke::new(1.0, theme::border()))
                .inner_margin(egui::Margin::symmetric(28, 12)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(12.0, 36.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(
                        RichText::new(title)
                            .font(theme::medium(14.0))
                            .color(theme::text()),
                    );
                    ui.label(RichText::new(sub).size(12.0).color(theme::dim()));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), buttons);
            });
        });
}

/// A clickable line: a name on the left, its value and a chevron on the
/// right.
pub fn link_row(ui: &mut Ui, name: &str, value: &str, value_colour: Color32) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let p = ui.painter();
    if r.hovered() {
        p.rect_filled(rect, CornerRadius::same(8), theme::card_hi());
    }
    let y = rect.center().y;
    p.text(
        pos2(rect.left() + 10.0, y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.5),
        theme::dim(),
    );
    let chevron = rect.right() - 12.0;
    let c = if r.hovered() {
        theme::text()
    } else {
        theme::faint()
    };
    p.line_segment(
        [pos2(chevron - 4.0, y - 4.0), pos2(chevron, y)],
        Stroke::new(1.5, c),
    );
    p.line_segment(
        [pos2(chevron, y), pos2(chevron - 4.0, y + 4.0)],
        Stroke::new(1.5, c),
    );
    // the value gives way to the name, cut short if it must
    let name_w = p
        .layout_no_wrap(
            name.to_owned(),
            egui::FontId::proportional(13.5),
            theme::dim(),
        )
        .size()
        .x;
    let room = (chevron - 14.0 - (rect.left() + 10.0 + name_w + 16.0)).max(20.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        value.to_owned(),
        theme::medium(13.5),
        value_colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let g = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(chevron - 14.0 - g.size().x, y - g.size().y / 2.0),
        g,
        value_colour,
    );
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Small rounded tag with an optional status dot.
pub fn pill(ui: &mut Ui, text: &str, dot: Option<Color32>) -> Response {
    let font = theme::medium(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, theme::text());
    let extra = if dot.is_some() { 16.0 } else { 0.0 };
    let (rect, r) =
        ui.allocate_exact_size(vec2(galley.size().x + 22.0 + extra, 26.0), Sense::hover());
    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius::same(13),
        theme::card_hi(),
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );
    let mut x = rect.left() + 11.0;
    if let Some(c) = dot {
        p.circle_filled(pos2(x + 4.0, rect.center().y), 7.0, c.gamma_multiply(0.18));
        p.circle_filled(pos2(x + 4.0, rect.center().y), 3.5, c);
        x += extra;
    }
    p.galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::text(),
    );
    r
}

/// Selectable tile with a title and a description line.
pub fn tile(ui: &mut Ui, size: egui::Vec2, selected: bool, title: &str, sub: &str) -> Response {
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let t = ui.ctx().animate_bool_responsive(r.id, selected);
    let p = ui.painter();
    let bg = if r.hovered() && !selected {
        theme::hover()
    } else {
        theme::card_hi()
    };
    p.rect(
        rect,
        CornerRadius::same(12),
        lerp_colour(bg, accent().gamma_multiply(0.16), t),
        Stroke::new(
            if selected { 1.5 } else { 1.0 },
            lerp_colour(theme::border(), accent(), t),
        ),
        StrokeKind::Inside,
    );
    let tg = p.layout(
        title.to_string(),
        theme::medium(14.5),
        theme::text(),
        rect.width() - 24.0,
    );
    let th = tg.size().y;
    let sg = p.layout(
        sub.to_string(),
        egui::FontId::proportional(12.0),
        theme::dim(),
        rect.width() - 24.0,
    );
    let total = th
        + if sub.is_empty() {
            0.0
        } else {
            sg.size().y + 3.0
        };
    let top = rect.center().y - total / 2.0;
    p.galley(pos2(rect.left() + 12.0, top), tg, theme::text());
    if !sub.is_empty() {
        p.galley(pos2(rect.left() + 12.0, top + th + 3.0), sg, theme::dim());
    }
    r
}

/// One LED of a preview bar: its colour scaled by `k` (brightness) shows as
/// that much light over an unlit LED, so dim LEDs fade instead of going black
/// on a light theme.
pub fn led(p: &egui::Painter, r: Rect, corner: u8, c: [u8; 3], k: f32) {
    let pal = theme::palette();
    let c = c.map(|v| v as f32 * k);
    let m = c[0].max(c[1]).max(c[2]);
    p.rect_filled(r, CornerRadius::same(corner), pal.led_off);
    if m < 1.0 {
        return;
    }
    let lit = Color32::from_rgba_unmultiplied(
        (c[0] * 255.0 / m) as u8,
        (c[1] * 255.0 / m) as u8,
        (c[2] * 255.0 / m) as u8,
        m as u8,
    );
    p.rect(
        r,
        CornerRadius::same(corner),
        lit,
        Stroke::new(1.0, pal.led_rim.gamma_multiply(m / 255.0)),
        StrokeKind::Inside,
    );
}

/// Mixes in premultiplied space, so a see-through colour stays as see-through.
pub fn lerp_colour(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

// ------------------------------------------------------------------ icons

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Home,
    Light,
    Profiles,
    Settings,
    Input,
    Refresh,
    Shield,
    Screen,
    Waves,
    Minimize,
    Maximize,
    Restore,
    Close,
    More,
}

/// Simple line icons drawn into `rect`.
pub fn icon(p: &egui::Painter, rect: Rect, which: Icon, colour: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 2.0;
    let st = Stroke::new(1.7, colour);
    let at = |x: f32, y: f32| pos2(c.x + x * s, c.y + y * s);
    match which {
        Icon::Home => {
            // Gamepad silhouette.
            let body = Rect::from_min_max(at(-0.95, -0.45), at(0.95, 0.55));
            p.rect_stroke(
                body,
                CornerRadius::same((0.45 * s) as u8),
                st,
                StrokeKind::Middle,
            );
            p.line_segment([at(-0.62, 0.05), at(-0.28, 0.05)], st);
            p.line_segment([at(-0.45, -0.12), at(-0.45, 0.22)], st);
            p.circle_filled(at(0.42, -0.05), 0.09 * s, colour);
            p.circle_filled(at(0.62, 0.15), 0.09 * s, colour);
        }
        Icon::Light => {
            p.circle_stroke(c, 0.38 * s, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (x, y) = (a.cos(), a.sin());
                p.line_segment([at(x * 0.62, y * 0.62), at(x * 0.92, y * 0.92)], st);
            }
        }
        Icon::Profiles => {
            for (i, y) in [-0.5f32, 0.0, 0.5].iter().enumerate() {
                let w = 0.9 - i as f32 * 0.0;
                let pts = vec![at(0.0, y - 0.32), at(w, *y), at(0.0, y + 0.32), at(-w, *y)];
                if i == 0 {
                    p.add(egui::Shape::closed_line(pts, st));
                } else {
                    p.line_segment([at(-w, *y), at(0.0, y + 0.32)], st);
                    p.line_segment([at(0.0, y + 0.32), at(w, *y)], st);
                }
            }
        }
        Icon::Settings => {
            for (y, k) in [(-0.55f32, -0.3f32), (0.0, 0.35), (0.55, -0.05)] {
                p.line_segment([at(-0.9, y), at(0.9, y)], st);
                p.circle_filled(at(k, y), 0.2 * s, colour);
            }
        }
        Icon::Input => {
            let pts = [
                (-0.95, 0.0),
                (-0.5, 0.0),
                (-0.28, -0.7),
                (0.05, 0.7),
                (0.3, -0.25),
                (0.48, 0.0),
                (0.95, 0.0),
            ];
            p.add(egui::Shape::line(
                pts.iter().map(|(x, y)| at(*x, *y)).collect(),
                st,
            ));
        }
        Icon::Refresh => {
            let pts: Vec<_> = (0..=20)
                .map(|i| {
                    let a = -0.9 + i as f32 / 20.0 * 4.9;
                    at(0.7 * a.cos(), 0.7 * a.sin())
                })
                .collect();
            let end = *pts.last().unwrap();
            p.add(egui::Shape::line(pts, st));
            p.line_segment([end, end + vec2(0.45 * s, -0.05 * s)], st);
            p.line_segment([end, end + vec2(0.05 * s, -0.45 * s)], st);
        }
        Icon::Shield => {
            let pts = vec![
                at(0.0, -0.95),
                at(0.8, -0.6),
                at(0.7, 0.3),
                at(0.0, 0.95),
                at(-0.7, 0.3),
                at(-0.8, -0.6),
            ];
            p.add(egui::Shape::closed_line(pts, st));
            p.line_segment([at(-0.3, 0.0), at(-0.05, 0.28)], st);
            p.line_segment([at(-0.05, 0.28), at(0.38, -0.25)], st);
        }
        Icon::Screen => {
            // The pad's little 2:1 screen with a picture on it.
            let body = Rect::from_min_max(at(-0.95, -0.55), at(0.95, 0.55));
            p.rect_stroke(
                body,
                CornerRadius::same((0.2 * s) as u8),
                st,
                StrokeKind::Middle,
            );
            p.add(egui::Shape::line(
                vec![
                    at(-0.6, 0.3),
                    at(-0.2, -0.12),
                    at(0.1, 0.15),
                    at(0.35, -0.05),
                    at(0.6, 0.3),
                ],
                st,
            ));
        }
        Icon::Waves => {
            // Trigger feedback: a pulse between two brackets.
            let pts: Vec<_> = (0..=24)
                .map(|i| {
                    let x = -0.55 + i as f32 / 24.0 * 1.1;
                    at(x, 0.45 * (x * 9.0).sin() * (1.0 - (x / 0.55).abs()))
                })
                .collect();
            p.add(egui::Shape::line(pts, st));
            for k in [-1.0f32, 1.0] {
                p.add(egui::Shape::line(
                    vec![at(0.8 * k, -0.7), at(0.95 * k, 0.0), at(0.8 * k, 0.7)],
                    st,
                ));
            }
        }
        Icon::Minimize => {
            p.line_segment([at(-0.7, 0.0), at(0.7, 0.0)], st);
        }
        Icon::Maximize => {
            p.rect_stroke(
                Rect::from_min_max(at(-0.65, -0.65), at(0.65, 0.65)),
                CornerRadius::same(2),
                st,
                StrokeKind::Middle,
            );
        }
        Icon::Restore => {
            p.rect_stroke(
                Rect::from_min_max(at(-0.7, -0.4), at(0.4, 0.7)),
                CornerRadius::same(2),
                st,
                StrokeKind::Middle,
            );
            p.add(egui::Shape::line(
                vec![
                    at(-0.4, -0.4),
                    at(-0.4, -0.7),
                    at(0.7, -0.7),
                    at(0.7, 0.4),
                    at(0.4, 0.4),
                ],
                st,
            ));
        }
        Icon::Close => {
            p.line_segment([at(-0.62, -0.62), at(0.62, 0.62)], st);
            p.line_segment([at(-0.62, 0.62), at(0.62, -0.62)], st);
        }
        Icon::More => {
            for x in [-0.6f32, 0.0, 0.6] {
                p.circle_filled(at(x, 0.0), 0.13 * s, colour);
            }
        }
    }
}

/// Square icon button without a frame.
pub fn icon_button(ui: &mut Ui, which: Icon, tip: &str) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
    let bg = if r.hovered() {
        theme::hover()
    } else {
        theme::card_hi()
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(10),
        bg,
        Stroke::new(1.0, theme::border()),
        StrokeKind::Inside,
    );
    icon(
        ui.painter(),
        rect.shrink(9.0),
        which,
        if r.hovered() {
            theme::text()
        } else {
            theme::dim()
        },
    );
    r.on_hover_text(tip)
}

/// Battery glyph with `level` of 5 bars filled.
pub fn battery(ui: &mut Ui, level: u8, charging: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(30.0, 16.0), Sense::hover());
    let p = ui.painter();
    let body = Rect::from_min_max(rect.min, pos2(rect.right() - 3.0, rect.bottom()));
    p.rect_stroke(
        body,
        CornerRadius::same(4),
        Stroke::new(1.3, theme::dim()),
        StrokeKind::Inside,
    );
    p.rect_filled(
        Rect::from_min_max(
            pos2(body.right() + 0.5, rect.top() + 5.0),
            pos2(rect.right(), rect.bottom() - 5.0),
        ),
        CornerRadius::same(1),
        theme::dim(),
    );
    let inner = body.shrink(3.0);
    let frac = level.min(5) as f32 / 5.0;
    let colour = if level <= 1 {
        theme::bad()
    } else if charging {
        accent()
    } else {
        theme::ok()
    };
    p.rect_filled(
        Rect::from_min_max(
            inner.min,
            pos2(inner.left() + inner.width() * frac, inner.bottom()),
        ),
        CornerRadius::same(2),
        colour,
    );
}

/// Two columns side by side; `frac` of the width goes to the left one.
pub fn split<S>(
    ui: &mut Ui,
    frac: f32,
    state: &mut S,
    left: impl FnOnce(&mut S, &mut Ui),
    right: impl FnOnce(&mut S, &mut Ui),
) {
    let gap = 14.0;
    let w = ui.available_width();
    let lw = ((w - gap) * frac).floor();
    let rw = w - gap - lw;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui_with_layout(
            vec2(lw, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(lw);
                ui.spacing_mut().item_spacing.x = 10.0;
                left(state, ui)
            },
        );
        ui.add_space(gap);
        ui.allocate_ui_with_layout(
            vec2(rw, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(rw);
                ui.spacing_mut().item_spacing.x = 10.0;
                right(state, ui)
            },
        );
    });
}
