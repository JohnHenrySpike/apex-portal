"""SVG path data <-> skia-pathops paths, and boolean-op shorthands."""
import math
import re

import pathops
from pathops import ArcSize, Direction, LineCap, LineJoin, Path, PathOp, PathVerb

_TOK = re.compile(r"[MmLlHhVvCcSsQqTtAaZz]|[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?")
_ARGS = dict(M=2, L=2, H=1, V=1, C=6, S=4, Q=4, T=2, A=7, Z=0)


def parse(d: str) -> Path:
    """Parse SVG path data. Arcs become quadratics (within 0.01 units)."""
    toks = _TOK.findall(d)
    p = Path()
    i = 0
    cmd = None
    cur = start = (0.0, 0.0)
    last_c = last_q = None  # control points reflected by S / T
    while i < len(toks):
        if toks[i].isalpha():
            cmd = toks[i]
            i += 1
            if cmd in "Zz":
                p.close()
                cur = start
                last_c = last_q = None
                continue
        n = _ARGS[cmd.upper()]
        a = [float(x) for x in toks[i : i + n]]
        i += n
        ox, oy = cur if cmd.islower() else (0.0, 0.0)
        C = cmd.upper()
        if C == "M":
            cur = start = (a[0] + ox, a[1] + oy)
            p.moveTo(*cur)
            cmd = "l" if cmd.islower() else "L"
            last_c = last_q = None
        elif C in "LHV":
            if C == "L":
                cur = (a[0] + ox, a[1] + oy)
            elif C == "H":
                cur = (a[0] + ox, cur[1])
            else:
                cur = (cur[0], a[0] + oy)
            p.lineTo(*cur)
            last_c = last_q = None
        elif C in "CS":
            if C == "C":
                c1, c2, e = (a[0] + ox, a[1] + oy), (a[2] + ox, a[3] + oy), (a[4] + ox, a[5] + oy)
            else:
                c1 = (2 * cur[0] - last_c[0], 2 * cur[1] - last_c[1]) if last_c else cur
                c2, e = (a[0] + ox, a[1] + oy), (a[2] + ox, a[3] + oy)
            p.cubicTo(*c1, *c2, *e)
            last_c, last_q, cur = c2, None, e
        elif C in "QT":
            if C == "Q":
                c1, e = (a[0] + ox, a[1] + oy), (a[2] + ox, a[3] + oy)
            else:
                c1 = (2 * cur[0] - last_q[0], 2 * cur[1] - last_q[1]) if last_q else cur
                e = (a[0] + ox, a[1] + oy)
            p.quadTo(*c1, *e)
            last_q, last_c, cur = c1, None, e
        else:  # A
            e = (a[5] + ox, a[6] + oy)
            if math.dist(e, cur) > 1e-9:  # zero-length arcs draw nothing
                if a[0] == 0 or a[1] == 0:
                    p.lineTo(*e)
                else:
                    large = ArcSize.LARGE if a[3] else ArcSize.SMALL
                    sweep = Direction.CW if a[4] else Direction.CCW
                    p.arcTo(a[0], a[1], a[2], large, sweep, *e)
            cur = e
            last_c = last_q = None
    p.convertConicsToQuads(0.01)
    return p


def rect(x, y, w, h) -> Path:
    return poly([(x, y), (x + w, y), (x + w, y + h), (x, y + h)])


def poly(pts) -> Path:
    p = Path()
    p.moveTo(*pts[0])
    for q in pts[1:]:
        p.lineTo(*q)
    p.close()
    return p


def circle(cx, cy, r) -> Path:
    p = Path()
    p.moveTo(cx + r, cy)
    p.arcTo(r, r, 0, ArcSize.SMALL, Direction.CW, cx - r, cy)
    p.arcTo(r, r, 0, ArcSize.SMALL, Direction.CW, cx + r, cy)
    p.close()
    p.convertConicsToQuads(0.01)
    return p


def union(*ps) -> Path:
    out = Path()
    for q in ps:
        out = pathops.op(out, q, PathOp.UNION)
    return out


def diff(a, b) -> Path:
    return pathops.op(a, b, PathOp.DIFFERENCE)


def inter(a, b) -> Path:
    return pathops.op(a, b, PathOp.INTERSECTION)


def stroked(p: Path, width) -> Path:
    """The area a round-joined line of the given width along p covers."""
    q = Path(p)
    q.stroke(width, LineCap.ROUND_CAP, LineJoin.ROUND_JOIN, 4)
    q.convertConicsToQuads(0.01)
    q.simplify()
    return q


def outer(p: Path) -> Path:
    """The region inside the largest contour, holes dropped."""
    best = max(p.contours, key=lambda c: abs(c.area))
    out = Path()
    out.addPath(best)
    out.simplify()
    return out


def num(v: float, prec: int = 2) -> str:
    """Shortest form of v at the given precision: 0.5 -> .5, -0.5 -> -.5."""
    s = f"{v:.{prec}f}".rstrip("0").rstrip(".")
    if s in ("-0", ""):
        return "0"
    return s.replace("0.", ".", 1) if s.startswith(("0.", "-0.")) else s


def _join(nums):
    """Numbers with only the separators SVG needs."""
    out = ""
    prev = ""
    for t in nums:
        if out and not t.startswith("-") and not (t.startswith(".") and "." in prev):
            out += " "
        out += t
        prev = t
    return out


def to_d(p: Path, prec: int = 2) -> str:
    """Compact relative path data."""
    p = Path(p)
    p.convertConicsToQuads(0.05)
    cmds = []
    cur = start = (0.0, 0.0)
    eps = 10**-prec / 2
    for verb, pts in p:
        if verb == PathVerb.MOVE:
            x, y = pts[0]
            cmds.append(("m", [x - cur[0], y - cur[1]]) if cmds else ("M", [x, y]))
            cur = start = (x, y)
        elif verb == PathVerb.LINE:
            x, y = pts[0]
            dx, dy = x - cur[0], y - cur[1]
            if abs(dy) < eps:
                cmds.append(("h", [dx]))
            elif abs(dx) < eps:
                cmds.append(("v", [dy]))
            else:
                cmds.append(("l", [dx, dy]))
            cur = (x, y)
        elif verb in (PathVerb.CUBIC, PathVerb.QUAD):
            c = "c" if verb == PathVerb.CUBIC else "q"
            cmds.append((c, [v for q in pts for v in (q[0] - cur[0], q[1] - cur[1])]))
            cur = pts[-1]
        elif verb == PathVerb.CLOSE:
            cmds.append(("z", []))
            cur = start
    # Repeated commands are written once; their numbers just continue.
    out = ""
    prev = None
    run = []
    for c, nums in cmds + [(None, [])]:
        if c == prev and c not in "Mmz":
            run += nums
            continue
        if prev is not None:
            out += prev + _join([num(v, prec) for v in run])
        prev, run = c, list(nums)
    return out
