"""Build assets/apex5.svg from Flydigi's gamepad-on-dock line drawing.

    python build.py [SOURCE] [OUT]

SOURCE defaults to gamepad-dock.svg next to this script (see README.md for
where to get it), OUT to ../../assets/apex5.svg.

The source paints every line as a filled outline, inside two groups that carry
the opacity. The output keeps that look but rebuilds the lines as real strokes
(stroke.py), drops the dock, and adds labelled key areas, the shoulder and back
buttons as separate tiles around the pad, and the lighting zones. Coordinates
stay in the source's 984x680 space, which src/ui/pad.rs also uses; only the
viewBox is cropped.
"""
import math
import sys
from pathlib import Path

import numpy as np
from lxml import etree
from scipy.spatial import cKDTree

import stroke
import svgpath as sp

HERE = Path(__file__).resolve().parent
SRC = Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "gamepad-dock.svg"
OUT = Path(sys.argv[2]) if len(sys.argv) > 2 else HERE.parent.parent / "assets" / "apex5.svg"
NS = "{http://www.w3.org/2000/svg}"

if not SRC.exists():
    sys.exit(f"{SRC} not found; see README.md for the download command")

# Source elements in document order. The numbers below index this list; the
# dock (2-221) and the blue "pressed B" overlay are simply never used.
els = [e for e in etree.parse(str(SRC)).getroot().iter() if e.tag.replace(NS, "") in ("path", "rect", "polygon")]
if len(els) != 415:
    sys.exit(f"{SRC}: expected 415 shapes, found {len(els)}; the drawing has changed")


def d_of(i):
    """Path data of source element i (rects and polygons written as paths)."""
    e = els[i]
    tag = e.tag.replace(NS, "")
    if tag == "path":
        return e.get("d")
    if tag == "rect":
        x, y = float(e.get("x", 0)), float(e.get("y", 0))
        w, h = float(e.get("width")), float(e.get("height"))
        return f"M{x:g},{y:g}h{w:g}v{h:g}h{-w:g}Z"
    n = [float(v) for v in e.get("points").replace(",", " ").split()]
    pts = list(zip(n[0::2], n[1::2]))
    if pts[0] == pts[-1]:
        pts = pts[:-1]
    return "M" + "L".join(f"{x:g},{y:g}" for x, y in pts) + "Z"


def shape(*idx):
    return sp.union(*(sp.parse(d_of(i)) for i in idx))


def disc(i):
    """The area inside an outlined ring."""
    return sp.outer(sp.parse(d_of(i)))


# --- key areas ------------------------------------------------------------------

# Body: the two outline halves plus the raised lip at the top centre.
body = sp.outer(sp.union(sp.outer(shape(223, 228)), shape(222, 227)))
bx0, by0, bx1, by1 = body.bounds
MID = (bx0 + bx1) / 2

# Bumpers close against the body; triggers close against body + bumper.
lb = sp.diff(sp.outer(sp.union(body, shape(224))), body)
rb = sp.diff(sp.outer(sp.union(body, shape(229))), body)
lt = sp.diff(sp.outer(sp.union(body, lb, shape(224, 225, 226))), sp.union(body, lb))
rt = sp.diff(sp.outer(sp.union(body, rb, shape(229, 230, 231))), sp.union(body, rb))
pad = sp.union(body, lb, rb, lt, rt)

# D-pad: a cross bounded by the four corner L-lines (406-409) and the inner
# rim of the base (403), split into four arms along the diagonals.
well = sp.outer(min(sp.parse(d_of(403)).contours, key=lambda c: abs(c.area)))
x0, x1 = 393.4, 424.23  # vertical arm edges (inner corners of 408 / 407)
y0, y1 = 258.9, 289.44  # horizontal arm edges
cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
cross = sp.union(sp.rect(x0, cy - 120, x1 - x0, 240), sp.rect(cx - 120, y0, 240, y1 - y0))
r = 6.73  # the corner lines' fillet radius
for sx in (-1, 1):
    for sy in (-1, 1):
        ax, ay = (x1 if sx > 0 else x0), (y1 if sy > 0 else y0)
        square = sp.rect(min(ax, ax + sx * r), min(ay, ay + sy * r), r, r)
        cross = sp.union(cross, sp.diff(square, sp.circle(ax + sx * r, ay + sy * r, r)))
cross = sp.inter(cross, well)
arms = {}
for name, ang in (("up", -90), ("right", 0), ("down", 90), ("left", 180)):
    a = math.radians(ang)
    wedge = [(cx, cy)] + [(cx + 200 * math.cos(a + d), cy + 200 * math.sin(a + d)) for d in (-math.pi / 4, math.pi / 4)]
    arms[name] = sp.inter(cross, sp.poly(wedge))

# --- shoulder and back buttons ------------------------------------------------

# A front view cannot show these, so they are tiles around the pad, shaped and
# placed as on Flydigi Space Station's LED page (a 508x421 drawing whose body
# spans REF_BODY). Tiles keep their place relative to the body, mapped from
# its bounding box onto ours, and are scaled evenly about their centres. The
# right-hand tiles mirror the left ones.
REF_BODY = (34.0, 74.0, 473.4, 382.9)
TILES = [  # id, key, label, offset and label position in the reference, outline
    ("lt", 12, "LT", (4.5, 1.5), (26, 26.5),
     "M41.7938 40.221C29.9728 42.6567 0.933177 49.208 0.933177 49.208C0.933177 49.208 -1.47027 4.94522 16.6617 1.7115"
     "C34.7938 -1.52222 40.1071 22.2891 41.7938 40.221Z"),
    ("lb", 10, "LB", (7, 50), (59.78, 64.5),
     "M76.2081 29.1047V3.74751H46.3177C44.6828 3.74751 42.9385 2.44513 42.2707 1.79395C31.038 4.06008 3.73192 10.7926"
     " 1.10339 11.7303C7.01652 24.028 18.3094 29.1047 33.0548 29.1047H76.2081Z"),
    ("lm", 22, "LM", (53, 11.5), (82, 26.5),
     "M5.01499 1H45.9549C47.6683 1 49.1914 2.09128 49.7424 3.71367L56.7046 24.2137C57.5857 26.8079 55.6569 29.5 52.9171"
     " 29.5L9.69043 29.5C7.82397 29.5 6.2056 28.2092 5.79057 26.3894L1.11514 5.88944C0.544086 3.38561 2.44686 1 5.01499 1Z"),
    ("m2", 19, "M2", (153, 306.5), (187, 329.5),
     "M4.4952 24.4405L35.5354 2.43964C37.073 1.34978 39.1628 1.48114 40.5519 2.75497L65.5046 25.6374C67.1129 27.1123"
     " 67.2423 29.6042 65.7953 31.2377L56.37 41.8783C55.6682 42.6706 54.6811 43.1524 53.6247 43.2183L5.67508 46.2087"
     "C3.22089 46.3617 1.21098 44.2851 1.44412 41.8372L2.82627 27.3246C2.93659 26.1663 3.54586 25.1134 4.4952 24.4405Z"),
    ("m4", 21, "M4", (137, 367), (206.64, 381.5),
     "M89.1803 4.31986L29.0724 1.05819C28.3699 1.02007 27.6698 1.1679 27.0427 1.48677L3.35157 13.5331C0.945596 14.7565"
     " 0.417053 17.9627 2.30305 19.8936L17.3735 35.3229C18.63 36.6093 20.5905 36.8991 22.1654 36.0313L42.9708 24.5671"
     "C43.6427 24.1969 44.407 24.0277 45.1724 24.0797L90.6312 27.1691C93.1498 27.3403 95.1953 25.1642 94.8688 22.661"
     "L92.93 7.79663C92.6808 5.88579 91.1045 4.42428 89.1803 4.31986Z"),
]
MIRRORED = {"lt": ("rt", 13, "RT"), "lb": ("rb", 11, "RB"), "lm": ("rm", 23, "RM"),
            "m2": ("m1", 18, "M1"), "m4": ("m3", 20, "M3")}
sx = (bx1 - bx0) / (REF_BODY[2] - REF_BODY[0])
sy = (by1 - by0) / (REF_BODY[3] - REF_BODY[1])
s_tile = math.sqrt(sx * sy)
GAP = 6  # between the pad and the back-button tiles, which may otherwise touch it
clear = sp.union(pad, sp.stroked(pad, 2 * GAP))
placed = []
for id_, key, label, (ox, oy), (lx, ly), d in TILES:
    p = sp.parse(d).transform(1, 0, 0, 1, ox, oy)
    x0, y0, x1, y1 = p.bounds
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    px, py = bx0 + (cx - REF_BODY[0]) * sx, by0 + (cy - REF_BODY[1]) * sy
    p = p.transform(s_tile, 0, 0, s_tile, px - s_tile * cx, py - s_tile * cy)
    placed.append((id_, key, label, p, (px + s_tile * (lx - cx), py + s_tile * (ly - cy))))
# The back buttons move down together, as far as the nearest one needs.
drop = 0.0
while any(sp.inter(t[3].transform(1, 0, 0, 1, 0, drop), clear).area > 0 for t in placed if t[0][0] == "m"):
    drop += 0.5
tiles = []  # (id, key, label, outline, label position)
for id_, key, label, p, (lx, ly) in placed:
    dy = drop if id_[0] == "m" else 0.0
    p, at = p.transform(1, 0, 0, 1, 0, dy), (lx, ly + dy)
    assert sp.inter(p, clear).area == 0, f"{id_} overlaps the pad"
    tiles.append((id_, key, label, p, at))
    rid, rkey, rlabel = MIRRORED[id_]
    tiles.append((rid, rkey, rlabel, p.transform(-1, 0, 0, 1, 2 * MID, 0), (2 * MID - at[0], at[1])))

# --- line art -------------------------------------------------------------------

# Solid shapes among the lines: letters, logo, View/Menu icons, gauge marks.
GLYPHS = {241, 242, 243, 414, 373, 374, 375, 246, 247, 248, 249, 250,
          *range(380, 389), *range(393, 402), *range(361, 366), *range(368, 373)}
# Mirrored halves meeting on the centre line become one stroke, without a seam.
MERGE = [(223, 228), (222, 227), (404, 405), (410, 411)]
WIDTHS = {"hair": 0.31, "thin": 0.44, "mid": 0.75, "reg": 0.89, "bold": 1.78}

CSS = """
.key,.zone{fill:#9db7dc;fill-opacity:0}
.tile{fill:#1a2636;fill-opacity:.48;stroke:#9db7dc;stroke-opacity:.58;stroke-width:1.2;stroke-linejoin:round}
.lbl{fill:#9db7dc;fill-opacity:.75;font:500 17px sans-serif;text-anchor:middle;dominant-baseline:central}
.body{fill:#1a2636;fill-opacity:1;opacity:.48}
.ln{opacity:.42;pointer-events:none}
.ln-hi{opacity:.58;pointer-events:none}
.s{fill:none;stroke:#9db7dc;stroke-linecap:round;stroke-linejoin:round}
.gl{fill:#9db7dc}
.scr{fill:#9db7dc;opacity:.5}
""" + "".join(f".{k}{{stroke-width:{v:g}}}\n" for k, v in WIDTHS.items())

_strokes = {}


def recovered(key):
    if key not in _strokes:
        _strokes[key] = stroke.recover(shape(*key) if isinstance(key, tuple) else sp.parse(d_of(key)))
    return _strokes[key]


def width_class(w):
    name, v = min(WIDTHS.items(), key=lambda kv: abs(kv[1] - w))
    assert abs(v - w) / v < 0.12, f"stroke width {w:.3f} matches no class"
    return name


# --- lighting ---------------------------------------------------------------------

# The light strip runs along the bottom edge, in the band between the seam
# (410/411) and the body outline, and fades out on the grips below the
# X-HAPTICS pads (as on Flydigi's product photo). Its line is the band's middle.
STRIP_X = (285.0, 2 * MID - 285.0)
seam = stroke.sample([list(c) for c in recovered((410, 411)).segs], 0.5)
edge = stroke.sample([list(c) for c in recovered((223, 228)).segs], 0.3)
mid = (seam + edge[cKDTree(edge).query(seam)[1]]) / 2
mid = mid[(mid[:, 0] >= STRIP_X[0]) & (mid[:, 0] <= STRIP_X[1])]
mid = stroke.resample(mid[np.argsort(mid[:, 0])], 2.0, False)
strip = stroke.Stroke(stroke.fit_open(mid, 0.05, 6), False, WIDTHS["hair"])
# The replaceable FLYDIGI plate on the left grip (357) lights up as well.
plate = recovered(357)
assert plate.closed

# The LEDs, as they sit on the main board in a teardown photo (1280x720): ten
# side-firing ones along its bottom edge feed the strip's light guide, and two
# sit under the plate, where they are drawn evenly along its middle line
# rather than where the photo has them. Eight of the strip's show in the photo; one hides under
# the right stick's ball, and its twin on the left does not show at all. The
# photo is mapped onto the drawing by the stick centres, which both show.
# Strip LEDs go onto the strip line in mirrored pairs; the photo's error is a
# few units. A probe on the pad (apex5ctl led probe) confirmed ten on the strip.
BOARD_STICKS = ((528.3, 346.7), (695.0, 416.7))
BOARD_STRIP = [(525.8, 460.5), (565.0, 445.0), (614.0, 444.0), (634.5, 437.5),
               (648.8, 437.5), (669.5, 442.5), (719.0, 442.5), (758.0, 455.0)]
BOARD_HIDDEN = (695.0, 443.8)


def stick_centre(ring):
    x0, y0, x1, y1 = disc(ring).bounds
    return complex((x0 + x1) / 2, (y0 + y1) / 2)


(b0, b1), (d0, d1) = (complex(*q) for q in BOARD_STICKS), (stick_centre(252), stick_centre(304))
k = (d1 - d0) / (b1 - b0)  # similarity: drawing = d0 + k * (board - b0)
from_board = lambda q: d0 + k * (complex(*q) - b0)  # noqa: E731
xs = [from_board(q).real - MID for q in BOARD_STRIP]
offsets = sorted([(r - l) / 2 for l, r in zip(xs[:4], reversed(xs[4:]))] + [from_board(BOARD_HIDDEN).real - MID])
strip_leds = [(MID + o, float(np.interp(MID + o, mid[:, 0], mid[:, 1])))
              for o in [-o for o in reversed(offsets)] + offsets]
outline = stroke.sample([list(c) for c in plate.segs], 0.5)
centre = outline.mean(axis=0)
axis = np.linalg.svd(outline - centre)[2][0]
axis = axis if axis[0] > 0 else -axis
along = (outline - centre) @ axis
centre = centre + axis * (along.max() + along.min()) / 2
half = (along.max() - along.min()) / 2
plate_leds = [tuple(centre + axis * half * f) for f in (-0.5, 0.5)]  # left, right
assert all(sp.inter(plate.path(), sp.circle(x, y, 1)).area > 0 for x, y in plate_leds), "plate LEDs miss the plate"


# The pad on its own (src/ui/pad.rs shows just this where the tiles are not
# needed), and the whole drawing, kept symmetric about the pad.
PAD_VIEW = (166, 40, 652, 432)
vx0, vy0, vx1, vy1 = PAD_VIEW[0], PAD_VIEW[1], PAD_VIEW[0] + PAD_VIEW[2], PAD_VIEW[1] + PAD_VIEW[3]
for t in tiles:
    x0, y0, x1, y1 = t[3].bounds
    vx0, vy0, vx1, vy1 = min(vx0, x0 - 4), min(vy0, y0 - 4), max(vx1, x1 + 4), max(vy1, y1 + 4)
vx0 = math.floor(min(vx0, 2 * MID - vx1))
vy0, vy1 = math.floor(vy0), math.ceil(vy1)
view = (vx0, vy0, math.ceil(2 * (MID - vx0)), vy1 - vy0)

svg = etree.Element(NS + "svg", nsmap={None: NS[1:-1]})
svg.set("viewBox", " ".join(map(str, view)))
svg.set("data-pad-view", " ".join(map(str, PAD_VIEW)))
svg.set("id", "apex5")
etree.SubElement(svg, NS + "title").text = "Flydigi Apex 5"
etree.SubElement(svg, NS + "style").text = CSS


def g(parent, id_, key=None, cls=None):
    e = etree.SubElement(parent, NS + "g", id=id_)
    if cls:
        e.set("class", cls)
    if key is not None:
        e.set("data-key", str(key))
    return e


def path(parent, cls, d, id_=None):
    e = etree.SubElement(parent, NS + "path", {"class": cls, "d": d if isinstance(d, str) else sp.to_d(d)})
    if id_:
        e.set("id", id_)
    return e


def circle_key(parent, ring_idx):
    """Key area of a round button: a <circle> filling its outer ring."""
    bx0, by0, bx1, by1 = disc(ring_idx).bounds
    attrs = {"cx": (bx0 + bx1) / 2, "cy": (by0 + by1) / 2, "r": (bx1 - bx0 + by1 - by0) / 4}
    etree.SubElement(parent, NS + "circle", {"class": "key", **{k: sp.num(v) for k, v in attrs.items()}})


def lines(parent, *idx, id_, key=None):
    """Line art of source elements idx: one path per stroke width plus one for
    glyphs, wrapped in a <g> carrying id and key when there is more than one."""
    todo = []
    for i in idx:
        pair = next((m for m in MERGE if i in m and all(j in idx for j in m)), None)
        k = pair or i
        if i not in GLYPHS and k not in todo:
            todo.append(k)
    buckets = {}
    for k in todo:
        st = recovered(k)
        buckets.setdefault((width_class(st.width), st.cap, st.join), []).append(st.d())
    glyphs = [i for i in idx if i in GLYPHS]
    host = parent
    if len(buckets) + bool(glyphs) > 1:
        host = g(parent, id_, key)
        id_ = key = None
    out = []
    for (wc, cap, join), ds in sorted(buckets.items(), key=lambda kv: -WIDTHS[kv[0][0]]):
        e = path(host, f"s {wc}", "".join(ds), id_)
        if cap != "round":
            e.set("stroke-linecap", cap)
        if join != "round":
            e.set("stroke-linejoin", join)
        out.append(e)
    if glyphs:
        out.append(path(host, "gl", "".join(d_of(i) for i in glyphs), id_))
    if key is not None:
        out[0].set("data-key", str(key))


# Layers as in the source: body fill, key areas, then the line art in two
# groups that carry the opacity, so crossing lines never double up.
path(svg, "body", pad, "body-fill")

keys = g(svg, "keys")
etree.SubElement(g(keys, "home", 27), NS + "circle", {"class": "key", "cx": "492.9", "cy": "103.5", "r": "22"})
path(g(keys, "view", 6), "key", disc(238))
path(g(keys, "menu", 9), "key", disc(239))
circle_key(g(keys, "ls", 14), 252)
circle_key(g(keys, "rs", 15), 304)
for name, key in (("up", 0), ("right", 1), ("down", 2), ("left", 3)):
    path(g(keys, "dpad-" + name, key), "key", arms[name])
for id_, key, ring in (("y", 8, 232), ("x", 7, 236), ("b", 5, 412), ("a", 4, 234)):
    circle_key(g(keys, id_, key), ring)

hi = g(svg, "lines-hi", cls="ln-hi")
for id_, idx in (("lt", (225, 226)), ("rt", (230, 231)), ("lb", (224,)), ("rb", (229,))):
    lines(hi, *idx, id_=id_ + "-ln")
lines(hi, 223, 228, 222, 227, id_="body-outline")

ln = g(svg, "lines", cls="ln")
lines(ln, 245, id_="usb")
lines(ln, 410, 411, id_="body-seam")
lines(ln, 376, 377, 378, 379, id_="grip-l")
lines(ln, *range(380, 389), id_="xhaptics-l")
lines(ln, 389, 390, 391, 392, id_="grip-r")
lines(ln, *range(393, 402), id_="xhaptics-r")
lines(ln, 357, 358, id_="led-plate")
path(ln, "s hair", strip.d(), "led-strip")
lines(ln, 355, 356, id_="wheel")
lines(ln, 404, 405, id_="plate-outline")
screen = g(ln, "screen")
path(screen, "scr", d_of(240), "screen-face")
path(screen, "gl", d_of(244), "screen-glare")
lines(ln, 373, 374, 375, id_="logo", key=27)
lines(ln, 238, 249, 250, id_="view-ln", key=6)
lines(ln, 239, 246, 247, 248, id_="menu-ln", key=9)
for id_, key, base, cap, ticks, dial in (
    ("ls", 14, 251, 252, range(253, 303), range(359, 366)),
    ("rs", 15, 303, 304, range(305, 355), range(366, 373)),
):
    lines(ln, base, id_=id_ + "-well")
    lines(ln, cap, *ticks, id_=id_ + "-cap", key=key)
    lines(ln, *dial, id_=id_ + "-dial")
lines(ln, 402, 403, id_="dpad-base")
lines(ln, 406, 407, 408, 409, id_="dpad-cross")
for id_, key, idx in (("y", 8, (232, 233, 241)), ("x", 7, (236, 237, 243)),
                      ("b", 5, (412, 413, 414)), ("a", 4, (234, 235, 242))):
    lines(ln, *idx, id_=id_ + "-ln", key=key)

# Lighting zones, for the app to paint the LED colours into.
leds = g(svg, "leds")
path(leds, "zone", plate.path(), "led-plate-area")
for where, pts in (("strip", strip_leds), ("plate", plate_leds)):
    for x, y in pts:
        etree.SubElement(leds, NS + "circle", {"class": "zone", "cx": sp.num(x), "cy": sp.num(y), "r": "2", "data-led": where})

# Shoulder and back buttons. Text is not rendered by the app, which draws the
# labels itself, so they can light up with the key.
tl = g(svg, "tiles")
for id_, key, label, outline, (lx, ly) in sorted(tiles, key=lambda t: t[1]):
    path(tl, "tile", outline, id_).set("data-key", str(key))
for id_, key, label, outline, (lx, ly) in sorted(tiles, key=lambda t: t[1]):
    etree.SubElement(tl, NS + "text", {"class": "lbl", "x": sp.num(lx), "y": sp.num(ly), "data-key": str(key)}).text = label

etree.indent(svg, space=" ")
OUT.parent.mkdir(parents=True, exist_ok=True)
OUT.write_bytes(etree.tostring(svg, encoding="utf-8") + b"\n")
errs = [st.err for st in _strokes.values()]
print(f"{OUT}: {OUT.stat().st_size} bytes, {len(errs)} strokes, "
      f"edge error mean {sum(errs) / len(errs):.3f} max {max(errs):.3f}")
