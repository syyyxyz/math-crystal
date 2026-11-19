//! Special points of curves inside a window: zeros, extrema, inflection
//! points and asymptotes of y = f(x); axis crossings, horizontal and
//! vertical tangents, inflection and singular points of other curves; and
//! the intersections of two curves.
//!
//! Roots are found from sign changes on a fine sample and refined with
//! Illinois; sign changes across poles end on large values and are thrown
//! out, unless the curve itself is continuous there (|x| has a minimum
//! where its derivative jumps).

use super::nice::snap;
use super::sample::Event;
use super::trace::{illinois, is_root};
use super::{Def, Form, Frame2};
use crate::i18n::tr;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Zero,
    Max,
    Min,
    Inflection,
    YIntercept,
    XAxis,
    YAxis,
    Origin,
    HTangent,
    VTangent,
    Singular,
    /// x′ = y′ = 0 on a parametric curve.
    Cusp,
}

/// Groups shown together (bits of a curve's marks).
pub const ZEROS: u8 = 1;
pub const EXTREMA: u8 = 2;
pub const INFLECTIONS: u8 = 4;
pub const ASYMPTOTES: u8 = 8;
pub const SINGULAR: u8 = 16;

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Zero => tr("zero", "零点"),
            Kind::Max => tr("local maximum", "极大值点"),
            Kind::Min => tr("local minimum", "极小值点"),
            Kind::Inflection => tr("inflection point", "拐点"),
            Kind::YIntercept => tr("y-intercept", "与 y 轴交点"),
            Kind::XAxis => tr("crosses the x-axis", "与 x 轴交点"),
            Kind::YAxis => tr("crosses the y-axis", "与 y 轴交点"),
            Kind::Origin => tr("through the origin", "经过原点"),
            Kind::HTangent => tr("horizontal tangent", "水平切线点"),
            Kind::VTangent => tr("vertical tangent", "竖直切线点"),
            Kind::Singular => tr("singular point", "奇点"),
            Kind::Cusp => tr("cusp (x′ = y′ = 0)", "尖点（x′ = y′ = 0）"),
        }
    }

    pub fn group(self) -> u8 {
        match self {
            Kind::Zero | Kind::YIntercept | Kind::XAxis | Kind::YAxis | Kind::Origin => ZEROS,
            Kind::Max | Kind::Min | Kind::HTangent | Kind::VTangent => EXTREMA,
            Kind::Inflection => INFLECTIONS,
            Kind::Singular | Kind::Cusp => SINGULAR,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub kind: Kind,
    pub x: f64,
    pub y: f64,
    /// Curve parameter (NaN for implicit curves).
    pub u: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Asym {
    Vertical(f64),
    /// y = k x + b.
    Line { k: f64, b: f64 },
}

#[derive(Default, Clone, Debug)]
pub struct Features {
    pub points: Vec<Point>,
    pub asymptotes: Vec<Asym>,
    /// Some kind had more points than are listed.
    pub truncated: bool,
}

/// Most points of one kind.
pub const MAX_EACH: usize = 40;
/// Samples over the range.
const N: usize = 4000;

struct Root {
    u: f64,
    /// The sign really changes (not just touching zero).
    change: bool,
    /// Sign before the root.
    before: f64,
    /// Ended on a small value (not a pole or jump).
    valid: bool,
}

fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect()
}

fn sgn(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Roots of h from its samples `hv` at `us` (refined by evaluating `h`).
fn roots(h: &dyn Fn(&[f64]) -> Vec<f64>, us: &[f64], hv: &[f64], truncated: &mut bool) -> Vec<Root> {
    let n = us.len();
    let mut out = Vec::new();
    let mut brackets: Vec<usize> = Vec::new();
    // A zero at the start of the range (say θ = 0 on a polar curve).
    if n > 1 && hv[0] == 0.0 && hv[1] != 0.0 {
        out.push(Root { u: us[0], change: false, before: 0.0, valid: true });
    }
    let mut i = 0;
    while i + 1 < n {
        let (a, b) = (hv[i], hv[i + 1]);
        if a.is_finite() && b.is_finite() && a * b < 0.0 {
            brackets.push(i);
        } else if b == 0.0 && a.is_finite() && a != 0.0 {
            // Exact zero at a sample: skip runs of zeros (h vanishes on an interval).
            let mut j = i + 1;
            while j < n && hv[j] == 0.0 {
                j += 1;
            }
            if j == i + 2 {
                let after = if j < n { hv[j] } else { f64::NAN };
                out.push(Root { u: us[i + 1], change: a * after < 0.0, before: sgn(a), valid: true });
            }
            i = j - 1;
            continue;
        }
        i += 1;
    }
    if brackets.len() > 6 * MAX_EACH {
        *truncated = true;
        brackets.truncate(6 * MAX_EACH);
    }
    if !brackets.is_empty() {
        let fa: Vec<f64> = brackets.iter().map(|&i| hv[i]).collect();
        let fb: Vec<f64> = brackets.iter().map(|&i| hv[i + 1]).collect();
        let eval = |s: &[f64]| {
            let u: Vec<f64> = brackets.iter().zip(s).map(|(&i, s)| us[i] + s * (us[i + 1] - us[i])).collect();
            h(&u)
        };
        let (s, fs) = illinois(&eval, &fa, &fb, 60);
        for (k, &i) in brackets.iter().enumerate() {
            out.push(Root { u: us[i] + s[k] * (us[i + 1] - us[i]), change: true, before: sgn(fa[k]), valid: is_root(fs[k], fa[k], fb[k]) });
        }
    }
    out.sort_by(|a, b| a.u.total_cmp(&b.u));
    out
}

fn median(mut v: Vec<f64>) -> f64 {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Drop repeats (within half a point of an earlier point of the same
/// kind), merge x- and y-axis crossings at the origin, keep at most
/// MAX_EACH of each kind.
fn tidy(points: Vec<Point>, f: &Frame2, truncated: &mut bool) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    // Rounding noise such as y = −3e−34 at the origin.
    let zero = 1e-12 * ((f.x1 - f.x0).abs() + (f.y1 - f.y0).abs());
    for mut p in points {
        if p.x.abs() < zero {
            p.x = 0.0;
        }
        if p.y.abs() < zero {
            p.y = 0.0;
        }
        if !(p.x.is_finite() && p.y.is_finite()) {
            continue;
        }
        let near = |q: &Point| f.dist([p.x, p.y], [q.x, q.y]) < 0.5;
        if let Some(q) = out.iter_mut().find(|q| near(q) && matches!((q.kind, p.kind), (Kind::XAxis, Kind::YAxis) | (Kind::YAxis, Kind::XAxis))) {
            q.kind = Kind::Origin;
            continue;
        }
        if out.iter().any(|q| near(q) && (q.kind == p.kind || q.kind.group() == ZEROS && p.kind.group() == ZEROS || matches!(q.kind, Kind::Singular | Kind::Cusp) && p.kind.group() == EXTREMA)) {
            continue;
        }
        if out.iter().filter(|q| q.kind == p.kind).count() >= MAX_EACH {
            *truncated = true;
            continue;
        }
        out.push(p);
    }
    out
}

/// Special points of a path over [u0, u1] (for y = f(x), the window's x
/// range). `marks` selects the groups.
pub fn path_features(def: &Def, u0: f64, u1: f64, f: &Frame2, marks: u8) -> Features {
    let mut feats = Features::default();
    if !def.form.is_path() || !(u1 > u0) {
        return feats;
    }
    let us = linspace(u0, u1, N);
    let [x, y] = def.path(&us);
    let [dx, dy] = def.path_d1(&us);
    let [ddx, ddy] = def.path_d2(&us);
    let span = u1 - u0;
    // Continuous at u: the gap across u shrinks with the step (a vertical
    // tangent like ∛x at 0 still has visible gaps at small steps).
    let cont = |u: f64| {
        let gap = |d: f64| {
            let [px, py] = def.path(&[u - d, u, u + d]);
            if (0..3).all(|k| px[k].is_finite() && py[k].is_finite()) {
                f.dist([px[0], py[0]], [px[1], py[1]]).max(f.dist([px[1], py[1]], [px[2], py[2]]))
            } else {
                f64::INFINITY
            }
        };
        let (g1, g2) = (gap(1e-6 * span), gap(1e-9 * span));
        g2 < 0.5 && (g2 < 0.02 || g2 < 0.5 * g1)
    };
    let at = |u: f64| {
        let [px, py] = def.path(&[u]);
        (px[0], py[0])
    };
    let comp = |k: usize, which: u8| {
        move |u: &[f64]| -> Vec<f64> {
            let v = match which {
                0 => def.path(u),
                1 => def.path_d1(u),
                _ => def.path_d2(u),
            };
            v[k].clone()
        }
    };
    let mut tr_ = false;
    let mut pts: Vec<Point> = Vec::new();
    let pt = |kind: Kind, u: f64| {
        let (x, y) = at(u);
        Point { kind, x, y, u }
    };
    if def.form == Form::FnX {
        if marks & ZEROS != 0 {
            for r in roots(&comp(1, 0), &us, &y, &mut tr_) {
                if r.valid {
                    pts.push(Point { kind: Kind::Zero, x: r.u, y: 0.0, u: r.u });
                }
            }
            if u0 < 0.0 && u1 > 0.0 {
                let p = pt(Kind::YIntercept, 0.0);
                if p.y.is_finite() {
                    pts.push(p);
                }
            }
        }
        if marks & (EXTREMA | ZEROS) != 0 {
            for r in roots(&comp(1, 1), &us, &dy, &mut tr_) {
                if !(r.change && cont(r.u)) {
                    continue;
                }
                let p = pt(if r.before > 0.0 { Kind::Max } else { Kind::Min }, r.u);
                // A zero that only touches the axis.
                if marks & ZEROS != 0 && p.y.abs() * f.ky < 1e-6 {
                    pts.push(Point { kind: Kind::Zero, y: 0.0, ..p });
                }
                if marks & EXTREMA != 0 {
                    pts.push(p);
                }
            }
        }
        if marks & INFLECTIONS != 0 {
            for r in roots(&comp(1, 2), &us, &ddy, &mut tr_) {
                if r.change && cont(r.u) {
                    pts.push(pt(Kind::Inflection, r.u));
                }
            }
        }
    } else {
        let speed = median((0..us.len()).map(|k| dx[k].hypot(dy[k])).collect());
        let tiny = |v: f64| v.abs() < 1e-6 * speed;
        let d1 = |u: f64| {
            let [a, b] = def.path_d1(&[u]);
            (a[0], b[0])
        };
        if marks & ZEROS != 0 {
            for (k, kind) in [(1, Kind::XAxis), (0, Kind::YAxis)] {
                let vals = if k == 1 { &y } else { &x };
                for r in roots(&comp(k, 0), &us, vals, &mut tr_) {
                    if r.valid {
                        let mut p = pt(kind, r.u);
                        if k == 1 {
                            p.y = 0.0;
                        } else {
                            p.x = 0.0;
                        }
                        pts.push(p);
                    }
                }
            }
        }
        if marks & (EXTREMA | SINGULAR) != 0 {
            for (k, kind) in [(1, Kind::HTangent), (0, Kind::VTangent)] {
                let vals = if k == 1 { &dy } else { &dx };
                for r in roots(&comp(k, 1), &us, vals, &mut tr_) {
                    if !(r.change && cont(r.u)) {
                        continue;
                    }
                    let (a, b) = d1(r.u);
                    let other = if k == 1 { a } else { b };
                    if tiny(other) || !(r.valid) && tiny(if k == 1 { b } else { a }) {
                        if marks & SINGULAR != 0 {
                            pts.push(pt(Kind::Cusp, r.u));
                        }
                    } else if marks & EXTREMA != 0 && r.valid {
                        pts.push(pt(kind, r.u));
                    }
                }
            }
        }
        if marks & INFLECTIONS != 0 {
            let kappa: Vec<f64> = (0..us.len()).map(|k| dx[k] * ddy[k] - dy[k] * ddx[k]).collect();
            let kf = |u: &[f64]| {
                let [a, b] = def.path_d1(u);
                let [c, d] = def.path_d2(u);
                (0..u.len()).map(|k| a[k] * d[k] - b[k] * c[k]).collect()
            };
            for r in roots(&kf, &us, &kappa, &mut tr_) {
                let (a, b) = d1(r.u);
                if r.change && cont(r.u) && !(tiny(a) && tiny(b)) {
                    pts.push(pt(Kind::Inflection, r.u));
                }
            }
        }
    }
    feats.points = tidy(pts, f, &mut tr_);
    feats.truncated = tr_;
    feats
}

/// Solve two equations by Newton's method from `p`; `sys` gives the two
/// values and the Jacobian at a point. None unless it converges to a
/// point within `reach` (data units) of the start.
fn newton2(sys: &dyn Fn(f64, f64) -> ([f64; 2], [[f64; 2]; 2]), p: [f64; 2], reach: f64) -> Option<[f64; 2]> {
    let (mut x, mut y) = (p[0], p[1]);
    for _ in 0..40 {
        let (g, j) = sys(x, y);
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if !(det.is_finite() && det != 0.0 && g[0].is_finite() && g[1].is_finite()) {
            return None;
        }
        let dx = (g[0] * j[1][1] - g[1] * j[0][1]) / det;
        let dy = (j[0][0] * g[1] - j[1][0] * g[0]) / det;
        x -= dx;
        y -= dy;
        if (x - p[0]).hypot(y - p[1]) > reach {
            return None;
        }
        if dx.hypot(dy) <= 1e-13 * (1.0 + x.abs() + y.abs()) {
            return Some([x, y]);
        }
    }
    None
}

/// Special points of an implicit curve F(x, y) = 0, from its traced
/// polylines in the window.
pub fn field_features(def: &Def, lines: &[Vec<[f64; 2]>], f: &Frame2, marks: u8) -> Features {
    let mut feats = Features::default();
    if def.form != Form::Implicit {
        return feats;
    }
    let mut tr_ = false;
    let mut pts: Vec<Point> = Vec::new();
    let reach = 6.0 / f.kx.min(f.ky);
    let win = (f.x1 - f.x0).max(f.y1 - f.y0);
    if marks & ZEROS != 0 {
        for axis in [0usize, 1] {
            let (a, b) = if axis == 0 { (f.x0, f.x1) } else { (f.y0, f.y1) };
            let us = linspace(a, b, N);
            let zeros = vec![0.0; us.len()];
            let vals = if axis == 0 { def.field(&us, &zeros) } else { def.field(&zeros, &us) };
            let h = |u: &[f64]| {
                let z = vec![0.0; u.len()];
                if axis == 0 { def.field(u, &z) } else { def.field(&z, u) }
            };
            for r in roots(&h, &us, &vals, &mut tr_) {
                if r.valid {
                    let (x, y) = if axis == 0 { (r.u, 0.0) } else { (0.0, r.u) };
                    pts.push(Point { kind: if axis == 0 { Kind::XAxis } else { Kind::YAxis }, x, y, u: f64::NAN });
                }
            }
        }
    }
    if marks & (EXTREMA | SINGULAR) != 0 && !lines.is_empty() {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for l in lines {
            for p in l {
                xs.push(p[0]);
                ys.push(p[1]);
            }
        }
        let [_, gx, gy] = def.field_grad(&xs, &ys);
        let gscale = median((0..xs.len()).map(|k| gx[k].hypot(gy[k])).collect());
        let h = |x: f64, y: f64| def.field_hess(x, y);
        let tangent_sys = |which: usize| {
            move |x: f64, y: f64| {
                let [ff, fx, fy, fxx, fxy, fyy] = h(x, y);
                if which == 0 {
                    ([ff, fx], [[fx, fy], [fxx, fxy]])
                } else {
                    ([ff, fy], [[fx, fy], [fxy, fyy]])
                }
            }
        };
        let sing_sys = |x: f64, y: f64| {
            let [_, fx, fy, fxx, fxy, fyy] = h(x, y);
            ([fx, fy], [[fxx, fxy], [fxy, fyy]])
        };
        let on_curve = |p: [f64; 2]| h(p[0], p[1])[0].abs() <= 1e-8 * gscale.max(1e-300) * win;
        let mut k0 = 0;
        let mut found = 0usize;
        for l in lines {
            for i in 0..l.len().saturating_sub(1) {
                let (a, b) = (k0 + i, k0 + i + 1);
                for (which, g) in [(0usize, &gx), (1, &gy)] {
                    let (ga, gb) = (g[a], g[b]);
                    if !(ga.is_finite() && gb.is_finite() && ga * gb < 0.0) {
                        continue;
                    }
                    found += 1;
                    if found > 8 * MAX_EACH {
                        tr_ = true;
                        continue;
                    }
                    let t = ga / (ga - gb);
                    let p0 = [l[i][0] + t * (l[i + 1][0] - l[i][0]), l[i][1] + t * (l[i + 1][1] - l[i][1])];
                    let tangent = newton2(&tangent_sys(which), p0, reach);
                    let singular_here = |p: [f64; 2]| {
                        let [_, fx, fy, ..] = h(p[0], p[1]);
                        fx.hypot(fy) < 1e-4 * gscale
                    };
                    let p = match tangent {
                        Some(p) if !singular_here(p) && on_curve(p) => {
                            if marks & EXTREMA != 0 {
                                pts.push(Point { kind: if which == 0 { Kind::HTangent } else { Kind::VTangent }, x: p[0], y: p[1], u: f64::NAN });
                            }
                            continue;
                        }
                        Some(p) => p,
                        None => p0,
                    };
                    if marks & SINGULAR != 0
                        && let Some(q) = newton2(&sing_sys, p, reach)
                        && on_curve(q)
                    {
                        pts.push(Point { kind: Kind::Singular, x: q[0], y: q[1], u: f64::NAN });
                    }
                }
            }
            k0 += l.len();
        }
    }
    feats.points = tidy(pts, f, &mut tr_);
    feats.truncated = tr_;
    feats
}

/// |values| grow without bound as the step shrinks (also slowly, like
/// ln x near 0).
fn diverges(v: &[f64]) -> bool {
    if v.iter().any(|x| x.is_nan()) || v.len() < 3 {
        return false;
    }
    if v.windows(2).any(|w| !(w[1] > w[0])) {
        return false;
    }
    let n = v.len();
    if v[n - 1].is_infinite() {
        return true;
    }
    let (d0, d1) = (v[1] - v[0], v[n - 1] - v[n - 2]);
    d1 >= 0.5 * d0 && v[n - 1] > v[0] + 3.0
}

/// Asymptotes of y = f(x): vertical ones at the breaks and ends found by
/// the sampler (in the window), horizontal or oblique ones at ±∞.
pub fn asymptotes(def: &Def, events: &[Event], f: &Frame2) -> Vec<Asym> {
    let mut out: Vec<Asym> = Vec::new();
    if def.form != Form::FnX {
        return out;
    }
    let w = f.x1 - f.x0;
    let fx = |xs: &[f64]| def.path(xs)[1].clone();
    let mut seen: Vec<f64> = Vec::new();
    for e in events {
        let u = match e {
            Event::Break(u) | Event::Edge(u) => *u,
        };
        if !(u > f.x0 && u < f.x1) || seen.iter().any(|s| (s - u).abs() < 1e-6 * w) || seen.len() > 2 * MAX_EACH {
            continue;
        }
        seen.push(u);
        let any = [-1.0, 1.0].iter().any(|s| {
            let xs: Vec<f64> = (2..=8).map(|k| u + s * w * 10f64.powi(-k)).collect();
            let v: Vec<f64> = fx(&xs).iter().map(|v| v.abs()).collect();
            diverges(&v)
        });
        if any {
            out.push(Asym::Vertical(snap(u, 1e-7 * w)));
        }
    }
    for s in [1.0, -1.0] {
        let xs: Vec<f64> = (3..=8).map(|j| s * 10f64.powi(j)).collect();
        let v = fx(&xs);
        if v.iter().any(|y| !y.is_finite()) {
            continue;
        }
        let k: Vec<f64> = v.iter().zip(&xs).map(|(y, x)| y / x).collect();
        let k8 = k[5];
        if (k[5] - k[4]).abs() > 1e-6 * (1.0 + k8.abs()) || (k[4] - k[3]).abs() > 1e-4 * (1.0 + k8.abs()) {
            continue;
        }
        let kk = if k8.abs() < 1e-7 { 0.0 } else { snap(k8, 1e-6 * (1.0 + k8.abs())) };
        // b from moderate x, where f(x) − kx loses fewer digits.
        let b: Vec<f64> = (0..4).map(|j| v[j] - kk * xs[j]).collect();
        let b6 = b[3];
        if (b[3] - b[2]).abs() > 1e-4 * (1.0 + b6.abs()) || (b[2] - b[1]).abs() > 1e-2 * (1.0 + b6.abs()) {
            continue;
        }
        let bb = snap(b6, 1e-4 * (1.0 + b6.abs()));
        let bb = if bb.abs() < 1e-6 { 0.0 } else { bb };
        let a = Asym::Line { k: kk, b: bb };
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// A drawn polyline with the curve parameter at each point (NaN when the
/// curve has none).
pub type Line3 = Vec<[f64; 3]>;

/// Intersections of two curves in the window, from their polylines,
/// refined with Newton's method where both have a formula to refine with.
pub fn intersections(a: &Def, la: &[Line3], b: &Def, lb: &[Line3], f: &Frame2, max: usize) -> Vec<[f64; 2]> {
    use std::collections::HashMap;
    let cell = [32.0 / f.kx, 32.0 / f.ky];
    // Cells of the window (one spare on each side); segments reaching far
    // outside (near a pole) only fill the cells inside.
    let (nx, ny) = (((f.x1 - f.x0) / cell[0]).ceil() as i64 + 1, ((f.y1 - f.y0) / cell[1]).ceil() as i64 + 1);
    let key = |x: f64, y: f64| (((x - f.x0) / cell[0]).floor().clamp(-1.0, nx as f64) as i64, ((y - f.y0) / cell[1]).floor().clamp(-1.0, ny as f64) as i64);
    let mut buckets: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
    for (li, l) in lb.iter().enumerate() {
        for si in 0..l.len().saturating_sub(1) {
            let (p, q) = (l[si], l[si + 1]);
            let (k0, k1) = (key(p[0].min(q[0]), p[1].min(q[1])), key(p[0].max(q[0]), p[1].max(q[1])));
            if (k1.0 - k0.0 + 1) * (k1.1 - k0.1 + 1) > 4096 {
                continue;
            }
            for i in k0.0..=k1.0 {
                for j in k0.1..=k1.1 {
                    buckets.entry((i, j)).or_default().push((li, si));
                }
            }
        }
    }
    // (point, u on a, u on b)
    let mut est: Vec<([f64; 2], f64, f64)> = Vec::new();
    'outer: for l in la {
        for si in 0..l.len().saturating_sub(1) {
            let (p, q) = (l[si], l[si + 1]);
            let (k0, k1) = (key(p[0].min(q[0]), p[1].min(q[1])), key(p[0].max(q[0]), p[1].max(q[1])));
            if (k1.0 - k0.0 + 1) * (k1.1 - k0.1 + 1) > 4096 {
                continue;
            }
            let mut tested: Vec<(usize, usize)> = Vec::new();
            for i in k0.0..=k1.0 {
                for j in k0.1..=k1.1 {
                    let Some(list) = buckets.get(&(i, j)) else { continue };
                    for &(li, sj) in list {
                        if tested.contains(&(li, sj)) {
                            continue;
                        }
                        tested.push((li, sj));
                        let (c, d) = (lb[li][sj], lb[li][sj + 1]);
                        let r = [q[0] - p[0], q[1] - p[1]];
                        let s = [d[0] - c[0], d[1] - c[1]];
                        let den = r[0] * s[1] - r[1] * s[0];
                        if den == 0.0 || !den.is_finite() {
                            continue;
                        }
                        let cp = [c[0] - p[0], c[1] - p[1]];
                        let t = (cp[0] * s[1] - cp[1] * s[0]) / den;
                        let w = (cp[0] * r[1] - cp[1] * r[0]) / den;
                        if (0.0..1.0).contains(&t) && (0.0..1.0).contains(&w) {
                            let pt = [p[0] + t * r[0], p[1] + t * r[1]];
                            if est.iter().any(|e| f.dist(e.0, pt) < 2.0) {
                                continue;
                            }
                            est.push((pt, p[2] + t * (q[2] - p[2]), c[2] + w * (d[2] - c[2])));
                            if est.len() >= max {
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
    }
    let reach = 4.0 / f.kx.min(f.ky);
    est.into_iter().map(|(p, ua, ub)| refine(a, b, p, ua, ub, reach).unwrap_or(p)).collect()
}

fn refine(a: &Def, b: &Def, p: [f64; 2], ua: f64, ub: f64, reach: f64) -> Option<[f64; 2]> {
    let close = |q: [f64; 2]| (q[0] - p[0]).hypot(q[1] - p[1]) <= reach;
    match (a.form.is_path(), b.form.is_path()) {
        (true, true) => {
            let sys = |u: f64, w: f64| {
                let (pa, pb) = (a.path_at(u), b.path_at(w));
                ([pa[0] - pb[0], pa[1] - pb[1]], [[pa[2], -pb[2]], [pa[3], -pb[3]]])
            };
            let r = newton2(&sys, [ua, ub], f64::INFINITY)?;
            let q = a.path_at(r[0]);
            Some([q[0], q[1]]).filter(|q| close(*q))
        }
        (true, false) | (false, true) if a.form == Form::Implicit || b.form == Form::Implicit => {
            let (path, field, u0) = if a.form.is_path() { (a, b, ua) } else { (b, a, ub) };
            let mut u = u0;
            for _ in 0..40 {
                let pp = path.path_at(u);
                let [fv, fx, fy] = field.field_grad(&[pp[0]], &[pp[1]]);
                let d = fx[0] * pp[2] + fy[0] * pp[3];
                if !(d.is_finite() && d != 0.0 && fv[0].is_finite()) {
                    return None;
                }
                let step = fv[0] / d;
                u -= step;
                if step.abs() <= 1e-14 * (1.0 + u.abs()) {
                    let q = path.path_at(u);
                    return Some([q[0], q[1]]).filter(|q| close(*q));
                }
            }
            None
        }
        (false, false) if a.form == Form::Implicit && b.form == Form::Implicit => {
            let sys = |x: f64, y: f64| {
                let ga = a.field_grad(&[x], &[y]);
                let gb = b.field_grad(&[x], &[y]);
                ([ga[0][0], gb[0][0]], [[ga[1][0], ga[2][0]], [gb[1][0], gb[2][0]]])
            };
            newton2(&sys, p, reach)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve2d::sample::sample;

    fn frame() -> Frame2 {
        Frame2 { x0: -6.0, x1: 6.0, y0: -4.0, y1: 4.0, kx: 80.0, ky: 80.0 }
    }

    fn kinds(fe: &Features, k: Kind) -> Vec<(f64, f64)> {
        fe.points.iter().filter(|p| p.kind == k).map(|p| (p.x, p.y)).collect()
    }

    #[test]
    fn cubic_extrema_and_inflection() {
        let d = Def::new("y = x^3 - 3x").unwrap();
        let fe = path_features(&d, -3.0, 3.0, &frame(), ZEROS | EXTREMA | INFLECTIONS);
        let z = kinds(&fe, Kind::Zero);
        assert_eq!(z.len(), 3);
        assert!((z[2].0 - 3f64.sqrt()).abs() < 1e-12);
        let mx = kinds(&fe, Kind::Max);
        assert_eq!(mx.len(), 1);
        assert!((mx[0].0 + 1.0).abs() < 1e-12 && (mx[0].1 - 2.0).abs() < 1e-12);
        let mn = kinds(&fe, Kind::Min);
        assert!((mn[0].0 - 1.0).abs() < 1e-12);
        let inf = kinds(&fe, Kind::Inflection);
        assert_eq!(inf.len(), 1);
        assert!(inf[0].0.abs() < 1e-12);
    }

    #[test]
    fn kinks_poles_and_double_roots() {
        let d = Def::new("y = |x| - 1").unwrap();
        let fe = path_features(&d, -3.0, 3.1, &frame(), EXTREMA);
        assert_eq!(kinds(&fe, Kind::Min).len(), 1);
        let d = Def::new("y = 1/x").unwrap();
        let fe = path_features(&d, -3.0, 3.1, &frame(), ZEROS | EXTREMA | INFLECTIONS);
        assert!(fe.points.is_empty(), "{:?}", fe.points);
        let d = Def::new("y = (x - 1)^2").unwrap();
        let fe = path_features(&d, -3.0, 3.1, &frame(), ZEROS);
        assert_eq!(kinds(&fe, Kind::Zero).len(), 1);
        let d = Def::new("y = cbrt(x)").unwrap();
        let fe = path_features(&d, -3.0, 3.1, &frame(), INFLECTIONS);
        assert_eq!(kinds(&fe, Kind::Inflection).len(), 1);
    }

    #[test]
    fn asymptotes_of_rational_and_log() {
        let d = Def::new("y = (x^2 + 1)/(x - 1)").unwrap();
        let fr = frame();
        let eval = |u: &[f64]| d.path(u);
        let s = sample(&eval, fr.x0, fr.x1, 960, &fr);
        let a = asymptotes(&d, &s.events, &fr);
        assert!(a.contains(&Asym::Vertical(1.0)), "{a:?}");
        assert!(a.contains(&Asym::Line { k: 1.0, b: 1.0 }), "{a:?}");
        let d = Def::new("y = ln(x)").unwrap();
        let eval = |u: &[f64]| d.path(u);
        let s = sample(&eval, fr.x0, fr.x1, 960, &fr);
        let a = asymptotes(&d, &s.events, &fr);
        assert_eq!(a.len(), 1, "{a:?}");
        assert!(matches!(a[0], Asym::Vertical(v) if v.abs() < 1e-9));
        let d = Def::new("y = atan(x)").unwrap();
        let a = asymptotes(&d, &[], &fr);
        assert!(a.contains(&Asym::Line { k: 0.0, b: std::f64::consts::FRAC_PI_2 }), "{a:?}");
        assert!(a.contains(&Asym::Line { k: 0.0, b: -std::f64::consts::FRAC_PI_2 }), "{a:?}");
    }

    #[test]
    fn cycloid_cusps_and_tangents() {
        let d = Def::new("(t - sin(t), 1 - cos(t))").unwrap();
        let fe = path_features(&d, -0.5, 4.0 * std::f64::consts::PI + 0.5, &frame(), EXTREMA | SINGULAR);
        assert_eq!(kinds(&fe, Kind::Cusp).len(), 3, "{:?}", fe.points);
        assert_eq!(kinds(&fe, Kind::HTangent).len(), 2);
    }

    #[test]
    fn folium_tangents_and_node() {
        use crate::curve2d::trace::{Grid, trace};
        let d = Def::new("x^3 + y^3 = 3x y").unwrap();
        let fr = Frame2 { x0: -3.0, x1: 3.0, y0: -3.0, y1: 3.0, kx: 100.0, ky: 100.0 };
        let g = Grid { u0: -3.0013, v0: -3.0007, du: 0.025, dv: 0.025, nu: 240, nv: 240 };
        let lines = trace(&|u: &[f64], v: &[f64]| d.field(u, v), &g);
        let fe = field_features(&d, &lines, &fr, EXTREMA | SINGULAR);
        let h = kinds(&fe, Kind::HTangent);
        // Horizontal tangent at (2^{1/3}, 2^{2/3}).
        assert!(h.iter().any(|p| (p.0 - 2f64.cbrt()).abs() < 1e-9 && (p.1 - 4f64.cbrt()).abs() < 1e-9), "{h:?}");
        let v = kinds(&fe, Kind::VTangent);
        assert!(v.iter().any(|p| (p.1 - 2f64.cbrt()).abs() < 1e-9), "{v:?}");
        let s = kinds(&fe, Kind::Singular);
        assert_eq!(s.len(), 1, "{:?}", fe.points);
        assert!(s[0].0.abs() < 1e-6 && s[0].1.abs() < 1e-6);
    }

    #[test]
    fn line_meets_circle() {
        use crate::curve2d::trace::{Grid, trace};
        let c = Def::new("x^2 + y^2 = 4").unwrap();
        let l = Def::new("y = x").unwrap();
        let fr = frame();
        let g = Grid { u0: -6.0, v0: -4.0, du: 0.03, dv: 0.03, nu: 400, nv: 267 };
        let lc: Vec<Line3> = trace(&|u: &[f64], v: &[f64]| c.field(u, v), &g).into_iter().map(|l| l.into_iter().map(|p| [p[0], p[1], f64::NAN]).collect()).collect();
        let eval = |u: &[f64]| l.path(u);
        let ll: Vec<Line3> = sample(&eval, -6.0, 6.0, 960, &fr).lines.into_iter().map(|l| l.into_iter().map(|p| [p.x, p.y, p.u]).collect()).collect();
        let pts = intersections(&l, &ll, &c, &lc, &fr, 20);
        assert_eq!(pts.len(), 2);
        let r = 2f64.sqrt();
        for p in pts {
            assert!((p[0].abs() - r).abs() < 1e-12 && (p[1] - p[0]).abs() < 1e-12, "{p:?}");
        }
    }
}
