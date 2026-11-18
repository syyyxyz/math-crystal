//! The edge of a graph's domain. A graph a = g(b, c) sampled on a grid
//! loses every cell with an undefined corner; where the surface is steep at
//! the edge of its domain (z = √(1 − x² − y²) at its rim) a whole band of the
//! surface goes missing. Here each such cell keeps its defined part: the
//! domain boundary is found by bisection along the cell's edges (the last
//! defined point, with its value) and that part is returned as polygons.

/// Bisection steps along a cell edge: the boundary to 2⁻²⁰ of a cell.
const STEPS: usize = 20;

/// A value counts as defined when finite and not absurdly large (the
/// samplers treat the rest as undefined too).
pub fn defined(v: f64) -> bool {
    v.is_finite() && v.abs() <= 1e30
}

/// Polygon vertex: fractional grid coordinates (i, j) and the value there.
pub type Vertex = [f64; 3];

/// Evaluates the graph at many points: `(bs, cs) → values`.
pub type Eval<'a> = &'a dyn Fn(&[f64], &[f64]) -> Vec<f64>;

/// For every cell of the `nb × nc` grid `v[j * nb + i]` (over `br × cr`)
/// with some but not all corners defined: the defined part of the cell as
/// polygons, counter-clockwise like the cell's corners. `g` evaluates the
/// graph at many (b, c) points.
pub fn edge_polygons(v: &[f64], [nb, nc]: [usize; 2], br: (f64, f64), cr: (f64, f64), g: Eval) -> Vec<Vec<Vertex>> {
    if nb < 2 || nc < 2 || v.len() < nb * nc {
        return Vec::new();
    }
    let ok = |i: usize, j: usize| defined(v[j * nb + i]);
    // Grid edges with one defined end: horizontal (i, j)–(i+1, j) has id
    // 2 * (j * nb + i), vertical (i, j)–(i, j+1) has id 2 * (j * nb + i) + 1.
    struct Edge {
        from: [f64; 2],
        to: [f64; 2],
        lo: f64,
        hi: f64,
        val: f64,
    }
    let mut edges: Vec<Edge> = Vec::new();
    let mut slot = std::collections::HashMap::new();
    let mut add = |id: usize, a: (usize, usize), b: (usize, usize), edges: &mut Vec<Edge>| {
        let (fa, fb) = (ok(a.0, a.1), ok(b.0, b.1));
        if fa == fb {
            return;
        }
        let ((d, dv), u) = if fa { ((a, v[a.1 * nb + a.0]), b) } else { ((b, v[b.1 * nb + b.0]), a) };
        slot.insert(id, edges.len());
        edges.push(Edge { from: [d.0 as f64, d.1 as f64], to: [u.0 as f64, u.1 as f64], lo: 0.0, hi: 1.0, val: dv });
    };
    for j in 0..nc {
        for i in 0..nb {
            if i + 1 < nb {
                add(2 * (j * nb + i), (i, j), (i + 1, j), &mut edges);
            }
            if j + 1 < nc {
                add(2 * (j * nb + i) + 1, (i, j), (i, j + 1), &mut edges);
            }
        }
    }
    if edges.is_empty() {
        return Vec::new();
    }
    // Bisect all edges together, one batch per step.
    let at = |e: &Edge, t: f64| [e.from[0] + (e.to[0] - e.from[0]) * t, e.from[1] + (e.to[1] - e.from[1]) * t];
    let to_b = |f: f64| crate::calc::lerp(br, f / (nb - 1) as f64);
    let to_c = |f: f64| crate::calc::lerp(cr, f / (nc - 1) as f64);
    for _ in 0..STEPS {
        let mids: Vec<[f64; 2]> = edges.iter().map(|e| at(e, (e.lo + e.hi) * 0.5)).collect();
        let bs: Vec<f64> = mids.iter().map(|p| to_b(p[0])).collect();
        let cs: Vec<f64> = mids.iter().map(|p| to_c(p[1])).collect();
        let vals = g(&bs, &cs);
        for (e, &val) in edges.iter_mut().zip(&vals) {
            let t = (e.lo + e.hi) * 0.5;
            if defined(val) {
                e.lo = t;
                e.val = val;
            } else {
                e.hi = t;
            }
        }
    }
    let point = |id: usize| -> Option<Vertex> {
        let e = &edges[*slot.get(&id)?];
        let p = at(e, e.lo);
        Some([p[0], p[1], e.val])
    };

    let mut out = Vec::new();
    for j in 0..nc - 1 {
        for i in 0..nb - 1 {
            // Corners and edges counter-clockwise.
            let corners = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            let mask = corners.map(|(a, b)| ok(a, b));
            let n_ok = mask.iter().filter(|m| **m).count();
            if n_ok == 0 || n_ok == 4 {
                continue;
            }
            let edge_ids = [2 * (j * nb + i), 2 * (j * nb + i + 1) + 1, 2 * ((j + 1) * nb + i), 2 * (j * nb + i) + 1];
            let corner = |k: usize| -> Vertex {
                let (a, b) = corners[k];
                [a as f64, b as f64, v[b * nb + a]]
            };
            if n_ok == 2 && mask[0] == mask[2] {
                // Opposite corners only: two separate corner triangles.
                for k in (0..4).filter(|&k| mask[k]) {
                    if let (Some(p), Some(q)) = (point(edge_ids[k]), point(edge_ids[(k + 3) % 4])) {
                        out.push(vec![corner(k), p, q]);
                    }
                }
                continue;
            }
            let mut poly = Vec::with_capacity(5);
            for k in 0..4 {
                if mask[k] {
                    poly.push(corner(k));
                }
                if mask[k] != mask[(k + 1) % 4]
                    && let Some(p) = point(edge_ids[k])
                {
                    poly.push(p);
                }
            }
            if poly.len() >= 3 {
                out.push(poly);
            }
        }
    }
    out
}

/// Fan triangles of the polygons.
pub fn fan(polys: &[Vec<Vertex>]) -> impl Iterator<Item = [Vertex; 3]> + '_ {
    polys.iter().flat_map(|p| (1..p.len().saturating_sub(1)).map(move |k| [p[0], p[k], p[k + 1]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// z = √(1 − x² − y²) on a coarse grid over [−1.5, 1.5]².
    fn hemisphere(n: usize) -> (Vec<f64>, (f64, f64)) {
        let r = (-1.5, 1.5);
        let mut v = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (crate::calc::lerp(r, i as f64 / (n - 1) as f64), crate::calc::lerp(r, j as f64 / (n - 1) as f64));
                v[j * n + i] = (1.0 - x * x - y * y).sqrt();
            }
        }
        (v, r)
    }

    fn g(xs: &[f64], ys: &[f64]) -> Vec<f64> {
        xs.iter().zip(ys).map(|(x, y)| (1.0 - x * x - y * y).sqrt()).collect()
    }

    #[test]
    fn the_rim_reaches_the_edge_of_the_domain() {
        let n = 21;
        let (v, r) = hemisphere(n);
        let polys = edge_polygons(&v, [n, n], r, r, &g);
        assert!(!polys.is_empty());
        let to = |f: f64| crate::calc::lerp(r, f / (n - 1) as f64);
        // Boundary points lie on the unit circle with z ≈ 0.
        let mut rim = 0;
        for p in polys.iter().flatten() {
            let (x, y) = (to(p[0]), to(p[1]));
            let rr = (x * x + y * y).sqrt();
            assert!(rr <= 1.0 + 1e-9, "outside the domain: {rr}");
            if p[0].fract() != 0.0 || p[1].fract() != 0.0 {
                rim += 1;
                assert!((rr - 1.0).abs() < 1e-5, "boundary point at radius {rr}");
                assert!(p[2] < 0.01, "rim height {}", p[2]);
            }
        }
        assert!(rim > 20);
        // The polygons fill the band the plain grid loses: total area of
        // grid cells plus edge polygons ≈ π.
        let cell = (r.1 - r.0) / (n - 1) as f64;
        let mut full = 0.0;
        for j in 0..n - 1 {
            for i in 0..n - 1 {
                if [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)].iter().all(|&(a, b)| defined(v[b * n + a])) {
                    full += cell * cell;
                }
            }
        }
        let edge: f64 = fan(&polys).map(|[a, b, c]| ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() * 0.5 * cell * cell).sum();
        let total = full + edge;
        assert!(full < 2.9, "the plain grid alone misses a band ({full})");
        assert!((total - std::f64::consts::PI).abs() < 0.02, "area {total}");
    }

    #[test]
    fn nothing_to_do_without_undefined_points() {
        let n = 9;
        let v = vec![1.0; n * n];
        assert!(edge_polygons(&v, [n, n], (0.0, 1.0), (0.0, 1.0), &|xs, _| vec![1.0; xs.len()]).is_empty());
    }
}
