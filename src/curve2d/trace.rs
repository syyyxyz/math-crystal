//! Zero sets of F(u, v) on a grid: marching squares whose crossing points
//! are solved on the grid edges (so every vertex lies on the curve), saddle
//! cells decided by the centre value, crossings across poles or jumps
//! rejected, and the segments chained into polylines.

/// A uniform grid of `nu` × `nv` cells starting at (u0, v0).
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub u0: f64,
    pub v0: f64,
    pub du: f64,
    pub dv: f64,
    pub nu: usize,
    pub nv: usize,
}

/// A segment and the ids of the grid edges its ends lie on.
pub type Seg = ([[f64; 2]; 2], [u32; 2]);

/// Sign used for crossings: zero counts as positive.
fn pos(v: f64) -> bool {
    v >= 0.0
}

/// Illinois (modified regula falsi) on many brackets at once. `eval` gets
/// one position s ∈ [0, 1] per bracket and returns the function there;
/// `fa`, `fb` are the values at s = 0 and 1 (of opposite sign). Returns
/// the final positions and values; a bracket around a pole or a jump ends
/// with a large value, see [`is_root`].
pub(crate) fn illinois(eval: &dyn Fn(&[f64]) -> Vec<f64>, fa: &[f64], fb: &[f64], iters: usize) -> (Vec<f64>, Vec<f64>) {
    let m = fa.len();
    let (mut sa, mut sb) = (vec![0.0; m], vec![1.0; m]);
    let (mut ga, mut gb) = (fa.to_vec(), fb.to_vec());
    let mut side = vec![0i8; m];
    let mut s = vec![0.5; m];
    let mut fs = vec![f64::NAN; m];
    // A bracket with a zero end is solved already.
    let mut done = vec![false; m];
    for k in 0..m {
        if fa[k] == 0.0 || fb[k] == 0.0 {
            let e = if fa[k] == 0.0 { 0.0 } else { 1.0 };
            (sa[k], sb[k], s[k], fs[k], done[k]) = (e, e, e, 0.0, true);
        }
    }
    if done.iter().all(|d| *d) {
        return (s, fs);
    }
    for _ in 0..iters {
        for k in 0..m {
            if done[k] {
                continue;
            }
            let d = gb[k] - ga[k];
            let t = if d != 0.0 && d.is_finite() { (sa[k] * gb[k] - sb[k] * ga[k]) / d } else { 0.5 * (sa[k] + sb[k]) };
            // Stay strictly inside the bracket.
            s[k] = if t > sa[k].min(sb[k]) && t < sa[k].max(sb[k]) { t } else { 0.5 * (sa[k] + sb[k]) };
        }
        let got = eval(&s);
        for k in 0..m {
            if done[k] {
                continue;
            }
            fs[k] = got[k];
            let v = fs[k];
            if !v.is_finite() {
                // Undefined inside: keep bisecting towards the end at s = 0.
                sb[k] = s[k];
                continue;
            }
            if v == 0.0 {
                sa[k] = s[k];
                sb[k] = s[k];
                continue;
            }
            if pos(v) == pos(gb[k]) {
                sb[k] = s[k];
                gb[k] = v;
                if side[k] == -1 {
                    ga[k] *= 0.5;
                }
                side[k] = -1;
            } else {
                sa[k] = s[k];
                ga[k] = v;
                if side[k] == 1 {
                    gb[k] *= 0.5;
                }
                side[k] = 1;
            }
        }
    }
    (s, fs)
}

/// Whether a bracket refined by [`illinois`] ended on a root rather than a
/// pole or a jump: the value must be far smaller than at the ends.
pub(crate) fn is_root(f: f64, fa: f64, fb: f64) -> bool {
    f.is_finite() && f.abs() <= 1e-3 * fa.abs().min(fb.abs()) + 1e-12 * fa.abs().max(fb.abs())
}

/// Segments of the zero set of `f` over the grid.
pub fn segments(f: &dyn Fn(&[f64], &[f64]) -> Vec<f64>, g: &Grid) -> Vec<Seg> {
    let (nu, nv) = (g.nu, g.nv);
    if nu == 0 || nv == 0 {
        return vec![];
    }
    let w = nu + 1;
    let n = w * (nv + 1);
    let mut us = Vec::with_capacity(n);
    let mut vs = Vec::with_capacity(n);
    for j in 0..=nv {
        for i in 0..=nu {
            us.push(g.u0 + i as f64 * g.du);
            vs.push(g.v0 + j as f64 * g.dv);
        }
    }
    let val = f(&us, &vs);
    let node = |i: usize, j: usize| j * w + i;
    let hcount = nu * (nv + 1);
    let edges = hcount + w * nv;
    // Crossing edges: (edge id, node a, node b).
    let mut cross: Vec<(u32, usize, usize)> = Vec::new();
    let crossing = |a: usize, b: usize| val[a].is_finite() && val[b].is_finite() && pos(val[a]) != pos(val[b]);
    for j in 0..=nv {
        for i in 0..nu {
            let (a, b) = (node(i, j), node(i + 1, j));
            if crossing(a, b) {
                cross.push(((j * nu + i) as u32, a, b));
            }
        }
    }
    for j in 0..nv {
        for i in 0..=nu {
            let (a, b) = (node(i, j), node(i, j + 1));
            if crossing(a, b) {
                cross.push(((hcount + j * w + i) as u32, a, b));
            }
        }
    }
    if cross.is_empty() {
        return vec![];
    }
    // Solve on each crossing edge.
    let fa: Vec<f64> = cross.iter().map(|c| val[c.1]).collect();
    let fb: Vec<f64> = cross.iter().map(|c| val[c.2]).collect();
    let at = |k: usize, s: f64| {
        let (_, a, b) = cross[k];
        [us[a] + s * (us[b] - us[a]), vs[a] + s * (vs[b] - vs[a])]
    };
    let eval = |ss: &[f64]| {
        let mut pu = Vec::with_capacity(ss.len());
        let mut pv = Vec::with_capacity(ss.len());
        for (k, s) in ss.iter().enumerate() {
            let p = at(k, *s);
            pu.push(p[0]);
            pv.push(p[1]);
        }
        f(&pu, &pv)
    };
    let (s, fs) = illinois(&eval, &fa, &fb, 10);
    // Dense edge → crossing index (u32::MAX: none or rejected).
    let mut on_edge = vec![u32::MAX; edges];
    let mut pts = Vec::with_capacity(cross.len());
    for k in 0..cross.len() {
        pts.push(at(k, s[k]));
        if is_root(fs[k], fa[k], fb[k]) {
            on_edge[cross[k].0 as usize] = k as u32;
        }
    }
    let mut segs: Vec<Seg> = Vec::new();
    let mut saddles: Vec<(usize, usize, [u32; 4])> = Vec::new();
    for j in 0..nv {
        for i in 0..nu {
            // Edge ids: bottom, right, top, left.
            let ids = [(j * nu + i) as u32, (hcount + j * w + i + 1) as u32, ((j + 1) * nu + i) as u32, (hcount + j * w + i) as u32];
            let hits: Vec<usize> = (0..4).filter(|&e| on_edge[ids[e] as usize] != u32::MAX).collect();
            let mk = |e1: usize, e2: usize| -> Seg {
                let (k1, k2) = (on_edge[ids[e1] as usize] as usize, on_edge[ids[e2] as usize] as usize);
                ([pts[k1], pts[k2]], [ids[e1], ids[e2]])
            };
            match hits.len() {
                2 => segs.push(mk(hits[0], hits[1])),
                4 => saddles.push((i, j, ids)),
                _ => {}
            }
        }
    }
    if !saddles.is_empty() {
        let cu: Vec<f64> = saddles.iter().map(|s| g.u0 + (s.0 as f64 + 0.5) * g.du).collect();
        let cv: Vec<f64> = saddles.iter().map(|s| g.v0 + (s.1 as f64 + 0.5) * g.dv).collect();
        let centre = f(&cu, &cv);
        for (k, &(i, j, ids)) in saddles.iter().enumerate() {
            let mk = |e1: usize, e2: usize| -> Seg {
                let (k1, k2) = (on_edge[ids[e1] as usize] as usize, on_edge[ids[e2] as usize] as usize);
                ([pts[k1], pts[k2]], [ids[e1], ids[e2]])
            };
            // Same sign as the bottom-left corner: the bottom-right and
            // top-left corners are cut off; otherwise the other two.
            if pos(centre[k]) == pos(val[node(i, j)]) {
                segs.push(mk(0, 1));
                segs.push(mk(2, 3));
            } else {
                segs.push(mk(0, 3));
                segs.push(mk(1, 2));
            }
        }
    }
    segs
}

/// Chain segments that share grid edges into polylines.
pub fn chain(segs: &[Seg]) -> Vec<Vec<[f64; 2]>> {
    use std::collections::HashMap;
    let mut by_edge: HashMap<u32, [u32; 2]> = HashMap::with_capacity(segs.len() * 2);
    for (k, s) in segs.iter().enumerate() {
        for e in s.1 {
            let slot = by_edge.entry(e).or_insert([u32::MAX; 2]);
            if slot[0] == u32::MAX {
                slot[0] = k as u32;
            } else {
                slot[1] = k as u32;
            }
        }
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    // Follow from segment `k`, leaving through edge `e`; returns the points.
    let walk = |start: usize, e0: u32, used: &mut Vec<bool>| -> Vec<[f64; 2]> {
        let mut pts = Vec::new();
        let (mut k, mut e) = (start, e0);
        loop {
            let next = by_edge.get(&e).and_then(|s| s.iter().copied().find(|&n| n != u32::MAX && n as usize != k && !used[n as usize]));
            let Some(n) = next else { break };
            let n = n as usize;
            used[n] = true;
            let s = &segs[n];
            let (p, ne) = if s.1[0] == e { (s.0[1], s.1[1]) } else { (s.0[0], s.1[0]) };
            pts.push(p);
            k = n;
            e = ne;
        }
        pts
    };
    for k in 0..segs.len() {
        if used[k] {
            continue;
        }
        used[k] = true;
        let s = &segs[k];
        let fwd = walk(k, s.1[1], &mut used);
        let back = walk(k, s.1[0], &mut used);
        let mut line: Vec<[f64; 2]> = back.into_iter().rev().collect();
        line.push(s.0[0]);
        line.push(s.0[1]);
        line.extend(fwd);
        out.push(line);
    }
    out
}

/// Polylines of the zero set of `f` over the grid.
pub fn trace(f: &dyn Fn(&[f64], &[f64]) -> Vec<f64>, g: &Grid) -> Vec<Vec<[f64; 2]>> {
    chain(&segments(f, g))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_is_one_closed_loop_on_the_curve() {
        let f = |u: &[f64], v: &[f64]| u.iter().zip(v).map(|(x, y)| x * x + y * y - 1.0).collect();
        let g = Grid { u0: -2.0, v0: -2.0, du: 0.05, dv: 0.05, nu: 80, nv: 80 };
        let lines = trace(&f, &g);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert!(l.len() > 100);
        for p in l {
            assert!((p[0].hypot(p[1]) - 1.0).abs() < 1e-9);
        }
        let (a, b) = (l[0], l[l.len() - 1]);
        assert!((a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12);
    }

    #[test]
    fn poles_are_not_curves() {
        // y·x = 1 written as 1/x − y: no crossing along x = 0.
        let f = |u: &[f64], v: &[f64]| u.iter().zip(v).map(|(x, y)| 1.0 / x - y).collect();
        let g = Grid { u0: -2.03, v0: -2.0, du: 0.05, dv: 0.05, nu: 80, nv: 80 };
        for l in trace(&f, &g) {
            for p in l {
                assert!((1.0 / p[0] - p[1]).abs() < 1e-6, "{p:?}");
            }
        }
    }

    #[test]
    fn crossing_lines() {
        let f = |u: &[f64], v: &[f64]| u.iter().zip(v).map(|(x, y)| x * x - y * y).collect();
        let g = Grid { u0: -1.013, v0: -1.007, du: 0.02, dv: 0.02, nu: 100, nv: 100 };
        let lines = trace(&f, &g);
        let n: usize = lines.iter().map(|l| l.len()).sum();
        assert!(n > 300);
        assert!(lines.len() <= 4);
    }
}
