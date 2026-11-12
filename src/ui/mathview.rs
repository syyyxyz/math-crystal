//! Small maths typesetter: lays out an expression tree as a formula
//! (fractions, powers, roots, delimiters, cases) and paints it with egui.

use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, Galley, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};

use crate::expr::{Bin, Id, Node, Pool, Un, fmt::format_num};

/// Expressions with more tree nodes than this are shown as text instead.
pub const MAX_NODES: usize = 400;

enum Prim {
    Text(Pos2, Arc<Galley>),
    Line(Pos2, Pos2, f32),
    Path(Vec<Pos2>, f32),
}

/// A laid-out box. Coordinates are relative to the left end of the
/// baseline; `asc` is above it, `desc` below.
struct L {
    w: f32,
    asc: f32,
    desc: f32,
    prims: Vec<Prim>,
}

impl L {
    fn empty() -> Self {
        L { w: 0.0, asc: 0.0, desc: 0.0, prims: vec![] }
    }

    fn shifted(mut self, d: Vec2) -> Self {
        for p in &mut self.prims {
            match p {
                Prim::Text(q, _) => *q += d,
                Prim::Line(a, b, _) => {
                    *a += d;
                    *b += d;
                }
                Prim::Path(ps, _) => ps.iter_mut().for_each(|q| *q += d),
            }
        }
        self
    }

    /// Append `o` on the same baseline.
    fn push(&mut self, o: L) {
        let dx = self.w;
        self.w += o.w;
        self.asc = self.asc.max(o.asc);
        self.desc = self.desc.max(o.desc);
        self.prims.extend(o.shifted(vec2(dx, 0.0)).prims);
    }

    fn space(&mut self, w: f32) {
        self.w += w;
    }
}

struct Ctx<'a> {
    ui: &'a Ui,
    pool: &'a Pool,
    vars: &'a [&'a str],
    color: Color32,
}

/// Precedence, as in the text printer.
fn prec(pool: &Pool, id: Id) -> u8 {
    match pool.node(id) {
        Node::Const(b) => {
            if f64::from_bits(b) < 0.0 {
                6
            } else {
                8
            }
        }
        Node::Var(_) | Node::I | Node::If(..) => 8,
        Node::Un(Un::Neg, _) => 6,
        Node::Un(..) => 8,
        Node::Bin(op, ..) => match op {
            Bin::Or => 1,
            Bin::And => 2,
            Bin::Lt | Bin::Le | Bin::Gt | Bin::Ge | Bin::Eq | Bin::Ne => 3,
            Bin::Add | Bin::Sub => 4,
            Bin::Mul => 5,
            Bin::Div => 8,
            Bin::Pow => 7,
            Bin::Mod | Bin::Atan2 | Bin::Min | Bin::Max => 8,
        },
    }
}

/// Number of tree nodes, stopping once `limit` is passed.
pub fn tree_size(pool: &Pool, id: Id, limit: usize) -> usize {
    fn go(pool: &Pool, id: Id, n: &mut usize, limit: usize) {
        *n += 1;
        if *n > limit {
            return;
        }
        match pool.node(id) {
            Node::Un(_, a) => go(pool, a, n, limit),
            Node::Bin(_, a, b) => {
                go(pool, a, n, limit);
                go(pool, b, n, limit);
            }
            Node::If(c, a, b) => {
                go(pool, c, n, limit);
                go(pool, a, n, limit);
                go(pool, b, n, limit);
            }
            _ => {}
        }
    }
    let mut n = 0;
    go(pool, id, &mut n, limit);
    n
}

impl Ctx<'_> {
    fn text(&self, s: &str, size: f32, italic: bool) -> L {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            s,
            0.0,
            egui::TextFormat {
                font_id: FontId::new(size, FontFamily::Proportional),
                color: self.color,
                italics: italic,
                ..Default::default()
            },
        );
        let g = self.ui.painter().layout_job(job);
        let h = g.size().y;
        // Italic letters lean right: add an italic correction.
        let (w, asc) = (g.size().x + if italic { size * 0.14 } else { 0.0 }, h * 0.78);
        L { w, asc, desc: h - asc, prims: vec![Prim::Text(pos2(0.0, -asc), g)] }
    }

    fn op(&self, s: &str, size: f32) -> L {
        let mut l = L::empty();
        l.space(size * 0.22);
        l.push(self.text(s, size, false));
        l.space(size * 0.22);
        l
    }

    /// Height of the maths axis (fraction bars) above the baseline.
    fn axis(size: f32) -> f32 {
        size * 0.3
    }

    fn delim(&self, inner: L, open: char, close: char, size: f32) -> L {
        let mut out = L::empty();
        let tall = inner.asc + inner.desc > size * 1.35;
        if !tall {
            out.push(self.text(&open.to_string(), size, false));
            out.push(inner);
            out.push(self.text(&close.to_string(), size, false));
            return out;
        }
        let pad = size * 0.08;
        let (top, bot) = (-inner.asc - pad, inner.desc + pad);
        // Taller delimiters get wider and curve more.
        let wd = size * 0.3 + (bot - top) * 0.05;
        let stroke = (size * 0.065).max(1.0);
        let shape = |c: char, x0: f32| -> L {
            let mut l = L { w: wd, asc: -top, desc: bot, prims: vec![] };
            let pts: Vec<Pos2> = (0..=16)
                .map(|k| {
                    let t = k as f32 / 16.0;
                    let y = top + (bot - top) * t;
                    let bulge = (std::f32::consts::PI * t).sin() * wd * 0.5;
                    let x = match c {
                        '(' => x0 + wd * 0.75 - bulge,
                        ')' => x0 + wd * 0.25 + bulge,
                        _ => x0 + wd * 0.5,
                    };
                    pos2(x - x0, y)
                })
                .collect();
            l.prims.push(Prim::Path(pts, stroke));
            l
        };
        out.push(shape(open, 0.0));
        out.push(inner);
        out.push(shape(close, 0.0));
        out
    }

    fn paren_if(&self, id: Id, min: u8, size: f32) -> L {
        let l = self.build(id, size);
        if prec(self.pool, id) < min { self.delim(l, '(', ')', size) } else { l }
    }

    fn func(&self, name: &str, args: &[Id], size: f32) -> L {
        let mut l = self.text(name, size, false);
        l.space(size * 0.08);
        let mut inner = L::empty();
        for (k, &a) in args.iter().enumerate() {
            if k > 0 {
                inner.push(self.text(",", size, false));
                inner.space(size * 0.25);
            }
            inner.push(self.build(a, size));
        }
        l.push(self.delim(inner, '(', ')', size));
        l
    }

    fn frac(&self, a: Id, b: Id, size: f32) -> L {
        let s2 = (size * 0.9).max(10.0);
        let num = self.build(a, s2);
        let den = self.build(b, s2);
        let w = num.w.max(den.w) + size * 0.3;
        let ax = Self::axis(size);
        let gap = size * 0.14;
        let stroke = (size * 0.06).max(1.0);
        let mut l = L { w, asc: ax + gap + num.asc + num.desc, desc: -ax + gap + den.asc + den.desc, prims: vec![] };
        let (nw, nd, da, dw) = (num.w, num.desc, den.asc, den.w);
        l.prims.extend(num.shifted(vec2((w - nw) / 2.0, -ax - gap - nd)).prims);
        l.prims.extend(den.shifted(vec2((w - dw) / 2.0, -ax + gap + da)).prims);
        l.prims.push(Prim::Line(pos2(size * 0.05, -ax), pos2(w - size * 0.05, -ax), stroke));
        let mut out = L::empty();
        out.space(size * 0.06);
        out.push(l);
        out.space(size * 0.06);
        out
    }

    fn sqrt(&self, inner: L, index: Option<&str>, size: f32) -> L {
        let gap = size * 0.12;
        let sw = size * 0.6;
        let top = -inner.asc - gap;
        let bot = inner.desc;
        let stroke = (size * 0.06).max(1.0);
        let h = bot - top;
        let mut l = L { w: sw + inner.w + size * 0.1, asc: -top + stroke, desc: bot, prims: vec![] };
        let pts = vec![
            pos2(0.0, bot - h * 0.42),
            pos2(sw * 0.25, bot - h * 0.5),
            pos2(sw * 0.55, bot),
            pos2(sw, top),
            pos2(l.w, top),
        ];
        l.prims.push(Prim::Path(pts, stroke));
        l.prims.extend(inner.shifted(vec2(sw, 0.0)).prims);
        if let Some(n) = index {
            let small = self.text(n, size * 0.6, false);
            let lift = h * 0.45;
            l.prims.extend(small.shifted(vec2(0.0, bot - lift - size * 0.05)).prims);
        }
        l
    }

    fn sup(&self, base: L, exp: L, size: f32) -> L {
        let raise = (base.asc * 0.62).max(size * 0.42);
        let mut l = base;
        let w = l.w;
        let e_asc = exp.asc;
        let e_w = exp.w;
        l.prims.extend(exp.shifted(vec2(w + size * 0.04, -raise)).prims);
        l.w += e_w + size * 0.06;
        l.asc = l.asc.max(raise + e_asc);
        l
    }

    fn cases(&self, rows: Vec<(L, Option<L>)>, size: f32) -> L {
        let row_gap = size * 0.35;
        let col_gap = size * 0.9;
        let ew = rows.iter().map(|r| r.0.w).fold(0.0, f32::max);
        let mut body = L::empty();
        let mut y = 0.0f32;
        let mut first_asc = 0.0;
        let mut row_pos = Vec::new();
        for (k, (e, c)) in rows.iter().enumerate() {
            let asc = e.asc.max(c.as_ref().map_or(0.0, |c| c.asc));
            let desc = e.desc.max(c.as_ref().map_or(0.0, |c| c.desc));
            if k == 0 {
                first_asc = asc;
            } else {
                y += asc;
            }
            row_pos.push(y);
            y += desc + row_gap;
        }
        let total_h = y - row_gap + first_asc;
        let wc = rows.iter().map(|r| r.1.as_ref().map_or(0.0, |c| c.w)).fold(0.0, f32::max);
        body.w = ew + col_gap + wc;
        for ((e, c), &ry) in rows.into_iter().zip(&row_pos) {
            body.prims.extend(e.shifted(vec2(0.0, ry)).prims);
            if let Some(c) = c {
                body.prims.extend(c.shifted(vec2(ew + col_gap, ry)).prims);
            }
        }
        // Centre the block on the maths axis.
        let mid = -first_asc + total_h / 2.0;
        let dy = -Self::axis(size) - mid;
        let body = body.shifted(vec2(0.0, dy));
        let (top, bot) = (-first_asc + dy, -first_asc + total_h + dy);
        let mut out = L::empty();
        // Left brace.
        let bw = size * 0.5;
        let stroke = (size * 0.06).max(1.0);
        let midy = (top + bot) / 2.0;
        let x = bw * 0.6;
        let tip = bw * 0.1;
        let brace = vec![
            pos2(x + bw * 0.3, top),
            pos2(x, top + size * 0.15),
            pos2(x, midy - size * 0.2),
            pos2(tip, midy),
            pos2(x, midy + size * 0.2),
            pos2(x, bot - size * 0.15),
            pos2(x + bw * 0.3, bot),
        ];
        out.prims.push(Prim::Path(brace, stroke));
        out.w = bw + size * 0.25;
        out.asc = -top;
        out.desc = bot;
        let bwid = body.w;
        out.prims.extend(body.shifted(vec2(out.w, 0.0)).prims);
        out.w += bwid;
        out
    }

    fn build(&self, id: Id, size: f32) -> L {
        match self.pool.node(id) {
            Node::Const(b) => {
                let v = f64::from_bits(b);
                // Small multiples of π (2π from differentiating sin(πx)).
                let k = v / std::f64::consts::PI;
                if k.abs() >= 2.0 && k.abs() <= 12.0 && (k - k.round()).abs() < 1e-12 {
                    return self.text(&format!("{}π", (k.round() as i64)).replace('-', "−"), size, false);
                }
                let s = format_num(v).replace('-', "−");
                let s = match s.as_str() {
                    "pi" => "π".to_string(),
                    "−pi" => "−π".to_string(),
                    "inf" => "∞".to_string(),
                    _ => s,
                };
                // Scientific notation (but not the constant e itself).
                if let Some((m, e)) = s.split_once('e').filter(|(m, e)| !m.is_empty() && m != &"−" && !e.is_empty()) {
                    let mut l = self.text(&format!("{m}×10"), size, false);
                    let ex = self.text(&e.replace('-', "−"), size * 0.7, false);
                    l = self.sup(l, ex, size);
                    return l;
                }
                self.text(&s, size, s == "e")
            }
            Node::Var(v) => self.text(self.vars.get(v as usize).copied().unwrap_or("?"), size, true),
            Node::I => self.text("i", size, true),
            Node::If(c, a, b) => {
                let mut cond = self.text(crate::i18n::tr("if ", "当 "), size * 0.9, false);
                cond.push(self.build(c, size));
                let other = self.text(crate::i18n::tr("otherwise", "其他"), size * 0.9, false);
                // Flatten else-if chains into more rows.
                let mut rows = vec![(self.build(a, size), Some(cond))];
                let mut rest = b;
                while let Node::If(c2, a2, b2) = self.pool.node(rest) {
                    let mut cnd = self.text(crate::i18n::tr("if ", "当 "), size * 0.9, false);
                    cnd.push(self.build(c2, size));
                    rows.push((self.build(a2, size), Some(cnd)));
                    rest = b2;
                }
                rows.push((self.build(rest, size), Some(other)));
                self.cases(rows, size)
            }
            Node::Un(Un::Neg, a) => {
                let mut l = self.text("−", size, false);
                l.push(self.paren_if(a, 5, size));
                l
            }
            Node::Un(Un::Abs, a) => self.delim(self.build(a, size), '|', '|', size),
            Node::Un(Un::Sqrt, a) => self.sqrt(self.build(a, size), None, size),
            Node::Un(Un::Cbrt, a) => self.sqrt(self.build(a, size), Some("3"), size),
            Node::Un(Un::Exp, a) => {
                let base = self.text("e", size, true);
                let exp = self.build(a, size * 0.7);
                self.sup(base, exp, size)
            }
            Node::Un(Un::Floor, a) => self.delim(self.build(a, size), '⌊', '⌋', size),
            Node::Un(Un::Ceil, a) => self.delim(self.build(a, size), '⌈', '⌉', size),
            Node::Un(Un::Not, a) => {
                let mut l = self.text("¬", size, false);
                l.push(self.paren_if(a, 8, size));
                l
            }
            Node::Un(op, a) => {
                let name = match op {
                    Un::Asin => "arcsin",
                    Un::Acos => "arccos",
                    Un::Atan => "arctan",
                    Un::Sign => "sgn",
                    Un::Re => "Re",
                    Un::Im => "Im",
                    Un::Conj => "conj",
                    _ => crate::expr::fmt::un_name(op),
                };
                self.func(name, &[a], size)
            }
            Node::Bin(op, a, b) => match op {
                Bin::Div => self.frac(a, b, size),
                Bin::Pow => {
                    // A fraction as a base needs parentheses too.
                    let base = if matches!(self.pool.node(a), Node::Bin(Bin::Div, ..)) {
                        self.delim(self.build(a, size), '(', ')', size)
                    } else {
                        self.paren_if(a, 8, size)
                    };
                    let exp = self.build(b, size * 0.7);
                    self.sup(base, exp, size)
                }
                Bin::Mul => {
                    let mut l = self.paren_if(a, 5, size);
                    // 2x, xy, x sin(y); but 2 · 3 and x · (−y).
                    let juxtapose = !matches!(self.pool.node(b), Node::Const(_) | Node::Un(Un::Neg, _));
                    if juxtapose {
                        l.space(size * 0.1);
                    } else {
                        l.push(self.op("·", size));
                    }
                    l.push(self.paren_if(b, 7, size));
                    l
                }
                Bin::Mod => self.func("mod", &[a, b], size),
                Bin::Atan2 => self.func("atan2", &[a, b], size),
                Bin::Min => self.func("min", &[a, b], size),
                Bin::Max => self.func("max", &[a, b], size),
                _ => {
                    // Hash-consing sorts commutative operands: show `x² + y² = 0`, not `0 = x² + y²`.
                    let (a, b) = if matches!(op, Bin::Eq | Bin::Ne) && self.pool.as_const(a).is_some() && self.pool.as_const(b).is_none() { (b, a) } else { (a, b) };
                    let (sym, lp, rp) = match op {
                        Bin::Or => ("or", 1, 2),
                        Bin::And => ("and", 2, 3),
                        Bin::Lt => ("<", 4, 4),
                        Bin::Le => ("≤", 4, 4),
                        Bin::Gt => (">", 4, 4),
                        Bin::Ge => ("≥", 4, 4),
                        Bin::Eq => ("=", 4, 4),
                        Bin::Ne => ("≠", 4, 4),
                        Bin::Add => ("+", 4, 4),
                        Bin::Sub => ("−", 4, 5),
                        _ => unreachable!(),
                    };
                    let mut l = self.paren_if(a, lp, size);
                    if op == Bin::Add
                        && let Some(c) = self.pool.as_const(b)
                        && c < 0.0
                    {
                        l.push(self.op("−", size));
                        l.push(self.text(&format_num(-c), size, false));
                        return l;
                    }
                    let sym = match sym {
                        "or" => crate::i18n::tr("or", "或"),
                        "and" => crate::i18n::tr("and", "且"),
                        s => s,
                    };
                    l.push(self.op(sym, size));
                    l.push(self.paren_if(b, rp, size));
                    l
                }
            },
        }
    }
}

/// Lay out `prefix` + the expression and paint it at the cursor. Returns
/// `false` (painting nothing) when the expression is too large.
pub fn show(ui: &mut Ui, pool: &Pool, id: Id, vars: &[&str], prefix: &str, size: f32, color: Color32) -> bool {
    if tree_size(pool, id, MAX_NODES) > MAX_NODES {
        return false;
    }
    let c = Ctx { ui, pool, vars, color };
    let mut l = L::empty();
    if !prefix.is_empty() {
        l.push(c.text(prefix, size, false));
        l.push(c.op("=", size));
    }
    l.push(c.build(id, size));
    let pad = 2.0;
    let (rect, _) = ui.allocate_exact_size(vec2(l.w + pad * 2.0, l.asc + l.desc + pad * 2.0), Sense::hover());
    paint(ui, rect, l, pad, color);
    true
}

fn paint(ui: &Ui, rect: Rect, l: L, pad: f32, color: Color32) {
    let origin = pos2(rect.min.x + pad, rect.min.y + pad + l.asc);
    let p = ui.painter();
    for prim in l.prims {
        match prim {
            Prim::Text(q, g) => {
                p.galley(origin + q.to_vec2(), g, color);
            }
            Prim::Line(a, b, w) => {
                p.line_segment([origin + a.to_vec2(), origin + b.to_vec2()], Stroke::new(w, color));
            }
            Prim::Path(ps, w) => {
                p.add(egui::Shape::line(ps.into_iter().map(|q| origin + q.to_vec2()).collect(), Stroke::new(w, color)));
            }
        };
    }
}
