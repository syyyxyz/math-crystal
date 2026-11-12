//! Expression engine: hash-consed expression DAG, parser, symbolic
//! differentiation, pretty printer and a batched bytecode evaluator for
//! real (`f64`) and complex (`Complex64`) numbers.
//!
//! Every node lives in a [`Pool`]. Structurally equal nodes share one id, so
//! common sub-expressions (which symbolic derivatives produce a lot of) are
//! stored and evaluated once.

pub mod diff;
pub mod eval;
pub mod fmt;
pub mod latex;
pub mod parse;

use std::collections::HashMap;

pub use eval::Program;
pub use parse::{ParseError, parse};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Id(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Un {
    Neg,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
    Exp,
    Ln,
    Sqrt,
    Cbrt,
    Abs,
    Sign,
    Floor,
    Ceil,
    Round,
    /// Heaviside step: 0 for x < 0, 1/2 at 0, 1 for x > 0.
    Step,
    /// Unnormalised sinc: sin(x)/x, 1 at x = 0.
    Sinc,
    Re,
    Im,
    Conj,
    Arg,
    Not,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
    Atan2,
    Min,
    Max,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

impl Bin {
    fn commutative(self) -> bool {
        matches!(self, Bin::Add | Bin::Mul | Bin::Min | Bin::Max | Bin::Eq | Bin::Ne | Bin::And | Bin::Or)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Node {
    /// f64 bits; `-0.0` is normalised to `0.0`.
    Const(u64),
    Var(u8),
    /// The imaginary unit `i` (NaN when evaluated as a real number).
    I,
    Un(Un, Id),
    Bin(Bin, Id, Id),
    If(Id, Id, Id),
}

#[derive(Default, Clone)]
pub struct Pool {
    nodes: Vec<Node>,
    map: HashMap<Node, Id>,
    diff_memo: HashMap<(Id, u8), Id>,
}

impl Pool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn node(&self, id: Id) -> Node {
        self.nodes[id.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn intern(&mut self, n: Node) -> Id {
        if let Some(&id) = self.map.get(&n) {
            return id;
        }
        let id = Id(self.nodes.len() as u32);
        self.nodes.push(n);
        self.map.insert(n, id);
        id
    }

    pub fn num(&mut self, v: f64) -> Id {
        let v = if v == 0.0 { 0.0 } else { v };
        let v = if v.is_nan() { f64::NAN } else { v };
        self.intern(Node::Const(v.to_bits()))
    }

    pub fn var(&mut self, index: u8) -> Id {
        self.intern(Node::Var(index))
    }

    pub fn imag(&mut self) -> Id {
        self.intern(Node::I)
    }

    pub fn as_const(&self, id: Id) -> Option<f64> {
        match self.node(id) {
            Node::Const(b) => Some(f64::from_bits(b)),
            _ => None,
        }
    }

    fn is_val(&self, id: Id, v: f64) -> bool {
        self.as_const(id) == Some(v)
    }

    pub fn neg(&mut self, a: Id) -> Id {
        if let Some(c) = self.as_const(a) {
            return self.num(-c);
        }
        match self.node(a) {
            Node::Un(Un::Neg, inner) => inner,
            Node::Bin(Bin::Sub, p, q) => self.sub(q, p),
            _ => self.intern(Node::Un(Un::Neg, a)),
        }
    }

    pub fn un(&mut self, op: Un, a: Id) -> Id {
        if op == Un::Neg {
            return self.neg(a);
        }
        if let Some(c) = self.as_const(a) {
            // Fold only when the real result is finite: sqrt(-1) must stay
            // symbolic so that complex evaluation can still give `i`.
            if !matches!(op, Un::Re | Un::Im | Un::Conj | Un::Arg) {
                let v = eval::un_f64(op, c);
                if v.is_finite() {
                    return self.num(v);
                }
            }
        }
        match (op, self.node(a)) {
            (Un::Abs, Node::Un(Un::Abs | Un::Neg, inner)) => return self.un(Un::Abs, inner),
            (Un::Conj, Node::Un(Un::Conj, inner)) => return inner,
            _ => {}
        }
        self.intern(Node::Un(op, a))
    }

    pub fn add(&mut self, a: Id, b: Id) -> Id {
        match (self.as_const(a), self.as_const(b)) {
            (Some(x), Some(y)) => return self.num(x + y),
            (Some(x), _) if x == 0.0 => return b,
            (_, Some(y)) if y == 0.0 => return a,
            _ => {}
        }
        if a == b {
            let two = self.num(2.0);
            return self.mul(two, a);
        }
        if let Node::Un(Un::Neg, c) = self.node(b) {
            return self.sub(a, c);
        }
        if let Node::Un(Un::Neg, c) = self.node(a) {
            return self.sub(b, c);
        }
        self.bin_raw(Bin::Add, a, b)
    }

    pub fn sub(&mut self, a: Id, b: Id) -> Id {
        match (self.as_const(a), self.as_const(b)) {
            (Some(x), Some(y)) => return self.num(x - y),
            (Some(x), _) if x == 0.0 => return self.neg(b),
            (_, Some(y)) if y == 0.0 => return a,
            _ => {}
        }
        if a == b {
            return self.num(0.0);
        }
        if let Node::Un(Un::Neg, c) = self.node(b) {
            return self.add(a, c);
        }
        self.bin_raw(Bin::Sub, a, b)
    }

    pub fn mul(&mut self, a: Id, b: Id) -> Id {
        match (self.as_const(a), self.as_const(b)) {
            (Some(x), Some(y)) => return self.num(x * y),
            (Some(x), _) | (_, Some(x)) if x == 0.0 => return self.num(0.0),
            (Some(x), _) if x == 1.0 => return b,
            (_, Some(y)) if y == 1.0 => return a,
            (Some(x), _) if x == -1.0 => return self.neg(b),
            (_, Some(y)) if y == -1.0 => return self.neg(a),
            _ => {}
        }
        // Flatten both sides into coefficient * product of base^exponent,
        // merge equal bases and rebuild in canonical (id) order, so that
        // y*(y*sin(x*y)) becomes y^2*sin(x*y) and x*(y*z) == y*(x*z).
        let mut coeff = 1.0;
        let mut factors: Vec<(Id, f64)> = Vec::new();
        self.collect_factors(a, &mut coeff, &mut factors);
        self.collect_factors(b, &mut coeff, &mut factors);
        factors.sort_by_key(|f| self.order_key(f.0));
        let mut merged: Vec<(Id, f64)> = Vec::new();
        for (base, e) in factors {
            if let Some(last) = merged.last_mut()
                && last.0 == base
                // x^0.5 * x^0.5 -> x would drop the x >= 0 domain; merge
                // only when at least one exponent is an integer.
                && (last.1.fract() == 0.0 || e.fract() == 0.0)
            {
                last.1 += e;
                continue;
            }
            merged.push((base, e));
        }
        if coeff == 0.0 {
            return self.num(0.0);
        }
        // Left-associated chain led by |coeff|; the sign goes outside so
        // that sums can turn `a + -(b)` into `a - b`.
        let mut prod: Option<Id> = (coeff.abs() != 1.0).then(|| self.num(coeff.abs()));
        for (base, e) in merged {
            if e == 0.0 {
                continue;
            }
            let f = if e == 1.0 {
                base
            } else {
                let k = self.num(e);
                self.bin_raw(Bin::Pow, base, k)
            };
            // Keep the canonical order (bin_raw would re-sort by id).
            prod = Some(match prod {
                None => f,
                Some(p) => self.intern(Node::Bin(Bin::Mul, p, f)),
            });
        }
        let Some(prod) = prod else {
            return self.num(coeff);
        };
        if coeff < 0.0 {
            if let Some(c) = self.as_const(prod) {
                return self.num(-c);
            }
            self.intern(Node::Un(Un::Neg, prod))
        } else {
            prod
        }
    }

    /// Structural ordering for product factors, independent of creation
    /// order: variables (x before y), then powers of variables, then
    /// functions, then everything else; ties by id.
    fn order_key(&self, id: Id) -> (u8, u8, Id) {
        match self.node(id) {
            Node::Var(v) => (1, v, id),
            Node::I => (1, 255, id),
            Node::Bin(Bin::Pow, b, _) => match self.node(b) {
                Node::Var(v) => (2, v, id),
                _ => (4, 0, id),
            },
            Node::Un(..) => (3, 0, id),
            _ => (4, 0, id),
        }
    }

    /// Factors of a product: constants go into `coeff`, everything else is
    /// (base, constant exponent).
    fn collect_factors(&self, id: Id, coeff: &mut f64, out: &mut Vec<(Id, f64)>) {
        match self.node(id) {
            Node::Const(b) => *coeff *= f64::from_bits(b),
            Node::Un(Un::Neg, a) => {
                *coeff = -*coeff;
                self.collect_factors(a, coeff, out);
            }
            Node::Bin(Bin::Mul, a, b) => {
                self.collect_factors(a, coeff, out);
                self.collect_factors(b, coeff, out);
            }
            Node::Bin(Bin::Pow, base, e) if self.as_const(e).is_some() => out.push((base, self.as_const(e).unwrap())),
            _ => out.push((id, 1.0)),
        }
    }

    pub fn div(&mut self, a: Id, b: Id) -> Id {
        match (self.as_const(a), self.as_const(b)) {
            (Some(x), Some(y)) if y != 0.0 => return self.num(x / y),
            (_, Some(y)) if y == 1.0 => return a,
            (_, Some(y)) if y == -1.0 => return self.neg(a),
            _ => {}
        }
        if self.is_val(a, 0.0) && self.as_const(b).is_some_and(|y| y != 0.0) {
            return self.num(0.0);
        }
        if let Node::Un(Un::Neg, c) = self.node(a) {
            let d = self.div(c, b);
            return self.neg(d);
        }
        self.bin_raw(Bin::Div, a, b)
    }

    pub fn pow(&mut self, a: Id, b: Id) -> Id {
        if let Some(e) = self.as_const(b) {
            if e == 0.0 {
                return self.num(1.0);
            }
            if e == 1.0 {
                return a;
            }
            if let Some(x) = self.as_const(a) {
                let v = x.powf(e);
                if v.is_finite() {
                    return self.num(v);
                }
            }
            // (u^p)^q -> u^(p*q) is only safe for integer q.
            if e.fract() == 0.0
                && let Node::Bin(Bin::Pow, u, p) = self.node(a)
                && let Some(p) = self.as_const(p)
            {
                let k = self.num(p * e);
                return self.pow(u, k);
            }
        }
        if self.is_val(a, 1.0) {
            return self.num(1.0);
        }
        self.bin_raw(Bin::Pow, a, b)
    }

    pub fn bin(&mut self, op: Bin, a: Id, b: Id) -> Id {
        match op {
            Bin::Add => self.add(a, b),
            Bin::Sub => self.sub(a, b),
            Bin::Mul => self.mul(a, b),
            Bin::Div => self.div(a, b),
            Bin::Pow => self.pow(a, b),
            _ => {
                if let (Some(x), Some(y)) = (self.as_const(a), self.as_const(b)) {
                    let v = eval::bin_f64(op, x, y);
                    if v.is_finite() {
                        return self.num(v);
                    }
                }
                self.bin_raw(op, a, b)
            }
        }
    }

    fn bin_raw(&mut self, op: Bin, a: Id, b: Id) -> Id {
        // Canonical operand order for commutative operators, constants first,
        // so `x*y` and `y*x` (and hence f_xy and f_yx) share nodes.
        let (a, b) = if op.commutative() {
            let ka = (self.as_const(a).is_none(), a);
            let kb = (self.as_const(b).is_none(), b);
            if ka <= kb { (a, b) } else { (b, a) }
        } else {
            (a, b)
        };
        self.intern(Node::Bin(op, a, b))
    }

    pub fn if_(&mut self, c: Id, a: Id, b: Id) -> Id {
        if a == b {
            return a;
        }
        if let Some(v) = self.as_const(c)
            && !v.is_nan()
        {
            return if v != 0.0 { a } else { b };
        }
        self.intern(Node::If(c, a, b))
    }

    /// Whether `id` depends on the imaginary unit or a non-holomorphic
    /// complex-only operation.
    pub fn uses_complex(&self, id: Id) -> bool {
        let mut seen = vec![false; self.nodes.len()];
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if std::mem::replace(&mut seen[n.0 as usize], true) {
                continue;
            }
            match self.node(n) {
                Node::I => return true,
                Node::Un(_, a) => stack.push(a),
                Node::Bin(_, a, b) => stack.extend([a, b]),
                Node::If(c, a, b) => stack.extend([c, a, b]),
                _ => {}
            }
        }
        false
    }

    /// Whether `id` depends on variable `v`.
    pub fn depends_on(&self, id: Id, v: u8) -> bool {
        let mut seen = vec![false; self.nodes.len()];
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            if std::mem::replace(&mut seen[n.0 as usize], true) {
                continue;
            }
            match self.node(n) {
                Node::Var(i) if i == v => return true,
                Node::Un(_, a) => stack.push(a),
                Node::Bin(_, a, b) => stack.extend([a, b]),
                Node::If(c, a, b) => stack.extend([c, a, b]),
                _ => {}
            }
        }
        false
    }
}

/// Variable names used by a parse context, e.g. `["x", "y"]`.
#[derive(Clone, Debug)]
pub struct Vars(pub Vec<&'static str>);

impl Vars {
    pub fn index(&self, name: &str) -> Option<u8> {
        self.0.iter().position(|v| *v == name).map(|i| i as u8)
    }
}

#[cfg(test)]
mod tests;
