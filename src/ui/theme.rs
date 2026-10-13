//! Palette, fonts and egui style. Graphite body, silver trim, ice-cyan accent.

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

/// Ice cyan: fixed, so labels on it stay readable whatever the pad is lit with.
pub const ACCENT: Color32 = Color32::from_rgb(0, 200, 255);
/// Text on top of the accent.
pub const ON_ACCENT: Color32 = Color32::from_rgb(10, 12, 15);

pub fn accent() -> Color32 {
    ACCENT
}

pub fn on_accent() -> Color32 {
    ON_ACCENT
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

fn apply_style(ctx: &egui::Context) {
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
