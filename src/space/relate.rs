//! How every pair of objects meets: intersection curves and points,
//! tangency, the textbook relations of lines and planes, and plane
//! sections of quadrics. Also the display meshes cut along all curves.

use glam::DVec3;

use super::curve::clip_line;
use super::intersect::{self, Conic, Field, Hit, curve_hits, march, refine, split, touches};
use super::mesh::DMesh;
use super::object::{Kind, Object};
use super::surface::SurfaceKind;

#[derive(Clone, Debug, PartialEq)]
pub enum Rel {
    /// Computed numerically only.
    General,
    PlanePlane { angle: f64, kind: PlanesKind, line: Option<(DVec3, DVec3)>, dist: f64 },
    LinePlane { angle: f64, kind: LinePlaneKind, point: Option<DVec3>, dist: f64 },
    LineLine { angle: f64, kind: LinesKind, point: Option<DVec3>, dist: f64, feet: Option<(DVec3, DVec3)> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlanesKind {
    Crossing,
    Parallel,
    Same,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinePlaneKind {
    Crossing,
    Parallel,
    InPlane,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinesKind {
    Crossing,
    Parallel,
    Same,
    Skew,
}

#[derive(Clone, Debug)]
pub struct Pair {
    pub i: usize,
    pub j: usize,
    pub rel: Rel,
    /// Intersection curves (data coordinates).
    pub curves: Vec<Vec<DVec3>>,
    /// Points where a line or curve meets the other object.
    pub hits: Vec<Hit>,
    /// Points where two surfaces touch.
    pub touches: Vec<DVec3>,
    /// A plane section of a quadric.
    pub conic: Option<Conic>,
    /// The line or curve lies on the surface (inside the box).
    pub lies_on: bool,
}

#[derive(Default)]
pub struct Analysis {
    pub pairs: Vec<Pair>,
    /// Meshes cut along every intersection (for drawing), per object.
    pub meshes: Vec<Option<DMesh>>,
}

const EPS: f64 = 1e-9;

fn angle_deg(cos: f64) -> f64 {
    cos.abs().clamp(0.0, 1.0).acos().to_degrees()
}

pub fn planes(n1: DVec3, d1: f64, n2: DVec3, d2: f64) -> Rel {
    let c = n1.cross(n2);
    let angle = angle_deg(n1.dot(n2) / (n1.length() * n2.length()));
    if c.length() <= EPS * n1.length() * n2.length() {
        let u = n1.normalize();
        let s = n2.dot(u);
        let dist = (d1 / n1.length() - d2 / s).abs();
        let kind = if dist <= EPS * (1.0 + d1.abs() / n1.length()) { PlanesKind::Same } else { PlanesKind::Parallel };
        return Rel::PlanePlane { angle: 0.0, kind, line: None, dist };
    }
    // Point nearest the origin on both planes.
    let m = glam::DMat3::from_cols(n1, n2, c).transpose();
    let p = m.inverse() * DVec3::new(d1, d2, 0.0);
    Rel::PlanePlane { angle, kind: PlanesKind::Crossing, line: Some((p, c)), dist: 0.0 }
}

pub fn line_plane(p: DVec3, d: DVec3, n: DVec3, dd: f64) -> Rel {
    let nd = n.dot(d);
    let angle = 90.0 - angle_deg(nd / (n.length() * d.length()));
    let off = n.dot(p) - dd;
    if nd.abs() <= EPS * n.length() * d.length() {
        let dist = off.abs() / n.length();
        let kind = if dist <= EPS * (1.0 + p.length()) { LinePlaneKind::InPlane } else { LinePlaneKind::Parallel };
        return Rel::LinePlane { angle: 0.0, kind, point: None, dist };
    }
    let t = -off / nd;
    Rel::LinePlane { angle, kind: LinePlaneKind::Crossing, point: Some(p + d * t), dist: 0.0 }
}

pub fn lines(p1: DVec3, d1: DVec3, p2: DVec3, d2: DVec3) -> Rel {
    let c = d1.cross(d2);
    let w = p2 - p1;
    let angle = angle_deg(d1.dot(d2) / (d1.length() * d2.length()));
    let scale = 1.0 + p1.length() + p2.length();
    if c.length() <= EPS * d1.length() * d2.length() {
        let dist = w.cross(d1).length() / d1.length();
        let kind = if dist <= EPS * scale { LinesKind::Same } else { LinesKind::Parallel };
        return Rel::LineLine { angle: 0.0, kind, point: None, dist, feet: None };
    }
    let s = w.cross(d2).dot(c) / c.length_squared();
    let t = w.cross(d1).dot(c) / c.length_squared();
    let (f1, f2) = (p1 + d1 * s, p2 + d2 * t);
    let dist = (f2 - f1).length();
    if dist <= EPS * scale {
        Rel::LineLine { angle, kind: LinesKind::Crossing, point: Some(f1), dist: 0.0, feet: None }
    } else {
        Rel::LineLine { angle, kind: LinesKind::Skew, point: None, dist, feet: Some((f1, f2)) }
    }
}

fn plane_of(o: &Object) -> Option<(DVec3, f64)> {
    (o.kind == Kind::Surface).then(|| o.plane()).flatten()
}

fn quadric_of(o: &Object) -> Option<&super::quadric::Quadric> {
    match &o.surface.as_ref()?.kind {
        SurfaceKind::Quadric(q) if o.kind == Kind::Surface => Some(q),
        _ => None,
    }
}

/// Parametrisation and t-range of a line or curve inside the box.
fn curve_param(o: &Object, lo: DVec3, hi: DVec3) -> Option<(Box<dyn Fn(f64) -> Option<DVec3> + '_>, f64, f64, usize)> {
    let c = o.curve.as_ref().filter(|c| c.error.is_none())?;
    if let Some((p, d)) = c.line {
        // A little past the box so touching points on its faces count.
        let pad = (hi - lo) * 0.01;
        let (t0, t1) = clip_line(p, d, lo - pad, hi + pad)?;
        return Some((Box::new(move |t| Some(p + d * t)), t0, t1, 800));
    }
    Some((Box::new(move |t| c.at(t)), o.t_range.0, o.t_range.1, 3000))
}

fn inside(p: DVec3, lo: DVec3, hi: DVec3) -> bool {
    let e = (hi - lo) * 1e-9;
    p.cmpge(lo - e).all() && p.cmple(hi + e).all()
}

/// Whether every sample of the curve inside the box is on the surface.
fn lies_on(r: &dyn Fn(f64) -> Option<DVec3>, t0: f64, t1: f64, f: &Field, scale: f64, lo: DVec3, hi: DVec3) -> bool {
    let mut count = 0;
    for k in 0..=200 {
        let Some(p) = r(t0 + (t1 - t0) * k as f64 / 200.0) else { continue };
        if !inside(p, lo, hi) {
            continue;
        }
        let (v, g) = f.vg(p);
        if !(v.is_finite() && g.length() > 0.0 && v.abs() / g.length() < 1e-7 * scale) {
            return false;
        }
        count += 1;
    }
    count >= 4
}

/// Analyse all pairs. Meshes must be up to date (`Object::mesh`); `h` is
/// the mesh cell size. `extra` cuts (such as a section plane) are applied
/// to the display meshes too.
pub fn analyse(objs: &[Object], lo: DVec3, hi: DVec3, h: f64, extra: &[Field]) -> Analysis {
    let scale = (hi - lo).max_element();
    let tol = 1e-9 * scale;
    let mut pairs = Vec::new();
    for i in 0..objs.len() {
        for j in i + 1..objs.len() {
            let (a, b) = (&objs[i], &objs[j]);
            let mut pair = Pair { i, j, rel: Rel::General, curves: vec![], hits: vec![], touches: vec![], conic: None, lies_on: false };
            match (a.field(), b.field()) {
                (Some(fa), Some(fb)) => {
                    let (pa, pb) = (plane_of(a), plane_of(b));
                    if let (Some((n1, d1)), Some((n2, d2))) = (pa, pb) {
                        pair.rel = planes(n1, d1, n2, d2);
                        if let Rel::PlanePlane { line: Some((p, d)), .. } = pair.rel
                            && let Some((t0, t1)) = clip_line(p, d, lo, hi)
                        {
                            pair.curves.push(vec![p + d * t0, p + d * t1]);
                        }
                    } else {
                        // March on the curved one's mesh.
                        let (on, fon, fother) = if pa.is_some() { (b, fb, fa) } else { (a, fa, fb) };
                        let m = on.cached_mesh();
                        let g = fother.vals(&m.pos);
                        let mut curves = march(m, &g, tol);
                        for c in &mut curves {
                            refine(c, &fon, &fother, h);
                        }
                        pair.touches = touches(m, &fon, &fother, &g, &curves, h, scale, lo, hi);
                        pair.curves = curves;
                        let q = quadric_of(a).zip(pb).or_else(|| quadric_of(b).zip(pa));
                        if let Some((q, (n, d))) = q {
                            pair.conic = Some(intersect::conic(q, n, d, scale));
                        }
                    }
                }
                (Some(f), None) | (None, Some(f)) => {
                    let (surf, cur) = if a.field().is_some() { (a, b) } else { (b, a) };
                    if let (Some((p, d)), Some((n, dd))) = (cur.line(), plane_of(surf)) {
                        pair.rel = line_plane(p, d, n, dd);
                        if let Rel::LinePlane { point: Some(x), .. } = pair.rel
                            && inside(x, lo, hi)
                        {
                            pair.hits.push(Hit { p: x, t: 0.0, tangent: false });
                        }
                    } else if let Some((r, t0, t1, n)) = curve_param(cur, lo, hi) {
                        pair.lies_on = lies_on(&*r, t0, t1, &f, scale, lo, hi);
                        if !pair.lies_on {
                            pair.hits = curve_hits(&*r, t0, t1, &f, n, scale).into_iter().filter(|h| inside(h.p, lo, hi)).collect();
                        }
                    }
                }
                (None, None) => {
                    if let (Some((p1, d1)), Some((p2, d2))) = (a.line(), b.line()) {
                        pair.rel = lines(p1, d1, p2, d2);
                        if let Rel::LineLine { point: Some(x), .. } = pair.rel
                            && inside(x, lo, hi)
                        {
                            pair.hits.push(Hit { p: x, t: 0.0, tangent: false });
                        }
                    }
                }
            }
            pairs.push(pair);
        }
    }
    // Display meshes cut along every other surface and the extra cuts.
    let meshes = objs
        .iter()
        .enumerate()
        .map(|(i, o)| {
            o.field()?;
            let mut m = o.cached_mesh().clone();
            let others: Vec<Field> = objs.iter().enumerate().filter(|(k, _)| *k != i).filter_map(|(_, q)| q.field()).chain(extra.iter().copied()).collect();
            for f in others {
                let g = f.vals(&m.pos);
                m = split(&m, &g);
            }
            Some(m)
        })
        .collect();
    Analysis { pairs, meshes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textbook_relations() {
        // Planes x + y + z = 1 and x − y = 0.
        match planes(DVec3::ONE, 1.0, DVec3::new(1.0, -1.0, 0.0), 0.0) {
            Rel::PlanePlane { angle, kind: PlanesKind::Crossing, line: Some((p, d)), .. } => {
                assert!((angle - 90.0).abs() < 1e-9);
                assert!((p.x + p.y + p.z - 1.0).abs() < 1e-12 && (p.x - p.y).abs() < 1e-12);
                assert!(d.dot(DVec3::ONE).abs() < 1e-12);
            }
            r => panic!("{r:?}"),
        }
        assert!(matches!(planes(DVec3::Z, 1.0, DVec3::Z * 2.0, 6.0), Rel::PlanePlane { kind: PlanesKind::Parallel, dist, .. } if (dist - 2.0).abs() < 1e-12));
        assert!(matches!(planes(DVec3::Z, 1.0, DVec3::Z * -2.0, -2.0), Rel::PlanePlane { kind: PlanesKind::Same, .. }));
        // Line along z through (1, 0, 0) meets z = 2 at (1, 0, 2), at 90°.
        match line_plane(DVec3::X, DVec3::Z, DVec3::Z, 2.0) {
            Rel::LinePlane { angle, kind: LinePlaneKind::Crossing, point: Some(p), .. } => {
                assert!((angle - 90.0).abs() < 1e-9 && (p - DVec3::new(1.0, 0.0, 2.0)).length() < 1e-12);
            }
            r => panic!("{r:?}"),
        }
        assert!(matches!(line_plane(DVec3::ZERO, DVec3::X, DVec3::Z, 0.0), Rel::LinePlane { kind: LinePlaneKind::InPlane, .. }));
        // Skew: x axis and the line (0, t, 1); distance 1, angle 90°.
        match lines(DVec3::ZERO, DVec3::X, DVec3::Z, DVec3::Y) {
            Rel::LineLine { angle, kind: LinesKind::Skew, dist, feet: Some((f1, f2)), .. } => {
                assert!((angle - 90.0).abs() < 1e-9 && (dist - 1.0).abs() < 1e-12);
                assert!(f1.length() < 1e-12 && (f2 - DVec3::Z).length() < 1e-12);
            }
            r => panic!("{r:?}"),
        }
        assert!(matches!(lines(DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::new(1.0, -1.0, 0.0)), Rel::LineLine { kind: LinesKind::Crossing, point: Some(p), .. } if (p - DVec3::X).length() < 1e-12));
        assert!(matches!(lines(DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::X * 2.0), Rel::LineLine { kind: LinesKind::Parallel, dist, .. } if (dist - 1.0).abs() < 1e-12));
    }

    #[test]
    fn scene_pairs() {
        let (lo, hi) = (DVec3::splat(-3.0), DVec3::splat(3.0));
        let mut objs = vec![
            Object::new(Kind::Surface, "x^2 + y^2 + z^2 = 4", 0),
            Object::new(Kind::Surface, "z = 2", 1),
            Object::new(Kind::Curve, "(t, 0, 1)", 2),
        ];
        for o in &mut objs {
            o.mesh(lo, hi, 48, "");
        }
        let an = analyse(&objs, lo, hi, 6.0 / 48.0, &[]);
        let sp = &an.pairs[0];
        assert_eq!(sp.touches.len(), 1, "{:?}", sp.touches);
        assert_eq!(sp.conic.as_ref().unwrap().kind, intersect::ConicKind::Point);
        let sl = &an.pairs[1];
        assert_eq!(sl.hits.len(), 2);
        assert!(sl.hits.iter().all(|h| (h.p.x.abs() - 3f64.sqrt()).abs() < 1e-9));
        assert!(matches!(an.pairs[2].rel, Rel::LinePlane { kind: LinePlaneKind::Parallel, .. }));
        assert!(an.meshes[0].as_ref().unwrap().tris.len() >= objs[0].cached_mesh().tris.len());
        assert!(an.meshes[2].is_none());
    }
}

#[cfg(test)]
mod lies_on_tests {
    use super::*;

    #[test]
    fn rulings_lie_on_the_hyperboloid() {
        let (lo, hi) = (DVec3::splat(-3.0), DVec3::splat(3.0));
        let mut objs = vec![Object::new(Kind::Surface, "x^2 + y^2 - z^2 = 1", 0), Object::new(Kind::Curve, "(1, t, t)", 1), Object::new(Kind::Curve, "(cos(t), sin(t), t/4)", 2)];
        for o in &mut objs {
            o.mesh(lo, hi, 32, "");
        }
        let an = analyse(&objs, lo, hi, 6.0 / 32.0, &[]);
        assert!(an.pairs[0].lies_on && an.pairs[0].hits.is_empty());
        // The helix x² + y² = 1 meets the hyperboloid only where z = 0.
        assert!(!an.pairs[1].lies_on);
        assert_eq!(an.pairs[1].hits.len(), 1, "{:?}", an.pairs[1].hits);
    }
}
