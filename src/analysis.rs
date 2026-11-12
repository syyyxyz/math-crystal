//! Analysis on f(x, y): level curves (marching squares), critical points
//! (Newton on the gradient) and double integrals over rectangles or regions.

use crate::calc::{Surface, lerp};
use crate::expr::{Pool, Program, Vars, parse};

/// A line segment in data coordinates.
pub type Segment = [(f64, f64); 2];

/// Level curves of a row-major `n x n` grid (`z[j * n + i]`) at `level`.
/// Cells with an undefined corner are skipped. Saddle cells are resolved
/// with the cell-centre average.
pub fn contour(z: &[f64], n: usize, xr: (f64, f64), yr: (f64, f64), level: f64) -> Vec<Segment> {
    let mut out = Vec::new();
    let x = |i: f64| lerp(xr, i / (n - 1) as f64);
    let y = |j: f64| lerp(yr, j / (n - 1) as f64);
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            // Corners counter-clockwise from (i, j).
            let v = [z[j * n + i], z[j * n + i + 1], z[(j + 1) * n + i + 1], z[(j + 1) * n + i]];
            if v.iter().any(|v| !v.is_finite()) {
                continue;
            }
            let c = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            let above: Vec<bool> = v.iter().map(|&w| w >= level).collect();
            // Crossing point on edge k (between corner k and k+1).
            let cross = |k: usize| {
                let (a, b) = (k, (k + 1) % 4);
                let t = if v[a] == v[b] { 0.5 } else { (level - v[a]) / (v[b] - v[a]) };
                let t = t.clamp(0.0, 1.0);
                let px = c[a].0 + (c[b].0 - c[a].0) * t;
                let py = c[a].1 + (c[b].1 - c[a].1) * t;
                (x(i as f64 + px), y(j as f64 + py))
            };
            let edges: Vec<usize> = (0..4).filter(|&k| above[k] != above[(k + 1) % 4]).collect();
            match edges.len() {
                2 => out.push([cross(edges[0]), cross(edges[1])]),
                4 => {
                    // Saddle: pair edges according to the centre value.
                    let centre = v.iter().sum::<f64>() / 4.0;
                    if (centre >= level) == above[0] {
                        out.push([cross(0), cross(1)]);
                        out.push([cross(2), cross(3)]);
                    } else {
                        out.push([cross(3), cross(0)]);
                        out.push([cross(1), cross(2)]);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// About `count` "nice" contour levels strictly inside `lo..hi`.
pub fn levels(lo: f64, hi: f64, count: usize) -> Vec<f64> {
    crate::render::scene::ticks(lo, hi, count).into_iter().filter(|v| *v > lo && *v < hi).collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CriticalKind {
    Minimum,
    Maximum,
    Saddle,
    Degenerate,
}

impl CriticalKind {
    pub fn name(self) -> &'static str {
        match self {
            CriticalKind::Minimum => crate::i18n::tr("minimum", "极小值"),
            CriticalKind::Maximum => crate::i18n::tr("maximum", "极大值"),
            CriticalKind::Saddle => crate::i18n::tr("saddle", "鞍点"),
            CriticalKind::Degenerate => crate::i18n::tr("degenerate", "退化"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Critical {
    pub x: f64,
    pub y: f64,
    pub f: f64,
    pub kind: CriticalKind,
}

/// Critical points inside the rectangle: seeds are local minima of |grad f|
/// on a coarse grid, refined by damped Newton steps on grad f = 0.
pub fn critical_points(s: &Surface, xr: (f64, f64), yr: (f64, f64), max: usize) -> Vec<Critical> {
    let n = 41;
    let (xs, ys) = grid_points(xr, yr, n);
    let all = s.eval_all(&xs, &ys);
    let g2: Vec<f64> = (0..n * n)
        .map(|k| {
            let (gx, gy) = (all[1][k], all[2][k]);
            if gx.is_finite() && gy.is_finite() { gx * gx + gy * gy } else { f64::INFINITY }
        })
        .collect();
    let mut seeds = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            if !g2[k].is_finite() {
                continue;
            }
            let mut is_min = true;
            for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                let (ii, jj) = (i as i64 + di, j as i64 + dj);
                if ii < 0 || jj < 0 || ii >= n as i64 || jj >= n as i64 {
                    continue;
                }
                if g2[jj as usize * n + ii as usize] < g2[k] {
                    is_min = false;
                    break;
                }
            }
            if is_min {
                seeds.push((xs[k], ys[k]));
            }
        }
    }
    let span = (xr.1 - xr.0).max(yr.1 - yr.0);
    let mut found: Vec<Critical> = Vec::new();
    for (mut x, mut y) in seeds {
        let mut ok = false;
        for _ in 0..40 {
            let v = s.eval_point_all(x, y);
            let (gx, gy, hxx, hyy, hxy) = (v[1], v[2], v[3], v[4], 0.5 * (v[5] + v[6]));
            if !(gx.is_finite() && gy.is_finite()) {
                break;
            }
            let scale = 1.0 + v[0].abs();
            if gx.hypot(gy) < 1e-10 * scale {
                ok = true;
                break;
            }
            let det = hxx * hyy - hxy * hxy;
            if !det.is_finite() || det.abs() < 1e-14 {
                // Singular Hessian: accept only if already tiny.
                ok = gx.hypot(gy) < 1e-7 * scale;
                break;
            }
            let dx = (hyy * gx - hxy * gy) / det;
            let dy = (hxx * gy - hxy * gx) / det;
            // Damp huge steps.
            let step = dx.hypot(dy);
            let lim = 0.25 * span;
            let k = if step > lim { lim / step } else { 1.0 };
            x -= dx * k;
            y -= dy * k;
            if step * k < 1e-13 * span {
                let v = s.eval_point_all(x, y);
                ok = v[1].hypot(v[2]) < 1e-7 * (1.0 + v[0].abs());
                break;
            }
        }
        let inside = x >= xr.0 && x <= xr.1 && y >= yr.0 && y <= yr.1;
        if !ok || !inside {
            continue;
        }
        if found.iter().any(|c| (c.x - x).hypot(c.y - y) < 1e-6 * span) {
            continue;
        }
        let d = s.derivs(x, y);
        if !d.f.is_finite() {
            continue;
        }
        let kind = match crate::calc::classify(&d) {
            Some("local minimum") => CriticalKind::Minimum,
            Some("local maximum") => CriticalKind::Maximum,
            Some("saddle point") => CriticalKind::Saddle,
            _ => CriticalKind::Degenerate,
        };
        found.push(Critical { x: snap(x, span), y: snap(y, span), f: d.f, kind });
        if found.len() >= max {
            break;
        }
    }
    found.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    found
}

/// Remove numerical dust like 1e-17.
fn snap(v: f64, span: f64) -> f64 {
    if v.abs() < 1e-12 * span { 0.0 } else { v }
}

fn grid_points(xr: (f64, f64), yr: (f64, f64), n: usize) -> (Vec<f64>, Vec<f64>) {
    let xs = (0..n * n).map(|k| lerp(xr, (k % n) as f64 / (n - 1) as f64)).collect();
    let ys = (0..n * n).map(|k| lerp(yr, (k / n) as f64 / (n - 1) as f64)).collect();
    (xs, ys)
}

/// Optional region condition such as `x^2 + y^2 <= 1`.
pub struct Region {
    prog: Program,
    /// For a single comparison `a <= b`: a smooth margin (positive inside)
    /// whose zero set is the boundary.
    margin: Option<Program>,
    pub source: String,
}

impl Region {
    pub fn new(src: &str) -> Result<Self, crate::expr::ParseError> {
        let mut pool = Pool::new();
        let id = parse(src, &mut pool, &Vars(vec!["x", "y"]), false)?;
        use crate::expr::{Bin, Node};
        let margin = match pool.node(id) {
            Node::Bin(Bin::Lt | Bin::Le, a, b) => Some(pool.sub(b, a)),
            Node::Bin(Bin::Gt | Bin::Ge, a, b) => Some(pool.sub(a, b)),
            _ => None,
        };
        Ok(Self {
            prog: Program::compile(&pool, &[id]),
            margin: margin.map(|m| Program::compile(&pool, &[m])),
            source: src.to_string(),
        })
    }

    /// Field whose 0-level curve is the region boundary (positive inside):
    /// the smooth margin when available, else indicator - 1/2.
    pub fn boundary_field(&self, xr: (f64, f64), yr: (f64, f64), n: usize) -> Vec<f64> {
        let (xs, ys) = grid_points(xr, yr, n);
        match &self.margin {
            Some(m) => m.eval_batch(&[&xs, &ys], xs.len()).swap_remove(0),
            None => self.inside(&xs, &ys).into_iter().map(|v| v - 0.5).collect(),
        }
    }

    /// Indicator values (1 inside, 0 outside or undefined).
    pub fn inside(&self, xs: &[f64], ys: &[f64]) -> Vec<f64> {
        self.prog.eval_batch(&[xs, ys], xs.len()).swap_remove(0).into_iter().map(|v| if v.is_finite() && v != 0.0 { 1.0 } else { 0.0 }).collect()
    }

}

#[derive(Clone, Copy, Debug)]
pub struct Integral {
    pub value: f64,
    /// Estimated absolute error (difference between two resolutions).
    pub error: f64,
    pub area: f64,
    /// Fraction of the region where f was undefined (excluded).
    pub undefined_fraction: f64,
}

// 4-point Gauss–Legendre on [-1, 1].
const GL_X: [f64; 4] = [-0.861_136_311_594_052_6, -0.339_981_043_584_856_3, 0.339_981_043_584_856_3, 0.861_136_311_594_052_6];
const GL_W: [f64; 4] = [0.347_854_845_137_453_9, 0.652_145_154_862_546_1, 0.652_145_154_862_546_1, 0.347_854_845_137_453_9];

/// Composite tensor Gauss–Legendre with `cells x cells` cells; with a region,
/// the indicator multiplies f (so accuracy is limited by the boundary).
fn integrate_once(s: &Surface, xr: (f64, f64), yr: (f64, f64), region: Option<&Region>, cells: usize) -> (f64, f64, f64) {
    let m = cells * 4;
    let hx = (xr.1 - xr.0) / cells as f64;
    let hy = (yr.1 - yr.0) / cells as f64;
    let mut xs = Vec::with_capacity(m * m);
    let mut ys = Vec::with_capacity(m * m);
    let mut ws = Vec::with_capacity(m * m);
    for cj in 0..cells {
        for (gj, wj) in GL_X.iter().zip(GL_W) {
            let y = yr.0 + hy * (cj as f64 + 0.5 + 0.5 * gj);
            for ci in 0..cells {
                for (gi, wi) in GL_X.iter().zip(GL_W) {
                    xs.push(xr.0 + hx * (ci as f64 + 0.5 + 0.5 * gi));
                    ys.push(y);
                    ws.push(wi * wj * hx * hy * 0.25);
                }
            }
        }
    }
    let f = s.eval_f_batch(&xs, &ys);
    let ind = region.map(|r| r.inside(&xs, &ys));
    let (mut sum, mut area, mut undefined) = (0.0, 0.0, 0.0);
    for k in 0..xs.len() {
        let w = ws[k] * ind.as_ref().map_or(1.0, |i| i[k]);
        if w == 0.0 {
            continue;
        }
        area += w;
        if f[k].is_finite() {
            sum += w * f[k];
        } else {
            undefined += w;
        }
    }
    (sum, area, undefined)
}

pub fn integrate(s: &Surface, xr: (f64, f64), yr: (f64, f64), region: Option<&Region>) -> Integral {
    // Regions have a jagged indicator, so use more cells there.
    let (c1, c2) = if region.is_some() { (96, 192) } else { (24, 48) };
    let (v1, _, _) = integrate_once(s, xr, yr, region, c1);
    let (v2, area, undefined) = integrate_once(s, xr, yr, region, c2);
    Integral {
        value: v2,
        error: (v2 - v1).abs(),
        area,
        undefined_fraction: if area > 0.0 { undefined / area } else { 0.0 },
    }
}

/// Midpoint Riemann sum on an `n x n` grid of cells. Returns the sum and
/// the height of each column, row-major (`NaN`: the midpoint is outside the
/// region or f is undefined there).
pub fn riemann(s: &Surface, xr: (f64, f64), yr: (f64, f64), region: Option<&Region>, n: usize) -> (f64, Vec<f64>) {
    let n = n.max(1);
    let xs: Vec<f64> = (0..n * n).map(|k| lerp(xr, ((k % n) as f64 + 0.5) / n as f64)).collect();
    let ys: Vec<f64> = (0..n * n).map(|k| lerp(yr, ((k / n) as f64 + 0.5) / n as f64)).collect();
    let mut h = s.eval_f_batch(&xs, &ys);
    if let Some(r) = region {
        for (v, inside) in h.iter_mut().zip(r.inside(&xs, &ys)) {
            if inside == 0.0 {
                *v = f64::NAN;
            }
        }
    }
    let cell = (xr.1 - xr.0) * (yr.1 - yr.0) / (n * n) as f64;
    let sum = h.iter().filter(|v| v.is_finite()).sum::<f64>() * cell;
    for v in &mut h {
        if !v.is_finite() {
            *v = f64::NAN;
        }
    }
    (sum, h)
}

/// The inner integral of an iterated integral: `A(c) = ∫ f(c, y) dy` over
/// `range` when `fix_x`, else `∫ f(x, c) dx` (restricted to the region).
pub fn slice_integral(s: &Surface, fix_x: bool, c: f64, range: (f64, f64), region: Option<&Region>) -> f64 {
    let cells = 256;
    let h = (range.1 - range.0) / cells as f64;
    let mut us = Vec::with_capacity(cells * 4);
    let mut ws = Vec::with_capacity(cells * 4);
    for k in 0..cells {
        for (g, w) in GL_X.iter().zip(GL_W) {
            us.push(range.0 + h * (k as f64 + 0.5 + 0.5 * g));
            ws.push(w * h * 0.5);
        }
    }
    let cs = vec![c; us.len()];
    let (xs, ys) = if fix_x { (&cs, &us) } else { (&us, &cs) };
    let f = s.eval_f_batch(xs, ys);
    let ind = region.map(|r| r.inside(xs, ys));
    (0..us.len()).filter(|&k| f[k].is_finite()).map(|k| ws[k] * f[k] * ind.as_ref().map_or(1.0, |i| i[k])).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riemann_sums_and_slices() {
        let s = Surface::new("x^2 + y^2").unwrap();
        // Midpoint rule on [-1,1]^2: error 8/(3 n^2) for this f.
        for n in [4usize, 16] {
            let (sum, h) = riemann(&s, (-1.0, 1.0), (-1.0, 1.0), None, n);
            assert_eq!(h.len(), n * n);
            assert!((sum - (8.0 / 3.0 - 8.0 / (3.0 * (n * n) as f64))).abs() < 1e-12, "{n}: {sum}");
        }
        // Columns outside the region are dropped.
        let disc = Region::new("x^2 + y^2 <= 1").unwrap();
        let (_, h) = riemann(&s, (-1.0, 1.0), (-1.0, 1.0), Some(&disc), 8);
        assert!(h[0].is_nan() && h[8 * 4 + 4].is_finite());
        // A(x) = ∫_{-1}^{1} (x^2 + y^2) dy = 2x^2 + 2/3.
        assert!((slice_integral(&s, true, 0.5, (-1.0, 1.0), None) - (0.5 + 2.0 / 3.0)).abs() < 1e-12);
        // Through the disc at x = 0.6: y in [-0.8, 0.8].
        let a = slice_integral(&s, true, 0.6, (-1.0, 1.0), Some(&disc));
        assert!((a - (0.36 * 1.6 + 2.0 * 0.512 / 3.0)).abs() < 2e-3, "{a}");
    }

    #[test]
    fn contour_of_circle() {
        let n = 101;
        let s = Surface::new("x^2 + y^2").unwrap();
        let z = s.sample((-2.0, 2.0), (-2.0, 2.0), n);
        let segs = contour(&z, n, (-2.0, 2.0), (-2.0, 2.0), 1.0);
        assert!(segs.len() > 50);
        for sgt in &segs {
            for (x, y) in sgt {
                assert!((x.hypot(*y) - 1.0).abs() < 0.01);
            }
        }
        let len: f64 = segs.iter().map(|s| (s[0].0 - s[1].0).hypot(s[0].1 - s[1].1)).sum();
        assert!((len - std::f64::consts::TAU).abs() < 0.02, "{len}");
    }

    #[test]
    fn levels_are_inside() {
        let l = levels(-1.0, 1.0, 8);
        assert!(l.iter().all(|v| *v > -1.0 && *v < 1.0));
        assert!(l.len() >= 5);
    }

    #[test]
    fn finds_and_classifies_critical_points() {
        // Monkey saddle has one degenerate point; x^3 - 3x + y^2 has a
        // minimum at (1, 0) and a saddle at (-1, 0).
        let s = Surface::new("x^3 - 3x + y^2").unwrap();
        let c = critical_points(&s, (-2.0, 2.0), (-2.0, 2.0), 20);
        assert_eq!(c.len(), 2, "{c:?}");
        assert!((c[0].x + 1.0).abs() < 1e-9 && c[0].kind == CriticalKind::Saddle);
        assert!((c[1].x - 1.0).abs() < 1e-9 && c[1].kind == CriticalKind::Minimum);
        let s = Surface::new("sin(x)*sin(y)").unwrap();
        let c = critical_points(&s, (-3.0, 3.0), (-3.0, 3.0), 50);
        // (0,0) saddle, (±pi/2, ±pi/2) extrema: 5 points in [-3, 3]^2.
        assert_eq!(c.len(), 5, "{c:?}");
        assert_eq!(c.iter().filter(|c| c.kind == CriticalKind::Saddle).count(), 1);
    }

    #[test]
    fn smooth_region_boundary() {
        let disc = Region::new("x^2 + y^2 <= 1").unwrap();
        let n = 41;
        let f = disc.boundary_field((-2.0, 2.0), (-2.0, 2.0), n);
        for seg in contour(&f, n, (-2.0, 2.0), (-2.0, 2.0), 0.0) {
            for (x, y) in seg {
                assert!((x.hypot(y) - 1.0).abs() < 0.01, "{x} {y}");
            }
        }
        // Compound conditions fall back to the indicator.
        let ring = Region::new("1 <= x^2 + y^2 <= 4").unwrap();
        assert!(ring.margin.is_none());
    }

    #[test]
    fn integrals() {
        let s = Surface::new("x^2 + y^2").unwrap();
        // Over [-1,1]^2: 8/3.
        let r = integrate(&s, (-1.0, 1.0), (-1.0, 1.0), None);
        assert!((r.value - 8.0 / 3.0).abs() < 1e-12, "{r:?}");
        assert!((r.area - 4.0).abs() < 1e-10, "area {}", r.area);
        // Over the unit disc: pi/2.
        let disc = Region::new("x^2 + y^2 <= 1").unwrap();
        let r = integrate(&s, (-1.0, 1.0), (-1.0, 1.0), Some(&disc));
        assert!((r.value - std::f64::consts::FRAC_PI_2).abs() < 2e-3, "{r:?}");
        assert!((r.area - std::f64::consts::PI).abs() < 2e-3, "{r:?}");
        assert!(r.error < 5e-3);
        // Gaussian over a big square ≈ pi.
        let g = Surface::new("exp(-(x^2+y^2))").unwrap();
        let r = integrate(&g, (-6.0, 6.0), (-6.0, 6.0), None);
        assert!((r.value - std::f64::consts::PI).abs() < 1e-9, "{r:?}");
    }
}
