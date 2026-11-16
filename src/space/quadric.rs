//! Classification of quadric surfaces: principal axes by a symmetric
//! eigen-decomposition, completing the square, and the textbook name and
//! standard form.
//!
//! The quadric is `uᵀA u + 2 b·u + c = 0` in coordinates `u = p − p0`.

use glam::DVec3;

use crate::i18n::tr;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Ellipsoid,
    Hyperboloid1,
    Hyperboloid2,
    Cone,
    EllipticParaboloid,
    HyperbolicParaboloid,
    EllipticCylinder,
    HyperbolicCylinder,
    ParabolicCylinder,
    IntersectingPlanes,
    ParallelPlanes,
    DoublePlane,
    Line,
    Point,
    Empty,
}

/// One term `k · v²` of the standard form, `v` measured along `axis` from
/// `origin`.
#[derive(Clone, Copy, Debug)]
pub struct Term {
    pub k: f64,
    pub axis: DVec3,
}

#[derive(Clone, Debug)]
pub struct Quadric {
    pub class: Class,
    /// Centre (central quadrics), vertex (paraboloids) or a point on the
    /// axis / centre line or plane.
    pub origin: DVec3,
    /// Squared terms with their principal directions.
    pub terms: Vec<Term>,
    /// Standard form `Σ k v² = rhs` where rhs is 1, 0 (cones and degenerate
    /// cases) or `w` along `w_axis` (paraboloids, parabolic cylinders).
    pub rhs: Rhs,
    /// Two equal coefficients: a surface of revolution (or a sphere when
    /// all three agree); the axis of revolution.
    pub revolution: Option<DVec3>,
    pub sphere: bool,
    /// The input `(A, b, c, p0)`: `uᵀA u + 2 b·u + c` with `u = p − p0`.
    pub raw: ([[f64; 3]; 3], DVec3, f64, DVec3),
}

#[derive(Clone, Copy, Debug)]
pub enum Rhs {
    One,
    Zero,
    /// Linear term along this direction.
    Linear(DVec3),
}

/// Eigenvalues and orthonormal eigenvectors of a symmetric 3×3 matrix
/// (cyclic Jacobi rotations).
pub fn eigen_sym(m: [[f64; 3]; 3]) -> ([f64; 3], [DVec3; 3]) {
    let mut a = m;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..60 {
        let off = a[0][1].powi(2) + a[0][2].powi(2) + a[1][2].powi(2);
        let diag = a[0][0].powi(2) + a[1][1].powi(2) + a[2][2].powi(2);
        if off <= 1e-30 * diag.max(1e-300) {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            // A ← Jᵀ A J
            for k in 0..3 {
                let akp = a[k][p];
                let akq = a[k][q];
                a[k][p] = c * akp - s * akq;
                a[k][q] = s * akp + c * akq;
            }
            for k in 0..3 {
                let apk = a[p][k];
                let aqk = a[q][k];
                a[p][k] = c * apk - s * aqk;
                a[q][k] = s * apk + c * aqk;
            }
            for row in &mut v {
                let vp = row[p];
                let vq = row[q];
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    let vals = [a[0][0], a[1][1], a[2][2]];
    let vecs = [0, 1, 2].map(|j| DVec3::new(v[0][j], v[1][j], v[2][j]).normalize());
    (vals, vecs)
}

/// Classify `uᵀA u + 2 b·u + c = 0`, `u = p − p0`. `scale` is a typical
/// length of the view (sets what counts as zero).
pub fn classify(a: [[f64; 3]; 3], b: DVec3, c: f64, p0: DVec3, scale: f64) -> Quadric {
    let raw = (a, b, c, p0);
    let l = scale.max(1e-9);
    // Normalise so the largest term over the view is about 1.
    let amax = a.iter().flatten().fold(0f64, |m, v| m.max(v.abs()));
    let norm = (amax * l * l).max(b.abs().max_element() * l).max(c.abs()).max(1e-300);
    let a = a.map(|r| r.map(|v| v / norm));
    let b = b / norm;
    let c = c / norm;
    let tol = 1e-9;
    let (lam, q) = eigen_sym(a);
    let bp = [0, 1, 2].map(|i| q[i].dot(b));
    let nonzero: Vec<usize> = (0..3).filter(|&i| lam[i].abs() * l * l > tol).collect();
    let mut origin = p0;
    let mut cp = c;
    for &i in &nonzero {
        origin += q[i] * (-bp[i] / lam[i]);
        cp -= bp[i] * bp[i] / lam[i];
    }
    let g: DVec3 = (0..3).filter(|i| !nonzero.contains(i)).map(|i| q[i] * bp[i]).sum();
    let beta = g.length();
    let mut terms: Vec<Term> = nonzero.iter().map(|&i| Term { k: lam[i], axis: q[i] }).collect();
    let pos = |t: &[Term]| t.iter().filter(|t| t.k > 0.0).count();
    let r = terms.len();
    let (class, rhs);
    if beta * l > tol {
        // Σ λ v² + 2β w + c' = 0 → shift w; then Σ (−λ / 2β) v² = w.
        let w = g / beta;
        origin -= w * (cp / (2.0 * beta));
        for t in &mut terms {
            t.k = -t.k / (2.0 * beta);
        }
        let mut w_axis = w;
        if pos(&terms) == 0 {
            for t in &mut terms {
                t.k = -t.k;
            }
            w_axis = -w;
        }
        rhs = Rhs::Linear(w_axis);
        class = match (r, pos(&terms)) {
            (2, 2) => Class::EllipticParaboloid,
            (2, _) => Class::HyperbolicParaboloid,
            (1, _) => Class::ParabolicCylinder,
            // Linear: not a quadric; the caller handles planes.
            _ => Class::DoublePlane,
        };
    } else if cp.abs() > tol {
        // Σ λ v² = −c' → Σ (−λ / c') v² = 1.
        for t in &mut terms {
            t.k = -t.k / cp;
        }
        rhs = Rhs::One;
        class = match (r, pos(&terms)) {
            (3, 3) => Class::Ellipsoid,
            (3, 2) => Class::Hyperboloid1,
            (3, 1) => Class::Hyperboloid2,
            (2, 2) => Class::EllipticCylinder,
            (2, 1) => Class::HyperbolicCylinder,
            (1, 1) => Class::ParallelPlanes,
            _ => Class::Empty,
        };
    } else {
        // Σ λ v² = 0: keep the majority sign positive, scale the last
        // negative (or the smallest) coefficient to 1.
        if pos(&terms) * 2 < r {
            for t in &mut terms {
                t.k = -t.k;
            }
        }
        rhs = Rhs::Zero;
        class = match (r, pos(&terms)) {
            (3, 3) => Class::Point,
            (3, _) => Class::Cone,
            (2, 2) => Class::Line,
            (2, _) => Class::IntersectingPlanes,
            _ => Class::DoublePlane,
        };
        let s = terms.iter().filter(|t| t.k < 0.0).map(|t| t.k.abs()).next().or_else(|| terms.iter().map(|t| t.k.abs()).reduce(f64::min)).unwrap_or(1.0);
        for t in &mut terms {
            t.k /= s;
        }
    }
    // Positive terms first; then by coordinate axis when the axes are the
    // coordinate axes, else larger semi-axes first.
    let key = |t: &Term| aligned_index(t.axis).map_or(-t.k.abs(), |i| i as f64);
    terms.sort_by(|a, b| (b.k > 0.0).cmp(&(a.k > 0.0)).then(key(a).total_cmp(&key(b))));
    let same = |x: f64, y: f64| (x - y).abs() <= 1e-7 * x.abs().max(y.abs());
    let mut revolution = None;
    let mut sphere = false;
    if r >= 2 {
        let n = terms.len();
        let pairs: Vec<(usize, usize)> = (0..n).flat_map(|i| (i + 1..n).map(move |j| (i, j))).collect();
        let eq: Vec<(usize, usize)> = pairs.iter().copied().filter(|&(i, j)| same(terms[i].k, terms[j].k)).collect();
        if n == 3 && eq.len() == 3 {
            sphere = class == Class::Ellipsoid;
        } else if let Some(&(i, j)) = eq.first() {
            // The axis of revolution is the remaining direction.
            let axis = match rhs {
                Rhs::Linear(w) if n == 2 => Some(w),
                _ if n == 3 => (0..3).find(|&k| k != i && k != j).map(|k| terms[k].axis),
                // Cylinders: along the null direction.
                _ => Some(terms[i].axis.cross(terms[j].axis).normalize()),
            };
            revolution = axis;
        }
    }
    Quadric { class, origin, terms, rhs, revolution, sphere, raw }
}

impl Quadric {
    /// Textbook name of the surface.
    pub fn name(&self) -> &'static str {
        let rev = self.revolution.is_some();
        match self.class {
            Class::Ellipsoid if self.sphere => tr("sphere", "球面"),
            Class::Ellipsoid if rev => tr("spheroid (ellipsoid of revolution)", "旋转椭球面"),
            Class::Ellipsoid => tr("ellipsoid", "椭球面"),
            Class::Hyperboloid1 if rev => tr("hyperboloid of one sheet (of revolution)", "旋转单叶双曲面"),
            Class::Hyperboloid1 => tr("hyperboloid of one sheet", "单叶双曲面"),
            Class::Hyperboloid2 if rev => tr("hyperboloid of two sheets (of revolution)", "旋转双叶双曲面"),
            Class::Hyperboloid2 => tr("hyperboloid of two sheets", "双叶双曲面"),
            Class::Cone if rev => tr("circular cone", "圆锥面"),
            Class::Cone => tr("elliptic cone", "椭圆锥面"),
            Class::EllipticParaboloid if rev => tr("paraboloid of revolution", "旋转抛物面"),
            Class::EllipticParaboloid => tr("elliptic paraboloid", "椭圆抛物面"),
            Class::HyperbolicParaboloid => tr("hyperbolic paraboloid (saddle)", "双曲抛物面（马鞍面）"),
            Class::EllipticCylinder if rev => tr("circular cylinder", "圆柱面"),
            Class::EllipticCylinder => tr("elliptic cylinder", "椭圆柱面"),
            Class::HyperbolicCylinder => tr("hyperbolic cylinder", "双曲柱面"),
            Class::ParabolicCylinder => tr("parabolic cylinder", "抛物柱面"),
            Class::IntersectingPlanes => tr("pair of intersecting planes", "一对相交平面"),
            Class::ParallelPlanes => tr("pair of parallel planes", "一对平行平面"),
            Class::DoublePlane => tr("a plane (counted twice)", "一个平面（二重）"),
            Class::Line => tr("a line (degenerate)", "一条直线（退化）"),
            Class::Point => tr("a single point (degenerate)", "一个点（退化）"),
            Class::Empty => tr("empty: no real points", "空集：没有实点"),
        }
    }

    /// Whether the principal axes are the coordinate axes (so the standard
    /// form can use x, y, z).
    pub fn axis_aligned(&self) -> bool {
        let dirs = self.terms.iter().map(|t| t.axis).chain(match self.rhs {
            Rhs::Linear(w) => Some(w),
            _ => None,
        });
        dirs.into_iter().all(|d| aligned_index(d).is_some())
    }

    /// Semi-axes `a = 1/√|k|` (central quadrics and cylinders, rhs 1).
    pub fn semi_axes(&self) -> Vec<f64> {
        self.terms.iter().map(|t| 1.0 / t.k.abs().sqrt()).collect()
    }

    /// The standard form as text, e.g. `x²/4 + y²/9 − (z − 1)² = 1`.
    pub fn standard_form(&self) -> String {
        let aligned = self.axis_aligned();
        let names = ["x", "y", "z"];
        let primes = ["x′", "y′", "z′"];
        // Coordinate label of a direction: `x`, `(x − 1)`, or `x′`.
        let mut used = 0usize;
        let mut label = |d: DVec3| -> (String, bool) {
            if aligned {
                let i = aligned_index(d).unwrap();
                let shift = self.origin[i];
                let neg = d[i] < 0.0;
                let base = if shift.abs() < 1e-9 {
                    names[i].to_string()
                } else {
                    format!("({} {} {})", names[i], if shift > 0.0 { "−" } else { "+" }, crate::app::fmt_short(shift.abs()))
                };
                (base, neg)
            } else {
                let s = primes[used.min(2)].to_string();
                used += 1;
                (s, false)
            }
        };
        let mut lhs = String::new();
        let mut rhs_neg: Vec<String> = Vec::new();
        let zero_rhs = matches!(self.rhs, Rhs::Zero);
        let sphere_form = self.sphere && matches!(self.rhs, Rhs::One);
        for (n, t) in self.terms.iter().enumerate() {
            let (v, _) = label(t.axis);
            let sq = format!("{v}²");
            // A sphere reads (x − a)² + … = r².
            let term = if sphere_form { sq } else { coef_term(&sq, t.k.abs()) };
            // A cone keeps the negative term on the right: x² + y² = z².
            if zero_rhs && t.k < 0.0 {
                rhs_neg.push(term);
                continue;
            }
            if n == 0 || lhs.is_empty() {
                if t.k < 0.0 {
                    lhs.push('−');
                }
            } else {
                lhs.push_str(if t.k < 0.0 { " − " } else { " + " });
            }
            lhs.push_str(&term);
        }
        let rhs = match self.rhs {
            Rhs::One if sphere_form => crate::app::fmt_short(1.0 / self.terms[0].k),
            Rhs::One => "1".to_string(),
            Rhs::Zero if rhs_neg.is_empty() => "0".to_string(),
            Rhs::Zero => rhs_neg.join(" + "),
            Rhs::Linear(w) => {
                let (v, neg) = label(w);
                if neg {
                    format!("−{v}")
                } else {
                    // A lone right-hand side needs no brackets: `= z − 1`.
                    v.strip_prefix('(').and_then(|v| v.strip_suffix(')')).unwrap_or(&v).to_string()
                }
            }
        };
        format!("{lhs} = {rhs}")
    }
}

/// Index of the coordinate axis `d` is parallel to.
fn aligned_index(d: DVec3) -> Option<usize> {
    (0..3).find(|&i| d[i].abs() > 1.0 - 1e-9)
}

/// `v²/a²` or `k v²` in whichever reads better.
fn coef_term(sq: &str, k: f64) -> String {
    use crate::app::fmt_short;
    let inv = 1.0 / k;
    let nice = |v: f64| (v * 100.0 - (v * 100.0).round()).abs() < 1e-6 * v.abs().max(1.0) * 100.0;
    if (k - 1.0).abs() < 1e-9 {
        sq.to_string()
    } else if inv > 1.0 && nice(inv) {
        format!("{sq}/{}", fmt_short(inv))
    } else if nice(k) {
        format!("{}{sq}", fmt_short(k))
    } else {
        format!("{sq}/{}", fmt_short(inv))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(a: f64, b: f64, c: f64) -> [[f64; 3]; 3] {
        [[a, 0.0, 0.0], [0.0, b, 0.0], [0.0, 0.0, c]]
    }

    fn q(a: [[f64; 3]; 3], b: [f64; 3], c: f64) -> Quadric {
        classify(a, DVec3::from(b), c, DVec3::ZERO, 3.0)
    }

    #[test]
    fn eigen_recovers_rotation() {
        // Rotate diag(1, 2, 3) by 30° about z.
        let (s, c) = (0.5f64, 0.75f64.sqrt());
        let r = [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]];
        let d = [1.0, 2.0, 3.0];
        let mut m = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] = (0..3).map(|k| r[i][k] * d[k] * r[j][k]).sum();
            }
        }
        let (vals, vecs) = eigen_sym(m);
        let mut sorted = vals;
        sorted.sort_by(f64::total_cmp);
        for (a, b) in sorted.iter().zip(d) {
            assert!((a - b).abs() < 1e-12);
        }
        for (k, v) in vecs.iter().enumerate() {
            let mv = DVec3::new((0..3).map(|j| m[0][j] * v[j]).sum(), (0..3).map(|j| m[1][j] * v[j]).sum(), (0..3).map(|j| m[2][j] * v[j]).sum());
            assert!((mv - *v * vals[k]).length() < 1e-12);
        }
    }

    #[test]
    fn names_of_the_standard_quadrics() {
        let cases: &[([[f64; 3]; 3], [f64; 3], f64, Class)] = &[
            (diag(1.0, 1.0, 1.0), [0.0; 3], -4.0, Class::Ellipsoid),
            (diag(1.0, 0.25, 4.0), [0.0; 3], -1.0, Class::Ellipsoid),
            (diag(1.0, 1.0, -1.0), [0.0; 3], -1.0, Class::Hyperboloid1),
            (diag(1.0, -1.0, -1.0), [0.0; 3], -1.0, Class::Hyperboloid2),
            (diag(1.0, 1.0, -1.0), [0.0; 3], 0.0, Class::Cone),
            (diag(1.0, 1.0, 0.0), [0.0, 0.0, -0.5], 0.0, Class::EllipticParaboloid),
            (diag(1.0, -1.0, 0.0), [0.0, 0.0, -0.5], 0.0, Class::HyperbolicParaboloid),
            (diag(1.0, 1.0, 0.0), [0.0; 3], -1.0, Class::EllipticCylinder),
            (diag(1.0, -1.0, 0.0), [0.0; 3], -1.0, Class::HyperbolicCylinder),
            (diag(1.0, 0.0, 0.0), [0.0, -0.5, 0.0], 0.0, Class::ParabolicCylinder),
            (diag(1.0, -1.0, 0.0), [0.0; 3], 0.0, Class::IntersectingPlanes),
            (diag(1.0, 0.0, 0.0), [0.0; 3], -1.0, Class::ParallelPlanes),
            (diag(1.0, 1.0, 1.0), [0.0; 3], 1.0, Class::Empty),
            (diag(1.0, 1.0, 1.0), [0.0; 3], 0.0, Class::Point),
            (diag(1.0, 1.0, 0.0), [0.0; 3], 0.0, Class::Line),
        ];
        for (a, b, c, want) in cases {
            assert_eq!(q(*a, *b, *c).class, *want, "{a:?} {b:?} {c}");
        }
    }

    #[test]
    fn sphere_centre_and_standard_form() {
        // x² + y² + z² − 2x + 4z − 4 = 0 → (x − 1)² + y² + (z + 2)² = 9
        let s = q(diag(1.0, 1.0, 1.0), [-1.0, 0.0, 2.0], -4.0);
        assert!(s.sphere);
        assert!((s.origin - DVec3::new(1.0, 0.0, -2.0)).length() < 1e-12);
        assert!((s.semi_axes()[0] - 3.0).abs() < 1e-12);
        assert_eq!(s.standard_form(), "(x − 1)² + y² + (z + 2)² = 9");
    }

    #[test]
    fn standard_forms_read_like_the_textbook() {
        assert_eq!(q(diag(1.0, 1.0, -1.0), [0.0; 3], 0.0).standard_form(), "x² + y² = z²");
        assert_eq!(q(diag(1.0, 1.0, 0.0), [0.0, 0.0, -0.5], 0.0).standard_form(), "x² + y² = z");
        // z = 1 − x² − y² opens downwards.
        let p = q(diag(1.0, 1.0, 0.0), [0.0, 0.0, 0.5], -1.0);
        assert_eq!(p.class, Class::EllipticParaboloid);
        assert_eq!(p.standard_form(), "x² + y² = −(z − 1)");
        let h = q(diag(0.25, 1.0 / 9.0, -1.0), [0.0; 3], -1.0);
        assert_eq!(h.standard_form(), "x²/4 + y²/9 − z² = 1");
        assert!(q(diag(1.0, 1.0, -1.0), [0.0; 3], -1.0).revolution.is_some_and(|a| a.z.abs() > 0.999));
    }

    #[test]
    fn rotated_saddle() {
        // z = xy is a saddle rotated by 45°.
        let a = [[0.0, 0.5, 0.0], [0.5, 0.0, 0.0], [0.0, 0.0, 0.0]];
        let s = q(a, [0.0, 0.0, -0.5], 0.0);
        assert_eq!(s.class, Class::HyperbolicParaboloid);
        assert!(!s.axis_aligned());
        assert!(s.standard_form().contains('′'));
    }
}
