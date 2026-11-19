//! What a line of input describes, recognised from how it is written:
//!
//! * `y = f(x)`, `f(x) = …` or just an expression in x: a graph over x,
//! * `x = g(y)`: a graph over y,
//! * `(x(t), y(t))` or `x = …, y = …`: a parametric curve (t or θ),
//! * `r = f(θ)` (also `ρ`, `theta`): a polar curve; any other equation in
//!   r and θ, such as `r^2 = 2cos(2θ)`, is a polar equation,
//! * any other equation in x and y: an implicit curve F(x, y) = 0,
//! * inequalities, joined by `and`, `or` or commas: a region. In a region
//!   r and θ are polar coordinates when θ appears or x and y do not.
//!
//! Other single letters are parameters with sliders.

use crate::expr::{Bin, Id, Node, ParseError, Pool, Program, Un, Vars, parse_with_params};
use crate::i18n::tr;
use crate::space::surface::{equals_signs, plain, split_top, strip_outer};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// y = f(x).
    FnX,
    /// x = g(y).
    FnY,
    /// (x(t), y(t)).
    Param,
    /// r = f(θ).
    Polar,
    /// F(r, θ) = 0.
    PolarImplicit,
    /// F(x, y) = 0.
    Implicit,
    /// Inequalities in x, y (or r, θ).
    Region,
}

impl Form {
    /// Drawn by sampling a parameter: x(u), y(u).
    pub fn is_path(self) -> bool {
        matches!(self, Form::FnX | Form::FnY | Form::Param | Form::Polar)
    }

    /// The zero set of a function of two variables.
    pub fn is_field(self) -> bool {
        matches!(self, Form::Implicit | Form::PolarImplicit)
    }

    /// Has its own range of t or θ.
    pub fn has_range(self) -> bool {
        matches!(self, Form::Param | Form::Polar | Form::PolarImplicit)
    }

    pub fn name(self) -> &'static str {
        match self {
            Form::FnX => tr("function y = f(x)", "函数 y = f(x)"),
            Form::FnY => tr("function x = g(y)", "函数 x = g(y)"),
            Form::Param => tr("parametric curve", "参数方程"),
            Form::Polar => tr("polar curve r = f(θ)", "极坐标曲线 r = f(θ)"),
            Form::PolarImplicit => tr("polar equation", "极坐标方程"),
            Form::Implicit => tr("implicit curve", "隐函数曲线"),
            Form::Region => tr("region", "区域"),
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Form::FnX | Form::FnY => tr("function", "函数"),
            Form::Param => tr("parametric", "参数方程"),
            Form::Polar | Form::PolarImplicit => tr("polar", "极坐标"),
            Form::Implicit => tr("implicit", "隐函数"),
            Form::Region => tr("region", "区域"),
        }
    }
}

/// x(u), y(u) with their first and second derivatives.
#[derive(Clone)]
struct Path {
    p: Program,
    d1: Program,
    d2: Program,
    /// r(θ), r′(θ) of a polar curve.
    polar: Option<Program>,
}

/// F(u, v) with its gradient and Hessian; (u, v) = (x, y), or (θ, r) for a
/// polar equation.
#[derive(Clone)]
struct Field {
    f: Program,
    grad: Program,
    hess: Program,
}

#[derive(Clone)]
struct RegionDef {
    cond: Program,
    /// lhs − rhs of each comparison, and whether it is strict.
    leaves: Vec<(Program, bool)>,
    /// Inputs are (x, y, r, θ) instead of (x, y).
    polar: bool,
}

#[derive(Clone)]
pub struct Def {
    pub source: String,
    pub form: Form,
    pub params: Vec<String>,
    /// Name of the curve parameter of a path ("x", "y", "t" or "θ"), or
    /// of the angle of a polar equation.
    pub var: &'static str,
    path: Option<Path>,
    field: Option<Field>,
    region: Option<RegionDef>,
}

fn err(msg: &str) -> ParseError {
    ParseError { msg: msg.to_string(), pos: usize::MAX }
}

/// Rename whole identifiers: `theta` → `θ`, `rho` / `ρ` → `r`.
fn rename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < s.len() {
                let ch = s[i..].chars().next().unwrap();
                if ch.is_alphanumeric() || ch == '_' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push_str(match &s[start..i] {
                "theta" => "θ",
                "rho" | "ρ" => "r",
                w => w,
            });
        } else if c.is_ascii_digit() {
            // A number such as 2e5: keep its letters out of the renaming.
            while i < s.len() && (s.as_bytes()[i].is_ascii_digit() || s.as_bytes()[i] == b'.') {
                out.push(s.as_bytes()[i] as char);
                i += 1;
            }
        } else {
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// Whether the identifier `name` occurs in `s`.
pub(crate) fn has_ident(s: &str, name: &str) -> bool {
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < s.len() {
                let ch = s[i..].chars().next().unwrap();
                if ch.is_alphanumeric() || ch == '_' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            if &s[start..i] == name {
                return true;
            }
        } else if c.is_ascii_digit() {
            while i < s.len() && (s.as_bytes()[i].is_ascii_digit() || s.as_bytes()[i] == b'.') {
                i += 1;
            }
        } else {
            i += c.len_utf8();
        }
    }
    false
}

/// `dep`, or a function name applied to `var` such as `f(x)`: the
/// left-hand side of a graph.
fn is_graph_lhs(s: &str, dep: &str, var: &str) -> bool {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if s == dep {
        return true;
    }
    let mut ch = s.chars();
    match ch.next() {
        Some(c) if c.is_ascii_alphabetic() => ch.as_str() == format!("({var})"),
        _ => false,
    }
}

/// Inequality signs outside brackets.
fn is_condition(s: &str) -> bool {
    let mut depth = 0i32;
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'<' | b'>' if depth == 0 => return true,
            b'!' if depth == 0 && b.get(i + 1) == Some(&b'=') => return true,
            _ => {}
        }
    }
    false
}

/// Whether `src` depends on variable `k` of `vars`.
fn depends(src: &str, vars: &[&'static str], k: u8) -> Result<bool, ParseError> {
    let mut pool = Pool::new();
    let mut params = Vec::new();
    let id = parse_with_params(src, &mut pool, &Vars(vars.to_vec()), false, &mut params)?;
    Ok(pool.depends_on(id, k))
}

fn is_comparison(pool: &Pool, id: Id) -> bool {
    matches!(pool.node(id), Node::Bin(Bin::Lt | Bin::Le | Bin::Gt | Bin::Ge | Bin::Eq | Bin::Ne | Bin::And | Bin::Or, ..) | Node::Un(Un::Not, _))
}

/// The comparisons of a condition as (lhs − rhs, strict).
fn leaves(pool: &mut Pool, id: Id, out: &mut Vec<(Id, bool)>) {
    match pool.node(id) {
        Node::Bin(op @ (Bin::Lt | Bin::Le | Bin::Gt | Bin::Ge | Bin::Eq), a, b) => {
            let d = pool.sub(a, b);
            if !out.iter().any(|(e, _)| *e == d) {
                out.push((d, matches!(op, Bin::Lt | Bin::Gt)));
            }
        }
        Node::Bin(Bin::And | Bin::Or, a, b) => {
            leaves(pool, a, out);
            leaves(pool, b, out);
        }
        Node::Un(Un::Not, a) => leaves(pool, a, out),
        _ => {}
    }
}

fn path_of(pool: &mut Pool, x: Id, y: Id, polar: Option<Id>) -> Path {
    let (dx, dy) = (pool.diff(x, 0), pool.diff(y, 0));
    let (ddx, ddy) = (pool.diff(dx, 0), pool.diff(dy, 0));
    let polar = polar.map(|r| {
        let dr = pool.diff(r, 0);
        Program::compile(pool, &[r, dr])
    });
    Path { p: Program::compile(pool, &[x, y]), d1: Program::compile(pool, &[dx, dy]), d2: Program::compile(pool, &[ddx, ddy]), polar }
}

fn field_of(pool: &mut Pool, f: Id) -> Field {
    let (fu, fv) = (pool.diff(f, 0), pool.diff(f, 1));
    let (fuu, fuv, fvv) = (pool.diff(fu, 0), pool.diff(fu, 1), pool.diff(fv, 1));
    Field {
        f: Program::compile(pool, &[f]),
        grad: Program::compile(pool, &[f, fu, fv]),
        hess: Program::compile(pool, &[f, fu, fv, fuu, fuv, fvv]),
    }
}

fn two(mut v: Vec<Vec<f64>>) -> [Vec<f64>; 2] {
    let b = v.pop().unwrap_or_default();
    let a = v.pop().unwrap_or_default();
    [a, b]
}

impl Def {
    pub fn new(src: &str) -> Result<Self, ParseError> {
        let s = rename(&plain(src)?);
        let s = s.trim();
        if s.is_empty() {
            return Err(err(tr("Type a curve, e.g. y = x^2", "请输入曲线，例如 y = x^2")));
        }
        let parts = split_top(strip_outer(s));
        let mut def = if parts.len() > 1 {
            if parts.iter().any(|p| is_condition(p)) {
                Self::region(&parts)?
            } else if parts.len() == 2 {
                Self::param(&parts)?
            } else {
                return Err(err(tr(
                    "A parametric curve has two coordinates: (x(t), y(t))",
                    "参数方程只有两个坐标：(x(t), y(t))",
                )));
            }
        } else {
            let eq = equals_signs(s);
            match eq.len() {
                0 if is_condition(s) => Self::region(&[s])?,
                0 => Self::bare(s)?,
                1 => Self::equation(&s[..eq[0]], &s[eq[0] + 1..])?,
                _ => return Err(err(tr("A curve is one equation (only one '=')", "曲线只需一个方程（只能有一个 '='）"))),
            }
        };
        def.source = src.trim().to_string();
        Ok(def)
    }

    fn make(form: Form, var: &'static str, params: Vec<String>) -> Self {
        Self { source: String::new(), form, params, var, path: None, field: None, region: None }
    }

    /// An expression without '='.
    fn bare(s: &str) -> Result<Self, ParseError> {
        if has_ident(s, "θ") {
            return if depends(s, &["θ", "r"], 1)? { Self::polar_implicit(s, None) } else { Self::polar(s) };
        }
        if depends(s, &["x", "y"], 1)? { Self::implicit(s, None) } else { Self::fn_x(s) }
    }

    fn equation(l: &str, r: &str) -> Result<Self, ParseError> {
        let (lt, rt) = (l.trim(), r.trim());
        if lt.is_empty() || rt.is_empty() {
            return Err(err(tr("One side of the equation is empty", "等式的一边是空的")));
        }
        let polar = has_ident(l, "θ") || has_ident(r, "θ") || is_graph_lhs(lt, "r", "θ") || is_graph_lhs(rt, "r", "θ");
        if polar {
            let pv = ["θ", "r"];
            if is_graph_lhs(lt, "r", "θ") && !depends(rt, &pv, 1)? {
                return Self::polar(rt);
            }
            if is_graph_lhs(rt, "r", "θ") && !depends(lt, &pv, 1)? {
                return Self::polar(lt);
            }
            return Self::polar_implicit(lt, Some(rt));
        }
        let xy = ["x", "y"];
        if is_graph_lhs(lt, "y", "x") && !depends(rt, &xy, 1)? {
            return Self::fn_x(rt);
        }
        if is_graph_lhs(rt, "y", "x") && !depends(lt, &xy, 1)? {
            return Self::fn_x(lt);
        }
        if is_graph_lhs(lt, "x", "y") && !depends(rt, &xy, 0)? {
            return Self::fn_y(rt);
        }
        if is_graph_lhs(rt, "x", "y") && !depends(lt, &xy, 0)? {
            return Self::fn_y(lt);
        }
        Self::implicit(lt, Some(rt))
    }

    fn fn_x(src: &str) -> Result<Self, ParseError> {
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let f = parse_with_params(src, &mut pool, &Vars(vec!["x"]), false, &mut params)?;
        let x = pool.var(0);
        let mut d = Self::make(Form::FnX, "x", params);
        d.path = Some(path_of(&mut pool, x, f, None));
        Ok(d)
    }

    fn fn_y(src: &str) -> Result<Self, ParseError> {
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let g = parse_with_params(src, &mut pool, &Vars(vec!["y"]), false, &mut params)?;
        let y = pool.var(0);
        let mut d = Self::make(Form::FnY, "y", params);
        d.path = Some(path_of(&mut pool, g, y, None));
        Ok(d)
    }

    fn polar(src: &str) -> Result<Self, ParseError> {
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let r = parse_with_params(src, &mut pool, &Vars(vec!["θ"]), false, &mut params)?;
        let th = pool.var(0);
        let (c, s) = (pool.un(Un::Cos, th), pool.un(Un::Sin, th));
        let (x, y) = (pool.mul(r, c), pool.mul(r, s));
        let mut d = Self::make(Form::Polar, "θ", params);
        d.path = Some(path_of(&mut pool, x, y, Some(r)));
        Ok(d)
    }

    fn polar_implicit(l: &str, r: Option<&str>) -> Result<Self, ParseError> {
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let vars = Vars(vec!["θ", "r"]);
        let a = parse_with_params(l, &mut pool, &vars, false, &mut params)?;
        let f = match r {
            Some(r) => {
                let b = parse_with_params(r, &mut pool, &vars, false, &mut params)?;
                pool.sub(a, b)
            }
            None => a,
        };
        let mut d = Self::make(Form::PolarImplicit, "θ", params);
        d.field = Some(field_of(&mut pool, f));
        Ok(d)
    }

    fn implicit(l: &str, r: Option<&str>) -> Result<Self, ParseError> {
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let vars = Vars(vec!["x", "y"]);
        let a = parse_with_params(l, &mut pool, &vars, false, &mut params)?;
        let f = match r {
            Some(r) => {
                let b = parse_with_params(r, &mut pool, &vars, false, &mut params)?;
                pool.sub(a, b)
            }
            None => a,
        };
        if !pool.depends_on(f, 0) && !pool.depends_on(f, 1) {
            return Err(err(tr("The equation does not involve x or y", "方程中没有 x 或 y")));
        }
        let mut d = Self::make(Form::Implicit, "x", params);
        d.field = Some(field_of(&mut pool, f));
        Ok(d)
    }

    fn param(parts: &[&str]) -> Result<Self, ParseError> {
        let all = parts.join(",");
        let var = if has_ident(&all, "θ") && !has_ident(&all, "t") { "θ" } else { "t" };
        let vars = Vars(vec![var]);
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let mut comps: [Option<Id>; 2] = [None, None];
        for p in parts {
            let eq = equals_signs(p);
            let (slot, rhs) = match eq.len() {
                0 => (comps.iter().position(Option::is_none).unwrap_or(0), *p),
                1 => {
                    let name = p[..eq[0]].trim();
                    let k = if is_graph_lhs(name, "x", var) {
                        0
                    } else if is_graph_lhs(name, "y", var) {
                        1
                    } else {
                        return Err(err(tr("Write x = …, y = … (functions of t)", "请写成 x = …, y = …（t 的函数）")));
                    };
                    (k, &p[eq[0] + 1..])
                }
                _ => return Err(err(tr("Too many '=' in a coordinate", "一个坐标里有多个 '='"))),
            };
            if comps[slot].is_some() {
                return Err(err(tr("A coordinate is given twice", "某个坐标给了两次")));
            }
            comps[slot] = Some(parse_with_params(rhs, &mut pool, &vars, false, &mut params)?);
        }
        let (x, y) = (comps[0].unwrap(), comps[1].unwrap());
        let mut d = Self::make(Form::Param, var, params);
        d.path = Some(path_of(&mut pool, x, y, None));
        Ok(d)
    }

    fn region(parts: &[&str]) -> Result<Self, ParseError> {
        let text = parts.join(" ");
        let polar = has_ident(&text, "θ") || !(has_ident(&text, "x") || has_ident(&text, "y"));
        let vars = if polar { Vars(vec!["x", "y", "r", "θ"]) } else { Vars(vec!["x", "y"]) };
        let mut pool = Pool::new();
        let mut params = Vec::new();
        let mut cond: Option<Id> = None;
        for p in parts {
            let id = parse_with_params(p, &mut pool, &vars, false, &mut params)?;
            if !is_comparison(&pool, id) {
                return Err(err(tr(
                    "In a region every part separated by commas is an inequality",
                    "区域里用逗号隔开的每一部分都应是不等式",
                )));
            }
            cond = Some(match cond {
                None => id,
                Some(c) => pool.bin(Bin::And, c, id),
            });
        }
        let cond = cond.unwrap();
        let mut ls = Vec::new();
        leaves(&mut pool, cond, &mut ls);
        if ls.is_empty() {
            return Err(err(tr("No inequality found", "没有找到不等式")));
        }
        let mut d = Self::make(Form::Region, "x", params);
        d.region = Some(RegionDef {
            cond: Program::compile(&pool, &[cond]),
            leaves: ls.iter().map(|(id, strict)| (Program::compile(&pool, &[*id]), *strict)).collect(),
            polar,
        });
        Ok(d)
    }

    /// Default range of t or θ.
    pub fn default_range(&self) -> (f64, f64) {
        (0.0, std::f64::consts::TAU)
    }

    /// Set parameter values by name (missing names keep NaN).
    pub fn set_params(&mut self, named: &[(String, f64)]) {
        let v: Vec<f64> = self.params.iter().map(|p| named.iter().find(|(n, _)| n == p).map_or(f64::NAN, |(_, v)| *v)).collect();
        if let Some(p) = &mut self.path {
            for prog in [&mut p.p, &mut p.d1, &mut p.d2] {
                prog.set_params(&v);
            }
            if let Some(r) = &mut p.polar {
                r.set_params(&v);
            }
        }
        if let Some(f) = &mut self.field {
            for prog in [&mut f.f, &mut f.grad, &mut f.hess] {
                prog.set_params(&v);
            }
        }
        if let Some(r) = &mut self.region {
            r.cond.set_params(&v);
            for (l, _) in &mut r.leaves {
                l.set_params(&v);
            }
        }
    }

    // ---- paths ----

    fn path_prog(&self, which: u8, us: &[f64]) -> [Vec<f64>; 2] {
        match &self.path {
            Some(p) if !us.is_empty() => {
                let prog = match which {
                    0 => &p.p,
                    1 => &p.d1,
                    _ => &p.d2,
                };
                two(prog.eval_batch(&[us], us.len()))
            }
            _ => [vec![f64::NAN; us.len()], vec![f64::NAN; us.len()]],
        }
    }

    /// Points (x(u), y(u)).
    pub fn path(&self, us: &[f64]) -> [Vec<f64>; 2] {
        self.path_prog(0, us)
    }

    /// (x′(u), y′(u)).
    pub fn path_d1(&self, us: &[f64]) -> [Vec<f64>; 2] {
        self.path_prog(1, us)
    }

    /// (x″(u), y″(u)).
    pub fn path_d2(&self, us: &[f64]) -> [Vec<f64>; 2] {
        self.path_prog(2, us)
    }

    /// One point with its derivatives: [x, y, x′, y′, x″, y″].
    pub fn path_at(&self, u: f64) -> [f64; 6] {
        let [x, y] = self.path(&[u]);
        let [dx, dy] = self.path_d1(&[u]);
        let [ddx, ddy] = self.path_d2(&[u]);
        [x[0], y[0], dx[0], dy[0], ddx[0], ddy[0]]
    }

    /// (r(θ), r′(θ)) of a polar curve.
    pub fn polar_r(&self, us: &[f64]) -> Option<[Vec<f64>; 2]> {
        let r = self.path.as_ref()?.polar.as_ref()?;
        Some(two(r.eval_batch(&[us], us.len())))
    }

    // ---- fields ----

    /// F at many points (u, v): (x, y), or (θ, r) for a polar equation.
    pub fn field(&self, us: &[f64], vs: &[f64]) -> Vec<f64> {
        match &self.field {
            Some(f) if !us.is_empty() => f.f.eval_batch(&[us, vs], us.len()).swap_remove(0),
            _ => vec![f64::NAN; us.len()],
        }
    }

    /// [F, F_u, F_v].
    pub fn field_grad(&self, us: &[f64], vs: &[f64]) -> [Vec<f64>; 3] {
        match &self.field {
            Some(f) if !us.is_empty() => {
                let mut v = f.grad.eval_batch(&[us, vs], us.len());
                let c = v.pop().unwrap();
                let b = v.pop().unwrap();
                [v.pop().unwrap(), b, c]
            }
            _ => std::array::from_fn(|_| vec![f64::NAN; us.len()]),
        }
    }

    /// [F, F_u, F_v, F_uu, F_uv, F_vv] at one point.
    pub fn field_hess(&self, u: f64, v: f64) -> [f64; 6] {
        match &self.field {
            Some(f) => {
                let r = f.hess.eval(&[u, v]);
                std::array::from_fn(|k| r.get(k).copied().unwrap_or(f64::NAN))
            }
            None => [f64::NAN; 6],
        }
    }

    // ---- regions ----

    fn region_inputs(&self, xs: &[f64], ys: &[f64]) -> Vec<Vec<f64>> {
        let mut cols = vec![xs.to_vec(), ys.to_vec()];
        if self.region.as_ref().is_some_and(|r| r.polar) {
            cols.push(xs.iter().zip(ys).map(|(x, y)| x.hypot(*y)).collect());
            cols.push(xs.iter().zip(ys).map(|(x, y)| y.atan2(*x)).collect());
        }
        cols
    }

    /// Whether points lie in the region.
    pub fn region_has(&self, xs: &[f64], ys: &[f64]) -> Vec<bool> {
        let Some(r) = &self.region else { return vec![false; xs.len()] };
        if xs.is_empty() {
            return vec![];
        }
        let cols = self.region_inputs(xs, ys);
        let refs: Vec<&[f64]> = cols.iter().map(|c| c.as_slice()).collect();
        r.cond.eval_batch(&refs, xs.len()).swap_remove(0).into_iter().map(|v| v.is_finite() && v != 0.0).collect()
    }

    /// Number of comparisons and whether each one is strict.
    pub fn region_leaves(&self) -> Vec<bool> {
        self.region.as_ref().map(|r| r.leaves.iter().map(|l| l.1).collect()).unwrap_or_default()
    }

    /// lhs − rhs of comparison `k` at many points.
    pub fn region_leaf(&self, k: usize, xs: &[f64], ys: &[f64]) -> Vec<f64> {
        let Some(r) = &self.region else { return vec![f64::NAN; xs.len()] };
        if xs.is_empty() {
            return vec![];
        }
        let cols = self.region_inputs(xs, ys);
        let refs: Vec<&[f64]> = cols.iter().map(|c| c.as_slice()).collect();
        r.leaves[k].0.eval_batch(&refs, xs.len()).swap_remove(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(s: &str) -> Form {
        Def::new(s).unwrap_or_else(|e| panic!("{s}: {e}")).form
    }

    #[test]
    fn forms_are_recognised() {
        assert_eq!(form("y = sin(x)"), Form::FnX);
        assert_eq!(form("x^2 - 1"), Form::FnX);
        assert_eq!(form("f(x) = x^3 - x"), Form::FnX);
        assert_eq!(form("sin(x) = y"), Form::FnX);
        assert_eq!(form("y = 2"), Form::FnX);
        assert_eq!(form("x = y^2 - 1"), Form::FnY);
        assert_eq!(form("x = 3"), Form::FnY);
        assert_eq!(form("x^2 + y^2 = 1"), Form::Implicit);
        assert_eq!(form("x^3 + y^3 = 3x y"), Form::Implicit);
        assert_eq!(form("y = x + sin(y)"), Form::Implicit);
        assert_eq!(form("x^2 + y^2 = r^2"), Form::Implicit);
        assert_eq!(form("(cos(t), sin(t))"), Form::Param);
        assert_eq!(form("x = t - sin(t), y = 1 - cos(t)"), Form::Param);
        assert_eq!(form("(cos(θ), sin(2θ))"), Form::Param);
        assert_eq!(form("r = 1 + cos(θ)"), Form::Polar);
        assert_eq!(form("r = 2"), Form::Polar);
        assert_eq!(form("ρ = theta"), Form::Polar);
        assert_eq!(form("cos(3θ)"), Form::Polar);
        assert_eq!(form("r^2 = 2cos(2θ)"), Form::PolarImplicit);
        assert_eq!(form("x^2 + y^2 <= 4"), Form::Region);
        assert_eq!(form("y >= x^2, y <= 2"), Form::Region);
        assert_eq!(form("0 < x < 1 and x^2 < y < x"), Form::Region);
        assert_eq!(form("r <= 2cos(θ)"), Form::Region);
        assert_eq!(form("y = if(x < 0, -x, x^2)"), Form::FnX);
        assert_eq!(form("\\frac{x^2}{4} + y^2 = 1"), Form::Implicit);
        assert_eq!(form("y = \\sqrt{x}"), Form::FnX);
        assert_eq!(form("\\rho = 2\\sin\\theta"), Form::Polar);
        assert_eq!(form("x^2 + y^2 \\le 1"), Form::Region);
    }

    #[test]
    fn errors_are_reported() {
        assert!(Def::new("").is_err());
        assert!(Def::new("y = x = 1").is_err());
        assert!(Def::new("(1, 2, 3)").is_err());
        assert!(Def::new("x^2 + y^2 < 1, x").is_err());
        assert!(Def::new("1 = 2").is_err());
    }

    #[test]
    fn evaluation() {
        let d = Def::new("r = 1 + cos(θ)").unwrap();
        let [x, y] = d.path(&[0.0, std::f64::consts::FRAC_PI_2]);
        assert!((x[0] - 2.0).abs() < 1e-12 && y[0].abs() < 1e-12);
        assert!(x[1].abs() < 1e-12 && (y[1] - 1.0).abs() < 1e-12);
        let mut d = Def::new("y = a x^2").unwrap();
        assert_eq!(d.params, vec!["a".to_string()]);
        d.set_params(&[("a".into(), 3.0)]);
        let p = d.path_at(2.0);
        assert_eq!([p[0], p[1], p[2], p[3], p[5]], [2.0, 12.0, 1.0, 12.0, 6.0]);
        let d = Def::new("x^2 + y^2 = 1").unwrap();
        let g = d.field_grad(&[1.0], &[1.0]);
        assert_eq!([g[0][0], g[1][0], g[2][0]], [1.0, 2.0, 2.0]);
        let d = Def::new("r <= 1").unwrap();
        assert_eq!(d.region_has(&[0.5, 2.0], &[0.5, 0.0]), vec![true, false]);
        let d = Def::new("x^2 + y^2 < 1, y > 0").unwrap();
        assert_eq!(d.region_has(&[0.0, 0.0], &[0.5, -0.5]), vec![true, false]);
        assert_eq!(d.region_leaves(), vec![true, true]);
    }
}
