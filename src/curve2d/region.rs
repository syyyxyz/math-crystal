//! Regions given by inequalities: which cells of the window are inside,
//! the parts of each comparison's curve that bound the region (dashed for
//! strict inequalities in the drawing), and the area.

use super::trace::{Grid, chain, segments};
use super::{Def, Frame2};

pub struct RegionDraw {
    /// Inside flags of `nu` × `nv` cells, rows from the bottom (y0) up.
    pub mask: Vec<bool>,
    pub nu: usize,
    pub nv: usize,
    pub window: Frame2,
    /// Boundary polylines and whether that comparison is strict.
    pub bounds: Vec<(Vec<Vec<[f64; 2]>>, bool)>,
}

/// Cells of about `cell` points over the window.
fn grid_for(f: &Frame2, cell: f64) -> Grid {
    let [w, h] = f.size();
    let nu = ((w / cell).ceil() as usize).clamp(2, 4000);
    let nv = ((h / cell).ceil() as usize).clamp(2, 4000);
    Grid { u0: f.x0, v0: f.y0, du: (f.x1 - f.x0) / nu as f64, dv: (f.y1 - f.y0) / nv as f64, nu, nv }
}

pub fn draw(def: &Def, f: &Frame2, cell: f64) -> RegionDraw {
    let g = grid_for(f, cell);
    let mut xs = Vec::with_capacity(g.nu * g.nv);
    let mut ys = Vec::with_capacity(g.nu * g.nv);
    for j in 0..g.nv {
        for i in 0..g.nu {
            xs.push(g.u0 + (i as f64 + 0.5) * g.du);
            ys.push(g.v0 + (j as f64 + 0.5) * g.dv);
        }
    }
    let mask = def.region_has(&xs, &ys);
    let strict = def.region_leaves();
    let mut bounds = Vec::new();
    for (k, s) in strict.iter().enumerate() {
        let leaf = |u: &[f64], v: &[f64]| def.region_leaf(k, u, v);
        let segs = segments(&leaf, &g);
        if segs.is_empty() {
            continue;
        }
        // Keep the pieces with the region on exactly one side.
        let eps = 1.5;
        let mut tx = Vec::with_capacity(segs.len() * 2);
        let mut ty = Vec::with_capacity(segs.len() * 2);
        for (p, _) in &segs {
            let m = [(p[0][0] + p[1][0]) * 0.5, (p[0][1] + p[1][1]) * 0.5];
            // Normal in screen space, back to data units.
            let (dx, dy) = ((p[1][0] - p[0][0]) * f.kx, (p[1][1] - p[0][1]) * f.ky);
            let l = dx.hypot(dy).max(1e-12);
            let (nx, ny) = (-dy / l * eps / f.kx, dx / l * eps / f.ky);
            tx.extend([m[0] + nx, m[0] - nx]);
            ty.extend([m[1] + ny, m[1] - ny]);
        }
        let inside = def.region_has(&tx, &ty);
        // A segment of zero length (both ends on a grid node that lies on
        // the curve) has no sides to test: keep it so the line stays joined.
        let degenerate = |p: &[[f64; 2]; 2]| (p[1][0] - p[0][0]).abs() + (p[1][1] - p[0][1]).abs() < 1e-12 * (f.x1 - f.x0).abs();
        let kept: Vec<_> = segs.into_iter().enumerate().filter(|(i, s)| degenerate(&s.0) || inside[2 * i] != inside[2 * i + 1]).map(|(_, s)| s).collect();
        let mut lines = chain(&kept);
        lines.retain(|l| l.iter().any(|p| !degenerate(&[l[0], *p])));
        if !lines.is_empty() {
            bounds.push((lines, *s));
        }
    }
    RegionDraw { mask, nu: g.nu, nv: g.nv, window: *f, bounds }
}

/// Area of the part of the region inside the window `(x0, x1, y0, y1)`,
/// counting an n × n grid of cells, each split 4 × 4 where its corners
/// disagree.
pub fn area(def: &Def, xr: (f64, f64), yr: (f64, f64), n: usize) -> f64 {
    let (dx, dy) = ((xr.1 - xr.0) / n as f64, (yr.1 - yr.0) / n as f64);
    let mut xs = Vec::with_capacity((n + 1) * (n + 1));
    let mut ys = Vec::with_capacity((n + 1) * (n + 1));
    for j in 0..=n {
        for i in 0..=n {
            xs.push(xr.0 + i as f64 * dx);
            ys.push(yr.0 + j as f64 * dy);
        }
    }
    let c = def.region_has(&xs, &ys);
    let at = |i: usize, j: usize| c[j * (n + 1) + i];
    let mut full = 0usize;
    let mut mixed: Vec<(usize, usize)> = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let k = [at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1)].iter().filter(|b| **b).count();
            match k {
                4 => full += 1,
                0 => {}
                _ => mixed.push((i, j)),
            }
        }
    }
    let m = 4;
    let mut sx = Vec::with_capacity(mixed.len() * m * m);
    let mut sy = Vec::with_capacity(mixed.len() * m * m);
    for &(i, j) in &mixed {
        for b in 0..m {
            for a in 0..m {
                sx.push(xr.0 + (i as f64 + (a as f64 + 0.5) / m as f64) * dx);
                sy.push(yr.0 + (j as f64 + (b as f64 + 0.5) / m as f64) * dy);
            }
        }
    }
    let part = def.region_has(&sx, &sy).iter().filter(|b| **b).count();
    (full as f64 + part as f64 / (m * m) as f64) * dx * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_area_and_boundary() {
        let d = Def::new("x^2 + y^2 <= 4").unwrap();
        let a = area(&d, (-3.0, 3.0), (-3.0, 3.0), 600);
        assert!((a - 4.0 * std::f64::consts::PI).abs() < 2e-3, "{a}");
        let f = Frame2 { x0: -3.0, x1: 3.0, y0: -3.0, y1: 3.0, kx: 100.0, ky: 100.0 };
        let r = draw(&d, &f, 2.0);
        assert_eq!(r.bounds.len(), 1);
        assert!(!r.bounds[0].1);
        assert_eq!(r.bounds[0].0.len(), 1);
    }

    #[test]
    fn half_disc_keeps_only_the_real_boundary() {
        // The circle below y = 0 and the line outside the disc are not edges.
        let d = Def::new("x^2 + y^2 < 4, y > 0").unwrap();
        let f = Frame2 { x0: -3.0, x1: 3.0, y0: -3.0, y1: 3.0, kx: 100.0, ky: 100.0 };
        let r = draw(&d, &f, 2.0);
        for (lines, strict) in &r.bounds {
            assert!(strict);
            for l in lines {
                for p in l {
                    assert!(p[1] >= -1e-9 && p[0].hypot(p[1]) <= 2.0 + 1e-9, "{p:?}");
                }
            }
        }
        let polar = Def::new("r <= 2cos(θ)").unwrap();
        let a = area(&polar, (-3.0, 3.0), (-3.0, 3.0), 600);
        assert!((a - std::f64::consts::PI).abs() < 2e-3, "{a}");
    }
}
