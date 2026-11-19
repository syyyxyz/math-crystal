//! Adaptive sampling of a path (x(u), y(u)) for the screen.
//!
//! Start from a uniform grid of u and bisect, a level at a time (each level
//! one batched evaluation), every interval that is not yet a straight short
//! chord on screen. Intervals where the curve becomes undefined are
//! narrowed down to the end of the domain; intervals that stay long however
//! far they are bisected are jumps or poles and break the line, so tan x
//! is not joined across its asymptotes.

use super::Frame2;

#[derive(Clone, Copy, Debug)]
pub struct Pt {
    pub u: f64,
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub fn ok(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// Where the line stops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A jump or a pole just after this u.
    Break(f64),
    /// The curve is undefined on one side of this u.
    Edge(f64),
}

#[derive(Default, Clone)]
pub struct Sampled {
    pub lines: Vec<Vec<Pt>>,
    pub events: Vec<Event>,
    pub evals: usize,
}

/// Bisection levels below the initial grid.
const MAX_DEPTH: u8 = 24;
/// Most evaluations for one curve.
const BUDGET: usize = 200_000;

#[derive(Clone, Copy)]
struct Iv {
    depth: u8,
    refine: bool,
    brk: bool,
}

fn screen(p: &Pt, f: &Frame2) -> [f64; 2] {
    [f.sx(p.x), f.sy(p.y)]
}

/// Both points beyond the same edge of the window.
fn outside_same_side(a: &Pt, b: &Pt, f: &Frame2) -> bool {
    (a.y > f.y1 && b.y > f.y1) || (a.y < f.y0 && b.y < f.y0) || (a.x > f.x1 && b.x > f.x1) || (a.x < f.x0 && b.x < f.x0)
}

fn slen(a: &Pt, b: &Pt, f: &Frame2) -> f64 {
    f.dist([a.x, a.y], [b.x, b.y])
}

/// Whether m (the point halfway in u) lies close to the chord a–b and
/// near its middle: then the chord already draws this piece.
fn smooth(a: &Pt, m: &Pt, b: &Pt, f: &Frame2) -> bool {
    if outside_same_side(a, b, f) && outside_same_side(a, m, f) {
        return true;
    }
    let (sa, sm, sb) = (screen(a, f), screen(m, f), screen(b, f));
    let ab = [sb[0] - sa[0], sb[1] - sa[1]];
    let am = [sm[0] - sa[0], sm[1] - sa[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let l = l2.sqrt();
    if !l.is_finite() {
        return false;
    }
    let am_len = am[0].hypot(am[1]);
    if l < 0.5 && am_len < 0.5 {
        return true;
    }
    let dev = if l > 1e-12 { (ab[0] * am[1] - ab[1] * am[0]).abs() / l } else { am_len };
    let t = if l2 > 1e-24 { (ab[0] * am[0] + ab[1] * am[1]) / l2 } else { 0.5 };
    dev < 0.2 && ((0.2..=0.8).contains(&t) || l < 1.0)
}

/// Sample `eval` (u values → x and y) over [u0, u1] starting from `n0`
/// intervals, for the window and scale `f`.
pub fn sample(eval: &dyn Fn(&[f64]) -> [Vec<f64>; 2], u0: f64, u1: f64, n0: usize, f: &Frame2) -> Sampled {
    let n0 = n0.clamp(2, 50_000);
    if !(u1 > u0) || !u0.is_finite() || !u1.is_finite() {
        return Sampled::default();
    }
    let us: Vec<f64> = (0..=n0).map(|i| u0 + (u1 - u0) * i as f64 / n0 as f64).collect();
    let [xs, ys] = eval(&us);
    let mut pts: Vec<Pt> = (0..=n0).map(|i| Pt { u: us[i], x: xs[i], y: ys[i] }).collect();
    let first = |a: &Pt, b: &Pt| a.ok() != b.ok() || (a.ok() && b.ok() && !outside_same_side(a, b, f) && slen(a, b, f) > 2.0);
    let mut ivs: Vec<Iv> = (0..n0).map(|i| Iv { depth: 0, refine: first(&pts[i], &pts[i + 1]), brk: false }).collect();
    let mut evals = n0 + 1;
    loop {
        let idx: Vec<usize> = (0..ivs.len()).filter(|&i| ivs[i].refine).collect();
        if idx.is_empty() {
            break;
        }
        if evals + idx.len() > BUDGET {
            // Out of budget: join what is short, break what is not.
            for &i in &idx {
                let (a, b) = (&pts[i], &pts[i + 1]);
                ivs[i].refine = false;
                ivs[i].brk = a.ok() && b.ok() && !outside_same_side(a, b, f) && slen(a, b, f) > 24.0;
            }
            break;
        }
        let mids: Vec<f64> = idx.iter().map(|&i| 0.5 * (pts[i].u + pts[i + 1].u)).collect();
        let [mx, my] = eval(&mids);
        evals += mids.len();
        let mut npts = Vec::with_capacity(pts.len() + idx.len());
        let mut nivs = Vec::with_capacity(ivs.len() + idx.len());
        let mut k = 0;
        for i in 0..ivs.len() {
            npts.push(pts[i]);
            if k < idx.len() && idx[k] == i {
                let m = Pt { u: mids[k], x: mx[k], y: my[k] };
                let (a, b) = (pts[i], pts[i + 1]);
                let d = ivs[i].depth + 1;
                let all_ok = a.ok() && m.ok() && b.ok();
                let curvy = all_ok && !smooth(&a, &m, &b, f);
                let wants = |p: &Pt, q: &Pt| {
                    if p.ok() != q.ok() {
                        return true;
                    }
                    p.ok() && q.ok() && curvy && !outside_same_side(p, q, f) && slen(p, q, f) > 0.5
                };
                let half = |p: &Pt, q: &Pt| {
                    let r = wants(p, q);
                    if r && d >= MAX_DEPTH {
                        Iv { depth: d, refine: false, brk: p.ok() && q.ok() && slen(p, q, f) > 2.0 }
                    } else {
                        Iv { depth: d, refine: r, brk: false }
                    }
                };
                nivs.push(half(&a, &m));
                nivs.push(half(&m, &b));
                npts.push(m);
                k += 1;
            } else {
                nivs.push(ivs[i]);
            }
        }
        npts.push(*pts.last().unwrap());
        pts = npts;
        ivs = nivs;
    }
    let mut out = Sampled { evals, ..Default::default() };
    let mut line: Vec<Pt> = Vec::new();
    for i in 0..pts.len() {
        let p = pts[i];
        if p.ok() {
            line.push(p);
        }
        let joined = i < ivs.len() && p.ok() && pts[i + 1].ok() && !ivs[i].brk;
        if i < ivs.len() {
            if ivs[i].brk {
                out.events.push(Event::Break(0.5 * (p.u + pts[i + 1].u)));
            } else if p.ok() != pts[i + 1].ok() {
                out.events.push(Event::Edge(if p.ok() { p.u } else { pts[i + 1].u }));
            }
        }
        if !joined && !line.is_empty() {
            let l = std::mem::take(&mut line);
            // A lone point (say the value at a jump) draws nothing.
            if l.len() > 1 {
                out.lines.push(l);
            }
        }
    }
    if line.len() > 1 {
        out.lines.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame2 {
        Frame2 { x0: -6.0, x1: 6.0, y0: -4.0, y1: 4.0, kx: 80.0, ky: 80.0 }
    }

    fn graph(f: fn(f64) -> f64) -> impl Fn(&[f64]) -> [Vec<f64>; 2] {
        move |us: &[f64]| [us.to_vec(), us.iter().map(|&u| f(u)).collect()]
    }

    #[test]
    fn tan_breaks_at_its_poles() {
        let s = sample(&graph(f64::tan), -6.0, 6.0, 960, &frame());
        // Poles at ±π/2, ±3π/2 inside [-6, 6]: five pieces.
        assert_eq!(s.lines.len(), 5);
        let breaks: Vec<f64> = s.events.iter().filter_map(|e| if let Event::Break(u) = e { Some(*u) } else { None }).collect();
        assert_eq!(breaks.len(), 4);
        for b in breaks {
            let k = (b / std::f64::consts::FRAC_PI_2).round();
            assert!((b - k * std::f64::consts::FRAC_PI_2).abs() < 1e-8, "{b}");
        }
    }

    #[test]
    fn sqrt_reaches_the_end_of_its_domain() {
        let s = sample(&graph(|x| (1.0 - x * x).sqrt()), -6.0, 6.0, 960, &frame());
        assert_eq!(s.lines.len(), 1);
        let l = &s.lines[0];
        assert!((l[0].x + 1.0).abs() < 1e-6 && (l.last().unwrap().x - 1.0).abs() < 1e-6);
        assert!(s.evals < 20_000, "{}", s.evals);
    }

    #[test]
    fn steep_but_continuous_is_one_line() {
        let s = sample(&graph(f64::cbrt), -6.0, 6.0, 960, &frame());
        assert_eq!(s.lines.len(), 1);
        // Jumps beyond the window are joined (out of sight); inside it they break.
        let tall = Frame2 { y0: -7.0, y1: 7.0, ..frame() };
        let s = sample(&graph(|x| x.floor()), -6.0, 6.0, 960, &tall);
        assert_eq!(s.lines.len(), 12);
    }

    #[test]
    fn parametric_circle_is_smooth() {
        let circle = |us: &[f64]| [us.iter().map(|u| 3.0 * u.cos()).collect(), us.iter().map(|u| 3.0 * u.sin()).collect()];
        let f = frame();
        let s = sample(&circle, 0.0, std::f64::consts::TAU, 200, &f);
        assert_eq!(s.lines.len(), 1);
        for w in s.lines[0].windows(2) {
            assert!(f.dist([w[0].x, w[0].y], [w[1].x, w[1].y]) < 10.0);
        }
    }
}
