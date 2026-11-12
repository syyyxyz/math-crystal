//! Complex functions w = f(z): compiled f, its derivative, a holomorphy
//! check, grid sampling, zeros and poles (argument principle + Newton),
//! residues and contour integrals.
//!
//! Besides `z`, expressions may use `x = Re z` and `y = Im z`. Derivatives
//! are taken along x and y with the chain rule through z:
//! `d/dx = ∂/∂z + ∂/∂x`, `d/dy = i ∂/∂z + ∂/∂y`, and then
//! `f' = ∂f/∂z = (d/dx − i d/dy)/2`, `∂f/∂z̄ = (d/dx + i d/dy)/2`.
//! That is exact for expressions built from holomorphic operations; for
//! `conj`, `|z|`, `Re`, ... the derivatives are taken numerically.

use num_complex::Complex64 as C64;

use crate::expr::{Bin, Id, Node, Pool, Program, Un, Vars, parse};

pub const VARS: [&str; 3] = ["z", "x", "y"];
const TAU: f64 = std::f64::consts::TAU;

pub fn c(re: f64, im: f64) -> C64 {
    C64::new(re, im)
}

/// Operations that are not holomorphic in their argument (or not smooth).
fn nonholomorphic_op(pool: &Pool, id: Id) -> Option<&'static str> {
    fn go(pool: &Pool, id: Id, seen: &mut std::collections::HashSet<Id>) -> Option<&'static str> {
        if !seen.insert(id) {
            return None;
        }
        let depends = pool.depends_on(id, 0) || pool.depends_on(id, 1) || pool.depends_on(id, 2);
        match pool.node(id) {
            Node::Un(op, a) => {
                let bad = match op {
                    Un::Re => Some("Re"),
                    Un::Im => Some("Im"),
                    Un::Conj => Some("conj"),
                    Un::Abs => Some("|·|"),
                    Un::Arg => Some("arg"),
                    Un::Sign | Un::Floor | Un::Ceil | Un::Round | Un::Step | Un::Not => Some("a step function"),
                    _ => None,
                };
                if depends && bad.is_some() {
                    return bad;
                }
                go(pool, a, seen)
            }
            Node::Bin(op, a, b) => {
                let bad = match op {
                    Bin::Mod | Bin::Atan2 | Bin::Min | Bin::Max => Some("a non-smooth function"),
                    Bin::Lt | Bin::Le | Bin::Gt | Bin::Ge | Bin::Eq | Bin::Ne | Bin::And | Bin::Or => Some("a comparison"),
                    _ => None,
                };
                if depends && bad.is_some() {
                    return bad;
                }
                go(pool, a, seen).or_else(|| go(pool, b, seen))
            }
            Node::If(cnd, a, b) => {
                if depends {
                    return Some("if(…)");
                }
                go(pool, cnd, seen).or_else(|| go(pool, a, seen)).or_else(|| go(pool, b, seen))
            }
            _ => None,
        }
    }
    go(pool, id, &mut Default::default())
}

/// Local behaviour of f at one point.
#[derive(Clone, Copy, Debug)]
pub struct Local {
    pub f: C64,
    /// d f / d x and d f / d y (total derivatives along the axes).
    pub fx: C64,
    pub fy: C64,
    /// Wirtinger derivatives: f' = ∂f/∂z, and ∂f/∂z̄ (zero iff the
    /// Cauchy–Riemann equations hold).
    pub dz: C64,
    pub dzbar: C64,
    /// Exact symbolic derivatives (else difference quotients).
    pub symbolic: bool,
}

impl Local {
    /// Whether the Cauchy–Riemann equations hold here (to rounding).
    pub fn cauchy_riemann(&self) -> bool {
        let scale = self.fx.norm().max(self.fy.norm()).max(1e-300);
        let tol = if self.symbolic { 1e-9 } else { 1e-5 };
        self.dzbar.norm() <= tol * scale.max(1.0) || self.dzbar.norm() <= 1e-12
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Zero,
    Pole,
}

/// A zero or pole and its order.
#[derive(Clone, Copy, Debug)]
pub struct Special {
    pub z: C64,
    pub kind: Kind,
    pub order: i32,
    /// Winding number of f around the point: +order at a zero and −order at
    /// a pole of a holomorphic f; the sign flips for e.g. conj(z).
    pub index: i32,
    /// Residue at a pole of a holomorphic f.
    pub residue: Option<C64>,
}

#[derive(Clone, Copy, Debug)]
pub struct Contour {
    /// ∮ f dz.
    pub integral: C64,
    /// Difference to the half-resolution rule.
    pub error: f64,
    /// Winding number of f(γ) around 0 = zeros − poles inside (argument principle).
    pub winding: f64,
    /// The path passes through or next to a point where f is undefined,
    /// or f jumps along it (branch cut).
    pub singular: bool,
    pub jumps: bool,
}

pub struct CFunc {
    pub source: String,
    pool: Pool,
    f: Id,
    df: Id,
    prog: Program,
    dprog: Option<Program>,
    dxprog: Option<Program>,
    dyprog: Option<Program>,
    /// Holomorphic where defined (checked at sample points).
    pub holomorphic: bool,
    /// Why f is not holomorphic: the operation that breaks it, if any.
    pub reason: Option<&'static str>,
}

impl CFunc {
    pub fn new(source: &str) -> Result<Self, crate::expr::ParseError> {
        let mut pool = Pool::new();
        let vars = Vars(VARS.to_vec());
        let f = parse(source, &mut pool, &vars, true)?;
        let prog = Program::compile(&pool, &[f]);
        let reason = nonholomorphic_op(&pool, f);
        let (mut dprog, mut dxprog, mut dyprog, mut df) = (None, None, None, f);
        if reason.is_none() {
            let fz = pool.diff(f, 0);
            let fxp = pool.diff(f, 1);
            let fyp = pool.diff(f, 2);
            let i = pool.imag();
            let dx = pool.add(fz, fxp);
            let ifz = pool.mul(i, fz);
            let dy = pool.add(ifz, fyp);
            // f' = (dx − i dy) / 2; for holomorphic f this equals dx.
            let idy = pool.mul(i, dy);
            let s = pool.sub(dx, idy);
            let half = pool.num(0.5);
            df = pool.mul(half, s);
            dprog = Some(Program::compile(&pool, &[df]));
            dxprog = Some(Program::compile(&pool, &[dx]));
            dyprog = Some(Program::compile(&pool, &[dy]));
        }
        let mut cf = Self { source: source.to_string(), pool, f, df, prog, dprog, dxprog, dyprog, holomorphic: false, reason };
        cf.holomorphic = cf.check_holomorphic();
        Ok(cf)
    }

    fn check_holomorphic(&self) -> bool {
        let pts = [c(0.37, 0.61), c(-1.13, 0.29), c(0.83, -1.41), c(-0.52, -0.77), c(1.71, 1.23), c(-2.3, 1.9), c(2.9, -0.4)];
        let mut tested = 0;
        for z in pts {
            let l = self.local(z);
            if !(l.f.is_finite() && l.fx.is_finite() && l.fy.is_finite()) {
                continue;
            }
            tested += 1;
            if !l.cauchy_riemann() {
                return false;
            }
        }
        tested > 0
    }

    fn inputs(z: C64) -> [C64; 3] {
        [z, c(z.re, 0.0), c(z.im, 0.0)]
    }

    pub fn eval(&self, z: C64) -> C64 {
        self.prog.eval_complex(&Self::inputs(z))
    }

    /// f' where it is defined symbolically (None for non-holomorphic
    /// operations; then use [`Self::local`]).
    pub fn has_symbolic_derivative(&self) -> bool {
        self.dprog.is_some()
    }

    pub fn local(&self, z: C64) -> Local {
        let f = self.eval(z);
        if let (Some(dx), Some(dy)) = (&self.dxprog, &self.dyprog) {
            let fx = dx.eval_complex(&Self::inputs(z));
            let fy = dy.eval_complex(&Self::inputs(z));
            let i = c(0.0, 1.0);
            return Local { f, fx, fy, dz: (fx - i * fy) * 0.5, dzbar: (fx + i * fy) * 0.5, symbolic: true };
        }
        let h = 1e-6 * (1.0 + z.norm());
        let fx = (self.eval(z + h) - self.eval(z - h)) / (2.0 * h);
        let fy = (self.eval(z + c(0.0, h)) - self.eval(z - c(0.0, h))) / (2.0 * h);
        let i = c(0.0, 1.0);
        Local { f, fx, fy, dz: (fx - i * fy) * 0.5, dzbar: (fx + i * fy) * 0.5, symbolic: false }
    }

    /// The expression pool and the id of f (0) or f' (1).
    pub fn expr(&self, which: usize) -> (&Pool, Id) {
        (&self.pool, if which == 0 { self.f } else { self.df })
    }

    pub fn latex(&self, which: usize) -> String {
        self.pool.to_latex(if which == 0 { self.f } else { self.df }, &VARS)
    }

    pub fn text(&self, which: usize) -> String {
        self.pool.to_string(if which == 0 { self.f } else { self.df }, &VARS)
    }

    /// Evaluate at many points at once.
    pub fn eval_many(&self, zs: &[C64]) -> Vec<C64> {
        let xs: Vec<C64> = zs.iter().map(|z| c(z.re, 0.0)).collect();
        let ys: Vec<C64> = zs.iter().map(|z| c(z.im, 0.0)).collect();
        self.prog.eval_complex_batch(&[zs, &xs, &ys], zs.len())
    }

    /// Row-major samples over a rectangle; row 0 is the top (largest Im).
    pub fn sample(&self, re: (f64, f64), im: (f64, f64), w: usize, h: usize) -> Vec<C64> {
        let mut zs = Vec::with_capacity(w * h);
        for j in 0..h {
            let y = im.1 + (im.0 - im.1) * (j as f64 + 0.5) / h as f64;
            for i in 0..w {
                let x = re.0 + (re.1 - re.0) * (i as f64 + 0.5) / w as f64;
                zs.push(c(x, y));
            }
        }
        self.eval_many(&zs)
    }

    /// Winding number of f around 0 along a closed polygon of samples;
    /// None if f is undefined somewhere on it or the phase jumps.
    fn winding(ws: &[C64]) -> Option<f64> {
        let mut total = 0.0;
        for k in 0..ws.len() {
            let (a, b) = (ws[k], ws[(k + 1) % ws.len()]);
            if !(a.is_finite() && b.is_finite()) || a.norm() == 0.0 || b.norm() == 0.0 {
                return None;
            }
            let d = (b / a).arg();
            if d.abs() > 2.2 {
                return None;
            }
            total += d;
        }
        Some(total / TAU)
    }

    /// Phase change of f from `za` to `zb`, subdividing where it turns fast
    /// (near a zero or pole close to the segment). None if f is undefined
    /// or zero on the way, or jumps (branch cut).
    fn seg_winding(&self, za: C64, zb: C64, wa: C64, wb: C64, depth: u32) -> Option<f64> {
        if !(wa.is_finite() && wb.is_finite()) || wa.norm() == 0.0 || wb.norm() == 0.0 {
            return None;
        }
        let d = (wb / wa).arg();
        if d.abs() <= 0.8 {
            return Some(d);
        }
        if depth == 0 {
            return if d.abs() > 2.2 { None } else { Some(d) };
        }
        let zm = (za + zb) * 0.5;
        let wm = self.eval(zm);
        Some(self.seg_winding(za, zm, wa, wm, depth - 1)? + self.seg_winding(zm, zb, wm, wb, depth - 1)?)
    }

    fn circle(&self, center: C64, r: f64, n: usize) -> Vec<C64> {
        let zs: Vec<C64> = (0..n).map(|k| center + C64::from_polar(r, TAU * k as f64 / n as f64)).collect();
        self.eval_many(&zs)
    }

    /// 2D Newton on g = f or 1/f (numeric Jacobian, works for any f),
    /// with step multiplied by the multiplicity `m`.
    fn newton(&self, start: C64, pole: bool, m: f64, scale: f64) -> C64 {
        let g = |z: C64| {
            let w = self.eval(z);
            if pole { w.inv() } else { w }
        };
        let mut z = start;
        let mut gz = g(z);
        for _ in 0..80 {
            if !gz.is_finite() {
                break;
            }
            let h = 1e-7 * scale.max(1e-12);
            let gx = (g(z + h) - g(z - h)) / (2.0 * h);
            let gy = (g(z + c(0.0, h)) - g(z - c(0.0, h))) / (2.0 * h);
            // Solve [gx gy] [dx dy]^T = -g in real 2x2 form.
            let (a, b, cc, d) = (gx.re, gy.re, gx.im, gy.im);
            let det = a * d - b * cc;
            if det.abs() < 1e-300 || !det.is_finite() {
                break;
            }
            let dx = (-gz.re * d + gz.im * b) / det;
            let dy = (-a * gz.im + cc * gz.re) / det;
            let mut step = c(dx, dy) * m;
            if step.norm() > scale {
                step *= scale / step.norm();
            }
            let mut tried = 0;
            loop {
                let zn = z + step;
                let gn = g(zn);
                if gn.is_finite() && (gn.norm() < gz.norm() || tried > 6) {
                    z = zn;
                    gz = gn;
                    break;
                }
                step *= 0.5;
                tried += 1;
            }
            if step.norm() < 1e-15 * (1.0 + z.norm()) || gz.norm() < 1e-15 {
                break;
            }
        }
        z
    }

    /// Zeros and poles in a rectangle, found cell by cell with the argument
    /// principle and refined by Newton's method.
    pub fn zeros_poles(&self, re: (f64, f64), im: (f64, f64)) -> Vec<Special> {
        let (wr, hr) = (re.1 - re.0, im.1 - im.0);
        if !(wr > 0.0 && hr > 0.0) {
            return vec![];
        }
        let cells_x = 40usize;
        let cell = wr / cells_x as f64;
        let cells_y = ((hr / cell).round() as usize).clamp(4, 80);
        let cell_y = hr / cells_y as f64;
        let k = 6usize; // samples per cell edge
        let mut found: Vec<Special> = Vec::new();
        // Two lattices shifted by about half a cell: a point close to a cell
        // edge in one (where the phase turns too fast to count) lies well
        // inside a cell of the other.
        for (fx, fy) in [(0.137, 0.291)] {
            let (ox, oy) = (re.0 - fx * cell, im.0 - fy * cell_y);
            let (cx, cy) = (cells_x + 1, cells_y + 1);
            let (nx, ny) = (cx * k + 1, cy * k + 1);
            let mut zs = Vec::with_capacity(nx * ny);
            for j in 0..ny {
                for i in 0..nx {
                    zs.push(c(ox + cell * i as f64 / k as f64, oy + cell_y * j as f64 / k as f64));
                }
            }
            let ws = self.eval_many(&zs);
            let at = |i: usize, j: usize| ws[j * nx + i];
            for cj in 0..cy {
                for ci in 0..cx {
                    let (i0, j0) = (ci * k, cj * k);
                    // Corners of the ring of lattice points around the cell.
                    let mut ring: Vec<(usize, usize)> = Vec::with_capacity(4 * k);
                    for s in 0..k {
                        ring.push((i0 + s, j0));
                    }
                    for s in 0..k {
                        ring.push((i0 + k, j0 + s));
                    }
                    for s in 0..k {
                        ring.push((i0 + k - s, j0 + k));
                    }
                    for s in 0..k {
                        ring.push((i0, j0 + k - s));
                    }
                    let mut total = 0.0;
                    let mut ok = true;
                    for q in 0..ring.len() {
                        let (a, b) = (ring[q], ring[(q + 1) % ring.len()]);
                        match self.seg_winding(zs[a.1 * nx + a.0], zs[b.1 * nx + b.0], at(a.0, a.1), at(b.0, b.1), 14) {
                            Some(d) => total += d,
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if !ok {
                        continue;
                    }
                    let wn = total / TAU;
                    let wn = wn.round() as i32;
                    if wn == 0 {
                        continue;
                    }
                    let center = c(ox + cell * (ci as f64 + 0.5), oy + cell_y * (cj as f64 + 0.5));
                    // Zero or pole? Whichever Newton iteration (on f or on 1/f)
                    // converges. The winding sign alone is not enough: conj(z)
                    // has a zero of index −1.
                    let mut mags: Vec<f64> = ring.iter().map(|&(i, j)| at(i, j).norm()).collect();
                    mags.sort_by(f64::total_cmp);
                    let med = mags[mags.len() / 2].max(1e-300);
                    let m = wn.unsigned_abs() as f64;
                    let near_cell = |z: C64| (z - center).norm() < 1.5 * cell.max(cell_y);
                    let attempt = |pole: bool| -> Option<C64> {
                        let z = self.newton(center, pole, m, cell);
                        let w = self.eval(z);
                        let ok = if pole { !w.is_finite() || w.norm() > 1e6 * med } else { w.norm() < 1e-6 * med };
                        (ok && near_cell(z)).then_some(z)
                    };
                    let order_first = if wn > 0 { [false, true] } else { [true, false] };
                    let Some((z, pole)) = order_first.iter().find_map(|&pole| attempt(pole).map(|z| (z, pole))) else { continue };
                    let inside = z.re >= re.0 && z.re <= re.1 && z.im >= im.0 && z.im <= im.1;
                    if !inside || found.iter().any(|s| (s.z - z).norm() < 0.5 * cell.min(cell_y)) {
                        continue;
                    }
                    // Index (signed winding) on a small circle around the point.
                    let index = Self::winding(&self.circle(z, 0.4 * cell.min(cell_y), 96)).map_or(wn, |w| w.round() as i32);
                    let index = if index == 0 { wn } else { index };
                    found.push(Special { z, kind: if pole { Kind::Pole } else { Kind::Zero }, order: index.abs(), index, residue: None });
                    if found.len() >= 60 {
                        break;
                    }
                }
            }
        }
        // Residues of holomorphic f on circles that hold no other point.
        if self.holomorphic {
            let pts: Vec<C64> = found.iter().map(|s| s.z).collect();
            for s in found.iter_mut().filter(|s| s.kind == Kind::Pole) {
                let nearest = pts.iter().filter(|p| (**p - s.z).norm() > 1e-12).map(|p| (p - s.z).norm()).fold(f64::INFINITY, f64::min);
                let r = (0.3 * cell).min(0.4 * nearest);
                let n = 256;
                let ws = self.circle(s.z, r, n);
                // (1/2πi) ∮ f dz with dz = i r e^{iθ} dθ.
                let sum: C64 = ws.iter().enumerate().map(|(k, w)| w * C64::from_polar(1.0, TAU * k as f64 / n as f64)).sum();
                let res = sum * (r / n as f64);
                s.residue = Some(clean(res));
            }
        }
        found.sort_by(|a, b| (a.kind as u8, a.z.re, a.z.im).partial_cmp(&(b.kind as u8, b.z.re, b.z.im)).unwrap_or(std::cmp::Ordering::Equal));
        found
    }

    /// ∮ f dz over the circle |z − center| = r (counter-clockwise),
    /// trapezoid rule, plus the argument-principle winding number.
    pub fn circle_integral(&self, center: C64, r: f64) -> Contour {
        let n = 4096;
        let ws = self.circle(center, r, n);
        let rule = |step: usize| -> C64 {
            let m = n / step;
            let sum: C64 = (0..m).map(|k| ws[k * step] * C64::from_polar(1.0, TAU * (k * step) as f64 / n as f64)).sum();
            sum * c(0.0, TAU * r / m as f64)
        };
        let integral = rule(1);
        let error = (integral - rule(2)).norm();
        let singular = ws.iter().any(|w| !w.is_finite());
        let mut total = 0.0;
        let mut jumps = false;
        for k in 0..n {
            let (a, b) = (ws[k], ws[(k + 1) % n]);
            let d = (b / a).arg();
            if d.abs() > 1.5 {
                jumps = true;
            }
            total += d;
        }
        Contour { integral: clean(integral), error, winding: total / TAU, singular, jumps }
    }
}

/// Drop rounding noise in one component relative to the other.
pub fn clean(w: C64) -> C64 {
    let m = w.norm();
    let fix = |v: f64| if v.abs() < 1e-11 * m.max(1e-300) || v.abs() < 1e-14 { 0.0 } else { v };
    c(fix(w.re), fix(w.im))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: C64, b: C64, tol: f64) -> bool {
        (a - b).norm() < tol
    }

    #[test]
    fn derivative_and_holomorphy() {
        let f = CFunc::new("z^3 + exp(i z)").unwrap();
        assert!(f.holomorphic);
        let z = c(0.4, -0.3);
        let l = f.local(z);
        let exact = z * z * 3.0 + c(0.0, 1.0) * (c(0.0, 1.0) * z).exp();
        assert!(near(l.dz, exact, 1e-12), "{:?}", l.dz);
        assert!(l.dzbar.norm() < 1e-12);
        // x and y are allowed: x + i y is just z.
        let g = CFunc::new("(x + i y)^2").unwrap();
        assert!(g.holomorphic);
        assert!(near(g.local(z).dz, z * 2.0, 1e-12));
        // x^2 + y^2 is not holomorphic; neither is conj.
        let h = CFunc::new("x^2 + y^2").unwrap();
        assert!(!h.holomorphic);
        assert!(near(h.local(z).dzbar, z, 1e-12), "∂/∂z̄ (z z̄) = z");
        let k = CFunc::new("conj(z)").unwrap();
        assert!(!k.holomorphic && k.reason == Some("conj"));
        assert!(near(k.local(z).dzbar, c(1.0, 0.0), 1e-6));
        // Cauchy-Riemann: f = z^2 → u = x^2 - y^2, v = 2xy.
        let l = CFunc::new("z^2").unwrap().local(c(1.0, 2.0));
        assert!(near(l.fx, c(2.0, 4.0), 1e-12) && near(l.fy, c(-4.0, 2.0), 1e-12));
    }

    #[test]
    fn zeros_poles_residues() {
        // Zeros 1, -1 and a double zero at 2+i; poles at the roots of z^2 = -2-2i.
        let f = CFunc::new("(z^2 - 1)(z - 2 - i)^2 / (z^2 + 2 + 2i)").unwrap();
        let s = f.zeros_poles((-3.0, 3.0), (-3.0, 3.0));
        let zeros: Vec<_> = s.iter().filter(|s| s.kind == Kind::Zero).collect();
        let poles: Vec<_> = s.iter().filter(|s| s.kind == Kind::Pole).collect();
        assert_eq!(zeros.len(), 3, "{s:?}");
        assert_eq!(poles.len(), 2, "{s:?}");
        let dbl = zeros.iter().find(|s| near(s.z, c(2.0, 1.0), 1e-6)).expect("double zero");
        assert_eq!(dbl.order, 2);
        assert!(zeros.iter().any(|s| near(s.z, c(1.0, 0.0), 1e-9) && s.order == 1));
        // Residue of 1/(1+z^2) at i is -i/2.
        let g = CFunc::new("1/(1 + z^2)").unwrap();
        let s = g.zeros_poles((-2.0, 2.0), (-2.0, 2.0));
        let p = s.iter().find(|s| near(s.z, c(0.0, 1.0), 1e-9)).unwrap_or_else(|| panic!("pole at i: {s:?}"));
        assert_eq!((p.kind, p.order), (Kind::Pole, 1));
        assert!(near(p.residue.unwrap(), c(0.0, -0.5), 1e-9), "{:?}", p.residue);
        // Double pole: 1/z^2 has residue 0; e^z/z^3 has residue 1/2.
        let h = CFunc::new("exp(z)/z^3").unwrap();
        let s = h.zeros_poles((-1.0, 1.0), (-1.0, 1.0));
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].order, 3);
        assert!(near(s[0].residue.unwrap(), c(0.5, 0.0), 1e-9), "{:?}", s[0].residue);
    }

    #[test]
    fn zeros_poles_many_domains() {
        let count = |src: &str, re: (f64, f64), im: (f64, f64)| {
            let s = CFunc::new(src).unwrap().zeros_poles(re, im);
            let z: i32 = s.iter().filter(|s| s.kind == Kind::Zero).map(|s| s.order).sum();
            let p: i32 = s.iter().filter(|s| s.kind == Kind::Pole).map(|s| s.order).sum();
            (z, p)
        };
        for h in [1.3, 1.5, 2.0, 2.5, 3.0, 4.0, 7.3] {
            assert_eq!(count("z^5 - 1", (-h, h), (-h, h)), (5, 0), "h = {h}");
            assert_eq!(count("z^3 / (z^2 + 1)^2", (-h * 1.4, h * 1.4), (-h, h)), (3, 4), "h = {h}");
        }
        assert_eq!(count("sin(z)", (-7.0, 7.0), (-1.0, 1.0)), (5, 0));
        assert_eq!(count("tan(z)", (-5.0, 5.0), (-2.0, 2.0)), (3, 4));
        assert_eq!(count("exp(z)", (-3.0, 3.0), (-3.0, 3.0)), (0, 0));
    }

    #[test]
    fn contour_integrals() {
        let g = CFunc::new("1/(1 + z^2)").unwrap();
        // Around i only: 2πi · (-i/2) = π.
        let r = g.circle_integral(c(0.0, 1.0), 1.0);
        assert!(near(r.integral, c(std::f64::consts::PI, 0.0), 1e-10), "{:?}", r.integral);
        assert!((r.winding + 1.0).abs() < 1e-9);
        // Around both poles: residues cancel.
        let r = g.circle_integral(c(0.0, 0.0), 3.0);
        assert!(r.integral.norm() < 1e-10);
        assert!((r.winding + 2.0).abs() < 1e-9);
        // Branch cut of log crossed by the circle.
        let l = CFunc::new("ln(z)").unwrap();
        assert!(l.circle_integral(c(0.0, 0.0), 1.0).jumps);
        // conj(z) has a zero (not a pole) of index −1.
        let k = CFunc::new("conj(z)").unwrap();
        let s = k.zeros_poles((-2.0, 2.0), (-2.0, 2.0));
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!((s[0].kind, s[0].index), (Kind::Zero, -1));
        assert!(s[0].z.norm() < 1e-9);
        // Non-holomorphic: ∮ conj(z) dz = 2πi r^2.
        let k = CFunc::new("conj(z)").unwrap();
        let r = k.circle_integral(c(0.0, 0.0), 2.0);
        assert!(near(r.integral, c(0.0, TAU * 4.0), 1e-9), "{:?}", r.integral);
    }
}
