//! Pratt-style recursive descent parser.
//!
//! Precedence, low to high: `or`, `and`, comparisons (chainable:
//! `a < b < c`), `+ -`, `* / %` and implicit multiplication (`2x`, `3(x+1)`,
//! `x sin(y)`), unary `- + !`, `^` / `**` (right associative, so `-x^2` is
//! `-(x^2)` and `2^-x` works), then atoms.

use super::{Bin, Id, Pool, Un, Vars};
use crate::i18n::{tr, zh};

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub msg: String,
    /// Byte offset into the input where the problem was found.
    pub pos: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // LaTeX errors carry no column.
        if self.pos == usize::MAX {
            write!(f, "{}", self.msg)
        } else if zh() { write!(f, "{}（第 {} 列）", self.msg, self.pos + 1) } else { write!(f, "{} (at column {})", self.msg, self.pos + 1) }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(&'static str),
    End,
}

fn lex(src: &str) -> Result<Vec<(Tok, usize)>, ParseError> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = src[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let start = i;
        if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            // Exponent only when followed by a digit (so `2e` is 2*e).
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    i = j;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text = &src[start..i];
            let v: f64 = text.parse().map_err(|_| ParseError {
                msg: format!("{} '{text}'", tr("Bad number", "无效数字")),
                pos: start,
            })?;
            out.push((Tok::Num(v), start));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            while i < src.len() {
                let ch = src[i..].chars().next().unwrap();
                if ch.is_alphanumeric() || ch == '_' {
                    i += ch.len_utf8();
                } else {
                    break;
                }
            }
            out.push((Tok::Ident(src[start..i].to_string()), start));
            continue;
        }
        const OPS: [&str; 22] = [
            "**", "<=", ">=", "==", "!=", "&&", "+", "-", "*", "/", "^", "(", ")", ",", "|", "<",
            ">", "=", "!", "%", "·", "×",
        ];
        let Some(op) = OPS.iter().find(|op| src[i..].starts_with(**op)) else {
            return Err(ParseError {
                msg: format!("{} '{c}'", tr("Unexpected character", "无法识别的字符")),
                pos: start,
            });
        };
        i += op.len();
        let canon = match *op {
            "**" => "^",
            "=" => "==",
            "&&" => "and",
            "·" | "×" => "*",
            o => o,
        };
        out.push((Tok::Op(canon), start));
    }
    out.push((Tok::End, src.len()));
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    pool: &'a mut Pool,
    vars: &'a Vars,
    allow_i: bool,
    depth: usize,
    /// When present, unknown single-letter names become parameters.
    params: Option<&'a mut Vec<String>>,
}

const MAX_DEPTH: usize = 200;
/// Most parameters one expression may introduce.
pub const MAX_PARAMS: usize = 8;

/// Whether an otherwise unknown name is taken as a parameter: one Latin or
/// Greek letter, optionally followed by digits (`a`, `k2`, `T`, `ω`). The
/// usual variable names and constants (x, y, z, t, e, i, j, f, π, τ) are
/// left out so that typos in them still give an error.
pub fn is_param_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(c) = chars.next() else { return false };
    let letter = c.is_ascii_alphabetic() || ('α'..='ω').contains(&c) || ('Α'..='Ω').contains(&c);
    letter && !matches!(c, 'x' | 'y' | 'z' | 't' | 'e' | 'i' | 'j' | 'f' | 'π' | 'τ') && chars.all(|d| d.is_ascii_digit())
}

/// Parse `src` into `pool`. `allow_i` enables the imaginary unit `i`.
pub fn parse(src: &str, pool: &mut Pool, vars: &Vars, allow_i: bool) -> Result<Id, ParseError> {
    parse_inner(src, pool, vars, allow_i, None)
}

/// Like [`parse`], but names accepted by [`is_param_name`] become
/// parameters: variables numbered after `vars`, in order of first use;
/// their names are appended to `params`.
pub fn parse_with_params(src: &str, pool: &mut Pool, vars: &Vars, allow_i: bool, params: &mut Vec<String>) -> Result<Id, ParseError> {
    parse_inner(src, pool, vars, allow_i, Some(params))
}

fn parse_inner(src: &str, pool: &mut Pool, vars: &Vars, allow_i: bool, params: Option<&mut Vec<String>>) -> Result<Id, ParseError> {
    // LaTeX is translated first; error columns then refer to the translation,
    // which the UI shows next to the input.
    // Pasted text is tidied first (delimiters, typographic Unicode,
    // trailing punctuation); plain input that needs no tidying keeps its
    // original columns.
    let cleaned = super::latex::clean(src);
    let translated;
    let src = if super::latex::looks_like_latex(&cleaned) || super::latex::is_typeset(src) {
        translated = super::latex::to_plain(&cleaned)?;
        translated.as_str()
    } else if cleaned == src.trim() {
        super::latex::strip_lhs(src)
    } else {
        translated = cleaned;
        super::latex::strip_lhs(&translated)
    };
    if params.is_some() {
        pool.first_param = Some(vars.0.len() as u8);
    }
    let toks = lex(src)?;
    if toks.len() == 1 {
        return Err(ParseError {
            msg: tr("Expression is empty", "表达式为空").into(),
            pos: 0,
        });
    }
    let mut p = Parser {
        toks,
        pos: 0,
        pool,
        vars,
        allow_i,
        depth: 0,
        params,
    };
    let id = p.or()?;
    match p.peek() {
        Tok::End => Ok(id),
        Tok::Op(")") => Err(p.err(tr("Unmatched ')'", "多余的 ')'"))),
        t => Err(p.err(&format!("{} {}", tr("Unexpected", "多余的"), describe(t)))),
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Num(v) => format!("{} {v}", tr("number", "数字")),
        Tok::Ident(s) => format!("{} '{s}'", tr("name", "名称")),
        Tok::Op(o) => format!("'{o}'"),
        Tok::End => tr("end of input", "输入结尾").into(),
    }
}

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }

    fn at(&self) -> usize {
        self.toks[self.pos].1
    }

    fn err(&self, msg: &str) -> ParseError {
        ParseError {
            msg: msg.to_string(),
            pos: self.at(),
        }
    }

    fn eat_op(&mut self, op: &str) -> bool {
        if matches!(self.peek(), Tok::Op(o) if *o == op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, w: &str) -> bool {
        if matches!(self.peek(), Tok::Ident(s) if s == w) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, op: &str) -> Result<(), ParseError> {
        if self.eat_op(op) {
            Ok(())
        } else {
            Err(self.err(&format!("{} '{op}', {} {}", tr("Expected", "此处应为"), tr("found", "实际为"), describe(self.peek()))))
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(self.err(tr("Expression is nested too deeply", "表达式嵌套过深")))
        } else {
            Ok(())
        }
    }

    fn or(&mut self) -> Result<Id, ParseError> {
        self.enter()?;
        let mut a = self.and()?;
        while self.eat_word("or") {
            let b = self.and()?;
            a = self.pool.bin(Bin::Or, a, b);
        }
        self.depth -= 1;
        Ok(a)
    }

    fn and(&mut self) -> Result<Id, ParseError> {
        let mut a = self.cmp()?;
        while self.eat_word("and") || self.eat_op("and") {
            let b = self.cmp()?;
            a = self.pool.bin(Bin::And, a, b);
        }
        Ok(a)
    }

    fn cmp_op(&mut self) -> Option<Bin> {
        let op = match self.peek() {
            Tok::Op("<") => Bin::Lt,
            Tok::Op("<=") => Bin::Le,
            Tok::Op(">") => Bin::Gt,
            Tok::Op(">=") => Bin::Ge,
            Tok::Op("==") => Bin::Eq,
            Tok::Op("!=") => Bin::Ne,
            _ => return None,
        };
        self.pos += 1;
        Some(op)
    }

    fn cmp(&mut self) -> Result<Id, ParseError> {
        let mut left = self.sum()?;
        let mut acc: Option<Id> = None;
        while let Some(op) = self.cmp_op() {
            let right = self.sum()?;
            let c = self.pool.bin(op, left, right);
            acc = Some(match acc {
                None => c,
                Some(prev) => self.pool.bin(Bin::And, prev, c),
            });
            left = right;
        }
        Ok(acc.unwrap_or(left))
    }

    fn sum(&mut self) -> Result<Id, ParseError> {
        let mut a = self.term()?;
        loop {
            if self.eat_op("+") {
                let b = self.term()?;
                a = self.pool.add(a, b);
            } else if self.eat_op("-") {
                let b = self.term()?;
                a = self.pool.sub(a, b);
            } else {
                return Ok(a);
            }
        }
    }

    fn starts_atom(&self) -> bool {
        match self.peek() {
            Tok::Num(_) | Tok::Op("(") => true,
            Tok::Ident(s) => !matches!(s.as_str(), "and" | "or"),
            _ => false,
        }
    }

    fn term(&mut self) -> Result<Id, ParseError> {
        let mut a = self.unary()?;
        loop {
            if self.eat_op("*") {
                let b = self.unary()?;
                a = self.pool.mul(a, b);
            } else if self.eat_op("/") {
                let b = self.unary()?;
                a = self.pool.div(a, b);
            } else if self.eat_op("%") {
                let b = self.unary()?;
                a = self.pool.bin(Bin::Mod, a, b);
            } else if self.starts_atom() {
                // Implicit multiplication binds like `*`: 2x^2 = 2*(x^2).
                let b = self.power()?;
                a = self.pool.mul(a, b);
            } else {
                return Ok(a);
            }
        }
    }

    fn unary(&mut self) -> Result<Id, ParseError> {
        self.enter()?;
        let r = if self.eat_op("-") {
            let a = self.unary()?;
            Ok(self.pool.neg(a))
        } else if self.eat_op("+") {
            self.unary()
        } else if self.eat_op("!") || self.eat_word("not") {
            let a = self.unary()?;
            Ok(self.pool.un(Un::Not, a))
        } else {
            self.power()
        };
        self.depth -= 1;
        r
    }

    fn power(&mut self) -> Result<Id, ParseError> {
        let base = self.atom()?;
        if self.eat_op("^") {
            let e = self.unary()?;
            Ok(self.pool.pow(base, e))
        } else {
            Ok(base)
        }
    }

    fn args(&mut self, name: &str, pos: usize) -> Result<Vec<Id>, ParseError> {
        if !self.eat_op("(") {
            return Err(ParseError {
                msg: format!("{} '{name}' {} {name}(x)", tr("Function", "函数"), tr("needs parentheses, e.g.", "需要括号，例如")),
                pos,
            });
        }
        let mut args = vec![self.or()?];
        while self.eat_op(",") {
            args.push(self.or()?);
        }
        self.expect(")")?;
        Ok(args)
    }

    fn atom(&mut self) -> Result<Id, ParseError> {
        self.enter()?;
        let r = self.atom_inner();
        self.depth -= 1;
        r
    }

    fn atom_inner(&mut self) -> Result<Id, ParseError> {
        let pos = self.at();
        match self.peek().clone() {
            Tok::Num(v) => {
                self.pos += 1;
                Ok(self.pool.num(v))
            }
            Tok::Op("(") => {
                self.pos += 1;
                let e = self.or()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::Op("|") => {
                self.pos += 1;
                let e = self.sum()?;
                self.expect("|")?;
                Ok(self.pool.un(Un::Abs, e))
            }
            Tok::Ident(name) => {
                self.pos += 1;
                self.ident(&name, pos)
            }
            t => Err(self.err(&format!("{} {}", tr("Unexpected", "多余的"), describe(&t)))),
        }
    }

    fn ident(&mut self, name: &str, pos: usize) -> Result<Id, ParseError> {
        if let Some(v) = self.vars.index(name) {
            return Ok(self.pool.var(v));
        }
        match name {
            "pi" | "π" => return Ok(self.pool.num(std::f64::consts::PI)),
            "tau" | "τ" => return Ok(self.pool.num(std::f64::consts::TAU)),
            "e" => return Ok(self.pool.num(std::f64::consts::E)),
            "i" | "j" if self.allow_i => return Ok(self.pool.imag()),
            _ => {}
        }
        let Some(spec) = function(name) else {
            if let Some(params) = self.params.as_deref_mut()
                && is_param_name(name)
            {
                let k = match params.iter().position(|p| p == name) {
                    Some(k) => k,
                    None if params.len() >= MAX_PARAMS => {
                        return Err(ParseError {
                            msg: if zh() { format!("参数太多（最多 {MAX_PARAMS} 个）：'{name}'") } else { format!("Too many parameters (at most {MAX_PARAMS}): '{name}'") },
                            pos,
                        });
                    }
                    None => {
                        params.push(name.to_string());
                        params.len() - 1
                    }
                };
                return Ok(self.pool.var((self.vars.0.len() + k) as u8));
            }
            let with_params = self.params.is_some();
            let hint = if name.chars().count() > 1
                && name
                    .chars()
                    .all(|c| self.vars.index(&c.to_string()).is_some() || (with_params && is_param_name(&c.to_string())))
            {
                let parts: Vec<String> = name.chars().map(String::from).collect();
                format!(" ({} {})", tr("for a product write", "乘积请写成"), parts.join("*"))
            } else if (name == "i" || name == "j") && !self.allow_i {
                tr(" (complex numbers are only available in the Complex workspace)", "（复数只能在“复变函数”工作区使用）").to_string()
            } else {
                format!(" ({}: {})", tr("variables here", "可用变量"), self.vars.0.join(", "))
            };
            return Err(ParseError {
                msg: format!("{} '{name}'{hint}", tr("Unknown name", "未知名称")),
                pos,
            });
        };
        let args = self.args(name, pos)?;
        if !spec.arity.contains(&args.len()) {
            let want = match (spec.arity.start(), spec.arity.end()) {
                (a, b) if a == b => format!("{a}"),
                (a, b) => format!("{a}{}{b}", tr(" to ", "~")),
            };
            return Err(ParseError {
                msg: if zh() { format!("'{name}' 需要 {want} 个参数，实际 {} 个", args.len()) } else { format!("'{name}' takes {want} argument(s), got {}", args.len()) },
                pos,
            });
        }
        Ok((spec.build)(self.pool, &args))
    }
}

pub struct FnSpec {
    pub name: &'static str,
    pub arity: std::ops::RangeInclusive<usize>,
    pub help: &'static str,
    build: fn(&mut Pool, &[Id]) -> Id,
}

macro_rules! unary {
    ($name:literal, $op:expr, $help:literal) => {
        FnSpec {
            name: $name,
            arity: 1..=1,
            help: $help,
            build: |p, a| p.un($op, a[0]),
        }
    };
}

pub static FUNCTIONS: &[FnSpec] = &[
    unary!("sin", Un::Sin, "sine"),
    unary!("cos", Un::Cos, "cosine"),
    unary!("tan", Un::Tan, "tangent"),
    FnSpec { name: "cot", arity: 1..=1, help: "cotangent", build: |p, a| { let c = p.un(Un::Cos, a[0]); let s = p.un(Un::Sin, a[0]); p.div(c, s) } },
    FnSpec { name: "sec", arity: 1..=1, help: "secant", build: |p, a| { let one = p.num(1.0); let c = p.un(Un::Cos, a[0]); p.div(one, c) } },
    FnSpec { name: "csc", arity: 1..=1, help: "cosecant", build: |p, a| { let one = p.num(1.0); let s = p.un(Un::Sin, a[0]); p.div(one, s) } },
    unary!("asin", Un::Asin, "inverse sine"),
    unary!("acos", Un::Acos, "inverse cosine"),
    FnSpec { name: "atan", arity: 1..=2, help: "inverse tangent; atan(y, x) is the two-argument form", build: |p, a| if a.len() == 2 { p.bin(Bin::Atan2, a[0], a[1]) } else { p.un(Un::Atan, a[0]) } },
    FnSpec { name: "atan2", arity: 2..=2, help: "atan2(y, x)", build: |p, a| p.bin(Bin::Atan2, a[0], a[1]) },
    unary!("arcsin", Un::Asin, "inverse sine"),
    unary!("arccos", Un::Acos, "inverse cosine"),
    unary!("arctan", Un::Atan, "inverse tangent"),
    unary!("sinh", Un::Sinh, "hyperbolic sine"),
    unary!("cosh", Un::Cosh, "hyperbolic cosine"),
    unary!("tanh", Un::Tanh, "hyperbolic tangent"),
    unary!("asinh", Un::Asinh, "inverse hyperbolic sine"),
    unary!("acosh", Un::Acosh, "inverse hyperbolic cosine"),
    unary!("atanh", Un::Atanh, "inverse hyperbolic tangent"),
    unary!("exp", Un::Exp, "e^x"),
    unary!("ln", Un::Ln, "natural logarithm"),
    FnSpec { name: "log", arity: 1..=2, help: "natural log; log(b, x) is log base b", build: |p, a| if a.len() == 2 { let n = p.un(Un::Ln, a[1]); let d = p.un(Un::Ln, a[0]); p.div(n, d) } else { p.un(Un::Ln, a[0]) } },
    FnSpec { name: "log10", arity: 1..=1, help: "base-10 logarithm", build: |p, a| { let n = p.un(Un::Ln, a[0]); let d = p.num(std::f64::consts::LN_10); p.div(n, d) } },
    FnSpec { name: "log2", arity: 1..=1, help: "base-2 logarithm", build: |p, a| { let n = p.un(Un::Ln, a[0]); let d = p.num(std::f64::consts::LN_2); p.div(n, d) } },
    unary!("sqrt", Un::Sqrt, "square root"),
    unary!("cbrt", Un::Cbrt, "real cube root"),
    unary!("abs", Un::Abs, "absolute value / modulus, also |x|"),
    unary!("sign", Un::Sign, "sign: -1, 0 or 1"),
    unary!("sgn", Un::Sign, "sign: -1, 0 or 1"),
    unary!("floor", Un::Floor, "round down"),
    unary!("ceil", Un::Ceil, "round up"),
    unary!("round", Un::Round, "round to nearest"),
    unary!("step", Un::Step, "Heaviside step (1/2 at 0)"),
    unary!("heaviside", Un::Step, "Heaviside step (1/2 at 0)"),
    unary!("sinc", Un::Sinc, "sin(x)/x, 1 at 0"),
    FnSpec { name: "rect", arity: 1..=1, help: "1 for |x| < 1/2, else 0", build: |p, a| { let ax = p.un(Un::Abs, a[0]); let h = p.num(0.5); let d = p.sub(h, ax); p.un(Un::Step, d) } },
    FnSpec { name: "tri", arity: 1..=1, help: "max(1 - |x|, 0)", build: |p, a| { let ax = p.un(Un::Abs, a[0]); let one = p.num(1.0); let zero = p.num(0.0); let d = p.sub(one, ax); p.bin(Bin::Max, d, zero) } },
    FnSpec { name: "pow", arity: 2..=2, help: "pow(a, b) = a^b", build: |p, a| p.pow(a[0], a[1]) },
    FnSpec { name: "hypot", arity: 2..=2, help: "sqrt(a^2 + b^2)", build: |p, a| { let two = p.num(2.0); let x = p.pow(a[0], two); let y = p.pow(a[1], two); let s = p.add(x, y); p.un(Un::Sqrt, s) } },
    FnSpec { name: "mod", arity: 2..=2, help: "mod(a, b), result has the sign of b", build: |p, a| p.bin(Bin::Mod, a[0], a[1]) },
    FnSpec { name: "min", arity: 2..=8, help: "minimum", build: |p, a| a[1..].iter().fold(a[0], |acc, &x| p.bin(Bin::Min, acc, x)) },
    FnSpec { name: "max", arity: 2..=8, help: "maximum", build: |p, a| a[1..].iter().fold(a[0], |acc, &x| p.bin(Bin::Max, acc, x)) },
    FnSpec { name: "if", arity: 3..=3, help: "if(cond, a, b): a where cond holds, else b", build: |p, a| p.if_(a[0], a[1], a[2]) },
    unary!("re", Un::Re, "real part"),
    unary!("im", Un::Im, "imaginary part"),
    unary!("conj", Un::Conj, "complex conjugate"),
    unary!("arg", Un::Arg, "argument (phase angle)"),
];

pub fn function(name: &str) -> Option<&'static FnSpec> {
    FUNCTIONS.iter().find(|f| f.name == name)
}
