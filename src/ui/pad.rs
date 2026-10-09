//! The Apex 5 drawn from vectors: graphite body, the silver V with the
//! logo and screen, light strips and every button as a hit target.
//!
//! Proportions follow the Apex 5 line drawing on Flydigi's own site
//! (space_station.flydigi.com). Coordinates are in that drawing's 984-wide
//! space; the pad spans x 160..824 and is scaled to fit.

use std::sync::OnceLock;

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};

use super::theme::{self, accent, on_accent};
use super::widgets::lerp_colour;

const X0: f32 = 160.0;
const W: f32 = 664.0;
const H_FRONT: f32 = 478.0;
const H_BACK: f32 = 532.0;
const MID: f32 = 492.0;

/// Left half of the body outline as cubic segments, from the top centre
/// round the left grip to the bottom centre. The right half is mirrored.
const START: [f32; 2] = [MID, 61.0];
const LEFT_HALF: [[f32; 6]; 6] = [
    [400.0, 61.0, 310.0, 72.0, 258.0, 99.0],
    [236.0, 160.0, 205.0, 235.0, 185.0, 295.0],
    [174.0, 340.0, 166.0, 400.0, 185.0, 440.0],
    [200.0, 465.0, 228.0, 472.0, 245.0, 463.0],
    [280.0, 433.0, 325.0, 385.0, 374.0, 374.0],
    [410.0, 372.0, 450.0, 372.0, MID, 372.0],
];

const DPAD: [f32; 2] = [409.0, 274.5];
const LSTICK: [f32; 2] = [331.5, 171.5];
const RSTICK: [f32; 2] = [571.5, 273.5];
const SCREEN: [f32; 4] = [453.0, 144.0, 532.0, 182.0];

/// The silver V plate (convex).
const PLATE: [[f32; 2]; 11] = [
    [366.0, 71.0],
    [618.0, 71.0],
    [622.0, 78.0],
    [622.0, 88.0],
    [531.0, 197.0],
    [514.0, 209.0],
    [492.0, 211.0],
    [470.0, 209.0],
    [453.0, 197.0],
    [362.0, 88.0],
    [362.0, 78.0],
];

/// Light strips along the outer edge of each grip: left bottom→top, then
/// right top→bottom, so the 12 LEDs run clockwise.
const LEDS: [[f32; 2]; 12] = [
    [205.0, 418.0],
    [197.0, 388.0],
    [197.0, 357.0],
    [202.0, 325.0],
    [210.0, 294.0],
    [221.0, 264.0],
    [763.0, 264.0],
    [774.0, 294.0],
    [782.0, 325.0],
    [787.0, 357.0],
    [787.0, 388.0],
    [779.0, 418.0],
];

pub const BACK_KEYS: [(u8, &str); 6] = [
    (18, "M1"),
    (19, "M2"),
    (20, "M3"),
    (21, "M4"),
    (22, "LM"),
    (23, "RM"),
];

const LB: [[f32; 2]; 11] = [
    [271.0, 100.0],
    [272.0, 78.0],
    [283.0, 69.0],
    [303.0, 59.0],
    [331.0, 50.0],
    [360.0, 44.0],
    [376.0, 44.0],
    [404.0, 51.0],
    [434.0, 60.0],
    [440.0, 66.0],
    [300.0, 100.0],
];

#[derive(Clone, Copy)]
enum Geom {
    Circle(f32, f32, f32),
    Rect(f32, f32, f32, f32, f32),
    /// Slanted pill between two centres.
    Capsule(f32, f32, f32, f32, f32),
    Bumper(bool),
}

fn mirror(p: [f32; 2]) -> [f32; 2] {
    [2.0 * MID - p[0], p[1]]
}

fn geom(id: u8) -> Option<Geom> {
    let [dx, dy] = DPAD;
    let (a, l) = (10.0, 40.0);
    Some(match id {
        0 => Geom::Rect(dx - a, dy - l, dx + a, dy - a, 4.0),
        2 => Geom::Rect(dx - a, dy + a, dx + a, dy + l, 4.0),
        3 => Geom::Rect(dx - l, dy - a, dx - a, dy + a, 4.0),
        1 => Geom::Rect(dx + a, dy - a, dx + l, dy + a, 4.0),
        8 => Geom::Circle(656.0, 131.0, 22.0),
        7 => Geom::Circle(613.0, 174.0, 22.0),
        5 => Geom::Circle(699.0, 174.0, 22.0),
        4 => Geom::Circle(656.0, 217.0, 22.0),
        6 => Geom::Capsule(400.0, 95.0, 421.0, 121.0, 8.0),
        9 => Geom::Capsule(584.0, 95.0, 563.0, 121.0, 8.0),
        27 => Geom::Circle(MID, 104.0, 19.0),
        10 => Geom::Bumper(false),
        11 => Geom::Bumper(true),
        12 => Geom::Rect(300.0, 4.0, 404.0, 38.0, 13.0),
        13 => Geom::Rect(580.0, 4.0, 684.0, 38.0, 13.0),
        14 => Geom::Circle(LSTICK[0], LSTICK[1], 34.0),
        15 => Geom::Circle(RSTICK[0], RSTICK[1], 34.0),
        18..=23 => {
            let x = 247.0 + (id - 18) as f32 * 84.0;
            Geom::Rect(x, 500.0, x + 70.0, 528.0, 14.0)
        }
        _ => return None,
    })
}

const HIT_ORDER: [u8; 23] = [
    18, 19, 20, 21, 22, 23, 12, 13, 4, 5, 7, 8, 6, 9, 27, 0, 1, 2, 3, 14, 15, 10, 11,
];

fn label(id: u8) -> &'static str {
    match id {
        0 => "▲",
        1 => "▶",
        2 => "▼",
        3 => "◀",
        4 => "A",
        5 => "B",
        7 => "X",
        8 => "Y",
        10 => "LB",
        11 => "RB",
        12 => "LT",
        13 => "RT",
        14 => "L3",
        15 => "R3",
        18 => "M1",
        19 => "M2",
        20 => "M3",
        21 => "M4",
        22 => "LM",
        23 => "RM",
        _ => "",
    }
}

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

struct Xf {
    o: Pos2,
    s: f32,
}

impl Xf {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.o.x + (x - X0) * self.s, self.o.y + y * self.s)
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
        ((p.x - self.o.x) / self.s + X0, (p.y - self.o.y) / self.s)
    }
}

fn flatten(start: [f32; 2], segs: &[[f32; 6]], out: &mut Vec<[f32; 2]>) {
    let mut a = start;
    for seg in segs {
        let (c1, c2, b) = ([seg[0], seg[1]], [seg[2], seg[3]], [seg[4], seg[5]]);
        for i in 0..20 {
            let t = i as f32 / 20.0;
            let u = 1.0 - t;
            let k = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            out.push([
                k[0] * a[0] + k[1] * c1[0] + k[2] * c2[0] + k[3] * b[0],
                k[0] * a[1] + k[1] * c1[1] + k[2] * c2[1] + k[3] * b[1],
            ]);
        }
        a = b;
    }
}

fn outline() -> &'static (Vec<[f32; 2]>, Vec<u32>) {
    static CACHE: OnceLock<(Vec<[f32; 2]>, Vec<u32>)> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut left = vec![];
        flatten(START, &LEFT_HALF, &mut left);
        // Left half ends just before the bottom centre; the right half is the
        // same curve mirrored and walked back up to the top.
        let mut pts = left.clone();
        pts.push([MID, 372.0]);
        pts.extend(left.iter().skip(1).rev().map(|q| mirror(*q)));
        let tris = triangulate(&pts);
        (pts, tris)
    })
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

fn gradient(top: Color32, bottom: Color32, y: f32, y0: f32, y1: f32) -> Color32 {
    lerp_colour(top, bottom, ((y - y0) / (y1 - y0)).clamp(0.0, 1.0))
}

/// Convex polygon with a vertical gradient fill and an outline.
fn gradient_poly(p: &egui::Painter, pts: &[Pos2], top: Color32, bottom: Color32, stroke: Stroke) {
    let (y0, y1) = pts
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), q| (a.min(q.y), b.max(q.y)));
    let mut mesh = Mesh::default();
    for q in pts {
        mesh.colored_vertex(*q, gradient(top, bottom, q.y, y0, y1));
    }
    for i in 1..pts.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::closed_line(pts.to_vec(), stroke));
}

fn capsule_points(xf: &Xf, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Vec<Pos2> {
    let a = (y1 - y0).atan2(x1 - x0);
    let mut pts = vec![];
    for (cx, cy, from) in [
        (x1, y1, a - std::f32::consts::FRAC_PI_2),
        (x0, y0, a + std::f32::consts::FRAC_PI_2),
    ] {
        for i in 0..=10 {
            let t = from + i as f32 / 10.0 * std::f32::consts::PI;
            pts.push(xf.p(cx + r * t.cos(), cy + r * t.sin()));
        }
    }
    pts
}

fn bumper_points(xf: &Xf, right: bool) -> Vec<Pos2> {
    let pts: Vec<Pos2> = LB
        .iter()
        .map(|q| if right { xf.q(mirror(*q)) } else { xf.q(*q) })
        .collect();
    if right {
        pts.into_iter().rev().collect()
    } else {
        pts
    }
}

fn draw_body(p: &egui::Painter, xf: &Xf) {
    let (pts, tris) = outline();
    let top = Color32::from_rgb(47, 53, 63);
    let bottom = Color32::from_rgb(18, 20, 25);
    let shadow: Vec<Pos2> = pts.iter().map(|q| xf.p(q[0], q[1] + 7.0)).collect();
    p.add(Shape::closed_line(
        shadow,
        Stroke::new(xf.v(10.0), Color32::from_black_alpha(50)),
    ));
    let mut mesh = Mesh::default();
    for q in pts {
        mesh.colored_vertex(xf.q(*q), gradient(top, bottom, q[1], 60.0, 470.0));
    }
    mesh.indices = tris.clone();
    p.add(Shape::mesh(mesh));
    let line: Vec<Pos2> = pts.iter().map(|q| xf.q(*q)).collect();
    p.add(Shape::closed_line(
        line,
        Stroke::new(1.5, Color32::from_rgb(74, 82, 96)),
    ));
    // Highlight along the top edge.
    let hi: Vec<Pos2> = pts
        .iter()
        .filter(|q| q[1] < 100.0 && q[0] > 262.0 && q[0] < 722.0)
        .map(|q| xf.p(q[0], q[1] + 4.0))
        .collect();
    if hi.len() > 2 {
        p.add(Shape::line(
            hi,
            Stroke::new(1.0, Color32::from_white_alpha(20)),
        ));
    }

    // Grip details: the slider on the left and the wheel on the right.
    let groove = Stroke::new(1.0, Color32::from_rgb(58, 65, 77));
    let slider = [
        [216.0, 254.0],
        [313.0, 296.0],
        [294.0, 320.0],
        [205.0, 293.0],
    ];
    p.add(Shape::convex_polygon(
        slider.iter().map(|q| xf.q(*q)).collect(),
        Color32::from_rgb(24, 27, 33),
        groove,
    ));
    let wheel = capsule_points(xf, 760.0, 250.0, 779.0, 306.0, 10.0);
    p.add(Shape::convex_polygon(
        wheel,
        Color32::from_rgb(24, 27, 33),
        groove,
    ));
    // Stick-tension dials next to the sticks.
    for c in [[405.0, 174.0], [650.0, 270.0]] {
        p.circle_stroke(xf.q(c), xf.v(8.0), groove);
        p.circle_filled(xf.q(c), xf.v(2.0), Color32::from_rgb(70, 78, 92));
    }
}

fn draw_plate(p: &egui::Painter, xf: &Xf) {
    let pts: Vec<Pos2> = PLATE.iter().map(|q| xf.q(*q)).collect();
    gradient_poly(
        p,
        &pts,
        Color32::from_rgb(206, 212, 221),
        Color32::from_rgb(118, 125, 138),
        Stroke::new(1.0, Color32::from_white_alpha(80)),
    );
    // Inner bevel.
    let inner: Vec<Pos2> = PLATE
        .iter()
        .map(|q| {
            let c = [MID, 120.0];
            xf.p(c[0] + (q[0] - c[0]) * 0.93, c[1] + (q[1] - c[1]) * 0.9)
        })
        .collect();
    p.add(Shape::closed_line(
        inner,
        Stroke::new(1.0, Color32::from_black_alpha(45)),
    ));
}

fn draw_screen(p: &egui::Painter, xf: &Xf, text: Option<&str>) {
    let r = xf.rect(SCREEN[0], SCREEN[1], SCREEN[2], SCREEN[3]);
    p.rect(
        r,
        CornerRadius::same(xf.v(5.0) as u8),
        Color32::from_rgb(6, 8, 11),
        Stroke::new(1.0, Color32::from_rgb(46, 50, 58)),
        StrokeKind::Outside,
    );
    if let Some(t) = text {
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            t,
            theme::bold(xf.v(17.0).max(8.0)),
            accent(),
        );
    }
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
    for strip in [0..6usize, 6..12] {
        for i in strip.clone() {
            if i + 1 < strip.end {
                let c = lerp_colour(colour(i), colour(i + 1), 0.5);
                let seg = [xf.q(LEDS[i]), xf.q(LEDS[i + 1])];
                if lit(c) {
                    p.line_segment(seg, Stroke::new(xf.v(22.0), c.gamma_multiply(0.09)));
                }
                p.line_segment(
                    seg,
                    Stroke::new(xf.v(5.0), if lit(c) { c.gamma_multiply(0.75) } else { c }),
                );
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
            p.circle_filled(at, xf.v(3.5), Color32::from_rgb(46, 51, 60));
        }
    }
}

#[derive(Clone, Copy, Default)]
struct KeyState {
    hovered: bool,
    selected: bool,
    pressed: bool,
    marked: bool,
}

struct Look {
    fill: Color32,
    stroke: Stroke,
    text: Color32,
}

fn look(st: KeyState, base: Color32, idle_text: Color32) -> Look {
    let a = accent();
    Look {
        fill: if st.pressed {
            a
        } else if st.selected {
            lerp_colour(base, a, 0.28)
        } else if st.hovered {
            lerp_colour(base, Color32::WHITE, 0.08)
        } else {
            base
        },
        stroke: if st.selected || st.pressed {
            Stroke::new(2.0, a)
        } else if st.hovered {
            Stroke::new(1.5, a.gamma_multiply(0.7))
        } else {
            Stroke::new(1.0, Color32::from_rgb(70, 78, 92))
        },
        text: if st.pressed {
            on_accent()
        } else if st.selected || st.hovered {
            Color32::WHITE
        } else {
            idle_text
        },
    }
}

fn mark(p: &egui::Painter, xf: &Xf, at: Pos2) {
    p.circle_filled(at, xf.v(5.5).max(3.0), accent());
    p.circle_stroke(at, xf.v(5.5).max(3.0), Stroke::new(1.5, theme::BG));
}

fn draw_key(p: &egui::Painter, xf: &Xf, id: u8, st: KeyState, trigger: Option<f32>) {
    let Some(g) = geom(id) else { return };
    let base = Color32::from_rgb(29, 33, 40);
    let lk = look(st, base, theme::DIM);
    let (centre, corner) = match g {
        Geom::Circle(x, y, r) => {
            let c = xf.p(x, y);
            p.circle(c, xf.v(r), lk.fill, lk.stroke);
            (c, xf.p(x + r * 0.72, y - r * 0.72))
        }
        Geom::Rect(x0, y0, x1, y1, r) => {
            let rect = xf.rect(x0, y0, x1, y1);
            p.rect(
                rect,
                CornerRadius::same(xf.v(r) as u8),
                lk.fill,
                lk.stroke,
                StrokeKind::Inside,
            );
            if let Some(v) = trigger.filter(|v| *v > 0.01) {
                let h = rect.height() * v;
                let f =
                    Rect::from_min_max(pos2(rect.left(), rect.bottom() - h), rect.right_bottom());
                p.rect_filled(
                    f.shrink(1.5),
                    CornerRadius::same(xf.v(r - 2.0).max(0.0) as u8),
                    accent().gamma_multiply(0.85),
                );
            }
            (rect.center(), xf.p(x1 - 3.0, y0 + 3.0))
        }
        Geom::Capsule(x0, y0, x1, y1, r) => {
            let pts = capsule_points(xf, x0, y0, x1, y1, r);
            let fill = if st.pressed || st.selected || st.hovered {
                lk.fill
            } else {
                Color32::from_rgb(150, 157, 170)
            };
            p.add(Shape::convex_polygon(pts, fill, lk.stroke));
            let c = xf.p((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            (c, xf.p(x1.max(x0) + 4.0, y0.min(y1)))
        }
        Geom::Bumper(right) => {
            let pts = bumper_points(xf, right);
            p.add(Shape::convex_polygon(pts, lk.fill, lk.stroke));
            let x = if right { 2.0 * MID - 352.0 } else { 352.0 };
            (
                xf.p(x, 56.0),
                xf.p(if right { 2.0 * MID - 404.0 } else { 404.0 }, 50.0),
            )
        }
    };
    match id {
        // Home is the Flydigi mark on the plate: draw a winged V.
        27 => {
            let c = centre;
            let col = if st.pressed {
                on_accent()
            } else if st.selected || st.hovered {
                accent()
            } else {
                Color32::from_rgb(40, 45, 54)
            };
            let s = xf.v(1.0);
            let wing = |sx: f32| {
                vec![
                    c + vec2(sx * 2.0, 13.0) * s,
                    c + vec2(sx * 15.0, -12.0) * s,
                    c + vec2(sx * 6.0, -6.0) * s,
                    c + vec2(sx * 1.0, 4.0) * s,
                ]
            };
            p.add(Shape::convex_polygon(wing(-1.0), col, Stroke::NONE));
            p.add(Shape::convex_polygon(wing(1.0), col, Stroke::NONE));
        }
        6 => {
            for d in [vec2(-1.5, -1.5), vec2(1.5, 1.5)] {
                let r = Rect::from_center_size(centre + d * xf.v(1.0), vec2(xf.v(6.0), xf.v(6.0)));
                p.rect_stroke(
                    r,
                    1.0,
                    Stroke::new(1.0, Color32::from_rgb(40, 45, 54)),
                    StrokeKind::Middle,
                );
            }
        }
        9 => {
            for dy in [-3.0f32, 0.0, 3.0] {
                let y = centre.y + xf.v(dy);
                p.line_segment(
                    [pos2(centre.x - xf.v(5.0), y), pos2(centre.x + xf.v(5.0), y)],
                    Stroke::new(1.0, Color32::from_rgb(40, 45, 54)),
                );
            }
        }
        _ => {
            let t = label(id);
            let size = match id {
                0..=3 => 11.0,
                4 | 5 | 7 | 8 => 19.0,
                _ => 15.0,
            };
            if !t.is_empty() {
                p.text(
                    centre,
                    egui::Align2::CENTER_CENTER,
                    t,
                    theme::bold(xf.v(size).max(8.0)),
                    lk.text,
                );
            }
        }
    }
    if st.marked && !st.pressed {
        mark(p, xf, corner);
    }
}

fn draw_dpad(p: &egui::Painter, xf: &Xf, state: &dyn Fn(u8) -> KeyState) {
    let [cx, cy] = DPAD;
    // Rounded diamond base.
    let mut base = vec![];
    for i in 0..4 {
        let a = i as f32 * std::f32::consts::FRAC_PI_2;
        for k in 0..=6 {
            let t = a - 0.35 + k as f32 / 6.0 * 0.7;
            let r = 60.0 - (1.0 - ((t - a) / 0.35).powi(2)).max(0.0) * 6.0;
            base.push(xf.p(cx + r * t.cos(), cy + r * t.sin()));
        }
    }
    p.add(Shape::convex_polygon(
        base,
        Color32::from_rgb(14, 16, 20),
        Stroke::new(1.0, Color32::from_rgb(60, 67, 79)),
    ));
    p.rect_filled(
        xf.rect(cx - 10.0, cy - 10.0, cx + 10.0, cy + 10.0),
        0.0,
        Color32::from_rgb(29, 33, 40),
    );
    for id in 0..4u8 {
        draw_key(p, xf, id, state(id), None);
    }
}

fn draw_stick(p: &egui::Painter, xf: &Xf, id: u8, centre: [f32; 2], st: KeyState, pos: (f32, f32)) {
    let c = xf.q(centre);
    p.circle(
        c,
        xf.v(48.0),
        Color32::from_rgb(12, 14, 18),
        Stroke::new(1.0, Color32::from_rgb(60, 67, 79)),
    );
    let cap = c + vec2(pos.0, -pos.1) * xf.v(13.0);
    let lk = look(st, Color32::from_rgb(29, 33, 40), Color32::WHITE);
    p.circle(cap, xf.v(34.0), lk.fill, lk.stroke);
    p.circle(
        cap,
        xf.v(24.0),
        Color32::from_rgb(21, 24, 29),
        Stroke::new(1.0, Color32::from_rgb(52, 58, 69)),
    );
    // Knurled rim.
    for i in 0..24 {
        let a = i as f32 / 24.0 * std::f32::consts::TAU;
        let d = vec2(a.cos(), a.sin());
        p.line_segment(
            [cap + d * xf.v(27.0), cap + d * xf.v(31.0)],
            Stroke::new(1.0, Color32::from_rgb(46, 52, 62)),
        );
    }
    if st.selected || st.hovered || st.pressed {
        p.text(
            cap,
            egui::Align2::CENTER_CENTER,
            label(id),
            theme::bold(xf.v(15.0).max(8.0)),
            lk.text,
        );
    }
    if st.marked {
        mark(p, xf, xf.p(centre[0] + 26.0, centre[1] - 26.0));
    }
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

fn hit(g: Geom, q: (f32, f32)) -> bool {
    let (x, y) = q;
    match g {
        Geom::Circle(cx, cy, r) => (x - cx).powi(2) + (y - cy).powi(2) <= (r + 3.0).powi(2),
        Geom::Rect(x0, y0, x1, y1, _) => {
            x >= x0 - 2.0 && x <= x1 + 2.0 && y >= y0 - 2.0 && y <= y1 + 2.0
        }
        Geom::Capsule(x0, y0, x1, y1, r) => {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let t = (((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            let (px, py) = (x0 + t * dx, y0 + t * dy);
            (x - px).powi(2) + (y - py).powi(2) <= (r + 4.0).powi(2)
        }
        Geom::Bumper(right) => {
            let pts: Vec<[f32; 2]> = LB
                .iter()
                .map(|p| if right { mirror(*p) } else { *p })
                .collect();
            // Only the part that shows above the body.
            inside_poly(&pts, q) && y < 72.0
        }
    }
}

pub fn show(ui: &mut Ui, size: Vec2, o: &Opts) -> PadResponse {
    let h = if o.show_back { H_BACK } else { H_FRONT };
    let sense = if o.interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let s = (rect.width() / W).min(rect.height() / h);
    let origin = pos2(rect.center().x - W * s / 2.0, rect.center().y - h * s / 2.0);
    let xf = Xf { o: origin, s };

    let mut hovered = None;
    if o.interactive
        && let Some(pos) = resp.hover_pos()
    {
        let q = xf.inv(pos);
        hovered = HIT_ORDER
            .iter()
            .copied()
            .filter(|id| o.show_back || !(18..=23).contains(id))
            .find(|id| geom(*id).is_some_and(|g| hit(g, q)));
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

    // Triggers and bumpers sit behind the body.
    let (lt, rt) = o.triggers.unwrap_or((0.0, 0.0));
    draw_key(&p, &xf, 12, state(12), Some(lt));
    draw_key(&p, &xf, 13, state(13), Some(rt));
    draw_key(&p, &xf, 10, state(10), None);
    draw_key(&p, &xf, 11, state(11), None);
    draw_body(&p, &xf);
    draw_leds(&p, &xf, o.leds, o.brightness);
    draw_plate(&p, &xf);
    draw_screen(&p, &xf, o.screen);
    for id in [27u8, 6, 9] {
        draw_key(&p, &xf, id, state(id), None);
    }
    draw_dpad(&p, &xf, &state);
    for id in [4u8, 5, 7, 8] {
        draw_key(&p, &xf, id, state(id), None);
    }
    let [l, r] = o.sticks.unwrap_or([(0.0, 0.0); 2]);
    draw_stick(&p, &xf, 14, LSTICK, state(14), l);
    draw_stick(&p, &xf, 15, RSTICK, state(15), r);

    if o.show_back {
        p.text(
            xf.p(MID, 484.0),
            egui::Align2::CENTER_CENTER,
            "ЗАДНИЕ КНОПКИ",
            FontId::new(xf.v(13.0).max(9.0), egui::FontFamily::Name("medium".into())),
            theme::FAINT,
        );
        for (id, _) in BACK_KEYS {
            draw_key(&p, &xf, id, state(id), None);
        }
    }

    PadResponse { clicked, hovered }
}
