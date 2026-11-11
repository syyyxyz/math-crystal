//! Symbolic differentiation on the hash-consed DAG (memoised per node).
//!
//! Distributions are ignored: step, sign, floor and comparisons have
//! derivative 0 away from their jumps. Piecewise `if` differentiates each
//! branch, which is wrong exactly on a branch boundary; callers that care
//! (point derivatives) cross-check against difference quotients.

use super::{Bin, Id, Node, Pool, Un};

impl Pool {
    /// d(id)/d(var).
    pub fn diff(&mut self, id: Id, var: u8) -> Id {
        if let Some(&d) = self.diff_memo.get(&(id, var)) {
            return d;
        }
        let d = self.diff_node(id, var);
        self.diff_memo.insert((id, var), d);
        d
    }

    fn diff_node(&mut self, id: Id, v: u8) -> Id {
        let zero = self.num(0.0);
        if !self.depends_on(id, v) {
            return zero;
        }
        let one = self.num(1.0);
        let two = self.num(2.0);
        match self.node(id) {
            Node::Const(_) | Node::I => zero,
            Node::Var(i) => {
                if i == v {
                    one
                } else {
                    zero
                }
            }
            Node::If(c, a, b) => {
                let da = self.diff(a, v);
                let db = self.diff(b, v);
                self.if_(c, da, db)
            }
            Node::Un(op, a) => {
                let da = self.diff(a, v);
                // outer'(a); the result is outer'(a) * da.
                let outer = match op {
                    Un::Neg => return self.neg(da),
                    Un::Sin => self.un(Un::Cos, a),
                    Un::Cos => {
                        let s = self.un(Un::Sin, a);
                        self.neg(s)
                    }
                    Un::Tan => {
                        // 1 + tan^2
                        let t = self.un(Un::Tan, a);
                        let t2 = self.pow(t, two);
                        self.add(one, t2)
                    }
                    Un::Asin | Un::Acos => {
                        let a2 = self.pow(a, two);
                        let r = self.sub(one, a2);
                        let s = self.un(Un::Sqrt, r);
                        let q = self.div(one, s);
                        if op == Un::Acos { self.neg(q) } else { q }
                    }
                    Un::Atan => {
                        let a2 = self.pow(a, two);
                        let d = self.add(one, a2);
                        self.div(one, d)
                    }
                    Un::Sinh => self.un(Un::Cosh, a),
                    Un::Cosh => self.un(Un::Sinh, a),
                    Un::Tanh => {
                        let t = self.un(Un::Tanh, a);
                        let t2 = self.pow(t, two);
                        self.sub(one, t2)
                    }
                    Un::Asinh => {
                        let a2 = self.pow(a, two);
                        let r = self.add(a2, one);
                        let s = self.un(Un::Sqrt, r);
                        self.div(one, s)
                    }
                    Un::Acosh => {
                        let a2 = self.pow(a, two);
                        let r = self.sub(a2, one);
                        let s = self.un(Un::Sqrt, r);
                        self.div(one, s)
                    }
                    Un::Atanh => {
                        let a2 = self.pow(a, two);
                        let r = self.sub(one, a2);
                        self.div(one, r)
                    }
                    Un::Exp => id,
                    Un::Ln => self.div(one, a),
                    Un::Sqrt => {
                        let d = self.mul(two, id);
                        self.div(one, d)
                    }
                    Un::Cbrt => {
                        // 1 / (3 cbrt(a)^2)
                        let c2 = self.pow(id, two);
                        let three = self.num(3.0);
                        let d = self.mul(three, c2);
                        self.div(one, d)
                    }
                    Un::Abs => self.un(Un::Sign, a),
                    Un::Sinc => {
                        // (cos a - sinc a) / a, with the removable 0 at a = 0.
                        let c = self.un(Un::Cos, a);
                        let q = self.sub(c, id);
                        let r = self.div(q, a);
                        let at0 = self.bin(Bin::Eq, a, zero);
                        self.if_(at0, zero, r)
                    }
                    Un::Re => return self.un(Un::Re, da),
                    Un::Im => return self.un(Un::Im, da),
                    Un::Conj => return self.un(Un::Conj, da),
                    Un::Arg | Un::Sign | Un::Floor | Un::Ceil | Un::Round | Un::Step | Un::Not => {
                        return zero;
                    }
                };
                self.mul(outer, da)
            }
            Node::Bin(op, a, b) => {
                let da = self.diff(a, v);
                let db = self.diff(b, v);
                match op {
                    Bin::Add => self.add(da, db),
                    Bin::Sub => self.sub(da, db),
                    Bin::Mul => {
                        let l = self.mul(da, b);
                        let r = self.mul(a, db);
                        self.add(l, r)
                    }
                    Bin::Div => {
                        if self.as_const(db) == Some(0.0) {
                            return self.div(da, b);
                        }
                        // (a' b - a b') / b^2
                        let l = self.mul(da, b);
                        let r = self.mul(a, db);
                        let n = self.sub(l, r);
                        let b2 = self.pow(b, two);
                        self.div(n, b2)
                    }
                    Bin::Pow => {
                        if let Some(e) = self.as_const(b) {
                            // e a^(e-1) a'
                            let em1 = self.num(e - 1.0);
                            let p = self.pow(a, em1);
                            let k = self.num(e);
                            let t = self.mul(k, p);
                            return self.mul(t, da);
                        }
                        if !self.depends_on(a, v) {
                            // a^b ln(a) b'
                            let l = self.un(Un::Ln, a);
                            let t = self.mul(id, l);
                            return self.mul(t, db);
                        }
                        // a^b (b' ln a + b a'/a)
                        let l = self.un(Un::Ln, a);
                        let t1 = self.mul(db, l);
                        let q = self.div(da, a);
                        let t2 = self.mul(b, q);
                        let s = self.add(t1, t2);
                        self.mul(id, s)
                    }
                    Bin::Mod => {
                        // a - b floor(a/b)
                        let q = self.div(a, b);
                        let f = self.un(Un::Floor, q);
                        let t = self.mul(db, f);
                        self.sub(da, t)
                    }
                    Bin::Atan2 => {
                        // atan2(a, b) = angle of (b, a): (b a' - a b') / (a^2 + b^2)
                        let l = self.mul(b, da);
                        let r = self.mul(a, db);
                        let n = self.sub(l, r);
                        let a2 = self.pow(a, two);
                        let b2 = self.pow(b, two);
                        let d = self.add(a2, b2);
                        self.div(n, d)
                    }
                    Bin::Min | Bin::Max => {
                        let c = if op == Bin::Min {
                            self.bin(Bin::Le, a, b)
                        } else {
                            self.bin(Bin::Ge, a, b)
                        };
                        self.if_(c, da, db)
                    }
                    Bin::Lt | Bin::Le | Bin::Gt | Bin::Ge | Bin::Eq | Bin::Ne | Bin::And | Bin::Or => zero,
                }
            }
        }
    }
}
