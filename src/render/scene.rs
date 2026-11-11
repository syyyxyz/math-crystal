//! CPU side of the 3D view: camera, data-to-world mapping, geometry
//! builders and the colour palette. Everything is in world space; the data
//! box is mapped to `[-half, half]`.

use bytemuck::{Pod, Zeroable};
use glam::{DVec3, Mat4, Vec2, Vec3, Vec4};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct MeshVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// Linear RGBA.
    pub color: [f32; 4],
    /// Grid coordinates for the wireframe overlay (xy) and its strength (z).
    pub grid: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct LineInstance {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub color: [f32; 4],
    /// Width in physical pixels, clip-to-box flag (1 = clip).
    pub params: [f32; 2],
}

#[derive(Clone, Default)]
pub struct MeshData {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub translucent: bool,
}

impl MeshData {
    pub fn centroid(&self) -> Vec3 {
        if self.vertices.is_empty() {
            return Vec3::ZERO;
        }
        let s: Vec3 = self.vertices.iter().map(|v| Vec3::from(v.pos)).sum();
        s / self.vertices.len() as f32
    }
}

#[derive(Clone, Default)]
pub struct Scene {
    pub meshes: Vec<MeshData>,
    pub lines: Vec<LineInstance>,
}

/// sRGB (0..1) to linear.
pub fn lin(c: [f32; 3], a: f32) -> [f32; 4] {
    let f = |u: f32| {
        if u <= 0.04045 {
            u / 12.92
        } else {
            ((u + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(c[0]), f(c[1]), f(c[2]), a]
}

pub fn hex(rgb: u32, a: f32) -> [f32; 4] {
    let c = |s: u32| ((rgb >> s) & 0xff) as f32 / 255.0;
    lin([c(16), c(8), c(0)], a)
}

/// Viridis colormap (polynomial fit by Matt Zucker), t in 0..1, sRGB out.
pub fn viridis(t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let c = [
        Vec3::new(0.277_727_33, 0.005_407_344_5, 0.334_099_8),
        Vec3::new(0.105_093_04, 1.404_613_5, 1.384_590_2),
        Vec3::new(-0.330_861_83, 0.214_847_56, 0.095_095_16),
        Vec3::new(-4.634_230_5, -5.799_101, -19.332_441),
        Vec3::new(6.228_27, 14.179_933, 56.690_55),
        Vec3::new(4.776_385, -13.745_146, -65.353_03),
        Vec3::new(-5.435_456, 4.645_852_6, 26.312_435),
    ];
    let v = c[0] + t * (c[1] + t * (c[2] + t * (c[3] + t * (c[4] + t * (c[5] + t * c[6])))));
    v.clamp(Vec3::ZERO, Vec3::ONE).to_array()
}

/// Maps the data box to world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame3 {
    pub lo: DVec3,
    pub hi: DVec3,
    pub half: Vec3,
}

impl Frame3 {
    /// `equal`: true 1:1:1 proportions (largest extent = 1), otherwise a cube.
    pub fn new(lo: DVec3, hi: DVec3, equal: bool) -> Self {
        let ext = (hi - lo).max(DVec3::splat(1e-12));
        let half = if equal {
            let m = ext.max_element();
            // Keep very flat or very tall boxes usable.
            (ext / m).clamp(DVec3::splat(0.05), DVec3::ONE).as_vec3()
        } else {
            Vec3::ONE
        };
        Self { lo, hi, half }
    }

    pub fn map(&self, p: DVec3) -> Vec3 {
        let t = (p - self.lo) / (self.hi - self.lo);
        let w = (t * 2.0 - 1.0).as_vec3() * self.half;
        w.clamp(Vec3::splat(-1e3), Vec3::splat(1e3))
    }

    pub fn map_xyz(&self, x: f64, y: f64, z: f64) -> Vec3 {
        self.map(DVec3::new(x, y, z))
    }

    /// World units per data unit along each axis.
    pub fn scale(&self) -> Vec3 {
        (DVec3::from(self.half.as_dvec3()) * 2.0 / (self.hi - self.lo)).as_vec3()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Radians, around +z; 0 looks from +x.
    pub yaw: f32,
    /// Radians above the xy plane.
    pub pitch: f32,
    pub dist: f32,
    pub target: Vec3,
    pub ortho: bool,
    pub fov_deg: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: (-60f32).to_radians(),
            pitch: 25f32.to_radians(),
            dist: 5.3,
            target: Vec3::ZERO,
            ortho: true,
            fov_deg: 40.0,
        }
    }
}

impl Camera {
    pub fn forward_to_eye(&self) -> Vec3 {
        let (cp, sp) = (self.pitch.cos(), self.pitch.sin());
        Vec3::new(cp * self.yaw.cos(), cp * self.yaw.sin(), sp)
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.forward_to_eye() * self.dist
    }

    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.eye(), self.target, Vec3::Z)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        let fov = self.fov_deg.to_radians();
        if self.ortho {
            let h = self.dist * (fov * 0.5).tan();
            let w = h * aspect;
            glam::camera::rh::proj::directx::orthographic(-w, w, -h, h, 0.01, self.dist * 4.0 + 20.0)
        } else {
            glam::camera::rh::proj::directx::perspective(fov, aspect, 0.05, self.dist * 4.0 + 20.0)
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        // Nudge the picture up a little: the bottom of the canvas carries
        // the layer toolbar.
        Mat4::from_translation(Vec3::new(0.0, 0.05, 0.0)) * self.proj(aspect) * self.view()
    }

    /// Orbit by a mouse drag in points.
    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.008;
        self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.5705, 1.5705);
    }

    /// Pan by a mouse drag in points for a view `height` points tall.
    pub fn pan(&mut self, delta: Vec2, height: f32) {
        let h = self.dist * (self.fov_deg.to_radians() * 0.5).tan();
        let per_px = 2.0 * h / height.max(1.0);
        let fwd = -self.forward_to_eye();
        let right = fwd.cross(Vec3::Z).normalize_or_zero();
        let up = right.cross(fwd).normalize_or_zero();
        self.target += (-right * delta.x + up * delta.y) * per_px;
    }

    pub fn zoom(&mut self, factor: f32) {
        self.dist = (self.dist * factor).clamp(0.5, 60.0);
    }
}

/// Screen projection helper for overlays.
pub struct Projector {
    pub vp: Mat4,
    pub size: Vec2,
}

impl Projector {
    /// World point to pixel position (origin top-left), None if behind.
    pub fn project(&self, p: Vec3) -> Option<Vec2> {
        let c = self.vp * Vec4::new(p.x, p.y, p.z, 1.0);
        if c.w <= 1e-6 {
            return None;
        }
        let n = c.truncate() / c.w;
        Some(Vec2::new((n.x * 0.5 + 0.5) * self.size.x, (0.5 - n.y * 0.5) * self.size.y))
    }
}

/// "Nice" tick values covering `lo..hi`, about `target` of them.
pub fn ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    if !(hi > lo) || !lo.is_finite() || !hi.is_finite() {
        return vec![];
    }
    let raw = (hi - lo) / target.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag);
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    (first..=last)
        .map(|k| {
            let v = k as f64 * step;
            if v.abs() < step * 1e-9 { 0.0 } else { v }
        })
        .collect()
}

pub fn tick_label(v: f64, step: f64) -> String {
    let decimals = if step >= 1.0 { 0 } else { (-step.log10()).ceil() as usize + 1 }.min(6);
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

pub struct GridStyle {
    /// Per-vertex colour from the world-space height (None = solid colour).
    pub height_colors: bool,
    pub color: [f32; 4],
    /// Number of wire lines across each axis (0 = none) and their strength.
    pub wire_lines: f32,
    pub wire_strength: f32,
    pub translucent: bool,
}

/// Triangulated height field. `z` is row-major `z[j * n + i]` over the data
/// rectangle; NaN cells are left out.
pub fn height_mesh(z: &[f64], n: usize, xr: (f64, f64), yr: (f64, f64), frame: &Frame3, style: &GridStyle) -> MeshData {
    let pos: Vec<Option<Vec3>> = z
        .iter()
        .enumerate()
        .map(|(k, &zv)| {
            if !zv.is_finite() {
                return None;
            }
            let x = crate::calc::lerp(xr, (k % n) as f64 / (n - 1) as f64);
            let y = crate::calc::lerp(yr, (k / n) as f64 / (n - 1) as f64);
            Some(frame.map_xyz(x, y, zv))
        })
        .collect();
    let at = |i: isize, j: isize| -> Option<Vec3> {
        if i < 0 || j < 0 || i >= n as isize || j >= n as isize {
            None
        } else {
            pos[j as usize * n + i as usize]
        }
    };
    let step = if style.wire_lines > 0.0 {
        (n - 1) as f32 / style.wire_lines
    } else {
        1.0
    };
    let zh = frame.half.z;
    let mut vertices = Vec::with_capacity(n * n);
    for j in 0..n as isize {
        for i in 0..n as isize {
            let p = at(i, j).unwrap_or(Vec3::ZERO);
            let d = |a: Option<Vec3>, b: Option<Vec3>, c: Vec3| match (a, b) {
                (Some(a), Some(b)) => a - b,
                (Some(a), None) => a - c,
                (None, Some(b)) => c - b,
                _ => Vec3::ZERO,
            };
            let du = d(at(i + 1, j), at(i - 1, j), p);
            let dv = d(at(i, j + 1), at(i, j - 1), p);
            let nrm = du.cross(dv).try_normalize().unwrap_or(Vec3::Z);
            let color = if style.height_colors {
                let t = (p.z / zh * 0.5 + 0.5).clamp(0.0, 1.0);
                lin(viridis(0.08 + 0.84 * t), style.color[3])
            } else {
                style.color
            };
            vertices.push(MeshVertex {
                pos: p.to_array(),
                normal: nrm.to_array(),
                color,
                grid: [i as f32 / step, j as f32 / step, style.wire_strength],
            });
        }
    }
    let mut indices = Vec::with_capacity((n - 1) * (n - 1) * 6);
    let idx = |i: usize, j: usize| (j * n + i) as u32;
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let q = [idx(i, j), idx(i + 1, j), idx(i + 1, j + 1), idx(i, j + 1)];
            let ok = |k: u32| pos[k as usize].is_some();
            // Split each quad along whichever diagonal has both ends defined.
            for tri in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                if tri.iter().all(|&k| ok(k)) {
                    indices.extend_from_slice(&tri);
                }
            }
            if !(ok(q[0]) && ok(q[2])) && ok(q[1]) && ok(q[3]) {
                for tri in [[q[0], q[1], q[3]], [q[1], q[2], q[3]]] {
                    if tri.iter().all(|&k| ok(k)) {
                        indices.extend_from_slice(&tri);
                    }
                }
            }
        }
    }
    MeshData {
        vertices,
        indices,
        translucent: style.translucent,
    }
}

pub fn sphere(center: Vec3, r: f32, color: [f32; 4]) -> MeshData {
    let (rings, segs) = (14, 24);
    let mut vertices = Vec::new();
    for i in 0..=rings {
        let th = std::f32::consts::PI * i as f32 / rings as f32;
        for j in 0..=segs {
            let ph = std::f32::consts::TAU * j as f32 / segs as f32;
            let n = Vec3::new(th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos());
            vertices.push(MeshVertex {
                pos: (center + n * r).to_array(),
                normal: n.to_array(),
                color,
                grid: [0.0, 0.0, 0.0],
            });
        }
    }
    let mut indices = Vec::new();
    for i in 0..rings {
        for j in 0..segs {
            let a = (i * (segs + 1) + j) as u32;
            let b = a + segs as u32 + 1;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    MeshData {
        vertices,
        indices,
        translucent: color[3] < 1.0,
    }
}

pub fn line(a: Vec3, b: Vec3, color: [f32; 4], width: f32, clip: bool) -> LineInstance {
    LineInstance {
        a: a.to_array(),
        b: b.to_array(),
        color,
        params: [width, clip as u8 as f32],
    }
}

/// Polyline through world points, broken wherever a point is None.
pub fn polyline(out: &mut Vec<LineInstance>, pts: &[Option<Vec3>], color: [f32; 4], width: f32) {
    for w in pts.windows(2) {
        if let (Some(a), Some(b)) = (w[0], w[1]) {
            out.push(line(a, b, color, width, true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_ticks() {
        assert_eq!(ticks(-2.0, 2.0, 5), vec![-2.0, -1.0, 0.0, 1.0, 2.0]);
        assert_eq!(ticks(0.0, 1.0, 5), vec![0.0, 0.2, 0.4, 0.6000000000000001, 0.8, 1.0]);
        assert_eq!(tick_label(0.6000000000000001, 0.2), "0.6");
        assert_eq!(tick_label(-1.0, 1.0), "-1");
        assert!(ticks(1.0, 1.0, 5).is_empty());
    }

    #[test]
    fn frame_maps_box_corners() {
        let f = Frame3::new(DVec3::new(-2.0, 0.0, -1.0), DVec3::new(2.0, 1.0, 1.0), false);
        assert_eq!(f.map_xyz(-2.0, 0.0, -1.0), Vec3::splat(-1.0));
        assert_eq!(f.map_xyz(2.0, 1.0, 1.0), Vec3::splat(1.0));
        let e = Frame3::new(DVec3::new(-2.0, -1.0, 0.0), DVec3::new(2.0, 1.0, 1.0), true);
        assert_eq!(e.half, Vec3::new(1.0, 0.5, 0.25));
    }

    #[test]
    fn mesh_skips_undefined_cells() {
        let n = 3;
        let mut z = vec![0.0; 9];
        z[4] = f64::NAN; // centre undefined: every quad touches it
        let f = Frame3::new(DVec3::splat(-1.0), DVec3::splat(1.0), false);
        let style = GridStyle { height_colors: true, color: [1.0; 4], wire_lines: 0.0, wire_strength: 0.0, translucent: false };
        let m = height_mesh(&z, n, (-1.0, 1.0), (-1.0, 1.0), &f, &style);
        assert!(m.indices.chunks(3).all(|t| !t.contains(&4)));
        // Each quad keeps the one triangle that avoids the centre.
        assert_eq!(m.indices.len(), 4 * 3);
    }

    #[test]
    fn projection_centres_target() {
        let cam = Camera::default();
        let p = Projector { vp: cam.view_proj(1.5), size: Vec2::new(300.0, 200.0) };
        let c = p.project(Vec3::ZERO).unwrap();
        // Centred horizontally, nudged up by 2.5% of the height.
        assert!((c - Vec2::new(150.0, 95.0)).length() < 1e-3);
    }
}
