//! The Apex 5, drawn from Flydigi's own line drawing.
//!
//! `assets/apex5.svg` (built by `tools/svg`) is rasterised with resvg at the
//! exact on-screen size. Key highlights, the moving stick caps, the lighting,
//! the screen text and the button labels are painted over it. Coordinates are
//! the drawing's 984-wide space. The pad itself spans its `data-pad-view`;
//! the whole viewBox adds the shoulder and back buttons as tiles around it.

use std::sync::OnceLock;

use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Id, Mesh, Pos2, Rect, Sense, Shape, Stroke,
    TextureHandle, TextureOptions, Ui, Vec2, epaint::TextShape, pos2, vec2,
};
use resvg::{tiny_skia, usvg};

use super::theme::{self, accent, on_accent};
use super::widgets::lerp_colour;

const SVG: &str = include_str!("../../assets/apex5.svg");

const SCREEN: [f32; 4] = [452.65, 144.48, 531.35, 182.33];
/// How far a stick cap moves at full deflection.
const STICK_TRAVEL: f32 = 13.0;
/// The drawing's line and body colours, swapped for the theme's.
const INK: &str = "#9db7dc";
const BODY: &str = "#1a2636";

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// Which of the lighting block's 12 colours each LED shows, as found on a
/// pad with `apex5ctl led probe`: ten along the strip, left to right, and the
/// two under the FLYDIGI plate (left, right) take the first two.
const STRIP_SLOTS: [usize; 10] = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const PLATE_SLOTS: [usize; 2] = [1, 0];

/// Key areas in the drawing, by key id: `#keys` on the pad, `#tiles` around it.
const KEYS: [(u8, &str); 23] = [
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
    (18, "m1"),
    (19, "m2"),
    (20, "m3"),
    (21, "m4"),
    (22, "lm"),
    (23, "rm"),
    (27, "home"),
];

/// Keys drawn as tiles around the pad rather than on it.
fn is_tile(id: u8) -> bool {
    matches!(id, 10..=13 | 18..=23)
}

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
    /// Show the shoulder and back buttons as tiles around the pad.
    pub show_tiles: bool,
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

    fn centre(&self) -> [f32; 2] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ]
    }
}

/// A point of the light strip: position, unit normal and how far along the
/// strip it is (0..1).
#[derive(Clone, Copy)]
struct StripPoint {
    at: [f32; 2],
    normal: [f32; 2],
    t: f32,
}

/// The drawing in one theme's ink.
struct Trees {
    /// The whole drawing without the stick caps.
    full: usvg::Tree,
    /// The pad alone: no stick caps, no tiles, cropped to the pad.
    pad: usvg::Tree,
    /// The drawing as it is, for rendering the caps on their own.
    src: usvg::Tree,
}

struct Art {
    /// Dark and light.
    trees: [Trees; 2],
    /// viewBox of the whole drawing and of the pad alone: x, y, width, height.
    view: [f32; 4],
    pad_view: [f32; 4],
    areas: Vec<(u8, Area)>,
    plate: Area,
    /// Direction of the plate's long side, in radians.
    plate_angle: f32,
    strip: Vec<StripPoint>,
    /// Where along the strip (0..1) each of its LEDs sits, left to right.
    strip_leds: Vec<f32>,
    /// The plate's LEDs.
    plate_leds: Vec<[f32; 2]>,
    /// Tile labels: key, text, centre.
    labels: Vec<(u8, String, [f32; 2])>,
}

fn art() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| {
        let doc = usvg::roxmltree::Document::parse(SVG).expect("assets/apex5.svg parses");
        let root = doc.root_element();
        let rect = |name: &str| -> [f32; 4] {
            let v: Vec<f32> = root
                .attribute(name)
                .unwrap_or_else(|| panic!("assets/apex5.svg has no {name}"))
                .split_whitespace()
                .map(|n| n.parse().expect("a number"))
                .collect();
            [v[0], v[1], v[2], v[3]]
        };
        let (view, pad_view) = (rect("viewBox"), rect("data-pad-view"));
        let led_points = |which: &str| -> Vec<[f32; 2]> {
            doc.descendants()
                .filter(|n| n.attribute("data-led") == Some(which))
                .map(|n| {
                    let num = |a: &str| n.attribute(a).and_then(|v| v.parse().ok());
                    [num("cx").expect("LED x"), num("cy").expect("LED y")]
                })
                .collect()
        };
        let labels = doc
            .descendants()
            .filter(|n| n.has_tag_name("text"))
            .filter_map(|n| {
                let num = |a: &str| n.attribute(a)?.parse::<f32>().ok();
                Some((
                    n.attribute("data-key")?.parse().ok()?,
                    n.text()?.to_owned(),
                    [num("x")?, num("y")?],
                ))
            })
            .collect();

        let opt = usvg::Options::default();
        let tree = |svg: &str| usvg::Tree::from_str(svg, &opt).expect("assets/apex5.svg parses");
        let trees = |pal: &theme::Palette| {
            let svg = SVG
                .replace(INK, &hex(pal.ink))
                .replace(BODY, &hex(pal.body));
            let no_caps = without_groups(&svg, &["ls-cap", "rs-cap"]);
            Trees {
                src: tree(&svg),
                full: tree(&no_caps),
                pad: tree(&without_groups(&no_caps, &["tiles"]).replacen(
                    &format!("viewBox=\"{}\"", root.attribute("viewBox").unwrap()),
                    &format!("viewBox=\"{}\"", root.attribute("data-pad-view").unwrap()),
                    1,
                )),
            }
        };
        let trees = [trees(&theme::DARK), trees(&theme::LIGHT)];
        let src = &trees[0].src;

        let node = |id: &str| {
            src.node_by_id(id)
                .unwrap_or_else(|| panic!("assets/apex5.svg has no #{id}"))
        };
        let mut areas: Vec<(u8, Area)> = KEYS
            .iter()
            .map(|&(key, id)| (key, area(node(id), view)))
            .collect();
        areas.sort_by_key(|(key, _)| *key);
        let plate = area(node("led-plate-area"), view);
        let plate_angle = long_side(&plate.contours[0]);
        let mut strip_line = vec![];
        collect_contours(node("led-strip"), view, &mut strip_line);
        let strip = strip_points(&strip_line.concat(), 3.0);
        let mut strip_leds: Vec<f32> = led_points("strip")
            .iter()
            .map(|q| {
                let d = |sp: &StripPoint| (sp.at[0] - q[0]).hypot(sp.at[1] - q[1]);
                strip
                    .iter()
                    .min_by(|a, b| d(a).total_cmp(&d(b)))
                    .expect("strip has points")
                    .t
            })
            .collect();
        strip_leds.sort_by(f32::total_cmp);
        let plate_leds = led_points("plate");
        Art {
            trees,
            view,
            pad_view,
            areas,
            plate,
            plate_angle,
            strip,
            strip_leds,
            plate_leds,
            labels,
        }
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

fn area(node: &usvg::Node, view: [f32; 4]) -> Area {
    let mut contours = vec![];
    collect_contours(node, view, &mut contours);
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
fn collect_contours(node: &usvg::Node, view: [f32; 4], out: &mut Vec<Vec<[f32; 2]>>) {
    use usvg::tiny_skia_path::PathSegment;
    match node {
        usvg::Node::Group(g) => {
            for child in g.children() {
                collect_contours(child, view, out);
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
                cur.push([q.x + view[0], q.y + view[1]]);
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

/// Angle of a polygon's longest side, folded into -90°..90°.
fn long_side(c: &[[f32; 2]]) -> f32 {
    let (a, b) = (0..c.len())
        .map(|i| (c[i], c[(i + 1) % c.len()]))
        .max_by(|(a, b), (p, q)| {
            let len = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]).hypot(b[1] - a[1]);
            len(*a, *b).total_cmp(&len(*p, *q))
        })
        .expect("polygon has sides");
    let mut ang = (b[1] - a[1]).atan2(b[0] - a[0]);
    if ang > std::f32::consts::FRAC_PI_2 {
        ang -= std::f32::consts::PI;
    } else if ang < -std::f32::consts::FRAC_PI_2 {
        ang += std::f32::consts::PI;
    }
    ang
}

/// The polyline resampled every `step` units, with normals and arc position.
fn strip_points(line: &[[f32; 2]], step: f32) -> Vec<StripPoint> {
    let mut dist = vec![0.0];
    for w in line.windows(2) {
        dist.push(dist.last().unwrap() + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    let total = *dist.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return vec![];
    }
    let n = (total / step).ceil() as usize;
    let at = |d: f32| -> [f32; 2] {
        let i = dist.partition_point(|&x| x <= d).clamp(1, line.len() - 1);
        let (a, b) = (line[i - 1], line[i]);
        let span = (dist[i] - dist[i - 1]).max(1e-6);
        let t = ((d - dist[i - 1]) / span).clamp(0.0, 1.0);
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    };
    (0..=n)
        .map(|k| {
            let d = total * k as f32 / n as f32;
            let (p, q) = (at((d - 1.0).max(0.0)), at((d + 1.0).min(total)));
            let len = (q[0] - p[0]).hypot(q[1] - p[1]).max(1e-6);
            StripPoint {
                at: at(d),
                normal: [-(q[1] - p[1]) / len, (q[0] - p[0]) / len],
                t: d / total,
            }
        })
        .collect()
}

// --- rasters -------------------------------------------------------------------

/// The drawing rendered for one on-screen size.
#[derive(Clone)]
struct Raster {
    px: [usize; 2],
    tiles: bool,
    light: bool,
    base: TextureHandle,
    /// Stick caps with their rest rectangles in drawing coordinates.
    caps: [(TextureHandle, [f32; 4]); 2],
}

fn render(tree: &usvg::Tree, px: [usize; 2], ts: tiny_skia::Transform) -> ColorImage {
    let mut pixmap = tiny_skia::Pixmap::new(px[0] as u32, px[1] as u32).expect("non-empty size");
    resvg::render(tree, ts, &mut pixmap.as_mut());
    ColorImage::from_rgba_premultiplied(px, pixmap.data())
}

fn render_node(node: &usvg::Node, px: [usize; 2], scale: f32, view: [f32; 4]) -> ColorImage {
    let mut pixmap = tiny_skia::Pixmap::new(px[0] as u32, px[1] as u32).expect("non-empty size");
    // render_node skips the ancestors' transforms, including the viewBox
    // shift that the node's bounding box already has.
    let ts = tiny_skia::Transform::from_scale(scale, scale).pre_translate(-view[0], -view[1]);
    resvg::render_node(node, ts, &mut pixmap.as_mut());
    ColorImage::from_rgba_premultiplied(px, pixmap.data())
}

/// Rasters for the given pixel size and theme, cached across frames. A few
/// are kept, since several screens show the pad at different sizes.
fn raster(ctx: &egui::Context, px: [usize; 2], tiles: bool) -> Raster {
    let id = Id::new("apex5-pad-rasters");
    let light = theme::is_light();
    if let Some(r) = ctx.data(|d| {
        d.get_temp::<Vec<Raster>>(id).and_then(|v| {
            v.iter()
                .find(|r| r.px == px && r.tiles == tiles && r.light == light)
                .cloned()
        })
    }) {
        return r;
    }
    let a = art();
    let t = &a.trees[light as usize];
    let (tree, view) = if tiles {
        (&t.full, a.view)
    } else {
        (&t.pad, a.pad_view)
    };
    let scale = px[0] as f32 / view[2];
    let base = ctx.load_texture(
        "apex5-pad",
        render(
            tree,
            px,
            tiny_skia::Transform::from_scale(scale, px[1] as f32 / view[3]),
        ),
        TextureOptions::LINEAR,
    );
    let caps = CAPS.map(|(_, id)| {
        let node = t.src.node_by_id(id).expect("cap group exists");
        let bb = node.abs_layer_bounding_box().expect("cap has a size");
        let size = [
            (bb.width() * scale).ceil() as usize + 1,
            (bb.height() * scale).ceil() as usize + 1,
        ];
        let tex = ctx.load_texture(
            id,
            render_node(node, size, scale, a.view),
            TextureOptions::LINEAR,
        );
        let x = bb.x() + a.view[0];
        let y = bb.y() + a.view[1];
        let rect = [x, y, x + size[0] as f32 / scale, y + size[1] as f32 / scale];
        (tex, rect)
    });
    let r = Raster {
        px,
        tiles,
        light,
        base,
        caps,
    };
    ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Vec<Raster>>(id);
        v.insert(0, r.clone());
        v.truncate(4);
    });
    r
}

// --- painting --------------------------------------------------------------------

/// Drawing coordinates to screen: `o` is where the view's corner `v` lands.
struct Xf {
    o: Pos2,
    v: [f32; 2],
    s: f32,
}

impl Xf {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(
            self.o.x + (x - self.v[0]) * self.s,
            self.o.y + (y - self.v[1]) * self.s,
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
            (p.x - self.o.x) / self.s + self.v[0],
            (p.y - self.o.y) / self.s + self.v[1],
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
        theme::wash()
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
    fill_area(p, xf, a, fill, stroke, shift);
}

fn fill_area(p: &egui::Painter, xf: &Xf, a: &Area, fill: Color32, stroke: Stroke, shift: Vec2) {
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
    p.circle_stroke(at, xf.v(5.5).max(3.0), Stroke::new(1.5, theme::bg()));
}

fn draw_screen(p: &egui::Painter, xf: &Xf, text: &str) {
    let r = xf.rect(SCREEN[0], SCREEN[1], SCREEN[2], SCREEN[3]);
    p.rect_filled(r, CornerRadius::same(xf.v(2.0) as u8), theme::palette().lcd);
    p.text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        theme::bold(xf.v(17.0).max(8.0)),
        accent(),
    );
}

/// A light colour as hue at full strength plus how strong it is (0..1), so
/// dark LEDs fade out instead of painting the drawing black.
fn light(c: [f32; 3], alpha: f32) -> Color32 {
    let m = c[0].max(c[1]).max(c[2]);
    if m < 1.0 {
        return Color32::TRANSPARENT;
    }
    let k = 255.0 / m;
    Color32::from_rgba_unmultiplied(
        (c[0] * k) as u8,
        (c[1] * k) as u8,
        (c[2] * k) as u8,
        (alpha * m / 255.0 * 255.0).min(255.0) as u8,
    )
}

/// The strip's light guide blends the colours of its LEDs, from the left
/// grip to the right one. The FLYDIGI plate glows with its own two LEDs.
fn draw_leds(p: &egui::Painter, xf: &Xf, leds: Option<&[[u8; 3]]>, brightness: f32) {
    let Some(leds) = leds.filter(|l| !l.is_empty()) else {
        return;
    };
    let a = art();
    let k = 0.3 + 0.7 * brightness.clamp(0.0, 1.0);
    let rgb = |c: [u8; 3]| [c[0] as f32 * k, c[1] as f32 * k, c[2] as f32 * k];
    let slot = |i: usize| rgb(leds.get(i).copied().unwrap_or_default());
    let at = &a.strip_leds;
    let colour_at = |t: f32| -> [f32; 3] {
        let i = at.partition_point(|&x| x < t);
        if i == 0 {
            return slot(STRIP_SLOTS[0]);
        }
        if i == at.len() {
            return slot(STRIP_SLOTS[at.len() - 1]);
        }
        let f = (t - at[i - 1]) / (at[i] - at[i - 1]).max(1e-6);
        let (c0, c1) = (slot(STRIP_SLOTS[i - 1]), slot(STRIP_SLOTS[i]));
        [0, 1, 2].map(|j| c0[j] + (c1[j] - c0[j]) * f)
    };
    // The strip fades out at its ends, as on the pad.
    let fade = |t: f32| (t.min(1.0 - t) / 0.06).clamp(0.0, 1.0);

    // A soft glow: each point has a transparent edge on either side.
    let mut glow = Mesh::default();
    let mut core = Mesh::default();
    for (i, sp) in a.strip.iter().enumerate() {
        let c = colour_at(sp.t);
        let f = fade(sp.t);
        let at = xf.q(sp.at);
        let nrm = vec2(sp.normal[0], sp.normal[1]);
        let edge = light(c, 0.0);
        let (w, cw) = (xf.v(11.0), xf.v(1.6));
        glow.colored_vertex(at - nrm * w, edge);
        glow.colored_vertex(at, light(c, 0.45 * f));
        glow.colored_vertex(at + nrm * w, edge);
        let hot = light(c, f);
        let hot = lerp_colour(
            hot,
            Color32::WHITE.gamma_multiply(hot.a() as f32 / 255.0),
            0.3,
        );
        core.colored_vertex(at - nrm * cw, hot);
        core.colored_vertex(at + nrm * cw, hot);
        if i > 0 {
            let (g, h) = (3 * i as u32, 2 * i as u32);
            for j in 0..2 {
                let (a0, a1, b0, b1) = (g - 3 + j, g - 2 + j, g + j, g + 1 + j);
                glow.indices.extend([a0, a1, b0, a1, b1, b0]);
            }
            core.indices.extend([h - 2, h - 1, h, h - 1, h + 1, h]);
        }
    }
    p.add(Shape::mesh(glow));
    p.add(Shape::mesh(core));

    let plate = PLATE_SLOTS
        .iter()
        .map(|&i| slot(i))
        .fold([0.0; 3], |m, c| [m[0] + c[0], m[1] + c[1], m[2] + c[2]])
        .map(|v| v / PLATE_SLOTS.len() as f32);
    let fill = light(plate, 0.22);
    if fill == Color32::TRANSPARENT {
        return;
    }
    fill_area(
        p,
        xf,
        &a.plate,
        fill,
        Stroke::new(xf.v(1.2), light(plate, 0.6)),
        Vec2::ZERO,
    );
    // Each LED is brightest right above it.
    for (q, &i) in a.plate_leds.iter().zip(&PLATE_SLOTS) {
        for (r, alpha) in [(16.0, 0.08), (9.0, 0.12), (4.0, 0.18)] {
            p.circle_filled(xf.q(*q), xf.v(r), light(slot(i), alpha));
        }
    }
    let galley = p.layout_no_wrap(
        "FLYDIGI".into(),
        theme::bold(xf.v(15.0).max(6.0)),
        lerp_colour(light(plate, 1.0), Color32::WHITE, 0.25),
    );
    let half = galley.size() / 2.0;
    let rot = egui::emath::Rot2::from_angle(a.plate_angle);
    let pos = xf.q(a.plate.centre()) - rot * half;
    p.add(TextShape::new(pos, galley, Color32::WHITE).with_angle(a.plate_angle));
}

fn hit(id: u8, q: (f32, f32), shift: (f32, f32)) -> bool {
    art()
        .areas
        .iter()
        .find(|(k, _)| *k == id)
        .is_some_and(|(_, a)| a.contains((q.0 - shift.0, q.1 - shift.1)))
}

pub fn show(ui: &mut Ui, size: Vec2, o: &Opts) -> PadResponse {
    let a = art();
    let view = if o.show_tiles { a.view } else { a.pad_view };
    let sense = if o.interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let ppp = ui.ctx().pixels_per_point();
    // Whole physical pixels for the drawing, so the raster maps 1:1.
    let px_w = ((rect.width() / view[2]).min(rect.height() / view[3]) * view[2] * ppp).floor();
    let px = [
        px_w.max(1.0) as usize,
        (px_w * view[3] / view[2]).round().max(1.0) as usize,
    ];
    let s = px[0] as f32 / ppp / view[2];
    let origin = rect.center() - vec2(view[2], view[3]) * s / 2.0;
    let xf = Xf {
        o: (origin.to_vec2() * ppp).round().to_pos2() / ppp,
        v: [view[0], view[1]],
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
    let shown = |id: u8| o.show_tiles || !is_tile(id);

    let mut hovered = None;
    if o.interactive
        && let Some(pos) = resp.hover_pos()
    {
        let q = xf.inv(pos);
        hovered = HIT_ORDER
            .iter()
            .copied()
            .filter(|id| shown(*id))
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

    let ras = raster(ui.ctx(), px, o.show_tiles);
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    p.image(
        ras.base.id(),
        xf.rect(view[0], view[1], view[0] + view[2], view[1] + view[3]),
        uv,
        Color32::WHITE,
    );
    draw_leds(&p, &xf, o.leds, o.brightness);
    if let Some(t) = o.screen {
        draw_screen(&p, &xf, t);
    }

    let (lt, rt) = o.triggers.unwrap_or((0.0, 0.0));
    for (id, area) in &a.areas {
        if matches!(id, 14 | 15) || !shown(*id) {
            continue;
        }
        let level = match id {
            12 => lt,
            13 => rt,
            _ => 0.0,
        };
        overlay(&p, &xf, area, state(*id), level, Vec2::ZERO);
    }
    if o.show_tiles {
        for (id, text, at) in &a.labels {
            let st = state(*id);
            let colour = if st.pressed {
                on_accent()
            } else if st.lit() {
                theme::strong()
            } else {
                theme::palette().ink.gamma_multiply(0.75)
            };
            p.text(
                xf.q(*at),
                egui::Align2::CENTER_CENTER,
                text,
                theme::bold(xf.v(17.0).max(8.0)),
                colour,
            );
        }
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
                let c = xf.q(area.centre()) + shift;
                let text = if *id == 14 { "L3" } else { "R3" };
                let colour = if st.pressed {
                    on_accent()
                } else {
                    theme::strong()
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
        if st.marked && !st.pressed && shown(*id) {
            let (dx, dy) = stick_shift(*id);
            mark(&p, &xf, xf.q(area.corner()) + vec2(xf.v(dx), xf.v(dy)));
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
        assert_eq!(a.areas.len(), KEYS.len());
        let plate = ("plate".to_owned(), &a.plate);
        let areas = a.areas.iter().map(|(id, ar)| (format!("key {id}"), ar));
        for (name, area) in areas.chain([plate]) {
            assert!(!area.contours.is_empty(), "{name} has no outline");
            for (c, t) in area.contours.iter().zip(&area.tris) {
                assert_eq!(t.len(), (c.len() - 2) * 3, "{name} did not triangulate");
            }
        }
    }

    #[test]
    fn key_areas_sit_where_the_drawing_has_them() {
        let a = art();
        let centre = |id: u8| a.areas.iter().find(|(k, _)| *k == id).unwrap().1.centre();
        let near =
            |p: [f32; 2], q: [f32; 2]| (p[0] - q[0]).abs() < 1.0 && (p[1] - q[1]).abs() < 1.0;
        assert!(near(centre(4), [655.67, 217.0]), "A at {:?}", centre(4));
        assert!(near(centre(14), [331.37, 171.8]), "LS at {:?}", centre(14));
        assert!(hit(0, (408.8, 245.0), (0.0, 0.0)), "D-pad up");
        assert!(
            !hit(0, (408.8, 274.0 + 20.0), (0.0, 0.0)),
            "D-pad up vs down"
        );
        // Tiles: shoulders above the pad, back buttons below it, mirrored.
        let mid = a.pad_view[0] + a.pad_view[2] / 2.0;
        for (l, r) in [(12, 13), (10, 11), (22, 23), (19, 18), (21, 20)] {
            let (cl, cr) = (centre(l), centre(r));
            assert!(near([2.0 * mid - cl[0], cl[1]], cr), "{l} vs {r}");
        }
        assert!(centre(12)[1] < a.pad_view[1] + 40.0, "LT is up top");
        assert!(centre(20)[1] > a.pad_view[1] + 300.0, "M3 is below");
    }

    #[test]
    fn every_tile_has_its_label_inside() {
        let a = art();
        let ids: Vec<u8> = a.labels.iter().map(|(id, ..)| *id).collect();
        assert_eq!(ids.len(), 10);
        for (id, text, at) in &a.labels {
            assert!(is_tile(*id), "{text} labels a tile");
            assert!(
                hit(*id, (at[0], at[1]), (0.0, 0.0)),
                "{text} sits in its tile"
            );
        }
    }

    #[test]
    fn light_strip_runs_along_the_bottom() {
        let a = art();
        let (first, last) = (a.strip.first().unwrap(), a.strip.last().unwrap());
        assert!(a.strip.len() > 100);
        assert!(first.at[0] < 300.0 && last.at[0] > 680.0);
        assert!((first.at[1] - last.at[1]).abs() < 1.0, "symmetric");
        let low = a.strip[a.strip.len() / 2];
        assert!(
            low.at[1] > 355.0 && low.at[1] < 370.0,
            "centre at {:?}",
            low.at
        );
        assert_eq!(a.strip_leds.len(), STRIP_SLOTS.len());
        assert!(
            a.strip_leds.windows(2).all(|w| w[0] < w[1]),
            "strip LEDs apart"
        );
        assert_eq!(a.plate_leds.len(), PLATE_SLOTS.len());
        for q in &a.plate_leds {
            assert!(a.plate.contains((q[0], q[1])), "plate LED at {q:?}");
        }
        // Lighting effects place the LEDs by these marks.
        let mut xs = [0.0; 12];
        for (&slot, &t) in STRIP_SLOTS.iter().zip(&a.strip_leds) {
            let q = a.strip.iter().find(|sp| sp.t >= t).unwrap().at;
            xs[slot] = q[0];
        }
        for (&slot, q) in PLATE_SLOTS.iter().zip(&a.plate_leds) {
            xs[slot] = q[0];
        }
        let (lo, hi) = (
            xs.iter().copied().fold(f32::MAX, f32::min),
            xs.iter().copied().fold(f32::MIN, f32::max),
        );
        for (slot, x) in xs.iter().enumerate() {
            let want = (x - lo) / (hi - lo);
            let have = apex5::lighting::LED_X[slot];
            assert!(
                (want - have).abs() < 0.01,
                "LED_X[{slot}] is {have}, the drawing says {want:.3}"
            );
        }
        let c = a.plate.centre();
        assert!(a.plate.contains((c[0], c[1])), "plate");
        assert!(
            c[0] < 320.0 && c[1] > 250.0,
            "plate on the left grip at {c:?}"
        );
    }
}
