//! Palettes, fonts and egui style: a dark and a light theme, picked in
//! Settings or following the system.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle,
};

/// The colours of one theme.
pub struct Palette {
    pub bg: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub card_hi: Color32,
    pub hover: Color32,
    pub hover_stroke: Color32,
    pub pressed: Color32,
    pub border: Color32,
    pub text: Color32,
    /// Hovered and pressed text, live markers: the strongest ink.
    pub strong: Color32,
    pub dim: Color32,
    pub faint: Color32,
    pub silver: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub accent: Color32,
    /// Text on top of the accent.
    pub on_accent: Color32,
    /// Sunken areas: graphs, stick gates, segmented controls, text fields.
    pub well: Color32,
    pub grid: Color32,
    pub grid_hi: Color32,
    /// Handles of hand-drawn sliders.
    pub knob: Color32,
    /// A faint wash of the strong ink, for hover over drawings.
    pub wash: Color32,
    pub shadow: Color32,
    /// LED previews: the bar, an unlit LED, and the rim of a lit one.
    pub led_bar: Color32,
    pub led_off: Color32,
    pub led_rim: Color32,
    /// The pad's own little screen.
    pub lcd: Color32,
    /// The pad drawing's lines and body.
    pub ink: Color32,
    pub body: Color32,
}

/// Graphite body, silver trim, ice-cyan accent.
pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(11, 13, 16),
    sidebar: Color32::from_rgb(15, 17, 21),
    card: Color32::from_rgb(21, 24, 30),
    card_hi: Color32::from_rgb(29, 33, 41),
    hover: Color32::from_rgb(37, 42, 52),
    hover_stroke: Color32::from_rgb(62, 70, 84),
    pressed: Color32::from_rgb(46, 52, 64),
    border: Color32::from_rgb(38, 43, 52),
    text: Color32::from_rgb(232, 235, 240),
    strong: Color32::WHITE,
    dim: Color32::from_rgb(140, 148, 161),
    faint: Color32::from_rgb(88, 96, 108),
    silver: Color32::from_rgb(200, 206, 215),
    ok: Color32::from_rgb(70, 214, 130),
    warn: Color32::from_rgb(255, 178, 64),
    bad: Color32::from_rgb(255, 96, 96),
    accent: Color32::from_rgb(0, 200, 255),
    on_accent: Color32::from_rgb(10, 12, 15),
    well: Color32::from_rgb(12, 14, 18),
    grid: Color32::from_rgb(30, 34, 41),
    grid_hi: Color32::from_rgb(48, 54, 64),
    knob: Color32::from_rgb(232, 235, 240),
    wash: Color32::from_rgba_premultiplied(14, 14, 14, 14),
    shadow: Color32::from_black_alpha(140),
    led_bar: Color32::from_rgb(10, 12, 15),
    led_off: Color32::from_rgb(10, 12, 15),
    led_rim: Color32::TRANSPARENT,
    lcd: Color32::from_rgb(6, 8, 11),
    ink: Color32::from_rgb(157, 183, 220),
    body: Color32::from_rgb(26, 38, 54),
};

/// Cool paper, graphite ink, and the ice cyan deepened to read on white.
pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(240, 242, 246),
    sidebar: Color32::from_rgb(231, 234, 239),
    card: Color32::from_rgb(255, 255, 255),
    card_hi: Color32::from_rgb(236, 239, 243),
    hover: Color32::from_rgb(225, 230, 237),
    hover_stroke: Color32::from_rgb(184, 192, 204),
    pressed: Color32::from_rgb(214, 220, 229),
    border: Color32::from_rgb(218, 223, 230),
    text: Color32::from_rgb(22, 26, 32),
    strong: Color32::from_rgb(10, 12, 15),
    dim: Color32::from_rgb(88, 97, 111),
    faint: Color32::from_rgb(140, 148, 160),
    silver: Color32::from_rgb(74, 82, 96),
    ok: Color32::from_rgb(22, 140, 74),
    warn: Color32::from_rgb(180, 104, 0),
    bad: Color32::from_rgb(210, 48, 48),
    accent: Color32::from_rgb(0, 119, 182),
    on_accent: Color32::WHITE,
    well: Color32::from_rgb(244, 246, 249),
    grid: Color32::from_rgb(222, 227, 234),
    grid_hi: Color32::from_rgb(196, 203, 214),
    knob: Color32::WHITE,
    wash: Color32::from_black_alpha(10),
    shadow: Color32::from_black_alpha(40),
    led_bar: Color32::from_rgb(222, 227, 234),
    led_off: Color32::from_rgb(240, 242, 246),
    led_rim: Color32::from_black_alpha(28),
    lcd: Color32::from_rgb(214, 222, 233),
    ink: Color32::from_rgb(40, 54, 78),
    body: Color32::from_rgb(196, 207, 222),
};

/// Whether this frame is drawn light, see [`sync`].
static LIGHT_ON: AtomicBool = AtomicBool::new(false);

pub fn palette() -> &'static Palette {
    if LIGHT_ON.load(Ordering::Relaxed) {
        &LIGHT
    } else {
        &DARK
    }
}

pub fn is_light() -> bool {
    LIGHT_ON.load(Ordering::Relaxed)
}

/// Run at the start of each frame: settles the theme for the choice made
/// in Settings and picks its palette.
pub fn sync(ctx: &egui::Context) {
    let theme = match choice() {
        Choice::Dark => egui::Theme::Dark,
        Choice::Light => egui::Theme::Light,
        Choice::System => match DESKTOP.load(Ordering::Relaxed) {
            PREFERS_DARK => egui::Theme::Dark,
            PREFERS_LIGHT => egui::Theme::Light,
            _ => ctx.system_theme().unwrap_or(egui::Theme::Dark),
        },
    };
    if ctx.options(|o| o.theme_preference) != theme.into() {
        ctx.set_theme(theme);
    }
    LIGHT_ON.store(theme == egui::Theme::Light, Ordering::Relaxed);
}

pub fn bg() -> Color32 {
    palette().bg
}
pub fn sidebar() -> Color32 {
    palette().sidebar
}
pub fn card() -> Color32 {
    palette().card
}
pub fn card_hi() -> Color32 {
    palette().card_hi
}
pub fn hover() -> Color32 {
    palette().hover
}
pub fn border() -> Color32 {
    palette().border
}
pub fn text() -> Color32 {
    palette().text
}
pub fn strong() -> Color32 {
    palette().strong
}
pub fn dim() -> Color32 {
    palette().dim
}
pub fn faint() -> Color32 {
    palette().faint
}
pub fn silver() -> Color32 {
    palette().silver
}
pub fn ok() -> Color32 {
    palette().ok
}
pub fn warn() -> Color32 {
    palette().warn
}
pub fn bad() -> Color32 {
    palette().bad
}
pub fn accent() -> Color32 {
    palette().accent
}
pub fn on_accent() -> Color32 {
    palette().on_accent
}
pub fn well() -> Color32 {
    palette().well
}
pub fn grid() -> Color32 {
    palette().grid
}
pub fn grid_hi() -> Color32 {
    palette().grid_hi
}
pub fn wash() -> Color32 {
    palette().wash
}

/// The theme picked in Settings.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    System,
    Dark,
    Light,
}

static CHOICE: AtomicU8 = AtomicU8::new(Choice::System as u8);

impl Choice {
    pub const ALL: [Choice; 3] = [Choice::System, Choice::Dark, Choice::Light];

    fn word(self) -> &'static str {
        match self {
            Choice::System => "system",
            Choice::Dark => "dark",
            Choice::Light => "light",
        }
    }
}

fn choice_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("apex5-control").join("theme"))
}

/// The theme picked earlier; the system's when none was. `APEX5_THEME=light`
/// (or `dark`, `system`) overrides it for this run.
fn load_choice() -> Choice {
    let word = std::env::var("APEX5_THEME")
        .ok()
        .or_else(|| choice_path().and_then(|p| std::fs::read_to_string(p).ok()));
    word.and_then(|w| Choice::ALL.into_iter().find(|c| c.word() == w.trim()))
        .unwrap_or(Choice::System)
}

pub fn choice() -> Choice {
    let n = CHOICE.load(Ordering::Relaxed);
    Choice::ALL
        .into_iter()
        .find(|c| *c as u8 == n)
        .unwrap_or(Choice::System)
}

/// Switches and remembers the theme; remembering is best effort.
pub fn pick(ctx: &egui::Context, c: Choice) {
    CHOICE.store(c as u8, Ordering::Relaxed);
    ctx.request_repaint();
    let Some(p) = choice_path() else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, c.word());
}

/// What the desktop portal says about light and dark, as its
/// `org.freedesktop.appearance color-scheme`: no preference, dark, light.
static DESKTOP: AtomicU8 = AtomicU8::new(0);
const PREFERS_DARK: u8 = 1;
const PREFERS_LIGHT: u8 = 2;

/// The window to repaint when the desktop switches.
static WATCHED: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Follows the desktop's light or dark preference from the XDG settings
/// portal: winit does not report it on Wayland. Without a portal the theme
/// stays egui's guess.
fn watch_desktop(ctx: &egui::Context) {
    *WATCHED.lock().unwrap() = Some(ctx.clone());
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            if let Err(e) = rt.block_on(follow_portal()) {
                eprintln!("theme: desktop preference unknown: {e}");
            }
        });
    });
}

async fn follow_portal() -> zbus::Result<()> {
    use futures_util::StreamExt;
    use zbus::zvariant::{OwnedValue, Value};

    fn scheme(v: &Value) -> u8 {
        match v {
            Value::U32(n) => u8::try_from(*n).unwrap_or(0),
            Value::Value(inner) => scheme(inner),
            _ => 0,
        }
    }
    fn set(n: u8) {
        if DESKTOP.swap(n, Ordering::Relaxed) != n
            && let Some(ctx) = &*WATCHED.lock().unwrap()
        {
            ctx.request_repaint();
        }
    }

    const NAMESPACE: &str = "org.freedesktop.appearance";
    let conn = zbus::Connection::session().await?;
    let portal = zbus::Proxy::new(
        &conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    )
    .await?;
    let mut changes = portal.receive_signal("SettingChanged").await?;
    let now: OwnedValue = portal.call("ReadOne", &(NAMESPACE, "color-scheme")).await?;
    set(scheme(&now));
    while let Some(m) = changes.next().await {
        let (ns, key, v): (String, String, OwnedValue) = m.body().deserialize()?;
        if ns == NAMESPACE && key == "color-scheme" {
            set(scheme(&v));
        }
    }
    Ok(())
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
    CHOICE.store(load_choice() as u8, Ordering::Relaxed);
    watch_desktop(ctx);
    ctx.set_fonts(fonts());
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| apply_style(style, theme));
    }
    sync(ctx);
}

fn apply_style(style: &mut egui::Style, theme: egui::Theme) {
    let pal = match theme {
        egui::Theme::Dark => &DARK,
        egui::Theme::Light => &LIGHT,
    };
    let a = pal.accent;
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
    *v = theme.default_visuals();
    v.panel_fill = pal.bg;
    v.window_fill = pal.card_hi;
    v.window_stroke = Stroke::new(1.0, pal.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: pal.shadow,
    };
    v.window_shadow = v.popup_shadow;
    v.extreme_bg_color = pal.well;
    v.faint_bg_color = pal.card_hi;
    v.override_text_color = None;
    v.hyperlink_color = a;
    v.warn_fg_color = pal.warn;
    v.error_fg_color = pal.bad;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.selection.bg_fill = a.gamma_multiply(0.85);
    v.selection.stroke = Stroke::new(1.5, a);
    v.text_cursor.stroke = Stroke::new(2.0, a);

    let r = CornerRadius::same(9);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = pal.card;
    w.noninteractive.weak_bg_fill = pal.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, pal.text);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = pal.card_hi;
    w.inactive.weak_bg_fill = pal.card_hi;
    w.inactive.bg_stroke = Stroke::new(1.0, pal.border);
    w.inactive.fg_stroke = Stroke::new(1.0, pal.text);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = pal.hover;
    w.hovered.weak_bg_fill = pal.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, pal.hover_stroke);
    w.hovered.fg_stroke = Stroke::new(1.5, pal.strong);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = pal.pressed;
    w.active.weak_bg_fill = pal.pressed;
    w.active.bg_stroke = Stroke::new(1.0, a);
    w.active.fg_stroke = Stroke::new(1.5, pal.strong);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open = w.active;
}
