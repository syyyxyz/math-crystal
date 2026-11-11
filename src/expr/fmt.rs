//! Pretty printer producing re-parseable text with minimal parentheses.

use super::{Bin, Id, Node, Pool, Un};

pub fn format_num(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    for (k, name) in [(std::f64::consts::PI, "pi"), (std::f64::consts::E, "e")] {
        if v == k {
            return name.into();
        }
        if v == -k {
            return format!("-{name}");
        }
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let a = v.abs();
    if (1e-4..1e6).contains(&a) {
        let s = format!("{v:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        return s.to_string();
    }
    format!("{v:.5e}")
}

fn un_name(op: Un) -> &'static str {
    match op {
        Un::Neg => "-",
        Un::Sin => "sin",
        Un::Cos => "cos",
        Un::Tan => "tan",
        Un::Asin => "asin",
        Un::Acos => "acos",
        Un::Atan => "atan",
        Un::Sinh => "sinh",
        Un::Cosh => "cosh",
        Un::Tanh => "tanh",
        Un::Asinh => "asinh",
        Un::Acosh => "acosh",
        Un::Atanh => "atanh",
        Un::Exp => "exp",
        Un::Ln => "ln",
        Un::Sqrt => "sqrt",
        Un::Cbrt => "cbrt",
        Un::Abs => "abs",
        Un::Sign => "sign",
        Un::Floor => "floor",
        Un::Ceil => "ceil",
        Un::Round => "round",
        Un::Step => "step",
        Un::Sinc => "sinc",
        Un::Re => "re",
        Un::Im => "im",
        Un::Conj => "conj",
        Un::Arg => "arg",
        Un::Not => "not",
    }
}

const P_OR: u8 = 1;
const P_AND: u8 = 2;
const P_CMP: u8 = 3;
const P_ADD: u8 = 4;
const P_MUL: u8 = 5;
const P_NEG: u8 = 6;
const P_POW: u8 = 7;
const P_ATOM: u8 = 8;

impl Pool {
    pub fn to_string(&self, id: Id, var_names: &[&str]) -> String {
        let mut s = String::new();
        self.write(&mut s, id, var_names);
        s
    }

    fn prec(&self, id: Id) -> u8 {
        match self.node(id) {
            Node::Const(b) => {
                if f64::from_bits(b) < 0.0 {
                    P_NEG
                } else {
                    P_ATOM
                }
            }
            Node::Var(_) | Node::I | Node::If(..) => P_ATOM,
            Node::Un(Un::Neg, _) => P_NEG,
            Node::Un(..) => P_ATOM,
            Node::Bin(op, ..) => match op {
                Bin::Or => P_OR,
                Bin::And => P_AND,
                Bin::Lt | Bin::Le | Bin::Gt | Bin::Ge | Bin::Eq | Bin::Ne => P_CMP,
                Bin::Add | Bin::Sub => P_ADD,
                Bin::Mul | Bin::Div => P_MUL,
                Bin::Pow => P_POW,
                Bin::Mod | Bin::Atan2 | Bin::Min | Bin::Max => P_ATOM,
            },
        }
    }

    fn write_min(&self, s: &mut String, id: Id, min: u8, vars: &[&str]) {
        if self.prec(id) < min {
            s.push('(');
            self.write(s, id, vars);
            s.push(')');
        } else {
            self.write(s, id, vars);
        }
    }

    fn write(&self, s: &mut String, id: Id, vars: &[&str]) {
        match self.node(id) {
            Node::Const(b) => s.push_str(&format_num(f64::from_bits(b))),
            Node::Var(v) => s.push_str(vars.get(v as usize).copied().unwrap_or("?")),
            Node::I => s.push('i'),
            Node::If(c, a, b) => {
                s.push_str("if(");
                self.write(s, c, vars);
                s.push_str(", ");
                self.write(s, a, vars);
                s.push_str(", ");
                self.write(s, b, vars);
                s.push(')');
            }
            Node::Un(Un::Neg, a) => {
                // -a*b reads as -(a*b), which has the same value.
                s.push('-');
                self.write_min(s, a, P_MUL, vars);
            }
            Node::Un(Un::Abs, a) => {
                s.push('|');
                self.write(s, a, vars);
                s.push('|');
            }
            Node::Un(op, a) => {
                s.push_str(un_name(op));
                s.push('(');
                self.write(s, a, vars);
                s.push(')');
            }
            Node::Bin(op, a, b) => {
                let func = match op {
                    Bin::Mod => Some("mod"),
                    Bin::Atan2 => Some("atan2"),
                    Bin::Min => Some("min"),
                    Bin::Max => Some("max"),
                    _ => None,
                };
                if let Some(f) = func {
                    s.push_str(f);
                    s.push('(');
                    self.write(s, a, vars);
                    s.push_str(", ");
                    self.write(s, b, vars);
                    s.push(')');
                    return;
                }
                let (sym, lp, rp) = match op {
                    Bin::Or => (" or ", P_OR, P_AND),
                    Bin::And => (" and ", P_AND, P_CMP),
                    Bin::Lt => (" < ", P_ADD, P_ADD),
                    Bin::Le => (" <= ", P_ADD, P_ADD),
                    Bin::Gt => (" > ", P_ADD, P_ADD),
                    Bin::Ge => (" >= ", P_ADD, P_ADD),
                    Bin::Eq => (" == ", P_ADD, P_ADD),
                    Bin::Ne => (" != ", P_ADD, P_ADD),
                    Bin::Add => (" + ", P_ADD, P_ADD),
                    Bin::Sub => (" - ", P_ADD, P_MUL),
                    Bin::Mul => ("*", P_MUL, P_NEG + 1),
                    Bin::Div => ("/", P_MUL, P_NEG + 1),
                    Bin::Pow => ("^", P_ATOM, P_ATOM),
                    _ => unreachable!(),
                };
                // `x + -y` reads better as `x - y` (only reachable for constants).
                if op == Bin::Add
                    && let Some(c) = self.as_const(b)
                    && c < 0.0
                {
                    self.write_min(s, a, lp, vars);
                    s.push_str(" - ");
                    s.push_str(&format_num(-c));
                    return;
                }
                self.write_min(s, a, lp, vars);
                s.push_str(sym);
                self.write_min(s, b, rp, vars);
            }
        }
    }
}
