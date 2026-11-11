//! Register bytecode compiled from the DAG and evaluated in batches.
//!
//! Each instruction writes its own register; registers hold `LANES` values,
//! so dispatch costs one `match` per instruction per 64 points and the inner
//! loops auto-vectorise. Blocks of points are spread over rayon threads.

use num_complex::Complex64 as C64;
use rayon::prelude::*;
use std::f64::consts::PI;

use super::{Bin, Id, Node, Pool, Un};

pub const LANES: usize = 64;
const BLOCK: usize = 4096;

#[derive(Clone, Copy, Debug)]
enum Ins {
    Const(f64),
    Var(u8),
    I,
    Un(Un, u32),
    Bin(Bin, u32, u32),
    If(u32, u32, u32),
}

#[derive(Clone, Debug)]
pub struct Program {
    ins: Vec<Ins>,
    out: Vec<u32>,
}

impl Program {
    /// Compile `roots` (evaluated together, sharing sub-expressions).
    pub fn compile(pool: &Pool, roots: &[Id]) -> Self {
        let mut used = vec![false; pool.len()];
        let mut stack: Vec<Id> = roots.to_vec();
        while let Some(n) = stack.pop() {
            if std::mem::replace(&mut used[n.0 as usize], true) {
                continue;
            }
            match pool.node(n) {
                Node::Un(_, a) => stack.push(a),
                Node::Bin(_, a, b) => stack.extend([a, b]),
                Node::If(c, a, b) => stack.extend([c, a, b]),
                _ => {}
            }
        }
        // Ids are created after their children, so id order is topological.
        let mut reg = vec![u32::MAX; pool.len()];
        let mut ins = Vec::new();
        for (i, _) in used.iter().enumerate().filter(|(_, u)| **u) {
            let r = |id: Id| reg[id.0 as usize];
            let op = match pool.node(Id(i as u32)) {
                Node::Const(b) => Ins::Const(f64::from_bits(b)),
                Node::Var(v) => Ins::Var(v),
                Node::I => Ins::I,
                Node::Un(op, a) => Ins::Un(op, r(a)),
                Node::Bin(op, a, b) => Ins::Bin(op, r(a), r(b)),
                Node::If(c, a, b) => Ins::If(r(c), r(a), r(b)),
            };
            reg[i] = ins.len() as u32;
            ins.push(op);
        }
        let out = roots.iter().map(|id| reg[id.0 as usize]).collect();
        Self { ins, out }
    }

    pub fn outputs(&self) -> usize {
        self.out.len()
    }

    pub fn len(&self) -> usize {
        self.ins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ins.is_empty()
    }

    /// Evaluate one point.
    pub fn eval(&self, vars: &[f64]) -> Vec<f64> {
        let mut regs = vec![0.0; self.ins.len() * LANES];
        let cols: Vec<[f64; 1]> = vars.iter().map(|v| [*v]).collect();
        let cols: Vec<&[f64]> = cols.iter().map(|c| &c[..]).collect();
        self.run_f64(&mut regs, &cols, 0, 1);
        self.out.iter().map(|&r| regs[r as usize * LANES]).collect()
    }

    /// Evaluate every output on `n` points; `inputs[v][k]` is variable `v` at
    /// point `k`. Returns `out[output][k]`.
    pub fn eval_batch(&self, inputs: &[&[f64]], n: usize) -> Vec<Vec<f64>> {
        let nout = self.out.len();
        let blocks: Vec<Vec<f64>> = (0..n.div_ceil(BLOCK))
            .into_par_iter()
            .map(|bi| {
                let start = bi * BLOCK;
                let end = (start + BLOCK).min(n);
                let mut regs = vec![0.0; self.ins.len() * LANES];
                let mut res = vec![0.0; nout * (end - start)];
                let mut s = start;
                while s < end {
                    let len = LANES.min(end - s);
                    self.run_f64(&mut regs, inputs, s, len);
                    for (o, &r) in self.out.iter().enumerate() {
                        let src = &regs[r as usize * LANES..r as usize * LANES + len];
                        let base = o * (end - start) + (s - start);
                        res[base..base + len].copy_from_slice(src);
                    }
                    s += len;
                }
                res
            })
            .collect();
        let mut out = vec![Vec::with_capacity(n); nout];
        for (bi, block) in blocks.iter().enumerate() {
            let len = (n - bi * BLOCK).min(BLOCK);
            for (o, col) in out.iter_mut().enumerate() {
                col.extend_from_slice(&block[o * len..(o + 1) * len]);
            }
        }
        out
    }

    fn run_f64(&self, regs: &mut [f64], inputs: &[&[f64]], s: usize, len: usize) {
        for (r, ins) in self.ins.iter().enumerate() {
            let (lo, hi) = regs.split_at_mut(r * LANES);
            let d = &mut hi[..len];
            let src = |i: u32| &lo[i as usize * LANES..i as usize * LANES + len];
            match *ins {
                Ins::Const(c) => d.fill(c),
                Ins::I => d.fill(f64::NAN),
                Ins::Var(v) => d.copy_from_slice(&inputs[v as usize][s..s + len]),
                Ins::Un(op, a) => {
                    let a = src(a);
                    macro_rules! map {
                        ($f:expr) => {
                            for (d, &x) in d.iter_mut().zip(a) {
                                *d = $f(x);
                            }
                        };
                    }
                    match op {
                        Un::Neg => map!(|x: f64| -x),
                        Un::Sin => map!(f64::sin),
                        Un::Cos => map!(f64::cos),
                        Un::Exp => map!(f64::exp),
                        Un::Sqrt => map!(f64::sqrt),
                        Un::Abs => map!(f64::abs),
                        _ => map!(|x| un_f64(op, x)),
                    }
                }
                Ins::Bin(op, a, b) => {
                    let (a, b) = (src(a), src(b));
                    macro_rules! zip {
                        ($f:expr) => {
                            for ((d, &x), &y) in d.iter_mut().zip(a).zip(b) {
                                *d = $f(x, y);
                            }
                        };
                    }
                    match op {
                        Bin::Add => zip!(|x, y| x + y),
                        Bin::Sub => zip!(|x, y| x - y),
                        Bin::Mul => zip!(|x, y| x * y),
                        Bin::Div => zip!(|x, y| x / y),
                        Bin::Pow => zip!(pow_f64),
                        _ => zip!(|x, y| bin_f64(op, x, y)),
                    }
                }
                Ins::If(c, a, b) => {
                    let (c, a, b) = (src(c), src(a), src(b));
                    for i in 0..len {
                        d[i] = if c[i].is_nan() {
                            f64::NAN
                        } else if c[i] != 0.0 {
                            a[i]
                        } else {
                            b[i]
                        };
                    }
                }
            }
        }
    }

    /// Evaluate the first output on complex inputs (one variable per slice).
    pub fn eval_complex_batch(&self, inputs: &[&[C64]], n: usize) -> Vec<C64> {
        let out = self.out[0];
        (0..n.div_ceil(BLOCK))
            .into_par_iter()
            .flat_map_iter(|bi| {
                let start = bi * BLOCK;
                let end = (start + BLOCK).min(n);
                let mut regs = vec![C64::new(0.0, 0.0); self.ins.len() * LANES];
                let mut res = Vec::with_capacity(end - start);
                let mut s = start;
                while s < end {
                    let len = LANES.min(end - s);
                    self.run_c64(&mut regs, inputs, s, len);
                    res.extend_from_slice(&regs[out as usize * LANES..out as usize * LANES + len]);
                    s += len;
                }
                res
            })
            .collect()
    }

    pub fn eval_complex(&self, vars: &[C64]) -> C64 {
        let mut regs = vec![C64::new(0.0, 0.0); self.ins.len() * LANES];
        let cols: Vec<[C64; 1]> = vars.iter().map(|v| [*v]).collect();
        let cols: Vec<&[C64]> = cols.iter().map(|c| &c[..]).collect();
        self.run_c64(&mut regs, &cols, 0, 1);
        regs[self.out[0] as usize * LANES]
    }

    fn run_c64(&self, regs: &mut [C64], inputs: &[&[C64]], s: usize, len: usize) {
        for (r, ins) in self.ins.iter().enumerate() {
            let (lo, hi) = regs.split_at_mut(r * LANES);
            let d = &mut hi[..len];
            let src = |i: u32| &lo[i as usize * LANES..i as usize * LANES + len];
            match *ins {
                Ins::Const(c) => d.fill(C64::new(c, 0.0)),
                Ins::I => d.fill(C64::new(0.0, 1.0)),
                Ins::Var(v) => d.copy_from_slice(&inputs[v as usize][s..s + len]),
                Ins::Un(op, a) => {
                    for (d, &x) in d.iter_mut().zip(src(a)) {
                        *d = un_c64(op, x);
                    }
                }
                Ins::Bin(op, a, b) => {
                    for ((d, &x), &y) in d.iter_mut().zip(src(a)).zip(src(b)) {
                        *d = bin_c64(op, x, y);
                    }
                }
                Ins::If(c, a, b) => {
                    let (c, a, b) = (src(c), src(a), src(b));
                    for i in 0..len {
                        d[i] = if c[i].re.is_nan() {
                            C64::new(f64::NAN, f64::NAN)
                        } else if c[i].re != 0.0 {
                            a[i]
                        } else {
                            b[i]
                        };
                    }
                }
            }
        }
    }
}

fn truth(x: f64) -> f64 {
    if x.is_nan() { f64::NAN } else { (x != 0.0) as u8 as f64 }
}

fn cmp(x: f64, y: f64, f: fn(f64, f64) -> bool) -> f64 {
    if x.is_nan() || y.is_nan() { f64::NAN } else { f(x, y) as u8 as f64 }
}

fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x // 0 or NaN
    }
}

pub fn un_f64(op: Un, x: f64) -> f64 {
    match op {
        Un::Neg => -x,
        Un::Sin => x.sin(),
        Un::Cos => x.cos(),
        Un::Tan => x.tan(),
        Un::Asin => x.asin(),
        Un::Acos => x.acos(),
        Un::Atan => x.atan(),
        Un::Sinh => x.sinh(),
        Un::Cosh => x.cosh(),
        Un::Tanh => x.tanh(),
        Un::Asinh => x.asinh(),
        Un::Acosh => x.acosh(),
        Un::Atanh => x.atanh(),
        Un::Exp => x.exp(),
        Un::Ln => x.ln(),
        Un::Sqrt => x.sqrt(),
        Un::Cbrt => x.cbrt(),
        Un::Abs => x.abs(),
        Un::Sign => sign(x),
        Un::Floor => x.floor(),
        Un::Ceil => x.ceil(),
        Un::Round => x.round(),
        Un::Step => {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                0.0
            } else if x == 0.0 {
                0.5
            } else {
                f64::NAN
            }
        }
        Un::Sinc => {
            if x.abs() < 1e-8 {
                1.0 - x * x / 6.0
            } else {
                x.sin() / x
            }
        }
        Un::Re | Un::Conj => x,
        Un::Im => {
            if x.is_nan() {
                x
            } else {
                0.0
            }
        }
        Un::Arg => {
            if x < 0.0 {
                PI
            } else if x >= 0.0 {
                0.0
            } else {
                f64::NAN
            }
        }
        Un::Not => {
            if x.is_nan() {
                x
            } else {
                (x == 0.0) as u8 as f64
            }
        }
    }
}

fn pow_f64(x: f64, y: f64) -> f64 {
    if y == 2.0 {
        x * x
    } else if y.fract() == 0.0 && y.abs() <= 64.0 {
        x.powi(y as i32)
    } else {
        x.powf(y)
    }
}

pub fn bin_f64(op: Bin, x: f64, y: f64) -> f64 {
    match op {
        Bin::Add => x + y,
        Bin::Sub => x - y,
        Bin::Mul => x * y,
        Bin::Div => x / y,
        Bin::Pow => pow_f64(x, y),
        Bin::Mod => x - y * (x / y).floor(),
        Bin::Atan2 => x.atan2(y),
        Bin::Min => {
            if x.is_nan() || y.is_nan() {
                f64::NAN
            } else {
                x.min(y)
            }
        }
        Bin::Max => {
            if x.is_nan() || y.is_nan() {
                f64::NAN
            } else {
                x.max(y)
            }
        }
        Bin::Lt => cmp(x, y, |a, b| a < b),
        Bin::Le => cmp(x, y, |a, b| a <= b),
        Bin::Gt => cmp(x, y, |a, b| a > b),
        Bin::Ge => cmp(x, y, |a, b| a >= b),
        Bin::Eq => cmp(x, y, |a, b| a == b),
        Bin::Ne => cmp(x, y, |a, b| a != b),
        Bin::And => {
            let (a, b) = (truth(x), truth(y));
            if a.is_nan() || b.is_nan() { f64::NAN } else { a * b }
        }
        Bin::Or => {
            let (a, b) = (truth(x), truth(y));
            if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
        }
    }
}

const fn re(x: f64) -> C64 {
    C64::new(x, 0.0)
}

pub fn un_c64(op: Un, z: C64) -> C64 {
    match op {
        Un::Neg => -z,
        Un::Sin => z.sin(),
        Un::Cos => z.cos(),
        Un::Tan => z.tan(),
        Un::Asin => z.asin(),
        Un::Acos => z.acos(),
        Un::Atan => z.atan(),
        Un::Sinh => z.sinh(),
        Un::Cosh => z.cosh(),
        Un::Tanh => z.tanh(),
        Un::Asinh => z.asinh(),
        Un::Acosh => z.acosh(),
        Un::Atanh => z.atanh(),
        Un::Exp => z.exp(),
        Un::Ln => z.ln(),
        Un::Sqrt => z.sqrt(),
        Un::Cbrt => z.cbrt(),
        Un::Abs => re(z.norm()),
        Un::Sign => {
            let n = z.norm();
            if n == 0.0 { re(0.0) } else { z / n }
        }
        Un::Floor => C64::new(z.re.floor(), z.im.floor()),
        Un::Ceil => C64::new(z.re.ceil(), z.im.ceil()),
        Un::Round => C64::new(z.re.round(), z.im.round()),
        Un::Step => re(un_f64(Un::Step, z.re)),
        Un::Sinc => {
            if z.norm() < 1e-8 {
                C64::new(1.0, 0.0) - z * z / 6.0
            } else {
                z.sin() / z
            }
        }
        Un::Re => re(z.re),
        Un::Im => re(z.im),
        Un::Conj => z.conj(),
        Un::Arg => re(z.arg()),
        Un::Not => re(un_f64(Un::Not, z.re)),
    }
}

fn pow_c64(z: C64, w: C64) -> C64 {
    if w.im == 0.0 {
        if w.re.fract() == 0.0 && w.re.abs() <= 64.0 {
            return z.powi(w.re as i32);
        }
        if z.re == 0.0 && z.im == 0.0 && w.re > 0.0 {
            return re(0.0);
        }
        return z.powf(w.re);
    }
    if z.re == 0.0 && z.im == 0.0 && w.re > 0.0 {
        return re(0.0);
    }
    z.powc(w)
}

pub fn bin_c64(op: Bin, a: C64, b: C64) -> C64 {
    match op {
        Bin::Add => a + b,
        Bin::Sub => a - b,
        Bin::Mul => a * b,
        Bin::Div => a / b,
        Bin::Pow => pow_c64(a, b),
        Bin::Min => {
            if a.re <= b.re { a } else { b }
        }
        Bin::Max => {
            if a.re >= b.re { a } else { b }
        }
        _ => re(bin_f64(op, a.re, b.re)),
    }
}
