//! Preset solids: ball, cylinder, cone and box, with axes along x, y or z.

use std::f64::consts::PI;

use glam::DVec3;

use crate::app::fmt_short;
use crate::i18n::tr;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SolidKind {
    Ball,
    Cylinder,
    Cone,
    Cuboid,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Solid {
    pub kind: SolidKind,
    /// Ball: centre. Cylinder and cone: centre of the base. Box: the
    /// corner with the smallest coordinates.
    pub anchor: DVec3,
    /// Axis of cylinders and cones: 0 = x, 1 = y, 2 = z.
    pub axis: usize,
    pub radius: f64,
    /// Along the axis from the base; negative points the other way.
    pub height: f64,
    /// Edge lengths of the box.
    pub size: DVec3,
}

impl Solid {
    pub fn preset(kind: SolidKind) -> Self {
        let mut s = Self { kind, anchor: DVec3::ZERO, axis: 2, radius: 1.0, height: 2.0, size: DVec3::new(2.0, 1.5, 1.0) };
        match kind {
            SolidKind::Ball => s.radius = 1.5,
            SolidKind::Cylinder => s.anchor = DVec3::new(0.0, 0.0, -1.0),
            SolidKind::Cone => s.anchor = DVec3::new(0.0, 0.0, -1.0),
            SolidKind::Cuboid => s.anchor = -s.size * 0.5,
        }
        s
    }

    pub fn name(&self) -> &'static str {
        match self.kind {
            SolidKind::Ball => tr("ball", "球体"),
            SolidKind::Cylinder => tr("cylinder", "圆柱体"),
            SolidKind::Cone => tr("cone", "圆锥体"),
            SolidKind::Cuboid => tr("box", "长方体"),
        }
    }

    /// Unit vector of the axis.
    pub fn axis_dir(&self) -> DVec3 {
        [DVec3::X, DVec3::Y, DVec3::Z][self.axis]
    }

    /// Continuous function that is negative inside, zero on the boundary
    /// and positive outside (a signed distance for the ball and the box).
    pub fn inside_fn(&self, p: DVec3) -> f64 {
        match self.kind {
            SolidKind::Ball => (p - self.anchor).length() - self.radius,
            SolidKind::Cuboid => {
                let c = self.anchor + self.size * 0.5;
                ((p - c).abs() - self.size * 0.5).max_element()
            }
            SolidKind::Cylinder | SolidKind::Cone => {
                let a = self.axis_dir();
                let d = p - self.anchor;
                let h = self.height.abs().max(1e-12);
                let s = d.dot(a) * self.height.signum();
                let rho = (d - a * d.dot(a)).length();
                let r = if self.kind == SolidKind::Cone {
                    // Radius shrinks to 0 at the apex; scale by cos of the
                    // half-angle so this is a distance near the side.
                    let k = self.radius / h;
                    (rho - self.radius * (1.0 - s / h)) / (1.0 + k * k).sqrt()
                } else {
                    rho - self.radius
                };
                r.max(-s).max(s - h)
            }
        }
    }

    pub fn volume(&self) -> f64 {
        let (r, h) = (self.radius, self.height.abs());
        match self.kind {
            SolidKind::Ball => 4.0 / 3.0 * PI * r.powi(3),
            SolidKind::Cylinder => PI * r * r * h,
            SolidKind::Cone => PI * r * r * h / 3.0,
            SolidKind::Cuboid => self.size.x * self.size.y * self.size.z,
        }
    }

    pub fn area(&self) -> f64 {
        let (r, h) = (self.radius, self.height.abs());
        match self.kind {
            SolidKind::Ball => 4.0 * PI * r * r,
            SolidKind::Cylinder => 2.0 * PI * r * (r + h),
            SolidKind::Cone => PI * r * (r + (r * r + h * h).sqrt()),
            SolidKind::Cuboid => {
                let s = self.size;
                2.0 * (s.x * s.y + s.y * s.z + s.x * s.z)
            }
        }
    }

    /// The solid as inequalities, e.g. `x² + y² ≤ 1, −1 ≤ z ≤ 1`.
    pub fn inequalities(&self) -> String {
        let names = ["x", "y", "z"];
        let sq = |i: usize, c: f64| shifted(names[i], c, true);
        match self.kind {
            SolidKind::Ball => {
                let c = self.anchor;
                format!("{} + {} + {} ≤ {}", sq(0, c.x), sq(1, c.y), sq(2, c.z), fmt_short(self.radius * self.radius))
            }
            SolidKind::Cuboid => (0..3)
                .map(|i| format!("{} ≤ {} ≤ {}", fmt_short(self.anchor[i]), names[i], fmt_short(self.anchor[i] + self.size[i])))
                .collect::<Vec<_>>()
                .join(",  "),
            SolidKind::Cylinder | SolidKind::Cone => {
                let i = self.axis;
                let (j, k) = ((i + 1) % 3, (i + 2) % 3);
                let (j, k) = (j.min(k), j.max(k));
                let disc = format!("{} + {}", sq(j, self.anchor[j]), sq(k, self.anchor[k]));
                let (a, b) = (self.anchor[i], self.anchor[i] + self.height);
                let range = format!("{} ≤ {} ≤ {}", fmt_short(a.min(b)), names[i], fmt_short(a.max(b)));
                if self.kind == SolidKind::Cylinder {
                    format!("{disc} ≤ {},  {range}", fmt_short(self.radius * self.radius))
                } else {
                    // Radius at the coordinate: r/h · (apex − coordinate).
                    let apex = b;
                    let k = self.radius / self.height.abs().max(1e-12);
                    let lin = shifted(names[i], apex, false);
                    let dist = if self.height > 0.0 { neg_of(&lin) } else { lin };
                    let kk = k * k;
                    let coef = if (kk - 1.0).abs() < 1e-9 { String::new() } else { fmt_short(kk) };
                    format!("{disc} ≤ {coef}{dist}²,  {range}")
                }
            }
        }
    }

    /// Short description of size and position.
    pub fn summary(&self) -> String {
        let p = |v: DVec3| format!("({}, {}, {})", fmt_short(v.x), fmt_short(v.y), fmt_short(v.z));
        match self.kind {
            SolidKind::Ball => format!("{} {}, r = {}", tr("centre", "球心"), p(self.anchor), fmt_short(self.radius)),
            SolidKind::Cylinder | SolidKind::Cone => {
                format!("{} {}, r = {}, h = {}", tr("base centre", "底面中心"), p(self.anchor), fmt_short(self.radius), fmt_short(self.height))
            }
            SolidKind::Cuboid => format!("{} {} … {}", tr("from", "从"), p(self.anchor), p(self.anchor + self.size)),
        }
    }
}

/// `(x − c)` / `x` (squared when `square`).
fn shifted(name: &str, c: f64, square: bool) -> String {
    let base = if c.abs() < 1e-12 {
        name.to_string()
    } else {
        format!("({name} {} {})", if c > 0.0 { "−" } else { "+" }, fmt_short(c.abs()))
    };
    if square { format!("{base}²") } else { base }
}

/// `(c − x)` from `(x − c)`, used for the cone's radius.
fn neg_of(lin: &str) -> String {
    match lin.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(inner) => {
            // inner is "x − c" or "x + c".
            let (name, rest) = inner.split_once(' ').unwrap_or((inner, ""));
            let rest = rest.trim();
            if let Some(c) = rest.strip_prefix("− ") {
                format!("({c} − {name})")
            } else if let Some(c) = rest.strip_prefix("+ ") {
                format!("(−{c} − {name})")
            } else {
                format!("(−{name})")
            }
        }
        None => format!("(−{lin})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_function_signs() {
        for kind in [SolidKind::Ball, SolidKind::Cylinder, SolidKind::Cone, SolidKind::Cuboid] {
            let s = Solid::preset(kind);
            let centre = match kind {
                SolidKind::Cuboid => s.anchor + s.size * 0.5,
                SolidKind::Ball => s.anchor,
                _ => s.anchor + s.axis_dir() * s.height * 0.3,
            };
            assert!(s.inside_fn(centre) < 0.0, "{kind:?}");
            assert!(s.inside_fn(DVec3::splat(5.0)) > 0.0, "{kind:?}");
        }
        let c = Solid::preset(SolidKind::Cone);
        // On the side halfway up: radius 0.5.
        assert!(c.inside_fn(DVec3::new(0.5, 0.0, 0.0)).abs() < 1e-12);
    }

    #[test]
    fn descriptions() {
        assert_eq!(Solid::preset(SolidKind::Ball).inequalities(), "x² + y² + z² ≤ 2.25");
        assert_eq!(Solid::preset(SolidKind::Cylinder).inequalities(), "x² + y² ≤ 1,  −1 ≤ z ≤ 1");
        let mut c = Solid::preset(SolidKind::Cone);
        c.radius = 2.0;
        assert_eq!(c.inequalities(), "x² + y² ≤ (1 − z)²,  −1 ≤ z ≤ 1");
        assert!((Solid::preset(SolidKind::Cuboid).volume() - 3.0).abs() < 1e-12);
    }
}
