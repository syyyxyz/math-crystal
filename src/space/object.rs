//! One object of the Space workspace: a surface, a line / curve or a
//! preset solid, with its appearance and a cached mesh.

use glam::DVec3;

use super::curve::{Curve, clip_line};
use super::mesh::{self, DMesh};
use super::solid::{Solid, SolidKind};
use super::surface::{Implicit, SurfaceKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Surface,
    Curve,
    Solid,
}

/// sRGB colours offered for objects.
pub const COLORS: [u32; 6] = [0x4c6ef5, 0xf08c00, 0x2f9e44, 0xae3ec9, 0xe03131, 0x1098ad];

pub const DEFAULT_SURFACE: &str = "x^2 + y^2 + z^2 = 4";
pub const DEFAULT_CURVE: &str = "(x - 1)/2 = (y + 1)/3 = z/1";

pub struct Object {
    /// Stable id for UI state (card open / closed).
    pub uid: u32,
    pub kind: Kind,
    pub src: String,
    pub solid: Solid,
    pub surface: Option<Implicit>,
    pub curve: Option<Curve>,
    pub error: Option<String>,
    pub color: usize,
    pub opacity: f32,
    pub visible: bool,
    /// Grid lines on planes, level lines z = const on other surfaces.
    pub lines: bool,
    /// Parameter range of curves that are not lines.
    pub t_range: (f64, f64),
    mesh: DMesh,
    mesh_key: String,
}

impl Object {
    pub fn new(kind: Kind, src: &str, color: usize) -> Self {
        let mut o = Self {
            uid: 0,
            kind,
            src: src.to_string(),
            solid: Solid::preset(SolidKind::Ball),
            surface: None,
            curve: None,
            error: None,
            color,
            opacity: 0.6,
            visible: true,
            lines: kind == Kind::Surface,
            t_range: (-std::f64::consts::TAU, std::f64::consts::TAU),
            mesh: DMesh::default(),
            mesh_key: String::new(),
        };
        o.recompile();
        o
    }

    pub fn solid(solid: Solid, color: usize) -> Self {
        let mut o = Self::new(Kind::Solid, "", color);
        o.solid = solid;
        o.opacity = 0.75;
        o
    }

    /// Parse `src` again (after editing or a language switch). The last
    /// valid surface or curve is kept when the new text has an error.
    pub fn recompile(&mut self) {
        self.error = None;
        match self.kind {
            Kind::Surface => match Implicit::new(&self.src) {
                Ok(s) => {
                    self.surface = Some(s);
                    self.curve = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            },
            Kind::Curve => match Curve::new(&self.src) {
                Ok(c) => {
                    self.curve = Some(c);
                    self.surface = None;
                }
                Err(e) => self.error = Some(e.to_string()),
            },
            Kind::Solid => {
                self.surface = None;
                self.curve = None;
            }
        }
        self.mesh_key.clear();
    }

    /// Switch kind, starting from a sensible default text.
    pub fn set_kind(&mut self, kind: Kind) {
        if kind == self.kind {
            return;
        }
        self.kind = kind;
        match kind {
            Kind::Surface => {
                self.src = DEFAULT_SURFACE.into();
                self.opacity = 0.6;
            }
            Kind::Curve => self.src = DEFAULT_CURVE.into(),
            Kind::Solid => self.opacity = 0.75,
        }
        self.surface = None;
        self.curve = None;
        self.recompile();
    }

    pub fn params(&self) -> Vec<String> {
        match self.kind {
            Kind::Surface => self.surface.as_ref().map(|s| s.params.clone()),
            Kind::Curve => self.curve.as_ref().map(|c| c.params.clone()),
            Kind::Solid => None,
        }
        .unwrap_or_default()
    }

    pub fn set_params(&mut self, named: &[(String, f64)]) {
        if let Some(s) = &mut self.surface {
            s.set_params(named);
        }
        if let Some(c) = &mut self.curve {
            c.set_params(named);
        }
        self.mesh_key.clear();
    }

    /// Whether this is a plane (drawn as an exact polygon).
    pub fn plane(&self) -> Option<(DVec3, f64)> {
        match &self.surface.as_ref()?.kind {
            SurfaceKind::Plane { n, d } => Some((*n, *d)),
            _ => None,
        }
    }

    /// Mesh for the current definition inside the box (`n` cells along its
    /// longest side for implicit surfaces), rebuilt only when needed.
    pub fn mesh(&mut self, lo: DVec3, hi: DVec3, n: usize, params_key: &str) -> &DMesh {
        let key = format!("{:?}|{}|{:?}|{lo}|{hi}|{n}|{params_key}", self.kind, self.src, self.solid);
        if key != self.mesh_key {
            if let Some(s) = &mut self.surface {
                s.classify(lo, hi);
            }
            self.mesh = match self.kind {
                Kind::Surface => match &self.surface {
                    Some(s) => match s.kind {
                        SurfaceKind::Plane { n: nrm, d } => mesh::plane_mesh(nrm, d, lo, hi, (hi - lo).max_element() / n as f64),
                        _ => mesh::surface_nets(s, lo, hi, n),
                    },
                    None => DMesh::default(),
                },
                Kind::Solid => mesh::solid_mesh(&self.solid),
                Kind::Curve => DMesh::default(),
            };
            self.mesh_key = key;
        }
        &self.mesh
    }

    /// The mesh built by the last call to [`Object::mesh`].
    pub fn cached_mesh(&self) -> &DMesh {
        &self.mesh
    }

    /// The function that vanishes on the surface (surfaces and solids).
    pub fn field(&self) -> Option<super::intersect::Field<'_>> {
        match self.kind {
            Kind::Surface => self.surface.as_ref().map(super::intersect::Field::Imp),
            Kind::Solid => Some(super::intersect::Field::Solid(&self.solid)),
            Kind::Curve => None,
        }
    }

    /// The line, when this is a valid straight line.
    pub fn line(&self) -> Option<(DVec3, DVec3)> {
        self.curve.as_ref().filter(|c| c.error.is_none()).and_then(|c| c.line)
    }

    /// Polyline of the curve inside the box (None entries break it).
    pub fn curve_points(&self, lo: DVec3, hi: DVec3) -> Vec<Option<DVec3>> {
        let Some(c) = &self.curve else { return vec![] };
        if c.error.is_some() {
            return vec![];
        }
        if let Some((p, d)) = c.line {
            return match clip_line(p, d, lo, hi) {
                Some((t0, t1)) => vec![Some(p + d * t0), Some(p + d * t1)],
                None => vec![],
            };
        }
        c.samples(self.t_range, 1500)
    }
}
