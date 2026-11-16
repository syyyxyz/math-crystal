//! Triangle meshes in data coordinates: implicit surfaces by surface nets
//! (one vertex per sign-changing cell, projected onto F = 0 by Newton
//! steps along ∇F), planes clipped to the box, and the preset solids.

use std::f64::consts::TAU;

use glam::DVec3;

use super::solid::{Solid, SolidKind};
use super::surface::Implicit;

#[derive(Clone, Default, Debug)]
pub struct DMesh {
    pub pos: Vec<DVec3>,
    pub nrm: Vec<DVec3>,
    pub tris: Vec<[u32; 3]>,
    /// Per-vertex coordinates for grid lines (data units), if any.
    pub uv: Option<Vec<[f64; 2]>>,
    /// Outlines (sharp edges of solids) as polylines.
    pub edges: Vec<Vec<DVec3>>,
}

impl DMesh {
    fn push(&mut self, p: DVec3, n: DVec3) -> u32 {
        self.pos.push(p);
        self.nrm.push(n);
        (self.pos.len() - 1) as u32
    }
}

/// Corner offsets of a cell, bit k of the index = offset along axis k.
const CORNER: [[usize; 3]; 8] = [[0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 1, 0], [0, 0, 1], [1, 0, 1], [0, 1, 1], [1, 1, 1]];
/// The 12 cell edges as corner pairs.
const EDGES: [(usize, usize); 12] = [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)];

/// Grid cell counts for a box with about `n` cells along its longest side.
pub fn grid_dims(lo: DVec3, hi: DVec3, n: usize) -> [usize; 3] {
    let ext = hi - lo;
    let h = ext.max_element() / n as f64;
    [0, 1, 2].map(|i| ((ext[i] / h).round() as usize).max(2))
}

/// Mesh of F = 0 inside the box, about `n` cells along the longest side.
pub fn surface_nets(s: &Implicit, lo: DVec3, hi: DVec3, n: usize) -> DMesh {
    let inner = grid_dims(lo, hi, n);
    let step = (hi - lo) / DVec3::new(inner[0] as f64, inner[1] as f64, inner[2] as f64);
    // Two extra cells beyond each face: the renderer clips exactly at the
    // box, so surfaces end in a clean edge instead of a staircase.
    const PAD: usize = 2;
    let lo = lo - step * PAD as f64;
    let dims = inner.map(|d| d + 2 * PAD);
    let (nx, ny, nz) = (dims[0] + 1, dims[1] + 1, dims[2] + 1);
    let gidx = |i: usize, j: usize, k: usize| i + nx * (j + ny * k);
    let total = nx * ny * nz;
    let mut xs = Vec::with_capacity(total);
    let mut ys = Vec::with_capacity(total);
    let mut zs = Vec::with_capacity(total);
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                xs.push(lo.x + step.x * i as f64);
                ys.push(lo.y + step.y * j as f64);
                zs.push(lo.z + step.z * k as f64);
            }
        }
    }
    let f = s.values(&xs, &ys, &zs);
    let gp = |i: usize, j: usize, k: usize| DVec3::new(xs[gidx(i, j, k)], ys[gidx(i, j, k)], zs[gidx(i, j, k)]);
    let inside = |v: f64| v < 0.0;

    // Candidate vertex per sign-changing cell.
    let (cx, cy, cz) = (dims[0], dims[1], dims[2]);
    let cidx = |i: usize, j: usize, k: usize| i + cx * (j + cy * k);
    let mut cell_vertex = vec![u32::MAX; cx * cy * cz];
    let mut cand_cell = Vec::new();
    let mut cand_pos = Vec::new();
    let mut cand_mag = Vec::new();
    for k in 0..cz {
        for j in 0..cy {
            for i in 0..cx {
                let v: [f64; 8] = CORNER.map(|o| f[gidx(i + o[0], j + o[1], k + o[2])]);
                if v.iter().any(|x| !x.is_finite()) {
                    continue;
                }
                let n_in = v.iter().filter(|x| inside(**x)).count();
                if n_in == 0 || n_in == 8 {
                    continue;
                }
                let mut sum = DVec3::ZERO;
                let mut mag = 0.0;
                let mut count = 0.0;
                for (a, b) in EDGES {
                    if inside(v[a]) == inside(v[b]) {
                        continue;
                    }
                    let t = v[a] / (v[a] - v[b]);
                    let pa = gp(i + CORNER[a][0], j + CORNER[a][1], k + CORNER[a][2]);
                    let pb = gp(i + CORNER[b][0], j + CORNER[b][1], k + CORNER[b][2]);
                    sum += pa + (pb - pa) * t;
                    mag += (v[a].abs() + v[b].abs()) * 0.5;
                    count += 1.0;
                }
                cand_cell.push(cidx(i, j, k));
                cand_pos.push(sum / count);
                cand_mag.push(mag / count);
            }
        }
    }

    // Project onto F = 0: two Newton steps, each at most one cell long.
    let hmin = step.min_element();
    let start = cand_pos.clone();
    let mut fstart = vec![f64::NAN; start.len()];
    let mut pos = cand_pos;
    for it in 0..3 {
        let vg = s.values_grads(&pos);
        for (k, (fv, g)) in vg.into_iter().enumerate() {
            if it == 0 {
                fstart[k] = fv;
            }
            let g2 = g.length_squared();
            if !(fv.is_finite() && g2.is_finite()) || g2 == 0.0 {
                continue;
            }
            let mut d = g * (fv / g2);
            let len = d.length();
            if len > hmin {
                d *= hmin / len;
            }
            pos[k] -= d;
        }
    }
    let vg = s.values_grads(&pos);
    let mut mesh = DMesh::default();
    for k in 0..pos.len() {
        let c = cand_cell[k];
        let (i, j, l) = (c % cx, (c / cx) % cy, c / (cx * cy));
        let cell_lo = gp(i, j, l) - step * 0.5;
        let cell_hi = gp(i + 1, j + 1, l + 1) + step * 0.5;
        let within = |p: DVec3| p.cmpge(cell_lo).all() && p.cmple(cell_hi).all();
        let (fv, g) = vg[k];
        let p = if fv.is_finite() && fv.abs() <= 0.25 * cand_mag[k] && within(pos[k]) {
            pos[k]
        } else if fstart[k].is_finite() && fstart[k].abs() <= 0.5 * cand_mag[k] {
            start[k]
        } else {
            // A jump of F (a pole), not a surface.
            continue;
        };
        let n = if p == pos[k] && g.is_finite() && g.length_squared() > 0.0 { g.normalize() } else { DVec3::ZERO };
        cell_vertex[c] = mesh.push(p, n);
    }

    // One quad per sign-changing grid edge, joining the 4 cells around it.
    let mut face_n = vec![DVec3::ZERO; mesh.pos.len()];
    for axis in 0..3 {
        let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
        let mut g = [0usize; 3];
        for k in 0..nz {
            for j in 0..ny {
                for i in 0..nx {
                    g[0] = i;
                    g[1] = j;
                    g[2] = k;
                    // The four cells around the edge must exist.
                    if g[axis] >= dims[axis] || g[b] == 0 || g[c] == 0 || g[b] >= dims[b] || g[c] >= dims[c] {
                        continue;
                    }
                    let mut h = g;
                    h[axis] += 1;
                    let fa = f[gidx(g[0], g[1], g[2])];
                    let fb = f[gidx(h[0], h[1], h[2])];
                    if !(fa.is_finite() && fb.is_finite()) || inside(fa) == inside(fb) {
                        continue;
                    }
                    let cell = |db: usize, dc: usize| {
                        let mut q = g;
                        q[b] -= db;
                        q[c] -= dc;
                        cell_vertex[cidx(q[0], q[1], q[2])]
                    };
                    let q = [cell(1, 1), cell(0, 1), cell(0, 0), cell(1, 0)];
                    if q.contains(&u32::MAX) {
                        continue;
                    }
                    let tri = if inside(fa) { [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] } else { [[q[0], q[2], q[1]], [q[0], q[3], q[2]]] };
                    for t in tri {
                        let [p0, p1, p2] = t.map(|v| mesh.pos[v as usize]);
                        let fnrm = (p1 - p0).cross(p2 - p0);
                        for v in t {
                            face_n[v as usize] += fnrm;
                        }
                        mesh.tris.push(t);
                    }
                }
            }
        }
    }
    for (n, fnrm) in mesh.nrm.iter_mut().zip(face_n) {
        if *n == DVec3::ZERO {
            *n = fnrm.try_normalize().unwrap_or(DVec3::Z);
        } else if n.dot(fnrm) < 0.0 {
            // Keep the gradient's direction consistent with the winding.
            *n = -*n;
        }
    }
    mesh
}

/// The polygon where the plane `n · p = d` meets the box.
pub fn plane_polygon(n: DVec3, d: f64, lo: DVec3, hi: DVec3) -> Vec<DVec3> {
    let len = n.length();
    if len == 0.0 {
        return vec![];
    }
    let nh = n / len;
    let bc = (lo + hi) * 0.5;
    let c = bc + nh * ((d - n.dot(bc)) / len);
    // In-plane axes: the coordinate axis least aligned with n.
    let i = (0..3).min_by(|&a, &b| nh[a].abs().total_cmp(&nh[b].abs())).unwrap();
    let e = [DVec3::X, DVec3::Y, DVec3::Z][i];
    let u = (e - nh * nh.dot(e)).normalize();
    let v = nh.cross(u);
    let r = (hi - lo).length();
    let mut poly = vec![c - u * r - v * r, c + u * r - v * r, c + u * r + v * r, c - u * r + v * r];
    for axis in 0..3 {
        for (bound, keep_greater) in [(lo[axis], true), (hi[axis], false)] {
            let inside = |p: DVec3| if keep_greater { p[axis] >= bound } else { p[axis] <= bound };
            let mut out = Vec::new();
            for k in 0..poly.len() {
                let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                if inside(a) {
                    out.push(a);
                }
                if inside(a) != inside(b) {
                    let t = (bound - a[axis]) / (b[axis] - a[axis]);
                    let mut p = a + (b - a) * t;
                    p[axis] = bound;
                    out.push(p);
                }
            }
            poly = out;
            if poly.is_empty() {
                return poly;
            }
        }
    }
    poly
}

/// Plane mesh with grid coordinates along the two axes least aligned with
/// the normal (so the grid lines are x = const, y = const etc.). Triangles
/// are at most about `h` across so that cuts along curves follow them.
pub fn plane_mesh(n: DVec3, d: f64, lo: DVec3, hi: DVec3, h: f64) -> DMesh {
    let poly = plane_polygon(n, d, lo, hi);
    let mut m = DMesh::default();
    if poly.len() < 3 {
        return m;
    }
    let nh = n.normalize();
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| nh[a].abs().total_cmp(&nh[b].abs()));
    let (a, b) = (order[0].min(order[1]), order[0].max(order[1]));
    let centre = poly.iter().copied().sum::<DVec3>() / poly.len() as f64;
    // Fan from the centre, each triangle split into k² smaller ones.
    for i in 0..poly.len() {
        let (p1, p2) = (poly[i], poly[(i + 1) % poly.len()]);
        let longest = (p1 - centre).length().max((p2 - centre).length()).max((p2 - p1).length());
        let k = ((longest / h.max(1e-9)).ceil() as usize).clamp(1, 200);
        let base = m.pos.len() as u32;
        // Barycentric grid: row r has r + 1 points.
        for r in 0..=k {
            for c in 0..=r {
                let (u, v) = (r as f64 / k as f64, c as f64 / k as f64);
                m.push(centre + (p1 - centre) * (u - v) + (p2 - centre) * v, nh);
            }
        }
        let at = |r: usize, c: usize| base + (r * (r + 1) / 2 + c) as u32;
        for r in 0..k {
            for c in 0..=r {
                m.tris.push([at(r, c), at(r + 1, c), at(r + 1, c + 1)]);
                if c < r {
                    m.tris.push([at(r, c), at(r + 1, c + 1), at(r, c + 1)]);
                }
            }
        }
    }
    m.uv = Some(m.pos.iter().map(|p| [p[a], p[b]]).collect());
    m.edges.push(poly.iter().copied().chain(std::iter::once(poly[0])).collect());
    m
}

/// Axis frame of a cylinder or cone: (axis, two perpendicular unit vectors).
fn frame(axis: usize) -> (DVec3, DVec3, DVec3) {
    let e = [DVec3::X, DVec3::Y, DVec3::Z];
    (e[axis], e[(axis + 1) % 3], e[(axis + 2) % 3])
}

/// Grid of `rows × segs` quads over a parametrised patch.
fn patch(m: &mut DMesh, rows: usize, segs: usize, wrap: bool, f: impl Fn(f64, f64) -> (DVec3, DVec3)) {
    let base = m.pos.len() as u32;
    let cols = if wrap { segs } else { segs + 1 };
    for r in 0..=rows {
        for s in 0..cols {
            let (p, n) = f(r as f64 / rows as f64, s as f64 / segs as f64);
            m.push(p, n);
        }
    }
    let at = |r: usize, s: usize| base + (r * cols + s % cols) as u32;
    for r in 0..rows {
        for s in 0..segs {
            let (a, b, c, d) = (at(r, s), at(r, s + 1), at(r + 1, s + 1), at(r + 1, s));
            m.tris.push([a, b, c]);
            m.tris.push([a, c, d]);
        }
    }
}

fn circle(c: DVec3, u: DVec3, v: DVec3, r: f64, n: usize) -> Vec<DVec3> {
    (0..=n).map(|k| {
        let t = TAU * k as f64 / n as f64;
        c + (u * t.cos() + v * t.sin()) * r
    }).collect()
}

/// Exact mesh of a preset solid with its sharp edges.
pub fn solid_mesh(s: &Solid) -> DMesh {
    let mut m = DMesh::default();
    const SEGS: usize = 96;
    match s.kind {
        SolidKind::Ball => {
            let r = s.radius;
            patch(&mut m, 48, SEGS, true, |a, b| {
                let th = std::f64::consts::PI * a;
                let ph = TAU * b;
                let n = DVec3::new(th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos());
                (s.anchor + n * r, n)
            });
        }
        SolidKind::Cylinder | SolidKind::Cone => {
            let (a, u, v) = frame(s.axis);
            let h = s.height;
            let r = s.radius;
            let cone = s.kind == SolidKind::Cone;
            let top = s.anchor + a * h;
            let sgn = h.signum();
            // Side.
            patch(&mut m, 24, SEGS, true, |t, w| {
                let ph = TAU * w;
                let radial = u * ph.cos() + v * ph.sin();
                let rr = if cone { r * (1.0 - t) } else { r };
                let p = s.anchor + a * (h * t) + radial * rr;
                let n = if cone { (radial * h.abs() + a * (r * sgn)).normalize() } else { radial };
                (p, n)
            });
            // Base (and top) discs, flat.
            let mut disc = |c: DVec3, nrm: DVec3| {
                patch(&mut m, 8, SEGS, true, |t, w| {
                    let ph = TAU * w;
                    (c + (u * ph.cos() + v * ph.sin()) * (r * t), nrm)
                });
            };
            disc(s.anchor, -a * sgn);
            if !cone {
                disc(top, a * sgn);
            }
            m.edges.push(circle(s.anchor, u, v, r, 128));
            if !cone {
                m.edges.push(circle(top, u, v, r, 128));
            }
        }
        SolidKind::Cuboid => {
            let lo = s.anchor;
            let hi = s.anchor + s.size;
            for axis in 0..3 {
                let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
                for side in [0.0, 1.0] {
                    let mut nrm = DVec3::ZERO;
                    nrm[axis] = if side > 0.5 { 1.0 } else { -1.0 };
                    patch(&mut m, 16, 16, false, |t, w| {
                        let mut p = DVec3::ZERO;
                        p[axis] = lo[axis] + (hi[axis] - lo[axis]) * side;
                        p[b] = lo[b] + (hi[b] - lo[b]) * t;
                        p[c] = lo[c] + (hi[c] - lo[c]) * w;
                        (p, nrm)
                    });
                }
            }
            for axis in 0..3 {
                let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
                for (sb, sc) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                    let mut p = lo;
                    p[b] = lo[b] + (hi[b] - lo[b]) * sb;
                    p[c] = lo[c] + (hi[c] - lo[c]) * sc;
                    let mut q = p;
                    p[axis] = lo[axis];
                    q[axis] = hi[axis];
                    m.edges.push(vec![p, q]);
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_vertices_lie_on_the_sphere() {
        let s = Implicit::new("x^2 + y^2 + z^2 = 4").unwrap();
        let m = surface_nets(&s, DVec3::splat(-3.0), DVec3::splat(3.0), 40);
        assert!(m.tris.len() > 1000, "{}", m.tris.len());
        for (p, n) in m.pos.iter().zip(&m.nrm) {
            assert!((p.length() - 2.0).abs() < 1e-6, "{p}");
            assert!((n.dot(p.normalize()).abs() - 1.0).abs() < 1e-6);
        }
        // Closed surface: every edge is shared by exactly two triangles.
        let mut count = std::collections::HashMap::new();
        for t in &m.tris {
            for k in 0..3 {
                let (a, b) = (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]));
                *count.entry((a, b)).or_insert(0) += 1;
            }
        }
        assert!(count.values().all(|&c| c == 2));
    }

    #[test]
    fn poles_are_not_surfaces() {
        // 1/x = 0 has no points, but 1/x changes sign at x = 0.
        let s = Implicit::new("1/x").unwrap();
        let m = surface_nets(&s, DVec3::new(-1.03, -1.0, -1.0), DVec3::new(0.97, 1.0, 1.0), 20);
        assert!(m.tris.is_empty(), "{} triangles", m.tris.len());
    }

    #[test]
    fn cone_apex_is_meshed() {
        let s = Implicit::new("x^2 + y^2 = z^2").unwrap();
        let m = surface_nets(&s, DVec3::splat(-1.0), DVec3::splat(1.0), 24);
        assert!(m.pos.iter().all(|p| (p.x * p.x + p.y * p.y - p.z * p.z).abs() < 0.05));
        assert!(m.pos.iter().any(|p| p.length() < 0.1));
    }

    #[test]
    fn plane_clipped_to_the_box() {
        // x + y + z = 0 cuts the cube [-1, 1]³ in a regular hexagon.
        let poly = plane_polygon(DVec3::ONE, 0.0, DVec3::splat(-1.0), DVec3::splat(1.0));
        assert_eq!(poly.len(), 6);
        for p in &poly {
            assert!((p.x + p.y + p.z).abs() < 1e-12);
            assert!(p.abs().max_element() <= 1.0 + 1e-12);
        }
        assert!(plane_polygon(DVec3::Z, 5.0, DVec3::splat(-1.0), DVec3::splat(1.0)).is_empty());
    }

    #[test]
    fn solid_meshes_lie_on_their_boundary() {
        for kind in [SolidKind::Ball, SolidKind::Cylinder, SolidKind::Cone, SolidKind::Cuboid] {
            let s = Solid::preset(kind);
            let m = solid_mesh(&s);
            assert!(!m.tris.is_empty());
            for p in &m.pos {
                assert!(s.inside_fn(*p).abs() < 1e-9, "{kind:?} {p}");
            }
        }
    }
}
