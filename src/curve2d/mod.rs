//! Plane curves: graphs y = f(x) and x = g(y), implicit curves
//! F(x, y) = 0, polar curves r = f(θ) (or any equation in r and θ),
//! parametric curves (x(t), y(t)) and regions given by inequalities.
//!
//! UI-independent: parsing and recognising the form, sampling for the
//! screen, tracing implicit curves, regions, special points, intersections
//! and integrals.

pub mod def;
pub mod features;
pub mod integral;
pub mod nice;
pub mod region;
pub mod sample;
pub mod trace;

pub use def::{Def, Form};

/// A data window and its scale on screen. Screen coordinates (points)
/// run right and down from the window's top-left corner (x0, y1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame2 {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    /// Points per data unit along x and y.
    pub kx: f64,
    pub ky: f64,
}

impl Frame2 {
    pub fn sx(&self, x: f64) -> f64 {
        (x - self.x0) * self.kx
    }

    pub fn sy(&self, y: f64) -> f64 {
        (self.y1 - y) * self.ky
    }

    /// Screen distance between two data points.
    pub fn dist(&self, a: [f64; 2], b: [f64; 2]) -> f64 {
        ((b[0] - a[0]) * self.kx).hypot((b[1] - a[1]) * self.ky)
    }

    /// The same scale over a window grown by `f` of its size on every side.
    pub fn grown(&self, f: f64) -> Self {
        let (w, h) = (self.x1 - self.x0, self.y1 - self.y0);
        Self { x0: self.x0 - f * w, x1: self.x1 + f * w, y0: self.y0 - f * h, y1: self.y1 + f * h, ..*self }
    }

    /// Whether `inner` lies inside this window.
    pub fn contains(&self, inner: &Frame2) -> bool {
        inner.x0 >= self.x0 && inner.x1 <= self.x1 && inner.y0 >= self.y0 && inner.y1 <= self.y1
    }

    /// Width and height in points.
    pub fn size(&self) -> [f64; 2] {
        [(self.x1 - self.x0) * self.kx, (self.y1 - self.y0) * self.ky]
    }
}
