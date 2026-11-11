//! Calculus workspace state and scene construction, independent of egui so
//! it can be rendered headlessly.

use glam::{DVec3, Vec2, Vec3};

use crate::calc::{PointDerivs, Quadratic, Surface};
use crate::render::scene::{
    Camera, Frame3, GridStyle, LineInstance, MeshData, Projector, Scene, hex, height_mesh, line, polyline, sphere,
    tick_label, ticks,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Coloring {
    Height,
    Solid,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Light,
    Dark,
}

/// Everything that determines the scene (camera excluded).
#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub a: f64,
    pub b: f64,
    pub xr: (f64, f64),
    pub yr: (f64, f64),
    pub zr: (f64, f64),
    pub resolution: usize,
    pub equal_aspect: bool,
    pub show_surface: bool,
    pub surface_coloring: Coloring,
    pub wireframe: bool,
    pub show_model: bool,
    pub model_coloring: Coloring,
    /// Curvatures used by the model (user-adjustable).
    pub model_fxx: f64,
    pub model_fyy: f64,
    pub model_fxy: f64,
    pub show_point: bool,
    pub show_tangent_x: bool,
    pub show_tangent_y: bool,
    pub show_plane: bool,
    pub show_sections: bool,
    pub show_gradient: bool,
    pub show_grid: bool,
    pub show_ticks: bool,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            a: 0.0,
            b: 0.0,
            xr: (-2.0, 2.0),
            yr: (-2.0, 2.0),
            zr: (-2.0, 2.0),
            resolution: 129,
            equal_aspect: false,
            show_surface: true,
            surface_coloring: Coloring::Height,
            wireframe: true,
            show_model: true,
            model_coloring: Coloring::Solid,
            model_fxx: 0.0,
            model_fyy: 0.0,
            model_fxy: 0.0,
            show_point: true,
            show_tangent_x: false,
            show_tangent_y: false,
            show_plane: false,
            show_sections: false,
            show_gradient: false,
            show_grid: false,
            show_ticks: true,
            theme: Theme::Light,
        }
    }
}

pub struct Palette {
    pub background: [f32; 4],
    pub wire: [f32; 3],
    pub frame: [f32; 4],
    pub grid: [f32; 4],
    pub text: [u8; 3],
    pub muted_text: [u8; 3],
}

impl Theme {
    pub fn palette(self) -> Palette {
        match self {
            Theme::Light => Palette {
                background: hex(0xfbfbfc, 1.0),
                wire: [0.0, 0.0, 0.0],
                frame: hex(0x5b6170, 1.0),
                grid: hex(0xc9ccd4, 1.0),
                text: [0x1d, 0x20, 0x26],
                muted_text: [0x5b, 0x61, 0x70],
            },
            Theme::Dark => Palette {
                background: hex(0x17181c, 1.0),
                wire: [0.0, 0.0, 0.0],
                frame: hex(0x9aa0ad, 1.0),
                grid: hex(0x3a3d46, 1.0),
                text: [0xe6, 0xe8, 0xee],
                muted_text: [0x9a, 0xa0, 0xad],
            },
        }
    }
}

// Series colours (sRGB hex), shared with the UI legend.
pub const C_MODEL: u32 = 0xf28e2b;
pub const C_PLANE: u32 = 0x1f77e8;
pub const C_TAN_X: u32 = 0x0aa1dd;
pub const C_TAN_Y: u32 = 0xd6337f;
pub const C_POINT: u32 = 0xe03131;
pub const C_GRAD: u32 = 0x2f9e44;
pub const C_SURFACE_SOLID: u32 = 0x8fa6c8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LabelKind {
    Axis,
    Tick,
    Point,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub world: Vec3,
    pub text: String,
    pub kind: LabelKind,
    /// Screen-space direction (pixels, y down) to push the label away from
    /// its anchor.
    pub push: Vec2,
}

/// Robust automatic z range: 1st..99th percentile of finite samples, padded.
pub fn auto_z(z: &[f64]) -> Option<(f64, f64)> {
    let mut v: Vec<f64> = z.iter().copied().filter(|v| v.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    let (lo, hi) = (q(0.01), q(0.99));
    let span = hi - lo;
    if span < 1e-9 * (1.0 + lo.abs()) {
        return Some((lo - 1.0, hi + 1.0));
    }
    Some((lo - 0.08 * span, hi + 0.08 * span))
}

pub struct Built {
    pub scene: Scene,
    pub frame: Frame3,
    pub derivs: Option<PointDerivs>,
    pub z_samples: Vec<f64>,
}

/// Build the meshes and camera-independent lines.
pub fn build(surface: Option<&Surface>, s: &Settings) -> Built {
    let frame = Frame3::new(
        DVec3::new(s.xr.0, s.yr.0, s.zr.0),
        DVec3::new(s.xr.1, s.yr.1, s.zr.1),
        s.equal_aspect,
    );
    let mut scene = Scene::default();
    let Some(surf) = surface else {
        return Built {
            scene,
            frame,
            derivs: None,
            z_samples: vec![],
        };
    };
    let n = s.resolution.clamp(9, 801);
    let z = surf.sample(s.xr, s.yr, n);
    if s.show_surface {
        let style = GridStyle {
            height_colors: s.surface_coloring == Coloring::Height,
            color: hex(C_SURFACE_SOLID, if s.surface_coloring == Coloring::Solid { 0.55 } else { 1.0 }),
            wire_lines: if s.wireframe { 16.0 } else { 0.0 },
            wire_strength: if s.surface_coloring == Coloring::Height { 0.55 } else { 0.35 },
            translucent: s.surface_coloring == Coloring::Solid,
        };
        scene.meshes.push(height_mesh(&z, n, s.xr, s.yr, &frame, &style));
    }
    let d = surf.derivs(s.a, s.b);
    let width = 3.0;
    if d.first_ok() {
        let q = Quadratic {
            a: s.a,
            b: s.b,
            f: d.f,
            fx: d.fx.v,
            fy: d.fy.v,
            fxx: s.model_fxx,
            fyy: s.model_fyy,
            fxy: s.model_fxy,
        };
        let grid_of = |m: usize, g: &dyn Fn(f64, f64) -> f64| {
            let mut out = Vec::with_capacity(m * m);
            for j in 0..m {
                for i in 0..m {
                    let x = crate::calc::lerp(s.xr, i as f64 / (m - 1) as f64);
                    let y = crate::calc::lerp(s.yr, j as f64 / (m - 1) as f64);
                    out.push(g(x, y));
                }
            }
            out
        };
        if s.show_model {
            let m = 65;
            let zm = grid_of(m, &|x, y| q.eval(x, y));
            let style = GridStyle {
                height_colors: s.model_coloring == Coloring::Height,
                color: hex(C_MODEL, 0.62),
                wire_lines: 16.0,
                wire_strength: 0.25,
                translucent: true,
            };
            let mut mesh = height_mesh(&zm, m, s.xr, s.yr, &frame, &style);
            if s.model_coloring == Coloring::Height {
                for v in &mut mesh.vertices {
                    v.color[3] = 0.62;
                }
            }
            scene.meshes.push(mesh);
        }
        if s.show_plane {
            let m = 17;
            let zp = grid_of(m, &|x, y| q.linear(x, y));
            let style = GridStyle {
                height_colors: false,
                color: hex(C_PLANE, 0.38),
                wire_lines: 8.0,
                wire_strength: 0.2,
                translucent: true,
            };
            scene.meshes.push(height_mesh(&zp, m, s.xr, s.yr, &frame, &style));
        }
        let p = frame.map_xyz(s.a, s.b, d.f);
        let span_x = (s.xr.1 - s.xr.0) * 0.28;
        let span_y = (s.yr.1 - s.yr.0) * 0.28;
        if s.show_tangent_x {
            let a = frame.map_xyz(s.a - span_x, s.b, d.f - d.fx.v * span_x);
            let b = frame.map_xyz(s.a + span_x, s.b, d.f + d.fx.v * span_x);
            scene.lines.push(line(a, b, hex(C_TAN_X, 1.0), width + 1.0, true));
        }
        if s.show_tangent_y {
            let a = frame.map_xyz(s.a, s.b - span_y, d.f - d.fy.v * span_y);
            let b = frame.map_xyz(s.a, s.b + span_y, d.f + d.fy.v * span_y);
            scene.lines.push(line(a, b, hex(C_TAN_Y, 1.0), width + 1.0, true));
        }
        if s.show_gradient {
            // Steepest ascent along the tangent plane: data direction
            // (fx, fy, fx^2 + fy^2), drawn in world space at a readable length.
            let sc = frame.scale();
            let g2 = d.fx.v * d.fx.v + d.fy.v * d.fy.v;
            let dir = Vec3::new(d.fx.v as f32 * sc.x, d.fy.v as f32 * sc.y, g2 as f32 * sc.z);
            if let Some(dir) = dir.try_normalize() {
                let tip = p + dir * 0.45;
                let c = hex(C_GRAD, 1.0);
                scene.lines.push(line(p, tip, c, width + 1.0, false));
                let side = dir.cross(Vec3::Z).try_normalize().unwrap_or(Vec3::X) * 0.04;
                let back = tip - dir * 0.09;
                scene.lines.push(line(tip, back + side, c, width + 1.0, false));
                scene.lines.push(line(tip, back - side, c, width + 1.0, false));
            }
        }
        if s.show_point {
            scene.meshes.push(sphere(p, 0.05, hex(C_POINT, 1.0)));
        }
    }
    if s.show_sections && d.f.is_finite() {
        let m = 241;
        let xs: Vec<Option<Vec3>> = (0..m)
            .map(|i| {
                let x = crate::calc::lerp(s.xr, i as f64 / (m - 1) as f64);
                let v = surf.f(x, s.b);
                v.is_finite().then(|| frame.map_xyz(x, s.b, v))
            })
            .collect();
        let ys: Vec<Option<Vec3>> = (0..m)
            .map(|j| {
                let y = crate::calc::lerp(s.yr, j as f64 / (m - 1) as f64);
                let v = surf.f(s.a, y);
                v.is_finite().then(|| frame.map_xyz(s.a, y, v))
            })
            .collect();
        polyline(&mut scene.lines, &xs, hex(C_TAN_X, 1.0), width);
        polyline(&mut scene.lines, &ys, hex(C_TAN_Y, 1.0), width);
    }
    Built {
        scene,
        frame,
        derivs: Some(d),
        z_samples: z,
    }
}

/// Camera-dependent box, grid and tick lines plus text labels.
pub fn decorations(s: &Settings, frame: &Frame3, cam: &Camera, proj: &Projector) -> (Vec<LineInstance>, Vec<Label>) {
    let pal = s.theme.palette();
    let h = frame.half;
    let eye = cam.eye();
    let mut lines = Vec::new();
    let mut labels = Vec::new();
    // Back walls are the ones on the far side from the eye.
    let back = Vec3::new(-eye.x.signum() * h.x, -eye.y.signum() * h.y, -h.z);
    let front = Vec3::new(eye.x.signum() * h.x, eye.y.signum() * h.y, h.z);
    let corners = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    // Box edges: draw the 3 back faces' edges solid plus the rest lighter.
    let fc = pal.frame;
    let light = [fc[0], fc[1], fc[2], 0.35];
    for &(sx, sy) in &[(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let (x, y) = (sx * h.x, sy * h.y);
        let is_front_edge = x == front.x && y == front.y;
        let c = if is_front_edge { light } else { fc };
        lines.push(line(corners(x, y, -h.z), corners(x, y, h.z), c, 1.5, false));
    }
    for &z in &[-h.z, h.z] {
        for &(a, b) in &[((-1.0f32, -1.0f32), (1.0f32, -1.0f32)), ((1.0, -1.0), (1.0, 1.0)), ((1.0, 1.0), (-1.0, 1.0)), ((-1.0, 1.0), (-1.0, -1.0))] {
            let pa = corners(a.0 * h.x, a.1 * h.y, z);
            let pb = corners(b.0 * h.x, b.1 * h.y, z);
            let on_back = z == back.z || pa.x == back.x && pb.x == back.x || pa.y == back.y && pb.y == back.y;
            lines.push(line(pa, pb, if on_back { fc } else { light }, 1.5, false));
        }
    }

    let axes = [
        (0usize, s.xr, "x"),
        (1usize, s.yr, "y"),
        (2usize, s.zr, "z"),
    ];
    for (axis, (lo, hi), name) in axes {
        let tv = ticks(lo, hi, 5);
        let step = if tv.len() > 1 { tv[1] - tv[0] } else { 1.0 };
        // Edge carrying the tick labels: x and y on the floor's front edges,
        // z on the vertical edge left-most on screen.
        let (fixed_a, fixed_b): (f32, f32) = match axis {
            0 => (front.y, -h.z),
            1 => (front.x, -h.z),
            _ => {
                let mut best = (h.x, h.y);
                let mut best_x = f32::INFINITY;
                for &(sx, sy) in &[(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    let c = Vec3::new(sx * h.x, sy * h.y, 0.0);
                    if let Some(p) = proj.project(c)
                        && p.x < best_x
                    {
                        best_x = p.x;
                        best = (c.x, c.y);
                    }
                }
                best
            }
        };
        let at = |t: f32| -> Vec3 {
            match axis {
                0 => Vec3::new(t, fixed_a, fixed_b),
                1 => Vec3::new(fixed_a, t, fixed_b),
                _ => Vec3::new(fixed_a, fixed_b, t),
            }
        };
        let world_of = |v: f64| -> f32 {
            let p = match axis {
                0 => DVec3::new(v, s.yr.0, s.zr.0),
                1 => DVec3::new(s.xr.0, v, s.zr.0),
                _ => DVec3::new(s.xr.0, s.yr.0, v),
            };
            frame.map(p)[axis]
        };
        // Outward direction (world) for ticks on this edge.
        let out = match axis {
            0 => Vec3::new(0.0, fixed_a.signum(), 0.0),
            1 => Vec3::new(fixed_a.signum(), 0.0, 0.0),
            _ => Vec3::new(fixed_a.signum(), fixed_b.signum(), 0.0).normalize() ,
        };
        let center_px = proj.project(Vec3::ZERO).unwrap_or(Vec2::ZERO);
        for &v in &tv {
            let t = world_of(v);
            let p = at(t);
            if s.show_grid {
                // Lines across the two back walls containing this axis.
                let g = pal.grid;
                match axis {
                    0 => {
                        lines.push(line(Vec3::new(t, -h.y, back.z), Vec3::new(t, h.y, back.z), g, 1.0, false));
                        lines.push(line(Vec3::new(t, back.y, -h.z), Vec3::new(t, back.y, h.z), g, 1.0, false));
                    }
                    1 => {
                        lines.push(line(Vec3::new(-h.x, t, back.z), Vec3::new(h.x, t, back.z), g, 1.0, false));
                        lines.push(line(Vec3::new(back.x, t, -h.z), Vec3::new(back.x, t, h.z), g, 1.0, false));
                    }
                    _ => {
                        lines.push(line(Vec3::new(back.x, -h.y, t), Vec3::new(back.x, h.y, t), g, 1.0, false));
                        lines.push(line(Vec3::new(-h.x, back.y, t), Vec3::new(h.x, back.y, t), g, 1.0, false));
                    }
                }
            }
            if s.show_ticks {
                lines.push(line(p, p + out * 0.04, pal.frame, 1.5, false));
                let anchor = p + out * 0.07;
                let push = proj
                    .project(anchor)
                    .map(|a| (a - center_px).normalize_or_zero())
                    .unwrap_or(Vec2::ZERO);
                labels.push(Label {
                    world: anchor,
                    text: tick_label(v, step),
                    kind: LabelKind::Tick,
                    push,
                });
            }
        }
        // Axis name beyond the middle of the labelled edge.
        let mid = at(0.0) + out * if s.show_ticks { 0.28 } else { 0.12 };
        let push = proj.project(mid).map(|a| (a - center_px).normalize_or_zero()).unwrap_or(Vec2::ZERO);
        labels.push(Label {
            world: mid,
            text: name.to_string(),
            kind: LabelKind::Axis,
            push,
        });
    }
    (lines, labels)
}

/// Mesh list helper: sphere-only scenes etc. are built by [`build`]; this
/// is used by tests to count meshes.
pub fn mesh_count(m: &[MeshData]) -> usize {
    m.iter().filter(|m| !m.indices.is_empty()).count()
}
