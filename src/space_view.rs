//! Scene of the Space workspace: object meshes, curves, solid edges and
//! coordinate axes, mapped into the world box.

use glam::{DVec3, Vec3};

use crate::render::scene::{Frame3, LineInstance, MeshData, MeshVertex, hex, line, polyline, ticks};
use crate::space::mesh::DMesh;

/// Spacing of grid and level lines over the box.
pub fn line_step(lo: DVec3, hi: DVec3) -> f64 {
    let t = ticks(0.0, (hi - lo).max_element(), 10);
    if t.len() > 1 { t[1] - t[0] } else { 1.0 }
}

/// `(r, g, b)` scaled towards black (for edges drawn on a body).
pub fn darker(rgb: u32, f: f32) -> [f32; 4] {
    let c = hex(rgb, 1.0);
    [c[0] * f, c[1] * f, c[2] * f, 1.0]
}

pub enum Lines {
    None,
    /// Grid from the mesh's own coordinates (planes).
    Grid,
    /// Level lines z = const.
    Levels,
}

/// World-space mesh of `m` in one colour.
pub fn to_mesh(m: &DMesh, frame: &Frame3, rgb: u32, opacity: f32, lines: Lines, step: f64) -> MeshData {
    let scale = frame.scale().as_dvec3();
    let color = hex(rgb, opacity);
    let strength = if matches!(lines, Lines::Grid) { 0.5 } else { 0.35 };
    let vertices = m
        .pos
        .iter()
        .zip(&m.nrm)
        .enumerate()
        .map(|(k, (p, n))| {
            let grid = match (&lines, &m.uv) {
                (Lines::Grid, Some(uv)) => [(uv[k][0] / step) as f32, (uv[k][1] / step) as f32, strength],
                (Lines::Levels, _) => [(p.z / step) as f32, 0.5, strength],
                _ => [0.0, 0.0, 0.0],
            };
            let nw = (*n / scale).normalize_or_zero().as_vec3();
            MeshVertex { pos: frame.map(*p).to_array(), normal: nw.to_array(), color, grid }
        })
        .collect();
    MeshData { vertices, indices: m.tris.iter().flatten().copied().collect(), translucent: opacity < 0.995 }
}

/// Solid outlines as lines.
pub fn edges(m: &DMesh, frame: &Frame3, color: [f32; 4], width: f32, out: &mut Vec<LineInstance>) {
    for e in &m.edges {
        let pts: Vec<Option<Vec3>> = e.iter().map(|p| Some(frame.map(*p))).collect();
        polyline(out, &pts, color, width);
    }
}

/// A curve given by data-space points.
pub fn curve(pts: &[Option<DVec3>], frame: &Frame3, color: [f32; 4], width: f32, out: &mut Vec<LineInstance>) {
    let w: Vec<Option<Vec3>> = pts.iter().map(|p| p.map(|p| frame.map(p))).collect();
    polyline(out, &w, color, width);
}

/// Coordinate axes through the origin, where the origin is in the box.
pub fn axes(lo: DVec3, hi: DVec3, frame: &Frame3, color: [f32; 4], out: &mut Vec<LineInstance>) {
    for i in 0..3 {
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        if lo[j] > 0.0 || hi[j] < 0.0 || lo[k] > 0.0 || hi[k] < 0.0 {
            continue;
        }
        let mut a = DVec3::ZERO;
        let mut b = DVec3::ZERO;
        a[i] = lo[i];
        b[i] = hi[i];
        out.push(line(frame.map(a), frame.map(b), color, 1.4, true));
    }
}
