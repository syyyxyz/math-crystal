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
    pub show_contours: bool,
    pub show_field: bool,
    pub show_direction: bool,
    /// Direction of u for the directional derivative, degrees from +x.
    pub dir_angle: f64,
    pub show_criticals: bool,
    pub show_region: bool,
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
            show_contours: false,
            show_field: false,
            show_direction: false,
            dir_angle: 45.0,
            show_criticals: false,
            show_region: false,
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
                background: hex(0xfcfcfe, 1.0),
                wire: [0.0, 0.0, 0.0],
                frame: hex(0x7a8194, 1.0),
                grid: hex(0xdfe2ea, 1.0),
                text: [0x1d, 0x20, 0x26],
                muted_text: [0x5b, 0x61, 0x70],
            },
            Theme::Dark => Palette {
                background: hex(0x161820, 1.0),
                wire: [0.0, 0.0, 0.0],
                frame: hex(0x9aa0ad, 1.0),
                grid: hex(0x2c303c, 1.0),
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
pub const C_DIR: u32 = 0x7048e8;
pub const C_REGION: u32 = 0xe8590c;
pub const C_MIN: u32 = 0x1c7ed6;
pub const C_MAX: u32 = 0xe03131;
pub const C_SADDLE: u32 = 0xf59f00;
pub const C_DEGENERATE: u32 = 0x868e96;

pub fn critical_color(k: crate::analysis::CriticalKind) -> u32 {
    use crate::analysis::CriticalKind::*;
    match k {
        Minimum => C_MIN,
        Maximum => C_MAX,
        Saddle => C_SADDLE,
        Degenerate => C_DEGENERATE,
    }
}

/// Contour levels for the current z range.
pub fn contour_levels(s: &Settings) -> Vec<f64> {
    crate::analysis::levels(s.zr.0, s.zr.1, 12)
}

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

/// Critical-point markers and the integration-region outline, drawn on top
/// of a built scene. `region` is a row-major indicator grid.
pub fn analysis_layers(
    surf: &Surface,
    s: &Settings,
    frame: &Frame3,
    criticals: &[crate::analysis::Critical],
    region: Option<(&[f64], usize)>,
    scene: &mut Scene,
) {
    if s.show_criticals {
        for c in criticals {
            let p = frame.map_xyz(c.x, c.y, c.f);
            if p.abs().cmpgt(frame.half * 1.001).any() {
                continue;
            }
            scene.meshes.push(sphere(p, 0.04, hex(critical_color(c.kind), 1.0)));
            // Drop line to the floor.
            let floor = Vec3::new(p.x, p.y, -frame.half.z);
            scene.lines.push(line(p, floor, hex(critical_color(c.kind), 0.6), 1.2, false));
        }
    }
    if s.show_region
        && let Some((ind, n)) = region
    {
        let floor = -frame.half.z + 0.003;
        let col = hex(C_REGION, 1.0);
        for seg in crate::analysis::contour(ind, n, s.xr, s.yr, 0.0) {
            let mut a = frame.map_xyz(seg[0].0, seg[0].1, s.zr.0);
            let mut b = frame.map_xyz(seg[1].0, seg[1].1, s.zr.0);
            a.z = floor;
            b.z = floor;
            scene.lines.push(line(a, b, col, 2.4, true));
            // Same boundary lifted onto the surface.
            let fa = surf.f(seg[0].0, seg[0].1);
            let fb = surf.f(seg[1].0, seg[1].1);
            if fa.is_finite() && fb.is_finite() {
                let a = frame.map_xyz(seg[0].0, seg[0].1, fa);
                let b = frame.map_xyz(seg[1].0, seg[1].1, fb);
                scene.lines.push(line(a, b, col, 2.4, true));
            }
        }
    }
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
    if s.show_direction && d.first_ok() {
        let (c, sn) = (s.dir_angle.to_radians().cos(), s.dir_angle.to_radians().sin());
        let du = d.fx.v * c + d.fy.v * sn;
        let span = (s.xr.1 - s.xr.0).min(s.yr.1 - s.yr.0) * 0.3;
        let a = frame.map_xyz(s.a - c * span, s.b - sn * span, d.f - du * span);
        let b = frame.map_xyz(s.a + c * span, s.b + sn * span, d.f + du * span);
        scene.lines.push(line(a, b, hex(C_DIR, 1.0), width + 1.0, true));
        // Its shadow on the floor shows the direction itself.
        let fa = frame.map_xyz(s.a, s.b, s.zr.0);
        let fb = frame.map_xyz(s.a + c * span, s.b + sn * span, s.zr.0);
        scene.lines.push(line(fa, fb, hex(C_DIR, 0.7), width, true));
    }
    if s.show_contours {
        let levels = contour_levels(s);
        let (lo, hi) = (s.zr.0, s.zr.1);
        let floor = frame.half.z * -1.0 + 0.002;
        for &lv in &levels {
            let t = ((lv - lo) / (hi - lo)) as f32;
            let col = crate::render::scene::lin(crate::render::scene::viridis(0.08 + 0.84 * t), 1.0);
            let on_surface = [0.05, 0.05, 0.08, 0.75];
            for seg in crate::analysis::contour(&z, n, s.xr, s.yr, lv) {
                let a = frame.map_xyz(seg[0].0, seg[0].1, lv);
                let b = frame.map_xyz(seg[1].0, seg[1].1, lv);
                scene.lines.push(line(a, b, on_surface, 1.6, true));
                let mut fa = a;
                let mut fb = b;
                fa.z = floor;
                fb.z = floor;
                scene.lines.push(line(fa, fb, col, 1.8, true));
            }
        }
    }
    if s.show_field {
        // Normalised gradient arrows on the floor, length ~ relative size.
        let m = 15;
        let mut xs = Vec::with_capacity(m * m);
        let mut ys = Vec::with_capacity(m * m);
        for j in 0..m {
            for i in 0..m {
                xs.push(crate::calc::lerp(s.xr, (i as f64 + 0.5) / m as f64));
                ys.push(crate::calc::lerp(s.yr, (j as f64 + 0.5) / m as f64));
            }
        }
        let all = surf.eval_all(&xs, &ys);
        let sc = frame.scale();
        let g: Vec<Vec2> = (0..m * m).map(|k| Vec2::new(all[1][k] as f32 * sc.x, all[2][k] as f32 * sc.y)).collect();
        let gmax = g.iter().map(|v| v.length()).filter(|v| v.is_finite()).fold(0.0f32, f32::max);
        let cell = (frame.half.x.min(frame.half.y) * 2.0) / m as f32;
        let floor = -frame.half.z + 0.004;
        // Neutral ink so the arrows read apart from the coloured contours.
        let col = if s.theme == Theme::Dark { hex(0xd5d9e2, 0.9) } else { hex(0x2f3542, 0.85) };
        for k in 0..m * m {
            let v = g[k];
            if !v.is_finite() || gmax <= 0.0 {
                continue;
            }
            let len = v.length();
            if len < 1e-9 * gmax {
                continue;
            }
            let dir = v / len;
            let l = cell * (0.25 + 0.6 * (len / gmax).sqrt());
            let c = frame.map_xyz(xs[k], ys[k], s.zr.0);
            let base = Vec3::new(c.x, c.y, floor) - Vec3::new(dir.x, dir.y, 0.0) * l * 0.5;
            let tip = base + Vec3::new(dir.x, dir.y, 0.0) * l;
            scene.lines.push(line(base, tip, col, 1.8, true));
            let side = Vec3::new(-dir.y, dir.x, 0.0) * l * 0.22;
            let back = tip - Vec3::new(dir.x, dir.y, 0.0) * l * 0.35;
            scene.lines.push(line(tip, back + side, col, 1.8, true));
            scene.lines.push(line(tip, back - side, col, 1.8, true));
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
        // An axis seen end-on (e.g. y in the front view) gets no labels.
        let hmax = h[axis];
        let edge_px = match (proj.project(at(-hmax)), proj.project(at(hmax))) {
            (Some(a), Some(b)) => a.distance(b),
            _ => 0.0,
        };
        let axis_visible = edge_px > 40.0;
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
            if s.show_ticks && axis_visible {
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
        if !axis_visible {
            continue;
        }
        // Axis name just past the positive end of the labelled edge, where
        // it cannot collide with tick labels in any view.
        let mid = at(hmax + 0.16) + out * 0.06;
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
