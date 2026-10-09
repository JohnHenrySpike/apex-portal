//! Palette, fonts and egui style. Graphite body, silver trim, and an accent
//! that follows the colour the pad's own lights are showing.

use std::sync::atomic::{AtomicU32, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle,
};

pub const BG: Color32 = Color32::from_rgb(11, 13, 16);
pub const SIDEBAR: Color32 = Color32::from_rgb(15, 17, 21);
pub const CARD: Color32 = Color32::from_rgb(21, 24, 30);
pub const CARD_HI: Color32 = Color32::from_rgb(29, 33, 41);
pub const HOVER: Color32 = Color32::from_rgb(37, 42, 52);
pub const BORDER: Color32 = Color32::from_rgb(38, 43, 52);
pub const TEXT: Color32 = Color32::from_rgb(232, 235, 240);
pub const DIM: Color32 = Color32::from_rgb(140, 148, 161);
pub const FAINT: Color32 = Color32::from_rgb(88, 96, 108);
pub const SILVER: Color32 = Color32::from_rgb(200, 206, 215);
pub const OK: Color32 = Color32::from_rgb(70, 214, 130);
pub const WARN: Color32 = Color32::from_rgb(255, 178, 64);
pub const BAD: Color32 = Color32::from_rgb(255, 96, 96);

const DEFAULT_ACCENT: u32 = 0x00_C8_FF;
static ACCENT: AtomicU32 = AtomicU32::new(DEFAULT_ACCENT);

pub fn accent() -> Color32 {
    let v = ACCENT.load(Ordering::Relaxed);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Text colour that reads on top of the accent.
pub fn on_accent() -> Color32 {
    let c = accent();
    let l = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if l > 150.0 {
        Color32::from_rgb(10, 12, 15)
    } else {
        Color32::WHITE
    }
}

/// Follow the pad's lighting. Dark or washed-out colours are lifted so the
/// accent stays readable on the graphite background.
pub fn set_accent_from_led(rgb: Option<[u8; 3]>) -> bool {
    let v = match rgb {
        Some([r, g, b]) if r.max(g).max(b) > 8 => {
            let (h, s, _) = rgb_to_hsv(r, g, b);
            let [r, g, b] = if s < 0.12 {
                [225, 232, 242]
            } else {
                hsv_to_rgb(h, s.clamp(0.55, 0.9), 1.0)
            };
            (r as u32) << 16 | (g as u32) << 8 | b as u32
        }
        _ => DEFAULT_ACCENT,
    };
    ACCENT.swap(v, Ordering::Relaxed) != v
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

/// Ask fontconfig for a face. Returns (path, index within a collection).
fn fc_match(pattern: &str, must_contain: &str) -> Option<(String, u32)> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}", pattern])
        .output()
        .ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let mut it = s.lines();
    let file = it.next()?.to_string();
    let index = it.next().and_then(|i| i.parse().ok()).unwrap_or(0);
    file.to_lowercase()
        .contains(&must_contain.to_lowercase())
        .then_some((file, index))
}

fn load(pattern: &str, must_contain: &str) -> Option<FontData> {
    let (file, index) = fc_match(pattern, must_contain)?;
    let bytes = std::fs::read(file).ok()?;
    let mut f = FontData::from_owned(bytes);
    f.index = index;
    Some(f)
}

fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    // System faces: Noto Sans covers Latin and Cyrillic, Noto Sans CJK the
    // Chinese factory profile titles, the symbol fonts arrows and shapes.
    let faces = [
        ("regular", "Noto Sans:style=Regular", "notosans"),
        ("medium", "Noto Sans:style=Medium", "notosans"),
        ("bold", "Noto Sans:style=SemiBold", "notosans"),
        ("bold2", "Noto Sans:style=Bold", "notosans"),
        ("cjk", "sans-serif:lang=zh-cn", "cjk"),
        ("sym2", "Noto Sans Symbols 2", "symbols2"),
        ("sym", "Noto Sans Symbols", "symbols"),
        ("mono", "Noto Sans Mono", "mono"),
    ];
    let mut have = std::collections::HashMap::new();
    for (name, pat, needle) in faces {
        if let Some(f) = load(pat, needle) {
            defs.font_data.insert(name.into(), f.into());
            have.insert(name, ());
        }
    }
    let fallbacks: Vec<String> = ["cjk", "sym2", "sym"]
        .iter()
        .filter(|n| have.contains_key(*n))
        .map(|s| s.to_string())
        .collect();
    let egui_defaults = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let family = |primary: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = primary
            .iter()
            .filter(|n| have.contains_key(*n))
            .map(|s| s.to_string())
            .take(1)
            .collect();
        v.extend(egui_defaults.iter().cloned());
        v.extend(fallbacks.iter().cloned());
        v
    };
    defs.families
        .insert(FontFamily::Proportional, family(&["regular"]));
    defs.families.insert(
        FontFamily::Name("medium".into()),
        family(&["medium", "regular"]),
    );
    defs.families.insert(
        FontFamily::Name("bold".into()),
        family(&["bold", "bold2", "medium", "regular"]),
    );
    let mut mono = family(&["mono"]);
    if let Some(m) = defs.families.get(&FontFamily::Monospace) {
        mono.extend(m.iter().cloned());
    }
    defs.families.insert(FontFamily::Monospace, mono);
    defs
}

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_fonts(fonts());
    apply_style(ctx);
}

/// Re-run when the accent changes.
pub fn apply_style(ctx: &egui::Context) {
    let a = accent();
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Body, FontId::proportional(14.5)),
            (TextStyle::Button, medium(14.0)),
            (TextStyle::Heading, bold(19.0)),
            (TextStyle::Monospace, FontId::monospace(13.5)),
        ]
        .into();

        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(10.0, 9.0);
        s.button_padding = egui::vec2(14.0, 7.0);
        s.interact_size = egui::vec2(40.0, 30.0);
        s.slider_width = 240.0;
        s.slider_rail_height = 6.0;
        s.combo_width = 180.0;
        s.icon_width = 18.0;
        s.icon_width_inner = 10.0;
        s.menu_margin = egui::Margin::same(6);
        s.scroll.bar_width = 6.0;

        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = CARD_HI;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.popup_shadow = Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(140),
        };
        v.window_shadow = v.popup_shadow;
        v.extreme_bg_color = Color32::from_rgb(9, 11, 14);
        v.faint_bg_color = CARD_HI;
        v.override_text_color = None;
        v.hyperlink_color = a;
        v.warn_fg_color = WARN;
        v.error_fg_color = BAD;
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        v.selection.bg_fill = a.gamma_multiply(0.85);
        v.selection.stroke = Stroke::new(1.5, a);
        v.text_cursor.stroke = Stroke::new(2.0, a);

        let r = CornerRadius::same(9);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = CARD;
        w.noninteractive.weak_bg_fill = CARD;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = CARD_HI;
        w.inactive.weak_bg_fill = CARD_HI;
        w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = HOVER;
        w.hovered.weak_bg_fill = HOVER;
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(62, 70, 84));
        w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.hovered.corner_radius = r;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = Color32::from_rgb(46, 52, 64);
        w.active.weak_bg_fill = Color32::from_rgb(46, 52, 64);
        w.active.bg_stroke = Stroke::new(1.0, a);
        w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.active.corner_radius = r;
        w.active.expansion = 0.0;
        w.open = w.active;
    });
}
