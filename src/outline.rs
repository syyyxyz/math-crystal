//! Outer outlines of surfaces seen along a coordinate axis, for the front /
//! side / top drawings.
//!
//! The triangles are clipped to the data box (the renderer clips the same
//! way), projected onto the view plane and filled into a grid of small
//! cells; the boundary of the covered cells is the outline. That works for
//! any surface: graphs with holes in their domain, implicit surfaces, solids,
//! and surfaces seen edge-on (whose triangle edges are drawn into the grid
//! too, so a plane seen side-on still shows as a line). With cells of about
//! a pixel and a half the steps of the grid are invisible.

use glam::DVec3;

/// A line segment in view-plane coordinates (horizontal, vertical axis).
pub type Seg = [[f64; 2]; 2];

/// Grids larger than this per side are coarsened.
const MAX_CELLS: usize = 3000;

/// The view: `axes = [horizontal, vertical, depth]` as indices of x, y, z.
#[derive(Clone, Copy, Debug)]
pub struct ViewPlane {
    pub axes: [usize; 3],
    /// Box of the data (triangles are clipped to it).
    pub lo: DVec3,
    pub hi: DVec3,
    /// Part of the view plane to trace: `[(u0, u1), (v0, v1)]`.
    pub window: [(f64, f64); 2],
    /// Cell size along the horizontal and vertical axis, in data units.
    pub cell: [f64; 2],
}

/// Outline of the union of `tris` seen along `plane.axes[2]`.
pub fn outline(tris: impl IntoIterator<Item = [DVec3; 3]>, plane: &ViewPlane) -> Vec<Seg> {
    let [ua, va, _] = plane.axes;
    let (lo, hi) = (plane.lo.to_array(), plane.hi.to_array());
    // Trace only where the window and the box overlap.
    let ur = (plane.window[0].0.max(lo[ua]), plane.window[0].1.min(hi[ua]));
    let vr = (plane.window[1].0.max(lo[va]), plane.window[1].1.min(hi[va]));
    if !(ur.1 > ur.0 && vr.1 > vr.0) || !(plane.cell[0] > 0.0 && plane.cell[1] > 0.0) {
        return Vec::new();
    }
    let nu = (((ur.1 - ur.0) / plane.cell[0]).ceil() as usize).clamp(1, MAX_CELLS);
    let nv = (((vr.1 - vr.0) / plane.cell[1]).ceil() as usize).clamp(1, MAX_CELLS);
    let mut grid = Grid { u0: ur.0, v0: vr.0, cu: (ur.1 - ur.0) / nu as f64, cv: (vr.1 - vr.0) / nv as f64, nu, nv, cov: vec![false; (nu + 2) * (nv + 2)] };

    let mut poly: Vec<DVec3> = Vec::with_capacity(12);
    let mut flat: Vec<[f64; 2]> = Vec::with_capacity(12);
    for tri in tris {
        poly.clear();
        poly.extend_from_slice(&tri);
        if !clip_to_box(&mut poly, plane.lo, plane.hi) {
            continue;
        }
        flat.clear();
        flat.extend(poly.iter().map(|p| [p[ua], p[va]]));
        grid.fill(&flat);
    }

    // One cell of margin on each side: empty where the box ends there (the
    // surface ends, so that edge is part of the outline), a copy of the
    // neighbour where only the window ends (the surface goes on).
    let box_side = [ur.0 <= lo[ua], ur.1 >= hi[ua], vr.0 <= lo[va], vr.1 >= hi[va]];
    grid.margins(box_side);
    grid.march()
}

struct Grid {
    u0: f64,
    v0: f64,
    cu: f64,
    cv: f64,
    nu: usize,
    nv: usize,
    /// Coverage with a one-cell margin: `(nu + 2) × (nv + 2)`, row-major.
    cov: Vec<bool>,
}

impl Grid {
    fn idx(&self, i: usize, j: usize) -> usize {
        j * (self.nu + 2) + i
    }

    /// Mark interior cell (i, j) (0-based, without the margin).
    fn mark(&mut self, i: isize, j: isize) {
        if i >= 0 && j >= 0 && (i as usize) < self.nu && (j as usize) < self.nv {
            let k = self.idx(i as usize + 1, j as usize + 1);
            self.cov[k] = true;
        }
    }

    /// Cells whose centres lie in the convex polygon, plus every cell its
    /// edges pass through.
    fn fill(&mut self, poly: &[[f64; 2]]) {
        let n = poly.len();
        let (mut vmin, mut vmax) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in poly {
            vmin = vmin.min(p[1]);
            vmax = vmax.max(p[1]);
        }
        // Rows whose centre line crosses the polygon.
        let j0 = (((vmin - self.v0) / self.cv - 0.5).ceil().max(0.0)) as isize;
        let j1 = (((vmax - self.v0) / self.cv - 0.5).floor()).min(self.nv as f64 - 1.0) as isize;
        for j in j0..=j1 {
            let v = self.v0 + (j as f64 + 0.5) * self.cv;
            let (mut a, mut b) = (f64::INFINITY, f64::NEG_INFINITY);
            for k in 0..n {
                let (p, q) = (poly[k], poly[(k + 1) % n]);
                if (p[1] <= v && q[1] >= v) || (q[1] <= v && p[1] >= v) {
                    let u = if (q[1] - p[1]).abs() < 1e-300 { p[0].min(q[0]) } else { p[0] + (v - p[1]) / (q[1] - p[1]) * (q[0] - p[0]) };
                    let u2 = if (q[1] - p[1]).abs() < 1e-300 { p[0].max(q[0]) } else { u };
                    a = a.min(u);
                    b = b.max(u2);
                }
            }
            if a > b {
                continue;
            }
            let i0 = (((a - self.u0) / self.cu - 0.5).ceil().max(0.0)) as isize;
            let i1 = (((b - self.u0) / self.cu - 0.5).floor()).min(self.nu as f64 - 1.0) as isize;
            for i in i0..=i1 {
                self.mark(i, j);
            }
        }
        // Edges, in steps of half a cell.
        for k in 0..n {
            let (p, q) = (poly[k], poly[(k + 1) % n]);
            let du = (q[0] - p[0]) / self.cu;
            let dv = (q[1] - p[1]) / self.cv;
            let steps = ((du.abs().max(dv.abs()) * 2.0).ceil() as usize).clamp(1, 2 * (self.nu + self.nv));
            for s in 0..=steps {
                let t = s as f64 / steps as f64;
                let u = (p[0] - self.u0) / self.cu + du * t;
                let v = (p[1] - self.v0) / self.cv + dv * t;
                self.mark(u.floor() as isize, v.floor() as isize);
            }
        }
    }

    fn margins(&mut self, box_side: [bool; 4]) {
        let (w, h) = (self.nu + 2, self.nv + 2);
        for j in 1..h - 1 {
            let (l, r) = (self.idx(0, j), self.idx(w - 1, j));
            self.cov[l] = !box_side[0] && self.cov[l + 1];
            self.cov[r] = !box_side[1] && self.cov[r - 1];
        }
        for i in 0..w {
            let (b, t) = (self.idx(i, 0), self.idx(i, h - 1));
            self.cov[b] = !box_side[2] && self.cov[self.idx(i, 1)];
            self.cov[t] = !box_side[3] && self.cov[self.idx(i, h - 2)];
        }
    }

    /// Marching squares over the cell centres.
    fn march(&self) -> Vec<Seg> {
        let (w, h) = (self.nu + 2, self.nv + 2);
        // Centre of margin-grid cell (i, j).
        let cu = |i: f64| self.u0 + (i - 0.5) * self.cu;
        let cv = |j: f64| self.v0 + (j - 0.5) * self.cv;
        let mut out = Vec::new();
        for j in 0..h - 1 {
            for i in 0..w - 1 {
                let a = self.cov[self.idx(i, j)] as u8;
                let b = self.cov[self.idx(i + 1, j)] as u8;
                let c = self.cov[self.idx(i + 1, j + 1)] as u8;
                let d = self.cov[self.idx(i, j + 1)] as u8;
                let code = a | b << 1 | c << 2 | d << 3;
                if code == 0 || code == 15 {
                    continue;
                }
                let (fi, fj) = (i as f64, j as f64);
                let bottom = [cu(fi + 0.5), cv(fj)];
                let top = [cu(fi + 0.5), cv(fj + 1.0)];
                let left = [cu(fi), cv(fj + 0.5)];
                let right = [cu(fi + 1.0), cv(fj + 0.5)];
                let mut seg = |p: [f64; 2], q: [f64; 2]| out.push([p, q]);
                match code {
                    1 | 14 => seg(left, bottom),
                    2 | 13 => seg(bottom, right),
                    3 | 12 => seg(left, right),
                    4 | 11 => seg(right, top),
                    6 | 9 => seg(bottom, top),
                    7 | 8 => seg(left, top),
                    5 => {
                        seg(left, bottom);
                        seg(right, top);
                    }
                    10 => {
                        seg(bottom, right);
                        seg(top, left);
                    }
                    _ => {}
                }
            }
        }
        out
    }
}

/// Join segments that share end points into polylines (closed loops repeat
/// their first point at the end) and smooth away the steps of the grid.
/// The smoothing moves points by less than a cell.
pub fn polylines(segs: &[Seg]) -> Vec<Vec<[f64; 2]>> {
    use std::collections::HashMap;
    let key = |p: [f64; 2]| (p[0].to_bits(), p[1].to_bits());
    // End point → segments touching it.
    let mut at: HashMap<(u64, u64), Vec<usize>> = HashMap::with_capacity(segs.len() * 2);
    for (k, s) in segs.iter().enumerate() {
        at.entry(key(s[0])).or_default().push(k);
        at.entry(key(s[1])).or_default().push(k);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    // Follow from `p` along unused segments.
    let walk = |start: [f64; 2], used: &mut Vec<bool>, line: &mut Vec<[f64; 2]>| {
        let mut p = start;
        while let Some(&k) = at.get(&key(p)).and_then(|v| v.iter().find(|&&k| !used[k])) {
            used[k] = true;
            p = if key(segs[k][0]) == key(p) { segs[k][1] } else { segs[k][0] };
            line.push(p);
        }
    };
    for k in 0..segs.len() {
        if used[k] {
            continue;
        }
        used[k] = true;
        let mut fwd = vec![segs[k][0], segs[k][1]];
        walk(segs[k][1], &mut used, &mut fwd);
        let closed = key(fwd[0]) == key(*fwd.last().unwrap());
        if !closed {
            let mut back = Vec::new();
            walk(segs[k][0], &mut used, &mut back);
            back.reverse();
            back.extend(fwd);
            fwd = back;
        }
        out.push(smooth(fwd));
    }
    out
}

/// Two passes of a [1 2 1]/4 average; end points of open lines stay.
fn smooth(mut line: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    let n = line.len();
    if n < 4 {
        return line;
    }
    let closed = line[0] == line[n - 1];
    for _ in 0..2 {
        let prev = line.clone();
        for i in 0..n {
            let (a, b) = if closed {
                // Skip the repeated end point when wrapping.
                (prev[if i == 0 { n - 2 } else { i - 1 }], prev[if i == n - 1 { 1 } else { i + 1 }])
            } else if i == 0 || i == n - 1 {
                continue;
            } else {
                (prev[i - 1], prev[i + 1])
            };
            let c = prev[i];
            line[i] = [(a[0] + 2.0 * c[0] + b[0]) / 4.0, (a[1] + 2.0 * c[1] + b[1]) / 4.0];
        }
    }
    line
}

/// Sample a graph z = f(x, y) for tracing `plane`: the visible part of the
/// box, finely along the drawing's axes (a sample per cell, so the outline
/// and the edge of the domain sharpen as one zooms in) and moderately along
/// the line of sight.
pub fn graph_tris(f: impl Fn(&[f64], &[f64]) -> Vec<f64>, plane: &ViewPlane) -> Vec<[DVec3; 3]> {
    let [ua, va, _] = plane.axes;
    let (lo, hi) = (plane.lo.to_array(), plane.hi.to_array());
    let range_n = |axis: usize| -> ((f64, f64), usize) {
        let full = (lo[axis], hi[axis]);
        let (w, cell) = if axis == ua {
            (plane.window[0], plane.cell[0])
        } else if axis == va {
            (plane.window[1], plane.cell[1])
        } else {
            return (full, 160);
        };
        let r = (w.0.max(full.0), w.1.min(full.1));
        if !(r.1 > r.0) {
            return (full, 2);
        }
        (r, (((r.1 - r.0) / cell).ceil() as usize).clamp(32, 800))
    };
    let ((xr, nx), (yr, ny)) = (range_n(0), range_n(1));
    let mut xs = Vec::with_capacity(nx * ny);
    let mut ys = Vec::with_capacity(nx * ny);
    for j in 0..ny {
        for i in 0..nx {
            xs.push(crate::calc::lerp(xr, i as f64 / (nx - 1) as f64));
            ys.push(crate::calc::lerp(yr, j as f64 / (ny - 1) as f64));
        }
    }
    let z: Vec<f64> = f(&xs, &ys).into_iter().map(|v| if crate::graph_mesh::defined(v) { v } else { f64::NAN }).collect();
    // Cells at the edge of the domain keep their defined part.
    let edge = crate::graph_mesh::edge_polygons(&z, [nx, ny], xr, yr, &f);
    let to = |v: crate::graph_mesh::Vertex| {
        DVec3::new(crate::calc::lerp(xr, v[0] / (nx - 1) as f64), crate::calc::lerp(yr, v[1] / (ny - 1) as f64), v[2])
    };
    let mut tris = height_tris_xy(&z, [nx, ny], xr, yr, true);
    tris.extend(crate::graph_mesh::fan(&edge).map(|t| t.map(to)));
    tris
}

/// Clip a convex polygon to the box (Sutherland–Hodgman). False when
/// nothing is left.
fn clip_to_box(poly: &mut Vec<DVec3>, lo: DVec3, hi: DVec3) -> bool {
    let mut tmp: Vec<DVec3> = Vec::with_capacity(poly.len() + 6);
    for axis in 0..3 {
        for (bound, keep_below) in [(lo[axis], false), (hi[axis], true)] {
            let inside = |p: &DVec3| if keep_below { p[axis] <= bound } else { p[axis] >= bound };
            // Fast path: all inside.
            if poly.iter().all(inside) {
                continue;
            }
            tmp.clear();
            let n = poly.len();
            for k in 0..n {
                let (p, q) = (poly[k], poly[(k + 1) % n]);
                let (pin, qin) = (inside(&p), inside(&q));
                if pin {
                    tmp.push(p);
                }
                if pin != qin {
                    let t = (bound - p[axis]) / (q[axis] - p[axis]);
                    tmp.push(p + (q - p) * t);
                }
            }
            std::mem::swap(poly, &mut tmp);
            if poly.len() < 2 {
                return false;
            }
        }
    }
    true
}

/// Triangles of a height field `z[j * n + i]` over `xr × yr`, leaving out
/// undefined samples the way the 3D mesh does.
pub fn height_tris(z: &[f64], n: usize, xr: (f64, f64), yr: (f64, f64)) -> Vec<[DVec3; 3]> {
    height_tris_xy(z, [n, n], xr, yr, false)
}

/// Like [`height_tris`] on an `nx × ny` grid; `full_only` leaves out every
/// cell with an undefined corner (for use with edge polygons).
pub fn height_tris_xy(z: &[f64], [nx, ny]: [usize; 2], xr: (f64, f64), yr: (f64, f64), full_only: bool) -> Vec<[DVec3; 3]> {
    if nx < 2 || ny < 2 || z.len() < nx * ny {
        return Vec::new();
    }
    let at = |i: usize, j: usize| -> Option<DVec3> {
        let v = z[j * nx + i];
        v.is_finite().then(|| {
            DVec3::new(crate::calc::lerp(xr, i as f64 / (nx - 1) as f64), crate::calc::lerp(yr, j as f64 / (ny - 1) as f64), v)
        })
    };
    let mut out = Vec::with_capacity(2 * (nx - 1) * (ny - 1));
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            let q = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            if full_only && q.iter().any(Option::is_none) {
                continue;
            }
            let mut tri = |a: usize, b: usize, c: usize| {
                if let (Some(p), Some(q2), Some(r)) = (q[a], q[b], q[c]) {
                    out.push([p, q2, r]);
                }
            };
            if q[0].is_some() && q[2].is_some() {
                tri(0, 1, 2);
                tri(0, 2, 3);
            } else {
                tri(0, 1, 3);
                tri(1, 2, 3);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether some segment passes within `r` of `(u, v)`.
    fn near(segs: &[Seg], u: f64, v: f64, r: f64) -> bool {
        segs.iter().any(|s| {
            let (p, q) = (s[0], s[1]);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let t = (((u - p[0]) * dx + (v - p[1]) * dy) / (dx * dx + dy * dy).max(1e-300)).clamp(0.0, 1.0);
            let (x, y) = (p[0] + t * dx - u, p[1] + t * dy - v);
            x * x + y * y <= r * r
        })
    }

    fn paraboloid() -> Vec<[DVec3; 3]> {
        let n = 81;
        let mut z = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (-1.0 + 2.0 * i as f64 / (n - 1) as f64, -1.0 + 2.0 * j as f64 / (n - 1) as f64);
                z[j * n + i] = x * x + y * y;
            }
        }
        height_tris(&z, n, (-1.0, 1.0), (-1.0, 1.0))
    }

    fn plane(axes: [usize; 3], window: [(f64, f64); 2], cell: f64) -> ViewPlane {
        ViewPlane { axes, lo: DVec3::new(-1.0, -1.0, -1.0), hi: DVec3::new(1.0, 1.0, 3.0), window, cell: [cell, cell] }
    }

    #[test]
    fn front_view_of_a_paraboloid() {
        // Seen along y: between z = x² (y = 0) and z = x² + 1 (y = ±1).
        let segs = outline(paraboloid(), &plane([0, 2, 1], [(-2.0, 2.0), (-2.0, 4.0)], 0.01));
        let r = 0.03;
        for x in [-0.6, 0.0, 0.5] {
            assert!(near(&segs, x, x * x, r), "lower envelope at {x}");
            assert!(near(&segs, x, x * x + 1.0, r), "upper envelope at {x}");
            assert!(!near(&segs, x, x * x + 0.5, 0.1), "nothing inside at {x}");
        }
        // The side edges x = ±1 run from z = 1 to z = 2.
        assert!(near(&segs, 1.0, 1.5, r) && near(&segs, -1.0, 1.5, r));
    }

    #[test]
    fn top_view_shows_the_domain() {
        // z = sqrt(1 - x² - y²): defined on the unit disc.
        let n = 161;
        let mut z = vec![f64::NAN; n * n];
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (-1.5 + 3.0 * i as f64 / (n - 1) as f64, -1.5 + 3.0 * j as f64 / (n - 1) as f64);
                z[j * n + i] = (1.0 - x * x - y * y).sqrt();
            }
        }
        let tris = height_tris(&z, n, (-1.5, 1.5), (-1.5, 1.5));
        let mut p = plane([0, 1, 2], [(-2.0, 2.0), (-2.0, 2.0)], 0.01);
        (p.lo, p.hi) = (DVec3::new(-1.5, -1.5, -1.0), DVec3::new(1.5, 1.5, 2.0));
        let segs = outline(tris, &p);
        for k in 0..12 {
            let a = k as f64 * std::f64::consts::TAU / 12.0;
            assert!(near(&segs, a.cos(), a.sin(), 0.05), "circle at angle {a}");
        }
        assert!(!near(&segs, 0.0, 0.0, 0.5));
        assert!(!near(&segs, 1.5, 1.5, 0.3), "no box corner: the domain is the disc");
    }

    #[test]
    fn window_edges_are_not_outline_but_box_edges_are() {
        // Zoomed into the middle of the front view: only the envelopes.
        let segs = outline(paraboloid(), &plane([0, 2, 1], [(-0.5, 0.5), (-0.5, 1.5)], 0.005));
        assert!(near(&segs, 0.0, 0.0, 0.02) && near(&segs, 0.0, 1.0, 0.02));
        // At x = ±0.5 the surface spans z = 0.25 … 1.25: no vertical line there.
        assert!(!near(&segs, 0.5, 0.75, 0.2) && !near(&segs, -0.5, 0.75, 0.2), "no line along the window sides");
        // A box lower than the surface cuts it: its top becomes outline.
        let mut p = plane([0, 2, 1], [(-2.0, 2.0), (-2.0, 4.0)], 0.01);
        p.hi.z = 0.5;
        let segs = outline(paraboloid(), &p);
        assert!(near(&segs, 0.0, 0.5, 0.02), "cut by the box top");
    }

    #[test]
    fn segments_join_into_smooth_lines() {
        let segs = outline(paraboloid(), &plane([0, 2, 1], [(-2.0, 2.0), (-2.0, 4.0)], 0.01));
        let lines = polylines(&segs);
        // One closed loop around the projected region.
        assert_eq!(lines.len(), 1, "{} lines", lines.len());
        let l = &lines[0];
        assert_eq!(l.first(), l.last());
        assert_eq!(l.len(), segs.len() + 1);
        // Still on the envelopes after smoothing.
        let as_segs: Vec<Seg> = l.windows(2).map(|w| [w[0], w[1]]).collect();
        assert!(near(&as_segs, 0.5, 0.25, 0.02) && near(&as_segs, 0.5, 1.25, 0.02));
    }

    #[test]
    fn surfaces_seen_edge_on_still_show() {
        // The plane z = 0.5 seen from the front is a line.
        let sq = [DVec3::new(-1.0, -1.0, 0.5), DVec3::new(1.0, -1.0, 0.5), DVec3::new(1.0, 1.0, 0.5), DVec3::new(-1.0, 1.0, 0.5)];
        let segs = outline([[sq[0], sq[1], sq[2]], [sq[0], sq[2], sq[3]]], &plane([0, 2, 1], [(-2.0, 2.0), (-2.0, 4.0)], 0.01));
        assert!(near(&segs, 0.0, 0.5, 0.02) && near(&segs, -0.9, 0.5, 0.02));
        assert!(!near(&segs, 0.0, 0.0, 0.2));
    }
}
