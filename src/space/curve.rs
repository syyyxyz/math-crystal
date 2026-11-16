//! Lines and space curves. Accepted forms:
//!
//! * parametric `(1 + 2t, -1 + 3t, t)` or `x = 1 + 2t, y = -1 + 3t, z = t`
//!   (any curve; a line when it is linear in t),
//! * symmetric `(x - 1)/2 = (y + 1)/3 = z/1` (a zero denominator such as
//!   `(y - 2)/0` means y = 2, as in textbooks),
//! * general form: two plane equations `x + y + z = 1, x - y = 0`.

use glam::DVec3;

use super::surface::{equals_signs, parse_equation, plain, split_top, strip_outer, xyz};
use crate::expr::{ParseError, Pool, Program, Vars};
use crate::i18n::tr;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Form {
    Parametric,
    Symmetric,
    Planes,
}

#[derive(Clone)]
enum Def {
    /// x(t), y(t), z(t).
    Param(Program),
    /// One program per part, each a function of its own coordinate; for a
    /// zero denominator the numerator instead and a fixed coordinate.
    Symmetric(Vec<(usize, Program, bool)>),
    /// F1, F2 (linear in x, y, z).
    Planes(Program),
}

#[derive(Clone)]
pub struct Curve {
    pub source: String,
    pub form: Form,
    def: Def,
    pub params: Vec<String>,
    /// `Some((point, direction))` when the curve is a straight line.
    pub line: Option<(DVec3, DVec3)>,
    pub error: Option<String>,
    pool: Pool,
    ids: Vec<crate::expr::Id>,
}

fn err(msg: &str) -> ParseError {
    ParseError { msg: msg.to_string(), pos: usize::MAX }
}

/// `(numerator, true)` when `part` is `numerator / 0`.
fn zero_denominator(part: &str) -> Option<&str> {
    let s = part.trim_end();
    let k = s.rfind('/')?;
    let den = s[k + 1..].trim();
    (!den.is_empty() && den.trim_start_matches('(').trim_end_matches(')').parse::<f64>().ok()? == 0.0).then(|| &s[..k])
}

impl Curve {
    pub fn new(src: &str) -> Result<Self, ParseError> {
        let s = plain(src)?;
        let inner = strip_outer(&s);
        let parts = split_top(inner);
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let t = Vars(vec!["t"]);
        let (form, def, ids) = match parts.len() {
            3 => {
                let mut comps = [None; 3];
                for p in &parts {
                    let eq = equals_signs(p);
                    let (slot, rhs) = match eq.len() {
                        0 => (comps.iter().position(Option::is_none).unwrap(), *p),
                        1 => {
                            let name = p[..eq[0]].trim();
                            let k = ["x", "y", "z"].iter().position(|n| *n == name).ok_or_else(|| err(tr("Write x = …, y = …, z = … (functions of t)", "请写成 x = …, y = …, z = …（t 的函数）")))?;
                            (k, &p[eq[0] + 1..])
                        }
                        _ => return Err(err(tr("Too many '=' in a coordinate", "一个坐标里有多个 '='"))),
                    };
                    if comps[slot].is_some() {
                        return Err(err(tr("A coordinate is given twice", "某个坐标给了两次")));
                    }
                    comps[slot] = Some(crate::expr::parse_with_params(rhs, &mut pool, &t, false, &mut params)?);
                }
                let ids: Vec<_> = comps.iter().map(|c| c.unwrap()).collect();
                (Form::Parametric, Def::Param(Program::compile(&pool, &ids)), ids)
            }
            2 => {
                let a = parse_equation(parts[0], &mut pool, &xyz(), &mut params)?;
                let b = parse_equation(parts[1], &mut pool, &xyz(), &mut params)?;
                (Form::Planes, Def::Planes(Program::compile(&pool, &[a, b])), vec![a, b])
            }
            1 => {
                let eq = equals_signs(inner);
                if eq.len() != 2 {
                    return Err(err(tr(
                        "A line: (1+2t, -1+3t, t), (x-1)/2 = (y+1)/3 = z/1, or two planes separated by a comma",
                        "直线可写成 (1+2t, -1+3t, t)、(x-1)/2 = (y+1)/3 = z/1，或用逗号隔开的两个平面方程",
                    )));
                }
                let pieces = [&inner[..eq[0]], &inner[eq[0] + 1..eq[1]], &inner[eq[1] + 1..]];
                let mut used = [false; 3];
                let mut out = Vec::new();
                let mut ids = Vec::new();
                for piece in pieces {
                    let (text, zero) = match zero_denominator(piece) {
                        Some(num) => (num, true),
                        None => (piece, false),
                    };
                    let id = crate::expr::parse_with_params(text, &mut pool, &xyz(), false, &mut params)?;
                    let deps: Vec<usize> = (0..3).filter(|&v| pool.depends_on(id, v as u8)).collect();
                    if deps.len() != 1 || used[deps[0]] {
                        return Err(err(tr("Each part of the symmetric form uses exactly one of x, y, z", "对称式的每一段恰好含 x、y、z 中的一个")));
                    }
                    used[deps[0]] = true;
                    out.push((deps[0], Program::compile(&pool, &[id]), zero));
                    ids.push(id);
                }
                (Form::Symmetric, Def::Symmetric(out), ids)
            }
            _ => return Err(err(tr("Too many commas", "逗号太多"))),
        };
        let mut c = Self { source: src.trim().to_string(), form, def, params, line: None, error: None, pool, ids };
        c.refresh();
        Ok(c)
    }

    pub fn set_params(&mut self, named: &[(String, f64)]) {
        let v: Vec<f64> = self.params.iter().map(|p| named.iter().find(|(n, _)| n == p).map_or(f64::NAN, |(_, v)| *v)).collect();
        match &mut self.def {
            Def::Param(p) | Def::Planes(p) => p.set_params(&v),
            Def::Symmetric(parts) => parts.iter_mut().for_each(|(_, p, _)| p.set_params(&v)),
        }
        self.refresh();
    }

    /// Recompute the line (after parameters changed).
    fn refresh(&mut self) {
        self.error = None;
        self.line = None;
        match &self.def {
            Def::Param(p) => {
                let r = |t: f64| {
                    let v = p.eval(&[t]);
                    DVec3::new(v[0], v[1], v[2])
                };
                let (p0, p1) = (r(0.0), r(1.0));
                let d = p1 - p0;
                let linear = [-1.7, 2.3, 0.37].iter().all(|&t| (r(t) - (p0 + d * t)).length() <= 1e-9 * (1.0 + p0.length() + d.length() * 3.0));
                if linear && d.length() > 0.0 && p0.is_finite() {
                    self.line = Some((p0, d));
                }
            }
            Def::Symmetric(parts) => {
                let mut point = DVec3::ZERO;
                let mut dir = DVec3::ZERO;
                for (v, prog, zero) in parts {
                    let at = |x: f64| prog.eval(&[if *v == 0 { x } else { 0.0 }, if *v == 1 { x } else { 0.0 }, if *v == 2 { x } else { 0.0 }])[0];
                    let (b, a1, a2) = (at(0.0), at(1.0), at(2.5));
                    let alpha = a1 - b;
                    if !(alpha.is_finite() && b.is_finite()) || alpha == 0.0 || ((a2 - b) - 2.5 * alpha).abs() > 1e-9 * (alpha.abs() + b.abs()) {
                        self.error = Some(tr("Each part must be linear, like (x − 1)/2", "每一段都应是一次式，如 (x − 1)/2").into());
                        return;
                    }
                    // part = α v + β = t  →  v = −β/α + t/α.
                    point[*v] = -b / alpha;
                    dir[*v] = if *zero { 0.0 } else { 1.0 / alpha };
                }
                if dir == DVec3::ZERO {
                    self.error = Some(tr("All direction numbers are zero", "方向数全为零").into());
                    return;
                }
                self.line = Some((point, dir));
            }
            Def::Planes(p) => {
                let at = |q: DVec3| {
                    let v = p.eval(&[q.x, q.y, q.z]);
                    (v[0], v[1])
                };
                let o = at(DVec3::ZERO);
                let e = [DVec3::X, DVec3::Y, DVec3::Z].map(|d| at(d));
                let n1 = DVec3::new(e[0].0 - o.0, e[1].0 - o.0, e[2].0 - o.0);
                let n2 = DVec3::new(e[0].1 - o.1, e[1].1 - o.1, e[2].1 - o.1);
                // Check linearity at another point.
                let q = DVec3::new(1.3, -0.7, 2.1);
                let v = at(q);
                let lin = |n: DVec3, c: f64, f: f64| (f - (c + n.dot(q))).abs() <= 1e-9 * (1.0 + c.abs() + n.length() * 3.0);
                if !(lin(n1, o.0, v.0) && lin(n2, o.1, v.1)) {
                    self.error = Some(tr("Both equations must be planes (linear in x, y, z)", "两个方程都应是平面（x、y、z 的一次式）").into());
                    return;
                }
                let dir = n1.cross(n2);
                if dir.length() <= 1e-12 * n1.length() * n2.length() {
                    self.error = Some(tr("The two planes are parallel: no line", "两平面平行，没有交线").into());
                    return;
                }
                // n1·p = −F1(0), n2·p = −F2(0), dir·p = 0 (the point nearest the origin).
                let m = glam::DMat3::from_cols(n1, n2, dir).transpose();
                let p = m.inverse() * DVec3::new(-o.0, -o.1, 0.0);
                self.line = Some((p, dir));
            }
        }
    }

    /// Position at parameter t (parametric form only).
    pub fn at(&self, t: f64) -> Option<DVec3> {
        match &self.def {
            Def::Param(p) => {
                let v = p.eval(&[t]);
                let r = DVec3::new(v[0], v[1], v[2]);
                r.is_finite().then_some(r)
            }
            _ => self.line.map(|(p, d)| p + d * t),
        }
    }

    /// Whether a t range applies (curves that are not lines).
    pub fn needs_range(&self) -> bool {
        self.line.is_none() && matches!(self.def, Def::Param(_))
    }

    /// Positions at `n` evenly spaced parameters over `tr` (parametric).
    pub fn samples(&self, tr: (f64, f64), n: usize) -> Vec<Option<DVec3>> {
        match &self.def {
            Def::Param(p) => {
                let ts: Vec<f64> = (0..n).map(|k| tr.0 + (tr.1 - tr.0) * k as f64 / (n - 1) as f64).collect();
                let r = p.eval_batch(&[&ts], n);
                (0..n).map(|k| Some(DVec3::new(r[0][k], r[1][k], r[2][k])).filter(|v| v.is_finite())).collect()
            }
            _ => vec![],
        }
    }

    /// For the formula preview: the component or part expressions.
    pub fn exprs(&self) -> (&Pool, &[crate::expr::Id]) {
        (&self.pool, &self.ids)
    }
}

/// The parameter interval where the line `p + t d` is inside the box.
pub fn clip_line(p: DVec3, d: DVec3, lo: DVec3, hi: DVec3) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for i in 0..3 {
        if d[i].abs() < 1e-300 {
            if p[i] < lo[i] || p[i] > hi[i] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[i] - p[i]) / d[i], (hi[i] - p[i]) / d[i]);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    (t0 < t1).then_some((t0, t1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(src: &str) -> (DVec3, DVec3) {
        let c = Curve::new(src).unwrap();
        assert!(c.error.is_none(), "{src}: {:?}", c.error);
        c.line.unwrap_or_else(|| panic!("{src}: not a line"))
    }

    #[test]
    fn line_forms_agree() {
        let (p, d) = line("(1 + 2t, -1 + 3t, t)");
        assert_eq!((p, d), (DVec3::new(1.0, -1.0, 0.0), DVec3::new(2.0, 3.0, 1.0)));
        let (p, d) = line("x = 1 + 2t, z = t, y = -1 + 3t");
        assert_eq!((p, d), (DVec3::new(1.0, -1.0, 0.0), DVec3::new(2.0, 3.0, 1.0)));
        let (p, d) = line("(x - 1)/2 = (y + 1)/3 = z/1");
        assert!((p - DVec3::new(1.0, -1.0, 0.0)).length() < 1e-12 && (d - DVec3::new(2.0, 3.0, 1.0)).length() < 1e-12);
        // Zero denominator: y stays 2.
        let (p, d) = line("(x - 1)/2 = (y - 2)/0 = z");
        assert!((p - DVec3::new(1.0, 2.0, 0.0)).length() < 1e-12 && (d - DVec3::new(2.0, 0.0, 1.0)).length() < 1e-12);
        // Two planes: z = 0 and y = 1 meet in a line parallel to x.
        let (p, d) = line("z = 0, y = 1");
        assert!(d.normalize().x.abs() > 0.999 && (p - DVec3::new(0.0, 1.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn curves_and_errors() {
        let helix = Curve::new("(cos(t), sin(t), t/4)").unwrap();
        assert!(helix.line.is_none() && helix.needs_range());
        assert!((helix.at(0.0).unwrap() - DVec3::X).length() < 1e-12);
        assert!(Curve::new("x = y").is_err());
        let par = Curve::new("x + y = 1, x + y = 2").unwrap();
        assert!(par.error.is_some());
        let bad = Curve::new("x^2 = y = z").unwrap();
        assert!(bad.error.is_some());
        let k = Curve::new("(a + t, 0, k t)").unwrap();
        assert_eq!(k.params, vec!["a".to_string(), "k".to_string()]);
    }

    #[test]
    fn clipping() {
        let r = clip_line(DVec3::ZERO, DVec3::X, DVec3::splat(-2.0), DVec3::splat(2.0)).unwrap();
        assert_eq!(r, (-2.0, 2.0));
        assert!(clip_line(DVec3::new(0.0, 5.0, 0.0), DVec3::X, DVec3::splat(-2.0), DVec3::splat(2.0)).is_none());
    }
}
