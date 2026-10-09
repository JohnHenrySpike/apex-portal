//! The Apex 5, drawn from Flydigi's own line drawing.
//!
//! `assets/apex5.svg` (built by `tools/svg`) is rasterised with resvg at the
//! exact on-screen size. Key highlights, the moving stick caps, the light
//! strips, the screen text and the back buttons are painted over it.
//! Coordinates are the drawing's 984-wide space; the pad spans x 166..818,
//! y 40..472 and is scaled to fit.

use std::sync::OnceLock;

use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, FontId, Id, Mesh, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2,
};
use resvg::{tiny_skia, usvg};

use super::theme::{self, accent, on_accent};
use super::widgets::lerp_colour;

const SVG: &str = include_str!("../../assets/apex5.svg");

/// The drawing's viewBox: x, y, width, height.
const VIEW: [f32; 4] = [166.0, 40.0, 652.0, 432.0];
/// Height with the back-button row below the pad.
const H_BACK: f32 = 492.0;
const MID: f32 = 492.0;
const SCREEN: [f32; 4] = [452.65, 144.48, 531.35, 182.33];
/// How far a stick cap moves at full deflection.
const STICK_TRAVEL: f32 = 13.0;

/// Light strips along the outer edge of each grip, 11 units inside the body
/// outline: left bottom→top, then right top→bottom, so the 12 LEDs run
/// clockwise.
const LEDS: [[f32; 2]; 12] = [
    [185.4, 416.5],
    [180.9, 387.7],
    [182.6, 357.4],
    [188.3, 326.4],
    [196.4, 295.0],
    [205.8, 263.3],
    [778.2, 263.3],
    [787.6, 295.0],
    [795.7, 326.4],
    [801.4, 357.4],
    [803.1, 387.7],
    [798.6, 416.5],
];

pub const BACK_KEYS: [(u8, &str); 6] = [
    (18, "M1"),
    (19, "M2"),
    (20, "M3"),
    (21, "M4"),
    (22, "LM"),
    (23, "RM"),
];

/// Key areas in the drawing (`#keys`), by key id.
const FRONT_KEYS: [(u8, &str); 17] = [
    (0, "dpad-up"),
    (1, "dpad-right"),
    (2, "dpad-down"),
    (3, "dpad-left"),
    (4, "a"),
    (5, "b"),
    (6, "view"),
    (7, "x"),
    (8, "y"),
    (9, "menu"),
    (10, "lb"),
    (11, "rb"),
    (12, "lt"),
    (13, "rt"),
    (14, "ls"),
    (15, "rs"),
    (27, "home"),
];

/// Stick caps are drawn separately so they can move.
const CAPS: [(u8, &str); 2] = [(14, "ls-cap"), (15, "rs-cap")];

/// Hit-test order: small keys before the large areas around them.
const HIT_ORDER: [u8; 23] = [
    18, 19, 20, 21, 22, 23, 4, 5, 7, 8, 6, 9, 27, 0, 1, 2, 3, 14, 15, 10, 11, 12, 13,
];

#[derive(Default)]
pub struct Opts<'a> {
    pub leds: Option<&'a [[u8; 3]]>,
    pub brightness: f32,
    pub interactive: bool,
    pub selected: Option<u8>,
    pub marked: &'a [u8],
    pub pressed: &'a [u8],
    pub sticks: Option<[(f32, f32); 2]>,
    pub triggers: Option<(f32, f32)>,
    pub screen: Option<&'a str>,
    pub show_back: bool,
}

pub struct PadResponse {
    pub clicked: Option<u8>,
    pub hovered: Option<u8>,
}

// --- the drawing -------------------------------------------------------------

/// A key area: polygons in drawing coordinates with their triangulation.
struct Area {
    contours: Vec<Vec<[f32; 2]>>,
    tris: Vec<Vec<u32>>,
    min: [f32; 2],
    max: [f32; 2],
}

impl Area {
    fn contains(&self, (x, y): (f32, f32)) -> bool {
        self.contours.iter().any(|c| inside_poly(c, (x, y)))
    }

    /// Where a key's mark dot goes: near the top-right of its area.
    fn corner(&self) -> [f32; 2] {
        let (w, h) = (self.max[0] - self.min[0], self.max[1] - self.min[1]);
        [self.max[0] - w * 0.15, self.min[1] + h * 0.15]
    }
}

struct Art {
    /// The drawing without the stick caps.
    base: usvg::Tree,
    /// The full drawing, for rendering the caps on their own.
    full: usvg::Tree,
    areas: Vec<(u8, Area)>,
}

fn art() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| {
        let opt = usvg::Options::default();
        let full = usvg::Tree::from_str(SVG, &opt).expect("assets/apex5.svg parses");
        let base = usvg::Tree::from_str(&without_groups(SVG, &["ls-cap", "rs-cap"]), &opt)
            .expect("assets/apex5.svg parses");
        let areas = FRONT_KEYS
            .iter()
            .map(|&(key, id)| {
                let node = full
                    .node_by_id(id)
                    .unwrap_or_else(|| panic!("assets/apex5.svg has no #{id}"));
                (key, area(node))
            })
            .collect();
        Art { base, full, areas }
    })
}

/// The SVG source with the `<g id=…>…</g>` groups removed. The groups are
/// flat (no nested groups), as tools/svg writes them.
fn without_groups(svg: &str, ids: &[&str]) -> String {
    let mut s = svg.to_owned();
    for id in ids {
        let start = s
            .find(&format!("<g id=\"{id}\""))
            .unwrap_or_else(|| panic!("assets/apex5.svg has no group #{id}"));
        let end = start + s[start..].find("</g>").expect("group is closed") + "</g>".len();
        s.replace_range(start..end, "");
    }
    s
}

fn area(node: &usvg::Node) -> Area {
    let mut contours = vec![];
    collect_contours(node, &mut contours);
    for c in &mut contours {
        tidy(c);
    }
    let tris = contours.iter().map(|c| triangulate(c)).collect();
    let pts = contours.iter().flatten();
    let min = pts
        .clone()
        .fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
    let max = pts.fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
    Area {
        contours,
        tris,
        min,
        max,
    }
}

/// Flattened outlines of every path under `node`, in drawing coordinates.
fn collect_contours(node: &usvg::Node, out: &mut Vec<Vec<[f32; 2]>>) {
    use usvg::tiny_skia_path::PathSegment;
    match node {
        usvg::Node::Group(g) => {
            for child in g.children() {
                collect_contours(child, out);
            }
        }
        usvg::Node::Path(path) => {
            let ts = path.abs_transform();
            let mut cur = vec![];
            let mut last = tiny_skia::Point::zero();
            let push = |p: tiny_skia::Point, cur: &mut Vec<[f32; 2]>| {
                let mut q = p;
                ts.map_point(&mut q);
                // Tree coordinates start at the viewBox origin.
                cur.push([q.x + VIEW[0], q.y + VIEW[1]]);
            };
            for seg in path.data().segments() {
                match seg {
                    PathSegment::MoveTo(p) => {
                        if cur.len() > 2 {
                            out.push(std::mem::take(&mut cur));
                        }
                        cur.clear();
                        push(p, &mut cur);
                        last = p;
                    }
                    PathSegment::LineTo(p) => {
                        push(p, &mut cur);
                        last = p;
                    }
                    PathSegment::QuadTo(c, p) => {
                        for i in 1..=8 {
                            let t = i as f32 / 8.0;
                            let u = 1.0 - t;
                            let q = tiny_skia::Point::from_xy(
                                u * u * last.x + 2.0 * u * t * c.x + t * t * p.x,
                                u * u * last.y + 2.0 * u * t * c.y + t * t * p.y,
                            );
                            push(q, &mut cur);
                        }
                        last = p;
                    }
                    PathSegment::CubicTo(c1, c2, p) => {
                        for i in 1..=12 {
                            let t = i as f32 / 12.0;
                            let u = 1.0 - t;
                            let k = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                            let q = tiny_skia::Point::from_xy(
                                k[0] * last.x + k[1] * c1.x + k[2] * c2.x + k[3] * p.x,
                                k[0] * last.y + k[1] * c1.y + k[2] * c2.y + k[3] * p.y,
                            );
                            push(q, &mut cur);
                        }
                        last = p;
                    }
                    PathSegment::Close => {}
                }
            }
            if cur.len() > 2 {
                out.push(cur);
            }
        }
        _ => {}
    }
}

/// Drops repeated points (including a closing copy of the first) and points
/// on a straight run, which ear clipping cannot remove.
fn tidy(c: &mut Vec<[f32; 2]>) {
    c.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3);
    while c.len() > 3 && {
        let (a, b) = (c[0], c[c.len() - 1]);
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    } {
        c.pop();
    }
    let mut i = 0;
    while c.len() > 3 && i < c.len() {
        let n = c.len();
        let (a, b, d) = (c[(i + n - 1) % n], c[i], c[(i + 1) % n]);
        let cross = (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]);
        if cross.abs() < 1e-4 {
            c.remove(i);
        } else {
            i += 1;
        }
    }
}

/// Ear clipping for a simple polygon.
fn triangulate(p: &[[f32; 2]]) -> Vec<u32> {
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let area: f32 = (0..p.len())
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    let mut idx: Vec<usize> = (0..p.len()).collect();
    if area < 0.0 {
        idx.reverse();
    }
    let mut out = vec![];
    while idx.len() > 3 {
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (a, b, c) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            if cross(p[a], p[b], p[c]) <= 0.0 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                j != a && j != b && j != c && {
                    let q = p[j];
                    cross(p[a], p[b], q) >= 0.0
                        && cross(p[b], p[c], q) >= 0.0
                        && cross(p[c], p[a], q) >= 0.0
                }
            });
            if inside {
                continue;
            }
            out.extend([a as u32, b as u32, c as u32]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        out.extend(idx.iter().map(|&i| i as u32));
    }
    out
}

fn inside_poly(pts: &[[f32; 2]], (x, y): (f32, f32)) -> bool {
    let mut c = false;
    let n = pts.len();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + n - 1) % n]);
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

// --- rasters -------------------------------------------------------------------

/// The drawing rendered for one on-screen size.
#[derive(Clone)]
struct Raster {
    px: [usize; 2],
    base: TextureHandle,
    /// Stick caps with their rest rectangles in drawing coordinates.
    caps: [(TextureHandle, [f32; 4]); 2],
}

fn render(tree: &usvg::Tree, px: [usize; 2], ts: tiny_skia::Transform) -> ColorImage {
    let mut pixmap = tiny_skia::Pixmap::new(px[0] as u32, px[1] as u32).expect("non-empty size");
    resvg::render(tree, ts, &mut pixmap.as_mut());
    ColorImage::from_rgba_premultiplied(px, pixmap.data())
}

fn render_node(node: &usvg::Node, px: [usize; 2], scale: f32) -> ColorImage {
    let mut pixmap = tiny_skia::Pixmap::new(px[0] as u32, px[1] as u32).expect("non-empty size");
    // render_node skips the ancestors' transforms, including the viewBox
    // shift that the node's bounding box already has.
    let ts = tiny_skia::Transform::from_scale(scale, scale).pre_translate(-VIEW[0], -VIEW[1]);
    resvg::render_node(node, ts, &mut pixmap.as_mut());
    ColorImage::from_rgba_premultiplied(px, pixmap.data())
}

/// Rasters for the given pixel size, cached across frames. A few sizes are
/// kept, since several screens show the pad at different sizes.
fn raster(ctx: &egui::Context, px: [usize; 2]) -> Raster {
    let id = Id::new("apex5-pad-rasters");
    if let Some(r) = ctx.data(|d| {
        d.get_temp::<Vec<Raster>>(id)
            .and_then(|v| v.iter().find(|r| r.px == px).cloned())
    }) {
        return r;
    }
    let a = art();
    let scale = px[0] as f32 / VIEW[2];
    let base = ctx.load_texture(
        "apex5-pad",
        render(
            &a.base,
            px,
            tiny_skia::Transform::from_scale(scale, px[1] as f32 / VIEW[3]),
        ),
        TextureOptions::LINEAR,
    );
    let caps = CAPS.map(|(_, id)| {
        let node = a.full.node_by_id(id).expect("cap group exists");
        let bb = node.abs_layer_bounding_box().expect("cap has a size");
        let size = [
            (bb.width() * scale).ceil() as usize + 1,
            (bb.height() * scale).ceil() as usize + 1,
        ];
        let tex = ctx.load_texture(id, render_node(node, size, scale), TextureOptions::LINEAR);
        let x = bb.x() + VIEW[0];
        let y = bb.y() + VIEW[1];
        let rect = [x, y, x + size[0] as f32 / scale, y + size[1] as f32 / scale];
        (tex, rect)
    });
    let r = Raster { px, base, caps };
    ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Vec<Raster>>(id);
        v.insert(0, r.clone());
        v.truncate(4);
    });
    r
}

// --- painting --------------------------------------------------------------------

struct Xf {
    o: Pos2,
    s: f32,
}

impl Xf {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(
            self.o.x + (x - VIEW[0]) * self.s,
            self.o.y + (y - VIEW[1]) * self.s,
        )
    }
    fn q(&self, q: [f32; 2]) -> Pos2 {
        self.p(q[0], q[1])
    }
    fn v(&self, v: f32) -> f32 {
        v * self.s
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }
    fn inv(&self, p: Pos2) -> (f32, f32) {
        (
            (p.x - self.o.x) / self.s + VIEW[0],
            (p.y - self.o.y) / self.s + VIEW[1],
        )
    }
}

#[derive(Clone, Copy, Default)]
struct KeyState {
    hovered: bool,
    selected: bool,
    pressed: bool,
    marked: bool,
}

impl KeyState {
    fn lit(&self) -> bool {
        self.hovered || self.selected || self.pressed
    }
}

/// Highlight over a key of the drawing: the drawing already shows the key
/// itself, so idle keys get nothing.
fn overlay(p: &egui::Painter, xf: &Xf, a: &Area, st: KeyState, level: f32, shift: Vec2) {
    let acc = accent();
    let fill = if st.pressed {
        acc.gamma_multiply(0.7)
    } else if st.selected {
        acc.gamma_multiply(0.35)
    } else if st.hovered {
        Color32::from_white_alpha(14)
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if st.pressed || st.selected {
        Stroke::new(2.0, acc)
    } else if st.hovered {
        Stroke::new(1.5, acc.gamma_multiply(0.7))
    } else if level > 0.01 {
        Stroke::new(1.0, acc.gamma_multiply(level))
    } else {
        Stroke::NONE
    };
    // Analogue triggers glow with their travel.
    let fill = if level > 0.01 && !st.pressed {
        acc.gamma_multiply(0.15 + 0.6 * level)
    } else {
        fill
    };
    for (c, tris) in a.contours.iter().zip(&a.tris) {
        let pts: Vec<Pos2> = c.iter().map(|q| xf.q(*q) + shift).collect();
        if fill != Color32::TRANSPARENT {
            let mut mesh = Mesh::default();
            for q in &pts {
                mesh.colored_vertex(*q, fill);
            }
            mesh.indices = tris.clone();
            p.add(Shape::mesh(mesh));
        }
        if stroke != Stroke::NONE {
            p.add(Shape::closed_line(pts, stroke));
        }
    }
}

fn mark(p: &egui::Painter, xf: &Xf, at: Pos2) {
    p.circle_filled(at, xf.v(5.5).max(3.0), accent());
    p.circle_stroke(at, xf.v(5.5).max(3.0), Stroke::new(1.5, theme::BG));
}

fn draw_screen(p: &egui::Painter, xf: &Xf, text: &str) {
    let r = xf.rect(SCREEN[0], SCREEN[1], SCREEN[2], SCREEN[3]);
    p.rect_filled(
        r,
        CornerRadius::same(xf.v(2.0) as u8),
        Color32::from_rgb(6, 8, 11),
    );
    p.text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        theme::bold(xf.v(17.0).max(8.0)),
        accent(),
    );
}

fn draw_leds(p: &egui::Painter, xf: &Xf, leds: Option<&[[u8; 3]]>, brightness: f32) {
    let colour = |i: usize| -> Color32 {
        match leds.and_then(|l| l.get(i)) {
            Some(c) => {
                let k = 0.3 + 0.7 * brightness.clamp(0.0, 1.0);
                Color32::from_rgb(
                    (c[0] as f32 * k) as u8,
                    (c[1] as f32 * k) as u8,
                    (c[2] as f32 * k) as u8,
                )
            }
            None => Color32::from_rgb(30, 34, 40),
        }
    };
    let lit = |c: Color32| c.r().max(c.g()).max(c.b()) > 40;
    // Unlit, the strips are drawn like the rest of the line art.
    let idle = Color32::from_rgb(157, 183, 220);
    for strip in [0..6usize, 6..12] {
        for i in strip.clone() {
            if i + 1 < strip.end {
                let c = lerp_colour(colour(i), colour(i + 1), 0.5);
                let seg = [xf.q(LEDS[i]), xf.q(LEDS[i + 1])];
                if lit(c) {
                    p.line_segment(seg, Stroke::new(xf.v(22.0), c.gamma_multiply(0.09)));
                    p.line_segment(seg, Stroke::new(xf.v(5.0), c.gamma_multiply(0.75)));
                } else {
                    p.line_segment(seg, Stroke::new(xf.v(0.9), idle.gamma_multiply(0.3)));
                }
            }
        }
    }
    for (i, q) in LEDS.iter().enumerate() {
        let c = colour(i);
        let at = xf.q(*q);
        if lit(c) {
            p.circle_filled(at, xf.v(14.0), c.gamma_multiply(0.13));
            p.circle_filled(at, xf.v(5.0), lerp_colour(c, Color32::WHITE, 0.35));
        } else {
            p.circle_filled(at, xf.v(2.2), idle.gamma_multiply(0.45));
        }
    }
}

fn back_key_rect(id: u8) -> [f32; 4] {
    let x = 247.0 + (id - 18) as f32 * 84.0;
    [x, 500.0, x + 70.0, 528.0]
}

fn draw_back_key(p: &egui::Painter, xf: &Xf, id: u8, label: &str, st: KeyState) {
    let [x0, y0, x1, y1] = back_key_rect(id);
    let rect = xf.rect(x0, y0, x1, y1);
    let a = accent();
    let base = Color32::from_rgb(29, 33, 40);
    let fill = if st.pressed {
        a
    } else if st.selected {
        lerp_colour(base, a, 0.28)
    } else if st.hovered {
        lerp_colour(base, Color32::WHITE, 0.08)
    } else {
        base
    };
    let stroke = if st.selected || st.pressed {
        Stroke::new(2.0, a)
    } else if st.hovered {
        Stroke::new(1.5, a.gamma_multiply(0.7))
    } else {
        Stroke::new(1.0, Color32::from_rgb(70, 78, 92))
    };
    let text = if st.pressed {
        on_accent()
    } else if st.lit() {
        Color32::WHITE
    } else {
        theme::DIM
    };
    p.rect(
        rect,
        CornerRadius::same(xf.v(14.0) as u8),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        theme::bold(xf.v(15.0).max(8.0)),
        text,
    );
    if st.marked && !st.pressed {
        mark(p, xf, xf.p(x1 - 3.0, y0 + 3.0));
    }
}

fn hit(id: u8, q: (f32, f32), shift: (f32, f32)) -> bool {
    if (18..=23).contains(&id) {
        let [x0, y0, x1, y1] = back_key_rect(id);
        return q.0 >= x0 - 2.0 && q.0 <= x1 + 2.0 && q.1 >= y0 - 2.0 && q.1 <= y1 + 2.0;
    }
    art()
        .areas
        .iter()
        .find(|(k, _)| *k == id)
        .is_some_and(|(_, a)| a.contains((q.0 - shift.0, q.1 - shift.1)))
}

pub fn show(ui: &mut Ui, size: Vec2, o: &Opts) -> PadResponse {
    let h = if o.show_back { H_BACK } else { VIEW[3] };
    let sense = if o.interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let ppp = ui.ctx().pixels_per_point();
    // Whole physical pixels for the drawing, so the raster maps 1:1.
    let px_w = ((rect.width() / VIEW[2]).min(rect.height() / h) * VIEW[2] * ppp).floor();
    let px = [
        px_w.max(1.0) as usize,
        (px_w * VIEW[3] / VIEW[2]).round().max(1.0) as usize,
    ];
    let s = px[0] as f32 / ppp / VIEW[2];
    let origin = pos2(
        rect.center().x - VIEW[2] * s / 2.0,
        rect.center().y - h * s / 2.0,
    );
    let xf = Xf {
        o: (origin.to_vec2() * ppp).round().to_pos2() / ppp,
        s,
    };

    let [l, r] = o.sticks.unwrap_or([(0.0, 0.0); 2]);
    let stick_shift = |id: u8| -> (f32, f32) {
        let (x, y) = match id {
            14 => l,
            15 => r,
            _ => return (0.0, 0.0),
        };
        (x * STICK_TRAVEL, -y * STICK_TRAVEL)
    };

    let mut hovered = None;
    if o.interactive
        && let Some(pos) = resp.hover_pos()
    {
        let q = xf.inv(pos);
        hovered = HIT_ORDER
            .iter()
            .copied()
            .filter(|id| o.show_back || !(18..=23).contains(id))
            .find(|id| hit(*id, q, stick_shift(*id)));
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let clicked = if resp.clicked() { hovered } else { None };

    let p = ui.painter_at(rect);
    let state = |id: u8| KeyState {
        hovered: hovered == Some(id),
        selected: o.selected == Some(id),
        pressed: o.pressed.contains(&id),
        marked: o.marked.contains(&id),
    };

    let ras = raster(ui.ctx(), px);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    p.image(
        ras.base.id(),
        xf.rect(VIEW[0], VIEW[1], VIEW[0] + VIEW[2], VIEW[1] + VIEW[3]),
        uv,
        Color32::WHITE,
    );
    draw_leds(&p, &xf, o.leds, o.brightness);
    if let Some(t) = o.screen {
        draw_screen(&p, &xf, t);
    }

    let (lt, rt) = o.triggers.unwrap_or((0.0, 0.0));
    let a = art();
    for (id, area) in &a.areas {
        if matches!(id, 14 | 15) {
            continue;
        }
        let level = match id {
            12 => lt,
            13 => rt,
            _ => 0.0,
        };
        overlay(&p, &xf, area, state(*id), level, Vec2::ZERO);
    }

    // Stick caps follow the sticks; the drawing's line layer is 42% opaque.
    for ((id, _), (tex, [x0, y0, x1, y1])) in CAPS.iter().zip(&ras.caps) {
        let (dx, dy) = stick_shift(*id);
        let shift = vec2(xf.v(dx), xf.v(dy));
        let st = state(*id);
        p.image(
            tex.id(),
            xf.rect(*x0, *y0, *x1, *y1).translate(shift),
            uv,
            Color32::from_white_alpha(107),
        );
        if let Some((_, area)) = a.areas.iter().find(|(k, _)| k == id) {
            overlay(&p, &xf, area, st, 0.0, shift);
            if st.lit() {
                let c = xf.p(
                    (area.min[0] + area.max[0]) / 2.0,
                    (area.min[1] + area.max[1]) / 2.0,
                ) + shift;
                let text = if *id == 14 { "L3" } else { "R3" };
                let colour = if st.pressed {
                    on_accent()
                } else {
                    Color32::WHITE
                };
                p.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    text,
                    theme::bold(xf.v(15.0).max(8.0)),
                    colour,
                );
            }
        }
    }

    for (id, area) in &a.areas {
        let st = state(*id);
        if st.marked && !st.pressed {
            let (dx, dy) = stick_shift(*id);
            mark(&p, &xf, xf.q(area.corner()) + vec2(xf.v(dx), xf.v(dy)));
        }
    }

    if o.show_back {
        p.text(
            xf.p(MID, 484.0),
            egui::Align2::CENTER_CENTER,
            "ЗАДНИЕ КНОПКИ",
            FontId::new(xf.v(13.0).max(9.0), egui::FontFamily::Name("medium".into())),
            theme::FAINT,
        );
        for (id, label) in BACK_KEYS {
            draw_back_key(&p, &xf, id, label, state(id));
        }
    }

    PadResponse { clicked, hovered }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawing_has_every_key() {
        let a = art();
        assert_eq!(a.areas.len(), FRONT_KEYS.len());
        for (id, area) in &a.areas {
            assert!(!area.contours.is_empty(), "key {id} has no outline");
            for (c, t) in area.contours.iter().zip(&area.tris) {
                assert_eq!(t.len(), (c.len() - 2) * 3, "key {id} did not triangulate");
            }
        }
    }

    #[test]
    fn key_areas_sit_where_the_drawing_has_them() {
        let a = art();
        let centre = |id: u8| {
            let (_, ar) = a.areas.iter().find(|(k, _)| *k == id).unwrap();
            [(ar.min[0] + ar.max[0]) / 2.0, (ar.min[1] + ar.max[1]) / 2.0]
        };
        let near =
            |p: [f32; 2], q: [f32; 2]| (p[0] - q[0]).abs() < 1.0 && (p[1] - q[1]).abs() < 1.0;
        assert!(near(centre(4), [655.67, 217.0]), "A at {:?}", centre(4));
        assert!(near(centre(14), [331.37, 171.8]), "LS at {:?}", centre(14));
        assert!(hit(10, (360.0, 55.0), (0.0, 0.0)), "LB");
        assert!(hit(0, (408.8, 245.0), (0.0, 0.0)), "D-pad up");
        assert!(
            !hit(0, (408.8, 274.0 + 20.0), (0.0, 0.0)),
            "D-pad up vs down"
        );
    }
}
