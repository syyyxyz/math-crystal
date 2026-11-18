//! Pictures of complex functions: domain colouring, the |f| relief mesh
//! and the grid lines for the conformal-mapping view.

use glam::DVec3;
use num_complex::Complex64 as C64;

use crate::complex::CFunc;
use crate::render::scene::{Frame3, GridStyle, MeshData, height_mesh, lin};

const TAU: f64 = std::f64::consts::TAU;

/// Domain-colouring options.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    /// Lightness from |f|: zeros black, poles white.
    pub lightness: bool,
    /// Sawtooth bands at |f| = 2^k (level curves of the modulus).
    pub modulus: bool,
    /// Sawtooth bands every 30° of arg f (level curves of the phase).
    pub phase_lines: bool,
    /// Lines where Re f or Im f is a multiple of the grid step.
    pub fgrid: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self { lightness: false, modulus: true, phase_lines: false, fgrid: false }
    }
}

/// Hue for a phase in [0, 1): red at arg 0, then yellow, green (arg = 2π/3),
/// cyan (π), blue, magenta.
pub fn hue_rgb(h: f64) -> [f32; 3] {
    let h6 = h.rem_euclid(1.0) * 6.0;
    let x = 1.0 - ((h6 % 2.0) - 1.0).abs();
    let (r, g, b) = match h6 as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    // Slightly soften the pure HSV primaries.
    let soft = |v: f64| (0.1 + 0.85 * v) as f32;
    [soft(r), soft(g), soft(b)]
}

/// sRGB colour (0..1) of the value w.
pub fn color(w: C64, st: &Style) -> [f32; 3] {
    if w.re.is_nan() || w.im.is_nan() {
        return [0.55, 0.55, 0.55];
    }
    let m = w.norm();
    if !m.is_finite() {
        return [1.0, 1.0, 1.0];
    }
    let h = w.arg() / TAU;
    let mut c = hue_rgb(h);
    let mut v = 1.0f32;
    if st.modulus && m > 0.0 {
        let k = m.log2().rem_euclid(1.0) as f32;
        v *= 0.7 + 0.3 * k;
    }
    if st.phase_lines {
        let k = (h * 12.0).rem_euclid(1.0) as f32;
        v *= 0.78 + 0.22 * k;
    }
    for x in &mut c {
        *x *= v;
    }
    if st.lightness {
        let l = (2.0 / std::f64::consts::PI * m.atan()) as f32;
        for x in &mut c {
            *x = if l < 0.5 { *x * 2.0 * l } else { *x + (1.0 - *x) * (2.0 * l - 1.0) };
        }
    }
    c
}

/// A "nice" grid step for the Re/Im lines of f: about a third of the
/// typical |f|.
pub fn auto_grid_step(ws: &[C64]) -> f64 {
    let mut m: Vec<f64> = ws.iter().map(|w| w.norm()).filter(|v| v.is_finite() && *v > 0.0).collect();
    if m.is_empty() {
        return 1.0;
    }
    let mid = m.len() / 2;
    let med = *m.select_nth_unstable_by(mid, |a, b| a.total_cmp(b)).1;
    let raw = (med / 3.0).max(1e-9);
    let p = 10f64.powf(raw.log10().floor());
    let r = raw / p;
    p * if r < 1.5 {
        1.0
    } else if r < 3.5 {
        2.0
    } else if r < 7.5 {
        5.0
    } else {
        10.0
    }
}

/// Domain-colouring image of row-major samples (row 0 at the top).
pub fn image(ws: &[C64], w: usize, h: usize, st: &Style, grid_step: f64) -> egui::ColorImage {
    let mut px = Vec::with_capacity(w * h);
    for j in 0..h {
        for i in 0..w {
            let v = ws[j * w + i];
            let mut c = color(v, st);
            if st.fgrid && v.is_finite() {
                // Local change per pixel decides the line width in f units.
                let nb = |a: usize, b: usize| ws.get(b * w + a).copied().filter(|q| q.is_finite()).map_or(0.0, |q| (q - v).norm());
                let g = nb((i + 1).min(w - 1), j).max(nb(i, (j + 1).min(h - 1))).max(1e-300);
                let line = |x: f64| {
                    let d = (x / grid_step - (x / grid_step).round()).abs() * grid_step;
                    (d / (0.9 * g)).clamp(0.0, 1.0) as f32
                };
                let k = line(v.re).min(line(v.im));
                let dark = 0.25 + 0.75 * k;
                for x in &mut c {
                    *x *= dark;
                }
            }
            px.push(egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
        }
    }
    egui::ColorImage::new([w, h], px)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Height {
    Modulus,
    LogModulus,
    Re,
    Im,
}

impl Height {
    pub fn of(self, w: C64) -> f64 {
        match self {
            Height::Modulus => w.norm(),
            Height::LogModulus => w.norm().ln(),
            Height::Re => w.re,
            Height::Im => w.im,
        }
    }
}

pub struct Relief {
    pub mesh: MeshData,
    pub frame: Frame3,
    pub xr: (f64, f64),
    pub yr: (f64, f64),
    pub zr: (f64, f64),
    /// Samples on the n×n grid, row-major from the bottom-left.
    pub ws: Vec<C64>,
    pub n: usize,
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    let k = ((v.len() - 1) as f64 * p).round() as usize;
    *v.select_nth_unstable_by(k, |a, b| a.total_cmp(b)).1
}

/// Height field over the square [c ± half]² coloured by the phase.
pub fn relief(f: &CFunc, center: C64, half: f64, n: usize, height: Height, st: &Style, wire: bool) -> Relief {
    let xr = (center.re - half, center.re + half);
    let yr = (center.im - half, center.im + half);
    let mut zs = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            zs.push(C64::new(crate::calc::lerp(xr, i as f64 / (n - 1) as f64), crate::calc::lerp(yr, j as f64 / (n - 1) as f64)));
        }
    }
    let ws = f.eval_many(&zs);
    let hs: Vec<f64> = ws.iter().map(|&w| height.of(w)).map(|v| if v.is_finite() { v } else { f64::NAN }).collect();
    let mut fin: Vec<f64> = hs.iter().copied().filter(|v| v.is_finite()).collect();
    let zr = if fin.is_empty() {
        (-1.0, 1.0)
    } else {
        match height {
            // Poles would flatten everything else: cap at a high percentile.
            Height::Modulus => (0.0, (percentile(&mut fin, 0.9) * 1.5).max(1e-9)),
            _ => {
                let lo = percentile(&mut fin, 0.02);
                let hi = percentile(&mut fin, 0.98);
                let pad = ((hi - lo) * 0.08).max(1e-9);
                (lo - pad, hi + pad)
            }
        }
    };
    let frame = Frame3::new(DVec3::new(xr.0, yr.0, zr.0), DVec3::new(xr.1, yr.1, zr.1), false);
    // Clamp spikes to just above the box so the mesh stays connected; the
    // renderer clips everything outside the box.
    let top = zr.1 + (zr.1 - zr.0) * 0.02;
    let bottom = zr.0 - (zr.1 - zr.0) * 0.02;
    let hs: Vec<f64> = hs.iter().map(|&v| if v.is_finite() { v.clamp(bottom, top) } else if height == Height::Modulus { top } else { f64::NAN }).collect();
    let style = GridStyle { height_colors: false, color: [1.0, 1.0, 1.0, 1.0], wire_lines: if wire { 16.0 } else { 0.0 }, wire_strength: if wire { 0.22 } else { 0.0 }, translucent: false };
    let mut mesh = height_mesh(&hs, n, xr, yr, &frame, &style, None);
    for (v, w) in mesh.vertices.iter_mut().zip(&ws) {
        let c = color(*w, st);
        v.color = lin(c, 1.0);
    }
    Relief { mesh, frame, xr, yr, zr, ws, n }
}

/// One line of the z-plane grid.
pub struct GridLine {
    pub pts: Vec<C64>,
    /// 0: Re z = const (or circles |z| = r), 1: Im z = const (or rays).
    pub family: u8,
    /// Position of the line within its family, 0..1.
    pub t: f32,
    /// The line is an axis (Re z = 0, Im z = 0) or the unit circle.
    pub axis: bool,
}

/// Cartesian or polar grid over the rectangle.
pub fn grid_lines(re: (f64, f64), im: (f64, f64), lines: usize, polar: bool) -> Vec<GridLine> {
    let samples = 240;
    let mut out = Vec::new();
    if polar {
        let rmax = [re.0, re.1].iter().flat_map(|&x| [im.0, im.1].map(|y| (x * x + y * y).sqrt())).fold(0.0, f64::max);
        let step = crate::render::scene::ticks(0.0, rmax, lines).windows(2).next().map_or(rmax / lines as f64, |w| w[1] - w[0]);
        let mut r = step;
        let mut k = 0;
        while r <= rmax + 1e-12 && k < 64 {
            let pts = (0..=samples * 2).map(|s| C64::from_polar(r, TAU * s as f64 / (samples * 2) as f64)).collect();
            out.push(GridLine { pts, family: 0, t: (r / rmax) as f32, axis: (r - 1.0).abs() < 1e-9 });
            r += step;
            k += 1;
        }
        let rays = 24;
        for s in 0..rays {
            let a = TAU * s as f64 / rays as f64;
            let pts = (0..=samples).map(|q| C64::from_polar(rmax * q as f64 / samples as f64, a)).collect();
            out.push(GridLine { pts, family: 1, t: s as f32 / rays as f32, axis: s % (rays / 4) == 0 });
        }
        return out;
    }
    let tx = crate::render::scene::ticks(re.0, re.1, lines);
    let ty = crate::render::scene::ticks(im.0, im.1, lines);
    // Finer than the labelled ticks: two lines per tick step.
    let fine = |t: &[f64], r: (f64, f64)| -> Vec<f64> {
        let step = if t.len() > 1 { (t[1] - t[0]) / 2.0 } else { (r.1 - r.0) / lines as f64 };
        let mut v = Vec::new();
        let mut x = (r.0 / step).ceil() * step;
        while x <= r.1 + 1e-12 && v.len() < 200 {
            v.push(if x.abs() < step * 1e-9 { 0.0 } else { x });
            x += step;
        }
        v
    };
    for x in fine(&tx, re) {
        let pts = (0..=samples).map(|s| C64::new(x, crate::calc::lerp(im, s as f64 / samples as f64))).collect();
        out.push(GridLine { pts, family: 0, t: ((x - re.0) / (re.1 - re.0)) as f32, axis: x == 0.0 });
    }
    for y in fine(&ty, im) {
        let pts = (0..=samples).map(|s| C64::new(crate::calc::lerp(re, s as f64 / samples as f64), y)).collect();
        out.push(GridLine { pts, family: 1, t: ((y - im.0) / (im.1 - im.0)) as f32, axis: y == 0.0 });
    }
    out
}

/// Colour of a grid line (sRGB).
pub fn line_color(family: u8, t: f32, dark: bool) -> egui::Color32 {
    // Family 0 runs blue → teal, family 1 orange → crimson.
    let (a, b) = if family == 0 { ([0x1c, 0x7e, 0xd6], [0x0c, 0xa6, 0x78]) } else { ([0xf0, 0x8c, 0x00], [0xd6, 0x33, 0x6c]) };
    let mix = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t.clamp(0.0, 1.0)) as u8;
    let c = egui::Color32::from_rgb(mix(0), mix(1), mix(2));
    if dark { c } else { c.gamma_multiply(0.95) }
}

/// Robust view of a cloud of image points: centre and half-size covering
/// the middle 94% of them.
pub fn fit(points: &[C64]) -> Option<(C64, f64)> {
    let mut re: Vec<f64> = points.iter().filter(|w| w.is_finite()).map(|w| w.re).collect();
    let mut im: Vec<f64> = points.iter().filter(|w| w.is_finite()).map(|w| w.im).collect();
    if re.len() < 4 {
        return None;
    }
    let (r0, r1) = (percentile(&mut re, 0.03), percentile(&mut re, 0.97));
    let (i0, i1) = (percentile(&mut im, 0.03), percentile(&mut im, 0.97));
    let half = ((r1 - r0).max(i1 - i0) * 0.5 * 1.2).max(1e-6);
    Some((C64::new((r0 + r1) / 2.0, (i0 + i1) / 2.0), half))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_wheel() {
        // arg 0 is red, arg π is cyan.
        let red = hue_rgb(0.0);
        assert!(red[0] > 0.9 && red[1] < 0.2 && red[2] < 0.2);
        let cyan = hue_rgb(0.5);
        assert!(cyan[0] < 0.2 && cyan[1] > 0.9 && cyan[2] > 0.9);
        let st = Style { lightness: true, ..Style::default() };
        let z = color(C64::new(1e-9, 0.0), &st);
        assert!(z.iter().all(|&c| c < 0.05), "zeros are black");
        let p = color(C64::new(1e9, 0.0), &st);
        assert!(p.iter().all(|&c| c > 0.95), "poles are white");
    }

    #[test]
    fn steps() {
        let ws: Vec<C64> = (1..100).map(|k| C64::new(k as f64 * 0.1, 0.0)).collect();
        assert_eq!(auto_grid_step(&ws), 2.0);
    }
}
