//! Two-variable calculus: compiled f(x, y) with exact symbolic partials,
//! robust point derivatives and grid sampling.

use crate::expr::{Id, Pool, Program, Vars, parse};

pub const VARS: [&str; 2] = ["x", "y"];

/// Outputs of [`Surface::prog`], in order.
const F: usize = 0;
const FX: usize = 1;
const FY: usize = 2;
const FXX: usize = 3;
const FYY: usize = 4;
const FXY: usize = 5; // d/dy (f_x)
const FYX: usize = 6; // d/dx (f_y)

pub struct Surface {
    pub source: String,
    pool: Pool,
    ids: [Id; 7],
    /// All seven outputs, sharing sub-expressions.
    prog: Program,
    /// Only f, for fast grid sampling.
    f_prog: Program,
}

/// Where a derivative value came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// Exact symbolic value, confirmed by a difference quotient.
    Exact,
    /// The symbolic formula disagrees with (or is undefined at) the point but
    /// the limit definition converges: e.g. at the special point of a
    /// piecewise definition like `if(x^2+y^2 == 0, 0, ...)`.
    Limit,
    Undefined,
}

#[derive(Clone, Copy, Debug)]
pub struct Value {
    pub v: f64,
    pub src: Source,
}

impl Value {
    const NAN: Value = Value {
        v: f64::NAN,
        src: Source::Undefined,
    };
    pub fn ok(&self) -> bool {
        self.v.is_finite()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PointDerivs {
    pub f: f64,
    pub fx: Value,
    pub fy: Value,
    pub fxx: Value,
    pub fyy: Value,
    /// (f_x)_y: differentiate in x first, then y.
    pub fxy: Value,
    /// (f_y)_x.
    pub fyx: Value,
}

impl PointDerivs {
    pub fn first_ok(&self) -> bool {
        self.f.is_finite() && self.fx.ok() && self.fy.ok()
    }
    pub fn second_ok(&self) -> bool {
        self.first_ok() && self.fxx.ok() && self.fyy.ok() && self.fxy.ok() && self.fyx.ok()
    }
    /// Whether the two mixed partials agree (Schwarz/Clairaut).
    pub fn mixed_equal(&self) -> Option<bool> {
        if !(self.fxy.ok() && self.fyx.ok()) {
            return None;
        }
        let scale = self.fxy.v.abs().max(self.fyx.v.abs()).max(1.0);
        Some((self.fxy.v - self.fyx.v).abs() <= 1e-4 * scale)
    }
}

impl Surface {
    pub fn new(source: &str) -> Result<Self, crate::expr::ParseError> {
        let mut pool = Pool::new();
        let vars = Vars(VARS.to_vec());
        let f = parse(source, &mut pool, &vars, false)?;
        let fx = pool.diff(f, 0);
        let fy = pool.diff(f, 1);
        let fxx = pool.diff(fx, 0);
        let fyy = pool.diff(fy, 1);
        let fxy = pool.diff(fx, 1);
        let fyx = pool.diff(fy, 0);
        let ids = [f, fx, fy, fxx, fyy, fxy, fyx];
        let prog = Program::compile(&pool, &ids);
        let f_prog = Program::compile(&pool, &[f]);
        Ok(Self {
            source: source.to_string(),
            pool,
            ids,
            prog,
            f_prog,
        })
    }

    pub fn f(&self, x: f64, y: f64) -> f64 {
        self.f_prog.eval(&[x, y])[0]
    }

    /// Text of a symbolic derivative: 1 = f_x, 2 = f_y, 3 = f_xx, 4 = f_yy,
    /// 5 = f_xy, 6 = f_yx.
    pub fn formula(&self, which: usize) -> String {
        self.pool.to_string(self.ids[which], &VARS)
    }

    pub fn formula_len(&self, which: usize) -> usize {
        Program::compile(&self.pool, &[self.ids[which]]).len()
    }

    fn all(&self, x: f64, y: f64) -> Vec<f64> {
        self.prog.eval(&[x, y])
    }

    /// Point derivatives with cross-checking (see [`Source`]).
    pub fn derivs(&self, a: f64, b: f64) -> PointDerivs {
        let s0 = self.all(a, b);
        let f = s0[F];
        if !f.is_finite() {
            return PointDerivs {
                f,
                fx: Value::NAN,
                fy: Value::NAN,
                fxx: Value::NAN,
                fyy: Value::NAN,
                fxy: Value::NAN,
                fyx: Value::NAN,
            };
        }
        let hx = 1e-5 * (1.0 + a.abs());
        let hy = 1e-5 * (1.0 + b.abs());
        let fval = |x: f64, y: f64| self.f(x, y);
        let fx = pick(s0[FX], diff_quotient(|t| fval(a + t, b), hx));
        let fy = pick(s0[FY], diff_quotient(|t| fval(a, b + t), hy));

        // Second derivatives: difference quotients of the first-derivative
        // *functions* near (but off) the point.
        let kx = 1e-4 * (1.0 + a.abs());
        let ky = 1e-4 * (1.0 + b.abs());
        let fx_at = |x: f64, y: f64| self.first_at(x, y, FX);
        let fy_at = |x: f64, y: f64| self.first_at(x, y, FY);
        let fxx = pick(s0[FXX], richardson(|t| fx_at(a + t, b), kx, fx.v));
        let fyy = pick(s0[FYY], richardson(|t| fy_at(a, b + t), ky, fy.v));
        let fxy = pick(s0[FXY], richardson(|t| fx_at(a, b + t), ky, fx.v));
        let fyx = pick(s0[FYX], richardson(|t| fy_at(a + t, b), kx, fy.v));
        PointDerivs {
            f,
            fx,
            fy,
            fxx,
            fyy,
            fxy,
            fyx,
        }
    }

    /// First partial at an off-point location: symbolic, or a central
    /// difference of f when the formula is undefined there.
    fn first_at(&self, x: f64, y: f64, which: usize) -> f64 {
        let v = self.all(x, y)[which];
        if v.is_finite() {
            return v;
        }
        let h = 1e-6 * (1.0 + x.abs().max(y.abs()));
        if which == FX {
            diff_quotient(|t| self.f(x + t, y), h)
        } else {
            diff_quotient(|t| self.f(x, y + t), h)
        }
    }

    /// Sample f on an `n x n` grid; returns row-major `z[j * n + i]` with
    /// x = x0 + i dx and y = y0 + j dy. Non-finite values become NaN.
    pub fn sample(&self, x: (f64, f64), y: (f64, f64), n: usize) -> Vec<f64> {
        let xs: Vec<f64> = (0..n * n).map(|k| lerp(x, (k % n) as f64 / (n - 1) as f64)).collect();
        let ys: Vec<f64> = (0..n * n).map(|k| lerp(y, (k / n) as f64 / (n - 1) as f64)).collect();
        let mut z = self.f_prog.eval_batch(&[&xs, &ys], n * n).swap_remove(0);
        for v in &mut z {
            if !v.is_finite() || v.abs() > 1e30 {
                *v = f64::NAN;
            }
        }
        z
    }
}

pub fn lerp(r: (f64, f64), t: f64) -> f64 {
    r.0 + (r.1 - r.0) * t
}

/// Central difference of g at 0, falling back to one-sided second-order
/// formulas at the edge of the domain.
fn diff_quotient(g: impl Fn(f64) -> f64, h: f64) -> f64 {
    let (p, m) = (g(h), g(-h));
    if p.is_finite() && m.is_finite() {
        return (p - m) / (2.0 * h);
    }
    let g0 = g(0.0);
    if p.is_finite() {
        let p2 = g(2.0 * h);
        if p2.is_finite() {
            return (-3.0 * g0 + 4.0 * p - p2) / (2.0 * h);
        }
        return (p - g0) / h;
    }
    if m.is_finite() {
        let m2 = g(-2.0 * h);
        if m2.is_finite() {
            return (3.0 * g0 - 4.0 * m + m2) / (2.0 * h);
        }
        return (g0 - m) / h;
    }
    f64::NAN
}

/// Derivative at 0 of g (a first-derivative function) by Richardson
/// extrapolation of central differences; `g0` is the robust value g(0),
/// used for one-sided limits at a domain edge.
fn richardson(g: impl Fn(f64) -> f64, k: f64, g0: f64) -> f64 {
    let central = |h: f64| {
        let (p, m) = (g(h), g(-h));
        if p.is_finite() && m.is_finite() {
            Some((p - m) / (2.0 * h))
        } else {
            None
        }
    };
    if let (Some(d1), Some(d2)) = (central(k), central(k / 2.0)) {
        return (4.0 * d2 - d1) / 3.0;
    }
    if !g0.is_finite() {
        return f64::NAN;
    }
    // One-sided: (g(h) - g0)/h has O(h) error; extrapolate h, h/2.
    for s in [1.0, -1.0] {
        let d = |h: f64| (g(s * h) - g0) / (s * h);
        let (d1, d2) = (d(k), d(k / 2.0));
        if d1.is_finite() && d2.is_finite() {
            return 2.0 * d2 - d1;
        }
    }
    f64::NAN
}

fn pick(symbolic: f64, numeric: f64) -> Value {
    match (symbolic.is_finite(), numeric.is_finite()) {
        (true, true) => {
            let tol = 1e-4 * (1.0 + symbolic.abs().max(numeric.abs()));
            if (symbolic - numeric).abs() <= tol {
                Value {
                    v: symbolic,
                    src: Source::Exact,
                }
            } else {
                Value {
                    v: numeric,
                    src: Source::Limit,
                }
            }
        }
        (true, false) => Value {
            v: symbolic,
            src: Source::Exact,
        },
        (false, true) => Value {
            v: numeric,
            src: Source::Limit,
        },
        (false, false) => Value::NAN,
    }
}

/// Quadratic Taylor model around (a, b).
#[derive(Clone, Copy, Debug)]
pub struct Quadratic {
    pub a: f64,
    pub b: f64,
    pub f: f64,
    pub fx: f64,
    pub fy: f64,
    pub fxx: f64,
    pub fyy: f64,
    pub fxy: f64,
}

impl Quadratic {
    pub fn eval(&self, x: f64, y: f64) -> f64 {
        let (dx, dy) = (x - self.a, y - self.b);
        self.f
            + self.fx * dx
            + self.fy * dy
            + 0.5 * self.fxx * dx * dx
            + self.fxy * dx * dy
            + 0.5 * self.fyy * dy * dy
    }

    pub fn linear(&self, x: f64, y: f64) -> f64 {
        self.f + self.fx * (x - self.a) + self.fy * (y - self.b)
    }
}

/// Classify a critical point from the Hessian.
pub fn classify(d: &PointDerivs) -> Option<&'static str> {
    if !d.second_ok() {
        return None;
    }
    let g = d.fx.v.hypot(d.fy.v);
    if g > 1e-6 * (1.0 + d.f.abs()) {
        return None;
    }
    let fxy = 0.5 * (d.fxy.v + d.fyx.v);
    let det = d.fxx.v * d.fyy.v - fxy * fxy;
    let eps = 1e-9 * (1.0 + d.fxx.v.abs() + d.fyy.v.abs()).powi(2);
    Some(if det > eps {
        if d.fxx.v > 0.0 { "local minimum" } else { "local maximum" }
    } else if det < -eps {
        "saddle point"
    } else {
        "degenerate (test inconclusive)"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6 * (1.0 + b.abs())
    }

    #[test]
    fn smooth_function_is_exact() {
        let s = Surface::new("sin(x)*cos(y) + x^2*y").unwrap();
        let d = s.derivs(0.3, -0.2);
        assert_eq!(d.fx.src, Source::Exact);
        assert!(near(d.fx.v, 0.3f64.cos() * 0.2f64.cos() + 2.0 * 0.3 * -0.2));
        assert!(near(d.fxy.v, -(0.3f64.cos()) * (-0.2f64).sin() + 0.6));
        assert_eq!(d.mixed_equal(), Some(true));
    }

    /// The classic f = xy(x^2-y^2)/(x^2+y^2), f(0,0) = 0: f_xy(0,0) = -1
    /// and f_yx(0,0) = 1. Branch-wise symbolic differentiation alone would
    /// say 0 for both.
    #[test]
    fn classic_unequal_mixed_partials() {
        let s = Surface::new("if(x^2 + y^2 == 0, 0, x*y*(x^2 - y^2)/(x^2 + y^2))").unwrap();
        let d = s.derivs(0.0, 0.0);
        assert!(near(d.f, 0.0));
        assert!(near(d.fx.v, 0.0) && near(d.fy.v, 0.0));
        assert!(near(d.fxy.v, -1.0), "fxy {:?}", d.fxy);
        assert!(near(d.fyx.v, 1.0), "fyx {:?}", d.fyx);
        assert_eq!(d.fxy.src, Source::Limit);
        assert_eq!(d.mixed_equal(), Some(false));
        // Away from the origin the formula is exact again.
        let d = s.derivs(0.5, 0.25);
        assert_eq!(d.fxy.src, Source::Exact);
        assert_eq!(d.mixed_equal(), Some(true));
    }

    #[test]
    fn domain_edge_uses_one_sided_limits() {
        let s = Surface::new("sqrt(x)").unwrap();
        let d = s.derivs(1.0, 0.0);
        assert!(near(d.fx.v, 0.5));
        let d = s.derivs(0.0, 0.0);
        assert!(!d.fx.ok() || d.fx.v > 1e2, "{:?}", d.fx);
        let s = Surface::new("|x| + y^2").unwrap();
        let d = s.derivs(0.5, 1.0);
        assert!(near(d.fx.v, 1.0) && near(d.fyy.v, 2.0));
    }

    #[test]
    fn critical_points() {
        let cl = |src: &str| classify(&Surface::new(src).unwrap().derivs(0.0, 0.0));
        assert_eq!(cl("x^2 + y^2"), Some("local minimum"));
        assert_eq!(cl("-x^2 - 3y^2"), Some("local maximum"));
        assert_eq!(cl("x^2 - y^2"), Some("saddle point"));
        assert_eq!(cl("x^4 + y^4"), Some("degenerate (test inconclusive)"));
        assert_eq!(cl("x + y"), None);
    }

    #[test]
    fn sampling_layout() {
        let s = Surface::new("x + 10y").unwrap();
        let z = s.sample((0.0, 1.0), (0.0, 2.0), 3);
        assert_eq!(z, vec![0.0, 0.5, 1.0, 10.0, 10.5, 11.0, 20.0, 20.5, 21.0]);
        let z = Surface::new("ln(x)").unwrap().sample((-1.0, 1.0), (0.0, 1.0), 3);
        assert!(z[0].is_nan() && z[1].is_nan() && z[2] == 0.0);
    }
}
