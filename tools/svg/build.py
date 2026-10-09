"""Build assets/apex5.svg from Flydigi's gamepad-on-dock line drawing.

    python build.py [SOURCE] [OUT]

SOURCE defaults to gamepad-dock.svg next to this script (see README.md for
where to get it), OUT to ../../assets/apex5.svg.

The source paints every line as a filled outline, inside two groups that carry
the opacity. The output keeps that look but rebuilds the lines as real strokes
(stroke.py), drops the dock, and adds labelled key areas. Coordinates stay in
the source's 984x680 space, which src/ui/pad.rs also uses; only the viewBox is
cropped to the pad.
"""
import math
import sys
from pathlib import Path

from lxml import etree

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

# Bumpers close against the body; triggers close against body + bumper.
lb = sp.diff(sp.outer(sp.union(body, shape(224))), body)
rb = sp.diff(sp.outer(sp.union(body, shape(229))), body)
lt = sp.diff(sp.outer(sp.union(body, lb, shape(224, 225, 226))), sp.union(body, lb))
rt = sp.diff(sp.outer(sp.union(body, rb, shape(229, 230, 231))), sp.union(body, rb))

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

# --- line art -------------------------------------------------------------------

# Solid shapes among the lines: letters, logo, View/Menu icons, gauge marks.
GLYPHS = {241, 242, 243, 414, 373, 374, 375, 246, 247, 248, 249, 250,
          *range(380, 389), *range(393, 402), *range(361, 366), *range(368, 373)}
# Mirrored halves meeting on the centre line become one stroke, without a seam.
MERGE = [(223, 228), (222, 227), (404, 405), (410, 411)]
WIDTHS = {"hair": 0.31, "thin": 0.44, "mid": 0.75, "reg": 0.89, "bold": 1.78}

CSS = """
.key{fill:#9db7dc;fill-opacity:0}
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


svg = etree.Element(NS + "svg", nsmap={None: NS[1:-1]})
svg.set("viewBox", "166 40 652 432")
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
path(svg, "body", body, "body-fill")

keys = g(svg, "keys")
for id_, key, area in (("lt", 12, lt), ("rt", 13, rt), ("lb", 10, lb), ("rb", 11, rb)):
    path(g(keys, id_, key), "key body", area)
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
for id_, key, idx in (("lt", 12, (225, 226)), ("rt", 13, (230, 231)), ("lb", 10, (224,)), ("rb", 11, (229,))):
    lines(hi, *idx, id_=id_ + "-ln", key=key)
lines(hi, 223, 228, 222, 227, id_="body-outline")

ln = g(svg, "lines", cls="ln")
lines(ln, 245, id_="usb")
lines(ln, 410, 411, id_="body-seam")
lines(ln, 376, 377, 378, 379, id_="grip-l")
lines(ln, *range(380, 389), id_="xhaptics-l")
lines(ln, 389, 390, 391, 392, id_="grip-r")
lines(ln, *range(393, 402), id_="xhaptics-r")
lines(ln, 357, 358, id_="slider")
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

etree.indent(svg, space=" ")
OUT.parent.mkdir(parents=True, exist_ok=True)
OUT.write_bytes(etree.tostring(svg, encoding="utf-8") + b"\n")
errs = [st.err for st in _strokes.values()]
print(f"{OUT}: {OUT.stat().st_size} bytes, {len(errs)} strokes, "
      f"edge error mean {sum(errs) / len(errs):.3f} max {max(errs):.3f}")
