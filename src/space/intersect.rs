//! Intersections: the zero set of one object's function on another
//! object's mesh (chained into curves and refined onto both surfaces by
//! Newton steps), cutting meshes along those curves, points where curves
//! meet surfaces, points of tangency, and the conic a plane cuts from a
//! quadric.

use std::collections::HashMap;

use glam::{DMat4, DVec3, DVec4};

use super::mesh::DMesh;
use super::quadric::Quadric;
use super::solid::Solid;
use super::surface::Implicit;

/// A function that is zero on an object's surface.
#[derive(Clone, Copy)]
pub enum Field<'a> {
    Imp(&'a Implicit),
    Solid(&'a Solid),
    /// `n · p − d`.
    Plane(DVec3, f64),
}

impl Field<'_> {
    pub fn val(&self, p: DVec3) -> f64 {
        match self {
            Field::Imp(s) => s.eval(p),
            Field::Solid(s) => s.inside_fn(p),
            Field::Plane(n, d) => n.dot(p) - d,
        }
    }

    pub fn vg(&self, p: DVec3) -> (f64, DVec3) {
        match self {
            Field::Imp(s) => s.eval_grad(p),
            Field::Solid(s) => {
                let e = 1e-6 * (1.0 + p.abs().max_element());
                let d = |u: DVec3| (s.inside_fn(p + u * e) - s.inside_fn(p - u * e)) / (2.0 * e);
                (s.inside_fn(p), DVec3::new(d(DVec3::X), d(DVec3::Y), d(DVec3::Z)))
            }
            Field::Plane(n, d) => (n.dot(p) - d, *n),
        }
    }

    pub fn vals(&self, pts: &[DVec3]) -> Vec<f64> {
        match self {
            Field::Imp(s) => {
                let xs: Vec<f64> = pts.iter().map(|p| p.x).collect();
                let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
                let zs: Vec<f64> = pts.iter().map(|p| p.z).collect();
                s.values(&xs, &ys, &zs)
            }
            Field::Solid(s) => pts.iter().map(|p| s.inside_fn(*p)).collect(),
            Field::Plane(n, d) => pts.iter().map(|p| n.dot(*p) - d).collect(),
        }
    }

    pub fn vgs(&self, pts: &[DVec3]) -> Vec<(f64, DVec3)> {
        match self {
            Field::Imp(s) => s.values_grads(pts),
            Field::Solid(_) | Field::Plane(..) => pts.iter().map(|p| self.vg(*p)).collect(),
        }
    }
}

/// Position on edge (a, b) where the linear interpolant of g vanishes.
fn cross(pa: DVec3, pb: DVec3, ga: f64, gb: f64) -> DVec3 {
    let t = (ga / (ga - gb)).clamp(0.0, 1.0);
    pa + (pb - pa) * t
}

fn positive(g: f64) -> bool {
    g >= 0.0
}

/// The zero set of `g` (values at the mesh vertices) as polylines; a
/// closed curve repeats its first point at the end.
pub fn march(m: &DMesh, g: &[f64], tol: f64) -> Vec<Vec<DVec3>> {
    let key = |p: DVec3| ((p.x / tol).round() as i64, (p.y / tol).round() as i64, (p.z / tol).round() as i64);
    let mut nodes: Vec<DVec3> = Vec::new();
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut node = |p: DVec3| *ids.entry(key(p)).or_insert_with(|| {
        nodes.push(p);
        nodes.len() - 1
    });
    let mut segs: Vec<(usize, usize)> = Vec::new();
    for t in &m.tris {
        let v = t.map(|k| g[k as usize]);
        if v.iter().any(|x| !x.is_finite()) {
            continue;
        }
        let s = v.map(positive);
        if s[0] == s[1] && s[1] == s[2] {
            continue;
        }
        let mut pts = Vec::with_capacity(2);
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            if s[a] != s[b] {
                // Same order for both triangles sharing the edge.
                let (ka, kb) = if t[a] < t[b] { (a, b) } else { (b, a) };
                pts.push(cross(m.pos[t[ka] as usize], m.pos[t[kb] as usize], v[ka], v[kb]));
            }
        }
        let (a, b) = (node(pts[0]), node(pts[1]));
        if a != b {
            segs.push((a, b));
        }
    }
    chain(&nodes, &segs)
}

/// Join segments into polylines.
fn chain(nodes: &[DVec3], segs: &[(usize, usize)]) -> Vec<Vec<DVec3>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (k, &(a, b)) in segs.iter().enumerate() {
        adj[a].push(k);
        adj[b].push(k);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    let walk = |start: usize, used: &mut Vec<bool>| {
        let mut line = vec![nodes[start]];
        let mut at = start;
        while let Some(&k) = adj[at].iter().find(|&&k| !used[k]) {
            used[k] = true;
            let (a, b) = segs[k];
            at = if a == at { b } else { a };
            line.push(nodes[at]);
        }
        line
    };
    // Open curves first (start at an end), then loops.
    for n in 0..nodes.len() {
        if adj[n].len() % 2 == 1 && adj[n].iter().any(|&k| !used[k]) {
            out.push(walk(n, &mut used));
        }
    }
    for n in 0..nodes.len() {
        if adj[n].iter().any(|&k| !used[k]) {
            out.push(walk(n, &mut used));
        }
    }
    out.retain(|l| l.len() >= 2);
    out
}

/// Move points onto both zero sets (minimum-norm Newton steps for the two
/// equations, at most `h` each). Points where the surfaces are tangent
/// (the system is singular) stay where they are.
pub fn refine(pts: &mut [DVec3], a: &Field, b: &Field, h: f64) {
    for _ in 0..4 {
        let va = a.vgs(pts);
        let vb = b.vgs(pts);
        for (k, p) in pts.iter_mut().enumerate() {
            let ((fa, ga), (fb, gb)) = (va[k], vb[k]);
            let (aa, ab, bb) = (ga.dot(ga), ga.dot(gb), gb.dot(gb));
            let det = aa * bb - ab * ab;
            if !(det.is_finite() && fa.is_finite() && fb.is_finite()) || det <= 1e-10 * aa * bb {
                continue;
            }
            // Δ = −Jᵀ (J Jᵀ)⁻¹ f
            let la = (bb * fa - ab * fb) / det;
            let lb = (aa * fb - ab * fa) / det;
            let mut d = ga * la + gb * lb;
            let len = d.length();
            if len > h {
                d *= h / len;
            }
            *p -= d;
        }
    }
}

/// Cut every triangle that `g` changes sign on along its zero line, so
/// that translucent surfaces crossing each other sort cleanly.
pub fn split(m: &DMesh, g: &[f64]) -> DMesh {
    let mut out = DMesh { pos: m.pos.clone(), nrm: m.nrm.clone(), tris: Vec::with_capacity(m.tris.len()), uv: m.uv.clone(), edges: m.edges.clone() };
    let mut made: HashMap<(u32, u32), u32> = HashMap::new();
    let mut point = |out: &mut DMesh, a: u32, b: u32| -> u32 {
        let (a, b) = (a.min(b), a.max(b));
        *made.entry((a, b)).or_insert_with(|| {
            let (ga, gb) = (g[a as usize], g[b as usize]);
            let t = (ga / (ga - gb)).clamp(0.0, 1.0);
            let lerp = |x: DVec3, y: DVec3| x + (y - x) * t;
            out.pos.push(lerp(m.pos[a as usize], m.pos[b as usize]));
            out.nrm.push(lerp(m.nrm[a as usize], m.nrm[b as usize]).normalize_or_zero());
            if let (Some(uv), Some(src)) = (&mut out.uv, &m.uv) {
                let (p, q) = (src[a as usize], src[b as usize]);
                uv.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
            (out.pos.len() - 1) as u32
        })
    };
    for t in &m.tris {
        let v = t.map(|k| g[k as usize]);
        let s = v.map(positive);
        if v.iter().any(|x| !x.is_finite()) || (s[0] == s[1] && s[1] == s[2]) {
            out.tris.push(*t);
            continue;
        }
        // The vertex alone on its side.
        let k = (0..3).find(|&k| s[k] != s[(k + 1) % 3] && s[k] != s[(k + 2) % 3]).unwrap();
        let (i, j, l) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
        let p = point(&mut out, i, j);
        let q = point(&mut out, i, l);
        out.tris.push([i, p, q]);
        out.tris.push([p, j, l]);
        out.tris.push([p, l, q]);
    }
    out
}

/// Where a parametrised curve meets a surface.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub p: DVec3,
    pub t: f64,
    /// The curve touches the surface here (tangent to it).
    pub tangent: bool,
}

/// Points where `r(t)`, `t ∈ [t0, t1]`, lies on the zero set of `f`:
/// sign changes refined by bisection, and touching points (|f| has a
/// local minimum at zero) found by golden-section search. `scale` is the
/// size of the view.
pub fn curve_hits(r: &dyn Fn(f64) -> Option<DVec3>, t0: f64, t1: f64, f: &Field, n: usize, scale: f64) -> Vec<Hit> {
    let ts: Vec<f64> = (0..=n).map(|k| t0 + (t1 - t0) * k as f64 / n as f64).collect();
    let pts: Vec<Option<DVec3>> = ts.iter().map(|&t| r(t)).collect();
    let h = |t: f64| r(t).map_or(f64::NAN, |p| f.val(p));
    let hs: Vec<f64> = pts.iter().map(|p| p.map_or(f64::NAN, |p| f.val(p))).collect();
    let mut hits: Vec<Hit> = Vec::new();
    let near_surface = |t: f64| {
        let Some(p) = r(t) else { return false };
        let (v, g) = f.vg(p);
        v.is_finite() && g.length() > 0.0 && v.abs() / g.length() < 1e-7 * scale
    };
    let tangent_at = |t: f64| {
        let dt = (t1 - t0) * 1e-6;
        match (r(t - dt), r(t + dt), r(t)) {
            (Some(a), Some(b), Some(p)) => {
                let d = (b - a).normalize_or_zero();
                let (_, g) = f.vg(p);
                g.length() > 0.0 && (g.dot(d) / g.length()).abs() < 2e-3
            }
            _ => false,
        }
    };
    for k in 0..n {
        let (a, b) = (hs[k], hs[k + 1]);
        if !(a.is_finite() && b.is_finite()) {
            continue;
        }
        if positive(a) != positive(b) {
            // Bisection.
            let (mut lo, mut hi, mut flo) = (ts[k], ts[k + 1], a);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                let fm = h(mid);
                if !fm.is_finite() {
                    break;
                }
                if positive(fm) == positive(flo) {
                    lo = mid;
                    flo = fm;
                } else {
                    hi = mid;
                }
            }
            let t = 0.5 * (lo + hi);
            // A jump of f (a pole) also changes sign: keep real zeros only.
            if near_surface(t) {
                hits.push(Hit { p: r(t).unwrap(), t, tangent: tangent_at(t) });
            }
        } else if k > 0 && hs[k - 1].is_finite() && positive(hs[k - 1]) == positive(a) && a.abs() < hs[k - 1].abs() && a.abs() <= b.abs() {
            // |f| has a local minimum near ts[k]: golden-section search.
            let (mut lo, mut hi) = (ts[k - 1], ts[k + 1]);
            let g = 0.618_033_988_749_894_9;
            for _ in 0..90 {
                let (c, d) = (hi - g * (hi - lo), lo + g * (hi - lo));
                if h(c).abs() < h(d).abs() {
                    hi = d;
                } else {
                    lo = c;
                }
            }
            let t = 0.5 * (lo + hi);
            if near_surface(t) {
                hits.push(Hit { p: r(t).unwrap(), t, tangent: true });
            }
        }
    }
    hits.dedup_by(|a, b| (a.p - b.p).length() < 1e-6 * scale);
    hits
}

/// Points where the surfaces of `a` and `b` touch: candidates from the
/// mesh of `a` (local extrema of b's function without a sign change) and
/// from the intersection curves (normals nearly parallel), refined to a
/// critical point of b's function on a's surface.
pub fn touches(ma: &DMesh, a: &Field, b: &Field, gb: &[f64], curves: &[Vec<DVec3>], h: f64, scale: f64, lo: DVec3, hi: DVec3) -> Vec<DVec3> {
    let mut cands: Vec<(f64, DVec3)> = Vec::new();
    // 1-ring neighbours.
    let mut adj: Vec<Vec<u32>> = vec![Vec::new(); ma.pos.len()];
    for t in &ma.tris {
        for k in 0..3 {
            let (u, v) = (t[k], t[(k + 1) % 3]);
            adj[u as usize].push(v);
            adj[v as usize].push(u);
        }
    }
    let grads = b.vgs(&ma.pos);
    for (v, nb) in adj.iter().enumerate() {
        let g = gb[v];
        if nb.is_empty() || !g.is_finite() {
            continue;
        }
        let gl = grads[v].1.length();
        if gl == 0.0 || g.abs() / gl > 2.0 * h {
            continue;
        }
        if nb.iter().all(|&u| gb[u as usize].is_finite() && positive(gb[u as usize]) == positive(g) && gb[u as usize].abs() >= g.abs()) {
            cands.push((g.abs() / gl, ma.pos[v]));
        }
    }
    for c in curves {
        let vg = a.vgs(c);
        let wg = b.vgs(c);
        for (k, p) in c.iter().enumerate() {
            let (na, nb) = (vg[k].1.normalize_or_zero(), wg[k].1.normalize_or_zero());
            let s = na.cross(nb).length();
            if s < 0.2 {
                cands.push((s * h, *p));
            }
        }
    }
    cands.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut found: Vec<DVec3> = Vec::new();
    for &(_, p0) in cands.iter().take(80) {
        if found.iter().any(|q| (*q - p0).length() < 2.0 * h) {
            continue;
        }
        let Some(p) = critical_point(a, b, p0, h, scale).or_else(|| descend(a, b, p0, h, scale)) else { continue };
        if p.cmplt(lo - DVec3::splat(1e-9 * scale)).any() || p.cmpgt(hi + DVec3::splat(1e-9 * scale)).any() {
            continue;
        }
        if !found.iter().any(|q| (*q - p).length() < 0.5 * h) {
            found.push(p);
        }
    }
    found
}

fn tangent_check(a: &Field, b: &Field, p: DVec3, scale: f64) -> bool {
    let (fa, ga) = a.vg(p);
    let (fb, gb) = b.vg(p);
    let (la, lb) = (ga.length(), gb.length());
    if !(la > 0.0 && lb > 0.0 && fa.is_finite() && fb.is_finite()) {
        return false;
    }
    fa.abs() / la < 1e-7 * scale && fb.abs() / lb < 1e-6 * scale && (ga / la).cross(gb / lb).length() < 2e-3
}

/// Newton on the Lagrange system ∇g = λ∇f, f = 0 (numerical Jacobian).
fn critical_point(a: &Field, b: &Field, p0: DVec3, h: f64, scale: f64) -> Option<DVec3> {
    let res = |p: DVec3, l: f64| {
        let (fa, ga) = a.vg(p);
        let (_, gb) = b.vg(p);
        let r = gb - ga * l;
        DVec4::new(r.x, r.y, r.z, fa)
    };
    let (_, ga) = a.vg(p0);
    let (_, gb) = b.vg(p0);
    let mut p = p0;
    let mut l = gb.dot(ga) / ga.length_squared().max(1e-300);
    let e = 1e-6 * scale;
    for _ in 0..30 {
        let r0 = res(p, l);
        let mut cols = [DVec4::ZERO; 4];
        for (i, u) in [DVec3::X, DVec3::Y, DVec3::Z].iter().enumerate() {
            cols[i] = (res(p + *u * e, l) - res(p - *u * e, l)) / (2.0 * e);
        }
        let (_, ga) = a.vg(p);
        cols[3] = DVec4::new(-ga.x, -ga.y, -ga.z, 0.0);
        let j = DMat4::from_cols(cols[0], cols[1], cols[2], cols[3]);
        let det = j.determinant();
        if !det.is_finite() || det.abs() < 1e-300 {
            break;
        }
        let d = j.inverse() * r0;
        let mut dp = d.truncate();
        let len = dp.length();
        if !len.is_finite() {
            return None;
        }
        if len > h {
            dp *= h / len;
        }
        p -= dp;
        l -= d.w;
        if (p - p0).length() > 4.0 * h {
            return None;
        }
        if len < 1e-13 * scale {
            break;
        }
    }
    tangent_check(a, b, p, scale).then_some(p)
}

/// For touching along a whole curve (the Lagrange system is singular
/// there): stay on a's surface and step along the tangential gradient of
/// b's function until it vanishes.
fn descend(a: &Field, b: &Field, p0: DVec3, h: f64, scale: f64) -> Option<DVec3> {
    let mut p = p0;
    for _ in 0..80 {
        let (fa, ga) = a.vg(p);
        if ga.length_squared() == 0.0 || !fa.is_finite() {
            return None;
        }
        p -= ga * (fa / ga.length_squared());
        let (fb, gb) = b.vg(p);
        let (_, ga) = a.vg(p);
        let n = ga.normalize_or_zero();
        let t = gb - n * gb.dot(n);
        if t.length_squared() < 1e-30 {
            break;
        }
        let mut d = t * (fb / t.length_squared());
        let len = d.length();
        if len > h {
            d *= h / len;
        }
        p -= d;
        if (p - p0).length() > 4.0 * h {
            return None;
        }
    }
    tangent_check(a, b, p, scale).then_some(p)
}

/// A plane section of a quadric, in the plane's own coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum ConicKind {
    Circle,
    Ellipse,
    Parabola,
    Hyperbola,
    TwoLines,
    ParallelLines,
    DoubleLine,
    Point,
    Empty,
}

#[derive(Clone, Debug)]
pub struct Conic {
    pub kind: ConicKind,
    /// Eccentricity (ellipse, circle, parabola, hyperbola).
    pub e: Option<f64>,
    /// Semi-axes (a ≥ b for an ellipse; real then imaginary for a hyperbola).
    pub axes: Option<(f64, f64)>,
    /// Centre or vertex, in space.
    pub centre: Option<DVec3>,
}

/// Orthonormal in-plane axes for the normal `n`.
pub fn plane_axes(n: DVec3) -> (DVec3, DVec3) {
    let nh = n.normalize();
    let i = (0..3).min_by(|&a, &b| nh[a].abs().total_cmp(&nh[b].abs())).unwrap();
    let e = [DVec3::X, DVec3::Y, DVec3::Z][i];
    let u = (e - nh * nh.dot(e)).normalize();
    (u, nh.cross(u))
}

/// The conic cut from quadric `q` by the plane `n · p = d`.
pub fn conic(q: &Quadric, n: DVec3, d: f64, scale: f64) -> Conic {
    let (am, bv, c, p0) = q.raw;
    let mul = |v: DVec3| DVec3::new(am[0][0] * v.x + am[0][1] * v.y + am[0][2] * v.z, am[1][0] * v.x + am[1][1] * v.y + am[1][2] * v.z, am[2][0] * v.x + am[2][1] * v.y + am[2][2] * v.z);
    let o = n * (d / n.length_squared());
    let (e1, e2) = plane_axes(n);
    let w0 = o - p0;
    // a u² + 2 b uv + c v² + 2 d u + 2 e v + f
    let (qa, qb, qc) = (e1.dot(mul(e1)), e1.dot(mul(e2)), e2.dot(mul(e2)));
    let lin = mul(w0) + bv;
    let (qd, qe) = (e1.dot(lin), e2.dot(lin));
    let qf = w0.dot(mul(w0)) + 2.0 * bv.dot(w0) + c;
    let l = scale.max(1e-9);
    let norm = (qa.abs().max(qb.abs()).max(qc.abs()) * l * l).max(qd.abs().max(qe.abs()) * l).max(qf.abs()).max(1e-300);
    let (qa, qb, qc, qd, qe, qf) = (qa / norm, qb / norm, qc / norm, qd / norm, qe / norm, qf / norm);
    let tol = 1e-9;
    // Eigen-decomposition of [[a, b], [b, c]].
    let tr = qa + qc;
    let disc = ((qa - qc) * (qa - qc) / 4.0 + qb * qb).sqrt();
    let lam = [tr / 2.0 + disc, tr / 2.0 - disc];
    let v1 = if qb.abs() > 1e-300 { (lam[0] - qc, qb) } else if qa >= qc { (1.0, 0.0) } else { (0.0, 1.0) };
    let len = (v1.0 * v1.0 + v1.1 * v1.1).sqrt();
    let v1 = (v1.0 / len, v1.1 / len);
    let vecs = [v1, (-v1.1, v1.0)];
    let dp = vecs.map(|v| v.0 * qd + v.1 * qe);
    let nz: Vec<usize> = (0..2).filter(|&i| lam[i].abs() * l * l > tol).collect();
    let to_space = |s: f64, t: f64| o + e1 * s + e2 * t;
    let mut centre_uv = (0.0, 0.0);
    let mut fp = qf;
    for &i in &nz {
        let shift = -dp[i] / lam[i];
        centre_uv.0 += vecs[i].0 * shift;
        centre_uv.1 += vecs[i].1 * shift;
        fp -= dp[i] * dp[i] / lam[i];
    }
    let none = |kind| Conic { kind, e: None, axes: None, centre: None };
    match nz.len() {
        2 if fp.abs() > tol => {
            let k = [-lam[0] / fp, -lam[1] / fp];
            let centre = Some(to_space(centre_uv.0, centre_uv.1));
            match (k[0] > 0.0, k[1] > 0.0) {
                (true, true) => {
                    let (a, b) = (1.0 / k[0].min(k[1]).sqrt(), 1.0 / k[0].max(k[1]).sqrt());
                    let circle = (a - b).abs() <= 1e-7 * a;
                    let e = if circle { 0.0 } else { (1.0 - (b / a).powi(2)).max(0.0).sqrt() };
                    Conic { kind: if circle { ConicKind::Circle } else { ConicKind::Ellipse }, e: Some(e), axes: Some((a, b)), centre }
                }
                (false, false) => none(ConicKind::Empty),
                _ => {
                    let (kp, kn) = if k[0] > 0.0 { (k[0], k[1]) } else { (k[1], k[0]) };
                    let (a, b) = (1.0 / kp.sqrt(), 1.0 / (-kn).sqrt());
                    Conic { kind: ConicKind::Hyperbola, e: Some((1.0 + (b / a).powi(2)).sqrt()), axes: Some((a, b)), centre }
                }
            }
        }
        2 => none(if lam[0] * lam[1] > 0.0 { ConicKind::Point } else { ConicKind::TwoLines }),
        1 => {
            let j = 1 - nz[0];
            if dp[j].abs() * l > tol {
                Conic { kind: ConicKind::Parabola, e: Some(1.0), axes: None, centre: None }
            } else if fp.abs() > tol {
                none(if -fp / lam[nz[0]] > 0.0 { ConicKind::ParallelLines } else { ConicKind::Empty })
            } else {
                none(ConicKind::DoubleLine)
            }
        }
        _ => none(if (qd.abs() + qe.abs()) * l > tol { ConicKind::DoubleLine } else { ConicKind::Empty }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::mesh::{plane_mesh, surface_nets};
    use crate::space::surface::SurfaceKind;

    fn imp(src: &str) -> Implicit {
        let mut s = Implicit::new(src).unwrap();
        s.classify(DVec3::splat(-3.0), DVec3::splat(3.0));
        s
    }

    #[test]
    fn viviani_curve_lies_on_both_surfaces() {
        let (lo, hi) = (DVec3::splat(-3.0), DVec3::splat(3.0));
        let sphere = imp("x^2 + y^2 + z^2 = 4");
        let cyl = imp("x^2 + y^2 = 2x");
        let m = surface_nets(&sphere, lo, hi, 48);
        let g = Field::Imp(&cyl).vals(&m.pos);
        let mut curves = march(&m, &g, 1e-9);
        assert!(!curves.is_empty());
        let h = 6.0 / 48.0;
        for c in &mut curves {
            refine(c, &Field::Imp(&sphere), &Field::Imp(&cyl), h);
        }
        let n: usize = curves.iter().map(Vec::len).sum();
        let good = curves.iter().flatten().filter(|p| (p.length_squared() - 4.0).abs() < 1e-9 && (p.x * p.x + p.y * p.y - 2.0 * p.x).abs() < 1e-9).count();
        // All but the points next to the double point (2, 0, 0).
        assert!(good as f64 > 0.95 * n as f64, "{good}/{n}");
    }

    #[test]
    fn plane_cuts_sphere_in_one_closed_circle() {
        let (lo, hi) = (DVec3::splat(-3.0), DVec3::splat(3.0));
        let plane = imp("z = 1");
        let SurfaceKind::Plane { n, d } = plane.kind else { panic!() };
        let m = plane_mesh(n, d, lo, hi, 0.1);
        let sphere = imp("x^2 + y^2 + z^2 = 4");
        let g = Field::Imp(&sphere).vals(&m.pos);
        let curves = march(&m, &g, 1e-9);
        assert_eq!(curves.len(), 1, "{:?}", curves.iter().map(Vec::len).collect::<Vec<_>>());
        let c = &curves[0];
        assert!((c[0] - c[c.len() - 1]).length() < 1e-12, "closed");
        let split_mesh = split(&m, &g);
        assert!(split_mesh.tris.len() > m.tris.len());
    }

    #[test]
    fn line_hits_and_tangency() {
        let sphere = imp("x^2 + y^2 + z^2 = 4");
        let f = Field::Imp(&sphere);
        let line = |p: DVec3, d: DVec3| move |t: f64| Some(p + d * t);
        let hits = curve_hits(&line(DVec3::ZERO, DVec3::X), -5.0, 5.0, &f, 500, 6.0);
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|h| !h.tangent && (h.p.x.abs() - 2.0).abs() < 1e-9));
        // z = 2 touches the sphere at (0, 0, 2).
        let hits = curve_hits(&line(DVec3::new(0.0, 0.0, 2.0), DVec3::X), -5.0, 5.0, &f, 500, 6.0);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!(hits[0].tangent && hits[0].p.length() - 2.0 < 1e-6);
        assert!(curve_hits(&line(DVec3::new(0.0, 0.0, 2.5), DVec3::X), -5.0, 5.0, &f, 500, 6.0).is_empty());
    }

    #[test]
    fn tangent_plane_touches_sphere() {
        let (lo, hi) = (DVec3::splat(-3.0), DVec3::splat(3.0));
        let sphere = imp("x^2 + y^2 + z^2 = 4");
        let plane = imp("z = 2");
        let m = surface_nets(&sphere, lo, hi, 48);
        let (a, b) = (Field::Imp(&sphere), Field::Imp(&plane));
        let g = b.vals(&m.pos);
        let t = touches(&m, &a, &b, &g, &[], 6.0 / 48.0, 6.0, lo, hi);
        assert_eq!(t.len(), 1, "{t:?}");
        assert!((t[0] - DVec3::new(0.0, 0.0, 2.0)).length() < 1e-6);
        // Saddle z = xy meets z = 0 in the two axes, tangent at the origin.
        let saddle = imp("z = x*y");
        let flat = imp("z = 0");
        let m = surface_nets(&saddle, lo, hi, 48);
        let (a, b) = (Field::Imp(&saddle), Field::Imp(&flat));
        let g = b.vals(&m.pos);
        let curves = march(&m, &g, 1e-9);
        let t = touches(&m, &a, &b, &g, &curves, 6.0 / 48.0, 6.0, lo, hi);
        assert_eq!(t.len(), 1, "{t:?}");
        assert!(t[0].length() < 1e-6);
    }

    #[test]
    fn conic_sections_of_a_cone() {
        let cone = imp("x^2 + y^2 = z^2");
        let SurfaceKind::Quadric(q) = &cone.kind else { panic!() };
        // z = k x + 1 meets the axis at θ with tan θ = k; half-angle 45°, so
        // e = sin θ / cos 45° = √2 k / √(1 + k²).
        for (k, want) in [(0.0, ConicKind::Circle), (0.5, ConicKind::Ellipse), (1.0, ConicKind::Parabola), (2.0, ConicKind::Hyperbola)] {
            let c = conic(q, DVec3::new(-k, 0.0, 1.0), 1.0, 3.0);
            assert_eq!(c.kind, want, "k = {k}");
            let e = 2f64.sqrt() * k / (1.0 + k * k).sqrt();
            assert!((c.e.unwrap() - e).abs() < 1e-9, "k = {k}: e = {:?}", c.e);
        }
        // Through the apex.
        assert_eq!(conic(q, DVec3::Z, 0.0, 3.0).kind, ConicKind::Point);
        assert_eq!(conic(q, DVec3::X, 0.0, 3.0).kind, ConicKind::TwoLines);
        let sphere = imp("x^2 + y^2 + z^2 = 4");
        let SurfaceKind::Quadric(s) = &sphere.kind else { panic!() };
        let c = conic(s, DVec3::Z, 1.0, 3.0);
        assert_eq!(c.kind, ConicKind::Circle);
        assert!((c.axes.unwrap().0 - 3f64.sqrt()).abs() < 1e-12);
        assert_eq!(conic(s, DVec3::Z, 3.0, 3.0).kind, ConicKind::Empty);
    }
}
