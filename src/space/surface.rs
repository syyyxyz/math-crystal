//! Implicit surfaces F(x, y, z) = 0: parsing, batched evaluation with the
//! symbolic gradient, and recognising planes and quadrics.

use glam::DVec3;

use super::quadric::{self, Quadric};
use crate::expr::{Id, ParseError, Pool, Program, Vars};
use crate::i18n::tr;

#[derive(Clone, Debug)]
pub enum SurfaceKind {
    /// `n · p = d`.
    Plane { n: DVec3, d: f64 },
    Quadric(Box<Quadric>),
    /// Anything else (or not decidable at the sample points).
    General,
    /// F does not depend on the point.
    Constant,
}

#[derive(Clone)]
pub struct Implicit {
    pub source: String,
    pool: Pool,
    f: Id,
    value: Program,
    with_grad: Program,
    second: Program,
    pub params: Vec<String>,
    pub kind: SurfaceKind,
}

pub fn xyz() -> Vars {
    Vars(vec!["x", "y", "z"])
}

/// LaTeX to plain syntax (if it looks like LaTeX), without `$`.
pub fn plain(src: &str) -> Result<String, ParseError> {
    let s = src.trim();
    if crate::expr::latex::looks_like_latex(s) {
        // The translation writes `=` as the comparison `==`.
        crate::expr::latex::to_plain(s).map(|t| t.replace("==", "="))
    } else {
        Ok(s.to_string())
    }
}

/// Byte offsets of the single `=` signs (not `<=`, `>=`, `==`, `!=`).
pub fn equals_signs(src: &str) -> Vec<usize> {
    let b = src.as_bytes();
    (0..b.len())
        .filter(|&i| {
            b[i] == b'=' && !matches!(i.checked_sub(1).map(|k| b[k]), Some(b'<' | b'>' | b'!' | b'=')) && b.get(i + 1) != Some(&b'=')
        })
        .collect()
}

/// Split at top-level commas or semicolons (outside brackets).
pub fn split_top(src: &str) -> Vec<&str> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in src.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' | ';' if depth == 0 => {
                out.push(&src[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&src[start..]);
    out
}

/// Remove brackets that enclose the whole string.
pub fn strip_outer(src: &str) -> &str {
    let s = src.trim();
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'(' && b[b.len() - 1] == b')' || b[0] == b'[' && b[b.len() - 1] == b']') {
        let mut depth = 0;
        for (i, c) in s.char_indices() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth == 0 && i != s.len() - 1 {
                        return s;
                    }
                }
                _ => {}
            }
        }
        return &s[1..s.len() - 1];
    }
    s
}

fn err(msg: &str) -> ParseError {
    ParseError { msg: msg.to_string(), pos: usize::MAX }
}

/// Parse `lhs = rhs` (or a bare expression, meaning `= 0`) as lhs − rhs.
pub fn parse_equation(src: &str, pool: &mut Pool, vars: &Vars, params: &mut Vec<String>) -> Result<Id, ParseError> {
    let s = plain(src)?;
    let eq = equals_signs(&s);
    if s.contains('<') || s.contains('>') {
        return Err(err(tr(
            "Inequalities are not surfaces: use a solid for a ball, cylinder, cone or box",
            "不等式不是曲面：球体、圆柱体、圆锥体、长方体请用「立体」",
        )));
    }
    match eq.len() {
        0 => crate::expr::parse_with_params(&s, pool, vars, false, params),
        1 => {
            let (l, r) = (&s[..eq[0]], &s[eq[0] + 1..]);
            if l.trim().is_empty() || r.trim().is_empty() {
                return Err(err(tr("One side of the equation is empty", "等式的一边是空的")));
            }
            let a = crate::expr::parse_with_params(l, pool, vars, false, params)?;
            let b = crate::expr::parse_with_params(r, pool, vars, false, params)?;
            Ok(pool.sub(a, b))
        }
        _ => Err(err(tr("A surface is one equation (only one '=')", "曲面只需一个方程（只能有一个 '='）"))),
    }
}

impl Implicit {
    pub fn new(src: &str) -> Result<Self, ParseError> {
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let f = parse_equation(src, &mut pool, &xyz(), &mut params)?;
        let grad = [0, 1, 2].map(|v| pool.diff(f, v));
        let mut second = vec![f, grad[0], grad[1], grad[2]];
        for (i, j) in [(0, 0), (1, 1), (2, 2), (0, 1), (0, 2), (1, 2)] {
            second.push(pool.diff(grad[i], j));
        }
        let value = Program::compile(&pool, &[f]);
        let with_grad = Program::compile(&pool, &[f, grad[0], grad[1], grad[2]]);
        let second = Program::compile(&pool, &second);
        Ok(Self { source: src.trim().to_string(), pool, f, value, with_grad, second, params, kind: SurfaceKind::General })
    }

    pub fn expr(&self) -> (&Pool, Id) {
        (&self.pool, self.f)
    }

    /// Set parameter values by name (missing names keep NaN).
    pub fn set_params(&mut self, named: &[(String, f64)]) {
        let v: Vec<f64> = self.params.iter().map(|p| named.iter().find(|(n, _)| n == p).map_or(f64::NAN, |(_, v)| *v)).collect();
        for p in [&mut self.value, &mut self.with_grad, &mut self.second] {
            p.set_params(&v);
        }
    }

    pub fn eval(&self, p: DVec3) -> f64 {
        self.value.eval(&[p.x, p.y, p.z])[0]
    }

    pub fn eval_grad(&self, p: DVec3) -> (f64, DVec3) {
        let r = self.with_grad.eval(&[p.x, p.y, p.z]);
        (r[0], DVec3::new(r[1], r[2], r[3]))
    }

    /// F on many points given as coordinate columns.
    pub fn values(&self, xs: &[f64], ys: &[f64], zs: &[f64]) -> Vec<f64> {
        self.value.eval_batch(&[xs, ys, zs], xs.len()).swap_remove(0)
    }

    /// F and ∇F at many points.
    pub fn values_grads(&self, pts: &[DVec3]) -> Vec<(f64, DVec3)> {
        let xs: Vec<f64> = pts.iter().map(|p| p.x).collect();
        let ys: Vec<f64> = pts.iter().map(|p| p.y).collect();
        let zs: Vec<f64> = pts.iter().map(|p| p.z).collect();
        let r = self.with_grad.eval_batch(&[&xs, &ys, &zs], pts.len());
        (0..pts.len()).map(|k| (r[0][k], DVec3::new(r[1][k], r[2][k], r[3][k]))).collect()
    }

    /// Decide whether F is linear or quadratic over the box (exactly, up
    /// to rounding, at sample points) and classify it.
    pub fn classify(&mut self, lo: DVec3, hi: DVec3) {
        self.kind = self.compute_kind(lo, hi);
    }

    fn compute_kind(&self, lo: DVec3, hi: DVec3) -> SurfaceKind {
        let ext = hi - lo;
        let l = ext.max_element().max(1e-9) * 0.5;
        // Taylor data at the centre (nudged if F is undefined there).
        let centre = (lo + hi) * 0.5;
        let nudges = [DVec3::ZERO, DVec3::new(0.0123, 0.0217, 0.0311), DVec3::new(-0.137, 0.091, 0.173)];
        let Some((p0, d)) = nudges.iter().map(|n| centre + *n * l).find_map(|p| {
            let d = self.second.eval(&[p.x, p.y, p.z]);
            d.iter().all(|v| v.is_finite()).then_some((p, d))
        }) else {
            return SurfaceKind::General;
        };
        let f0 = d[0];
        let g = DVec3::new(d[1], d[2], d[3]);
        let h = [[d[4], d[7], d[8]], [d[7], d[5], d[9]], [d[8], d[9], d[6]]];
        let model = |p: DVec3| {
            let u = p - p0;
            let hu = DVec3::new(h[0][0] * u.x + h[0][1] * u.y + h[0][2] * u.z, h[1][0] * u.x + h[1][1] * u.y + h[1][2] * u.z, h[2][0] * u.x + h[2][1] * u.y + h[2][2] * u.z);
            f0 + g.dot(u) + 0.5 * u.dot(hu)
        };
        let hmax = h.iter().flatten().fold(0f64, |m, v| m.max(v.abs()));
        let size = f0.abs() + g.abs().max_element() * l + hmax * l * l;
        if size == 0.0 {
            return SurfaceKind::Constant;
        }
        // Low-discrepancy points over the box.
        let mut exact = true;
        for k in 1..=16 {
            let fr = |a: f64| (k as f64 * a).fract();
            let p = lo + ext * DVec3::new(fr(0.618_033_988_7), fr(0.754_877_666_2), fr(0.569_840_290_9));
            let v = self.eval(p);
            if !v.is_finite() || (v - model(p)).abs() > 1e-8 * size {
                exact = false;
                break;
            }
        }
        if !exact {
            return SurfaceKind::General;
        }
        if hmax * l * l <= 1e-10 * size {
            if g.length() * l <= 1e-12 * size {
                return SurfaceKind::Constant;
            }
            return SurfaceKind::Plane { n: g, d: g.dot(p0) - f0 };
        }
        let a = h.map(|r| r.map(|v| v * 0.5));
        SurfaceKind::Quadric(Box::new(quadric::classify(a, g * 0.5, f0, p0, l)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::quadric::Class;

    fn kind(src: &str) -> SurfaceKind {
        let mut s = Implicit::new(src).unwrap();
        s.classify(DVec3::splat(-3.0), DVec3::splat(3.0));
        s.kind
    }

    fn class(src: &str) -> Class {
        match kind(src) {
            SurfaceKind::Quadric(q) => q.class,
            k => panic!("{src}: {k:?}"),
        }
    }

    #[test]
    fn recognises_planes_and_quadrics() {
        match kind("x + 2y - z = 3") {
            SurfaceKind::Plane { n, d } => {
                assert!((n - DVec3::new(1.0, 2.0, -1.0)).length() < 1e-12);
                assert!((d - 3.0).abs() < 1e-12);
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(class("x^2 + y^2 + z^2 = 4"), Class::Ellipsoid);
        assert_eq!(class("z = x^2 + y^2"), Class::EllipticParaboloid);
        assert_eq!(class("z = x*y"), Class::HyperbolicParaboloid);
        assert_eq!(class("x^2 + y^2 = z^2"), Class::Cone);
        assert_eq!(class("x^2 + y^2 - z^2 = 1"), Class::Hyperboloid1);
        assert_eq!(class("(x-1)^2 + y^2 = 1"), Class::EllipticCylinder);
        assert_eq!(class("y = x^2"), Class::ParabolicCylinder);
        assert_eq!(class("x^2/4 + y^2/9 + z^2 = 1"), Class::Ellipsoid);
        assert!(matches!(kind("z = sin(x) + y"), SurfaceKind::General));
        assert!(matches!(kind("x^2 + y^2 + z^2 = 1/(x^2+1)"), SurfaceKind::General));
    }

    #[test]
    fn latex_and_both_sides() {
        assert_eq!(class("\\frac{x^2}{4} + y^2 = z"), Class::EllipticParaboloid);
        assert!(Implicit::new("x^2 < 1").is_err());
        assert!(Implicit::new("x = y = z").is_err());
    }

    #[test]
    fn parameters_change_the_class() {
        let mut s = Implicit::new("x^2 + y^2 - z^2 = a").unwrap();
        assert_eq!(s.params, vec!["a".to_string()]);
        let lo = DVec3::splat(-3.0);
        let hi = DVec3::splat(3.0);
        for (a, want) in [(1.0, Class::Hyperboloid1), (0.0, Class::Cone), (-1.0, Class::Hyperboloid2)] {
            s.set_params(&[("a".into(), a)]);
            s.classify(lo, hi);
            match &s.kind {
                SurfaceKind::Quadric(q) => assert_eq!(q.class, want, "a = {a}"),
                k => panic!("{k:?}"),
            }
        }
    }

    #[test]
    fn splitting_helpers() {
        assert_eq!(strip_outer("(1+t, (2), t)"), "1+t, (2), t");
        assert_eq!(strip_outer("(x)+(y)"), "(x)+(y)");
        assert_eq!(split_top("f(a, b), c; d"), vec!["f(a, b)", " c", " d"]);
        assert_eq!(equals_signs("a <= b = c == d"), vec![7]);
    }
}
