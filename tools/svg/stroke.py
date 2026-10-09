"""Recover centre-line strokes from outlined strokes.

The source drawing paints every line as a filled outline. An outline is
either a *band* (one contour around an open line) or a *ring* (outer and inner
contour around a closed line). The centre line is found by pairing each point
on one side with the nearest point on the other side, then fitted with cubic
Béziers (Schneider's algorithm, Graphics Gems I). Each candidate is scored by
how far its stroked edge lies from the source outline, and the best one wins.
"""
import math
from dataclasses import dataclass

import numpy as np
from pathops import LineCap, LineJoin, Path, PathVerb
from scipy.spatial import cKDTree

import svgpath as sp

# --- sampling -----------------------------------------------------------------


def _bez_pts(ctrl, ts):
    pts = [np.repeat(np.asarray(c, float)[None], len(ts), 0) for c in ctrl]
    t = np.asarray(ts)[:, None]
    while len(pts) > 1:
        pts = [(1 - t) * pts[i] + t * pts[i + 1] for i in range(len(pts) - 1)]
    return pts[0]


def contours(path):
    """Contours as lists of segments; a segment is its control points incl. start."""
    out = []
    segs = None
    cur = start = None
    for verb, p in path:
        if verb == PathVerb.MOVE:
            if segs:
                out.append(segs)
            segs = []
            cur = start = p[0]
        elif verb == PathVerb.CLOSE:
            if math.dist(cur, start) > 1e-9:
                segs.append([cur, start])
            out.append(segs)
            segs = None
            cur = start
        else:
            segs.append([cur, *p])
            cur = p[-1]
    if segs:
        out.append(segs)
    return out


def sample(segs, step):
    """Points along a contour, about `step` apart."""
    pts = [np.asarray(segs[0][0], float)[None]]
    for c in segs:
        length = sum(math.dist(c[k], c[k + 1]) for k in range(len(c) - 1))
        n = max(1, math.ceil(length / step))
        pts.append(_bez_pts(c, np.linspace(0, 1, n + 1)[1:]))
    a = np.concatenate(pts)
    a = a[np.r_[True, np.linalg.norm(np.diff(a, axis=0), axis=1) > 1e-9]]
    if np.linalg.norm(a[0] - a[-1]) < 1e-6:
        a = a[:-1]
    return a


def resample(poly, step, closed):
    """The polyline re-sampled at equal arc-length steps."""
    p = np.vstack([poly, poly[:1]]) if closed else np.asarray(poly, float)
    d = np.r_[0, np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))]
    s = np.linspace(0, d[-1], max(2, round(d[-1] / step)) + 1)
    if closed:
        s = s[:-1]
    return np.c_[np.interp(s, d, p[:, 0]), np.interp(s, d, p[:, 1])]


def area(p):
    x, y = p[:, 0], p[:, 1]
    return 0.5 * np.sum(x * np.roll(y, -1) - np.roll(x, -1) * y)


def perimeter(p):
    return np.sum(np.linalg.norm(np.diff(np.vstack([p, p[:1]]), axis=0), axis=1))


# --- Bézier fitting -------------------------------------------------------------


def _unit(v):
    n = np.linalg.norm(v)
    return v / n if n > 1e-12 else v


def _bez(c, t):
    t = np.asarray(t)[:, None]
    mt = 1 - t
    return mt**3 * c[0] + 3 * mt**2 * t * c[1] + 3 * mt * t**2 * c[2] + t**3 * c[3]


def _bez1(c, t):
    t = np.asarray(t)[:, None]
    mt = 1 - t
    return 3 * mt**2 * (c[1] - c[0]) + 6 * mt * t * (c[2] - c[1]) + 3 * t**2 * (c[3] - c[2])


def _bez2(c, t):
    t = np.asarray(t)[:, None]
    return 6 * (1 - t) * (c[2] - 2 * c[1] + c[0]) + 6 * t * (c[3] - 2 * c[2] + c[1])


def _generate(p, u, t1, t2):
    """Least-squares cubic through p at parameters u with fixed end tangents."""
    a1 = np.outer(3 * u * (1 - u) ** 2, t1)
    a2 = np.outer(3 * u**2 * (1 - u), t2)
    c00, c01, c11 = np.sum(a1 * a1), np.sum(a1 * a2), np.sum(a2 * a2)
    b0 = np.outer((1 - u) ** 3 + 3 * u * (1 - u) ** 2, p[0]) + np.outer(3 * u**2 * (1 - u) + u**3, p[-1])
    x0, x1 = np.sum(a1 * (p - b0)), np.sum(a2 * (p - b0))
    det = c00 * c11 - c01**2
    seg = np.linalg.norm(p[-1] - p[0])
    al = ar = seg / 3
    if abs(det) > 1e-12:
        al, ar = (x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det
        if al < 1e-6 * seg or ar < 1e-6 * seg:
            al = ar = seg / 3
    return np.array([p[0], p[0] + t1 * al, p[-1] + t2 * ar, p[-1]])


def _reparam(c, p, u):
    """One Newton step moving each u to the closest point on the curve."""
    d = _bez(c, u) - p
    d1, d2 = _bez1(c, u), _bez2(c, u)
    num = np.sum(d * d1, axis=1)
    den = np.sum(d1 * d1, axis=1) + np.sum(d * d2, axis=1)
    with np.errstate(divide="ignore", invalid="ignore"):
        nu = np.where(np.abs(den) > 1e-12, u - num / den, u)
    return np.clip(nu, 0, 1)


def _walk(p, i, reach, closed, step):
    """Unit direction from p[i] to the point about `reach` along the polyline."""
    n = len(p)
    j = i
    acc = 0.0
    for _ in range(n):
        k = j + step
        if closed:
            k %= n
        elif not 0 <= k < n:
            break
        acc += np.linalg.norm(p[k] - p[j])
        j = k
        if acc >= reach:
            break
    return _unit(p[j] - p[i]) if j != i else None


def _fit(p, t1, t2, err, reach, depth=0):
    if len(p) == 2 or depth > 40:
        d = np.linalg.norm(p[-1] - p[0]) / 3
        return [np.array([p[0], p[0] + t1 * d, p[-1] + t2 * d, p[-1]])]
    u = np.r_[0, np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))]
    u /= u[-1]
    c = _generate(p, u, t1, t2)
    dist = np.linalg.norm(_bez(c, u) - p, axis=1)
    if dist.max() < err:
        return [c]
    if dist.max() < err * 6:
        for _ in range(30):
            u = _reparam(c, p, u)
            c = _generate(p, u, t1, t2)
            dist = np.linalg.norm(_bez(c, u) - p, axis=1)
            if dist.max() < err:
                return [c]
    split = min(max(int(np.argmax(dist)), 1), len(p) - 2)
    tc = _unit(_walk(p, split, reach, False, -1) - _walk(p, split, reach, False, 1))
    return _fit(p[: split + 1], t1, tc, err, reach, depth + 1) + _fit(p[split:], -tc, t2, err, reach, depth + 1)


def fit_open(p, err, reach):
    """Cubics (or a line, as a 2-point segment) through an open polyline."""
    chord = p[-1] - p[0]
    length = np.linalg.norm(chord)
    if length > 1e-9:
        normal = np.array([-chord[1], chord[0]]) / length
        if np.max(np.abs((p - p[0]) @ normal)) < err * 0.5:
            return [np.array([p[0], p[-1]])]
    return _fit(p, _walk(p, 0, reach, False, 1), _walk(p, len(p) - 1, reach, False, -1), err, reach)


def corners(p, closed, reach, angle=32):
    """Indices where the polyline turns by more than `angle` degrees."""
    n = len(p)
    straight = math.cos(math.radians(angle))
    rng = range(n) if closed else range(1, n - 1)
    turn = np.ones(n)
    for i in rng:
        a, b = _walk(p, i, reach, closed, -1), _walk(p, i, reach, closed, 1)
        if a is not None and b is not None:
            turn[i] = -np.dot(a, b)  # 1 = straight on, -1 = reversal
    out = []
    for i in rng:
        if turn[i] < straight:
            near = turn[[(i + k) % n if closed else min(max(i + k, 0), n - 1) for k in range(-6, 7)]]
            if turn[i] <= near.min() and (not out or i - out[-1] > 6):
                out.append(i)
    return out


def fit_poly(p, closed, err, reach):
    """Cubics through a polyline, split at its corners."""
    cs = corners(p, closed, reach)
    if not closed:
        bounds = [0] + cs + [len(p) - 1]
        return [s for a, b in zip(bounds, bounds[1:]) for s in fit_open(p[a : b + 1], err, reach)]
    smooth = not cs
    if smooth:  # no corners: split anywhere, keeping tangents continuous
        n = len(p)
        cs = [0, n // 4, n // 2, 3 * n // 4]
    out = []
    for a, b in zip(cs, cs[1:] + [cs[0] + len(p)]):
        q = p[np.arange(a, b + 1) % len(p)]
        if smooth:
            ia, ib = a % len(p), b % len(p)
            ta = _walk(p, ia, reach, True, 1) - _walk(p, ia, reach, True, -1)
            tb = _walk(p, ib, reach, True, -1) - _walk(p, ib, reach, True, 1)
            out += _fit(q, _unit(ta), _unit(tb), err, reach)
        else:
            out += fit_open(q, err, reach)
    return out


# --- strokes --------------------------------------------------------------------


@dataclass
class Stroke:
    segs: list  # 2x2 arrays (lines) and 4x2 arrays (cubics)
    closed: bool
    width: float
    cap: str = "round"
    join: str = "round"
    circle: tuple = None  # (cx, cy, r) when the line is a circle
    err: float = 0.0  # mean distance between stroked edge and source outline

    def path(self) -> Path:
        if self.circle:
            return sp.circle(*self.circle)
        q = Path()
        q.moveTo(*self.segs[0][0])
        for s in self.segs:
            if len(s) == 2:
                q.lineTo(*s[1])
            else:
                q.cubicTo(*s[1], *s[2], *s[3])
        if self.closed:
            q.close()
        return q

    def d(self, prec=2) -> str:
        if self.circle:
            cx, cy, r = (sp.num(v, prec) for v in self.circle)
            dr = sp.num(2 * self.circle[2], prec)
            return f"M{sp.num(self.circle[0] - self.circle[2], prec)} {cy}a{r} {r} 0 1 0 {dr} 0a{r} {r} 0 1 0 -{dr} 0z"
        return sp.to_d(self.path(), prec)

    def outline(self) -> Path:
        cap = {"round": LineCap.ROUND_CAP, "butt": LineCap.BUTT_CAP}[self.cap]
        join = {"round": LineJoin.ROUND_JOIN, "miter": LineJoin.MITER_JOIN}[self.join]
        q = self.path()
        q.stroke(self.width, cap, join, 10)
        q.convertConicsToQuads(0.005)
        q.simplify()
        return q


def _step(w):
    return float(np.clip(w / 6, 0.015, 0.25))


def _mids(a, b):
    """Midpoints between each point of a and its nearest point on b, and the gaps."""
    d, j = cKDTree(b).query(a)
    return (a + b[j]) / 2, d


def _smooth(m, closed, step):
    """Drop near-duplicates, then a light blur against nearest-point jitter."""
    out = [m[0]]
    for q in m[1:]:
        if np.linalg.norm(q - out[-1]) >= step * 0.5:
            out.append(q)
    out = np.array(out)
    if len(out) <= 5:
        return out
    k = np.array([1, 2, 3, 2, 1], float) / 9
    blur = lambda a: np.c_[np.convolve(a[:, 0], k, "valid"), np.convolve(a[:, 1], k, "valid")]
    if closed:
        return blur(np.vstack([out[-2:], out, out[:2]]))
    out[2:-2] = blur(out)
    return out


def _fit_circle(m):
    """Algebraic least-squares circle: centre, radius, worst deviation."""
    sol, *_ = np.linalg.lstsq(np.c_[2 * m, np.ones(len(m))], (m**2).sum(axis=1), rcond=None)
    c = sol[:2]
    r = math.sqrt(sol[2] + c @ c)
    return c, r, np.abs(np.linalg.norm(m - c, axis=1) - r).max()


def ring(path: Path, err) -> list:
    outer_c, inner_c = sorted((sample(c, 0.01) for c in contours(path)), key=lambda c: -abs(area(c)))
    st = _step((abs(area(outer_c)) - abs(area(inner_c))) / perimeter(outer_c))
    m, gaps = _mids(resample(outer_c, st, True), resample(inner_c, st, True))
    w = float(np.median(gaps))
    m = _smooth(m, True, st)
    c, r, dev = _fit_circle(m)
    if dev < max(0.01, 0.002 * r):
        return [Stroke([], True, w, circle=(float(c[0]), float(c[1]), float(r)))]
    if area(m) < 0:
        m = m[::-1]
    segs = fit_poly(m, True, err, max(w, 0.6))
    return [Stroke(segs, True, w), Stroke(segs, True, w, join="miter")]


def band(path: Path, err) -> list:
    (c,) = [sample(c, 0.01) for c in contours(path)]
    w0 = 2 * abs(area(c)) / perimeter(c)
    st = _step(w0)
    pts = resample(c, st, True)
    n = len(pts)
    s = np.arange(n) * st
    total = n * st

    # The caps are where a point's partner across the band is closest along
    # the outline. Partners are the nearest points more than 2.5 widths away
    # along the outline (so not on the same side).
    dd, jj = cKDTree(pts).query(pts, k=min(n, int(6 * w0 / st) + 40))
    gap = np.full(n, total)
    for i in range(n):
        along = np.abs(s[i] - s[jj[i]])
        along = np.minimum(along, total - along)
        hit = np.nonzero(along > 2.5 * w0)[0]
        if len(hit):
            gap[i] = along[hit[0]]
    c1 = int(np.argmin(gap))
    far = np.abs(s - s[c1])
    far = np.minimum(far, total - far)
    c2 = int(np.argmin(np.where(far > total / 4, gap, total)))
    c1, c2 = min(c1, c2), max(c1, c2)
    side_a = pts[c1 : c2 + 1]
    side_b = np.vstack([pts[c2:], pts[: c1 + 1]])[::-1]

    trim = max(1, int(1.2 * w0 / st))
    if len(side_a) > 2 * trim + 2:
        core = side_a[trim:-trim]
    else:
        core = side_a[len(side_a) // 3 : 2 * len(side_a) // 3 + 1]
    m, gaps = _mids(core, side_b)
    w = float(np.median(gaps))
    m = _smooth(m, False, st)
    if len(m) < 2:
        m = np.array([core[0], core[-1]])

    out = []
    for cap in ("round", "butt"):
        # Extend each end to the cap tip (a round cap adds w/2 by itself).
        ends = []
        for tip, end, inner in ((pts[c1], m[0], m[min(3, len(m) - 1)]), (pts[c2], m[-1], m[max(-4, -len(m))])):
            u = _unit(end - inner)
            reach = float(np.dot(tip - end, u)) - (w / 2 if cap == "round" else 0)
            ends.append(end + u * max(reach, 0))
        segs = fit_poly(np.vstack([ends[0], m, ends[1]]), False, err, max(w, 0.6))
        out += [Stroke(segs, False, w, cap=cap, join=join) for join in ("round", "miter")]
    return out


def score(st: Stroke, src: Path) -> float:
    """Mean distance (units) between the stroke's edge and the source outline:
    source points should lie w/2 from the centre line and centre-line points
    w/2 from the source. With butt caps or miter joins the source is measured
    against the stroke's actual outline instead."""
    line = np.vstack([sample(c, 0.02) for c in contours(st.path())])
    src_pts = np.vstack([sample(c, 0.02) for c in contours(src)])
    h = st.width / 2
    if st.cap == "round" and st.join == "round":
        e1 = np.abs(cKDTree(line).query(src_pts)[0] - h)
    else:
        edge = np.vstack([sample(c, 0.02) for c in contours(st.outline())])
        e1 = cKDTree(edge).query(src_pts)[0]
    e2 = np.abs(cKDTree(src_pts).query(line)[0] - h)
    return float(np.mean(e1) + np.mean(e2)) / 2


def recover(path: Path, err=0.02) -> Stroke:
    """The best centre-line stroke for an outlined stroke."""
    src = Path(path)
    src.simplify()
    cands = ring(src, err) if len(list(src.contours)) == 2 else band(src, err)
    for c in cands:
        c.err = score(c, src)
    # Round caps and joins match the drawing's style; others must clearly win.
    plain = lambda c: c.cap == "round" and c.join == "round"
    return min(cands, key=lambda c: c.err if plain(c) else c.err * 1.5 + 0.002)
