//! 2D contour map of f(x, y): heat map, level curves, gradient field,
//! critical points, integration region and the point P (draggable).

use egui::{Align2, Color32, ColorImage, FontId, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, TextureOptions, Ui, Vec2};

use crate::analysis::{Critical, contour};
use crate::calc::{PointDerivs, Surface, lerp};
use crate::calc_view::{self, Settings};
use crate::render::scene::{tick_label, ticks, viridis};
use crate::ui::theme::Tokens;

pub struct MapInput<'a> {
    pub surface: Option<&'a Surface>,
    pub settings: &'a Settings,
    /// Row-major n x n samples over the domain.
    pub z: &'a [f64],
    pub n: usize,
    pub criticals: &'a [Critical],
    pub region: Option<(&'a [f64], usize)>,
    pub derivs: Option<&'a PointDerivs>,
    pub version: u64,
}

/// Cached heat-map texture.
#[derive(Default)]
pub struct MapCache {
    heat: Option<(u64, TextureHandle)>,
}

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Draw the map into `rect`; returns a new P when the user clicked/dragged.
pub fn draw(ui: &mut Ui, rect: Rect, inp: &MapInput, cache: &mut MapCache, t: &Tokens) -> Option<(f64, f64)> {
    let s = inp.settings;
    let (xr, yr) = (s.xr, s.yr);
    // Equal-aspect plot area with room for tick labels.
    let avail = Rect::from_min_max(rect.min + Vec2::new(64.0, 96.0), rect.max - Vec2::new(28.0, 92.0));
    let (w, h) = ((xr.1 - xr.0) as f32, (yr.1 - yr.0) as f32);
    let scale = (avail.width() / w).min(avail.height() / h);
    let plot = Rect::from_center_size(avail.center(), Vec2::new(w * scale, h * scale));
    let to_screen = |x: f64, y: f64| {
        Pos2::new(
            plot.min.x + ((x - xr.0) / (xr.1 - xr.0)) as f32 * plot.width(),
            plot.max.y - ((y - yr.0) / (yr.1 - yr.0)) as f32 * plot.height(),
        )
    };
    let to_data = |p: Pos2| {
        (
            lerp(xr, ((p.x - plot.min.x) / plot.width()).clamp(0.0, 1.0) as f64),
            lerp(yr, ((plot.max.y - p.y) / plot.height()).clamp(0.0, 1.0) as f64),
        )
    };
    let resp = ui.interact(plot, ui.scope_id().with("map_plot"), Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    // Heat map.
    let n = inp.n;
    if n >= 2 && inp.z.len() == n * n {
        let stale = cache.heat.as_ref().is_none_or(|(v, _)| *v != inp.version);
        if stale {
            let (lo, hi) = s.zr;
            let mut img = ColorImage::filled([n, n], Color32::TRANSPARENT);
            for j in 0..n {
                for i in 0..n {
                    let v = inp.z[j * n + i];
                    if v.is_finite() {
                        let tt = (((v - lo) / (hi - lo)) as f32).clamp(0.0, 1.0);
                        let c = viridis(0.08 + 0.84 * tt);
                        // Rows are stored top-down; y grows upwards.
                        img.pixels[(n - 1 - j) * n + i] = Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8);
                    }
                }
            }
            cache.heat = Some((inp.version, ui.ctx().load_texture("heatmap", img, TextureOptions::LINEAR)));
        }
        if let Some((_, tex)) = &cache.heat {
            // Texel centres sit on the grid nodes.
            let half = 0.5 / n as f32;
            let uv = Rect::from_min_max(Pos2::new(half, half), Pos2::new(1.0 - half, 1.0 - half));
            painter.image(tex.id(), plot, uv, Color32::from_white_alpha(if t.dark { 230 } else { 215 }));
        }
    }
    painter.rect_stroke(plot, 0.0, Stroke::new(1.0, t.faint), egui::StrokeKind::Outside);

    // Level curves.
    if s.show_contours && n >= 2 {
        let stroke = Stroke::new(1.1, Color32::from_black_alpha(if t.dark { 170 } else { 120 }));
        for lv in calc_view::contour_levels(s) {
            for seg in contour(inp.z, n, xr, yr, lv) {
                painter.line_segment([to_screen(seg[0].0, seg[0].1), to_screen(seg[1].0, seg[1].1)], stroke);
            }
        }
    }

    // Gradient field.
    if s.show_field
        && let Some(surf) = inp.surface
    {
        let m = 21;
        let mut xs = Vec::with_capacity(m * m);
        let mut ys = Vec::with_capacity(m * m);
        for j in 0..m {
            for i in 0..m {
                xs.push(lerp(xr, (i as f64 + 0.5) / m as f64));
                ys.push(lerp(yr, (j as f64 + 0.5) / m as f64));
            }
        }
        let all = surf.eval_all(&xs, &ys);
        // Screen-space gradient (y flipped), scaled per axis like the plot.
        let g: Vec<Vec2> = (0..m * m)
            .map(|k| Vec2::new(all[1][k] as f32 / (xr.1 - xr.0) as f32, -all[2][k] as f32 / (yr.1 - yr.0) as f32))
            .collect();
        let gmax = g.iter().map(|v| v.length()).filter(|v| v.is_finite()).fold(0.0f32, f32::max);
        let cell = plot.width().min(plot.height()) / m as f32;
        let col = if t.dark { Color32::from_rgb(0xe6, 0xe9, 0xf0) } else { Color32::from_rgb(0x1b, 0x1f, 0x2a) };
        for k in 0..m * m {
            let v = g[k];
            let len = v.length();
            if !len.is_finite() || gmax <= 0.0 || len < 1e-9 * gmax {
                continue;
            }
            let dir = v / len;
            let l = cell * (0.3 + 0.55 * (len / gmax).sqrt());
            let c = to_screen(xs[k], ys[k]);
            arrow(&painter, c - dir * l * 0.5, dir, l, Stroke::new(1.3, col.gamma_multiply(0.85)));
        }
    }

    // Integration region.
    if s.show_region
        && let Some((ind, rn)) = inp.region
    {
        let stroke = Stroke::new(2.2, rgb(calc_view::C_REGION));
        for seg in contour(ind, rn, xr, yr, 0.0) {
            painter.line_segment([to_screen(seg[0].0, seg[0].1), to_screen(seg[1].0, seg[1].1)], stroke);
        }
    }

    // Axes ticks.
    for (axis, (lo, hi)) in [(0, xr), (1, yr)] {
        let tv = ticks(lo, hi, 6);
        let step = if tv.len() > 1 { tv[1] - tv[0] } else { 1.0 };
        for v in tv {
            let (p, q, anchor, off) = if axis == 0 {
                let p = to_screen(v, yr.0);
                (p, p + Vec2::new(0.0, 5.0), Align2::CENTER_TOP, Vec2::new(0.0, 8.0))
            } else {
                let p = to_screen(xr.0, v);
                (p, p - Vec2::new(5.0, 0.0), Align2::RIGHT_CENTER, Vec2::new(-8.0, 0.0))
            };
            painter.line_segment([p, q], Stroke::new(1.0, t.faint));
            painter.text(p + off, anchor, tick_label(v, step), FontId::monospace(11.5), t.muted);
        }
    }
    painter.text(Pos2::new(plot.max.x + 10.0, plot.max.y), Align2::LEFT_CENTER, "x", FontId::proportional(17.0), t.text);
    painter.text(Pos2::new(plot.min.x, plot.min.y - 10.0), Align2::CENTER_BOTTOM, "y", FontId::proportional(17.0), t.text);

    // Critical points.
    let hover = resp.hover_pos();
    if s.show_criticals {
        for c in inp.criticals {
            let p = to_screen(c.x, c.y);
            if !plot.expand(1.0).contains(p) {
                continue;
            }
            painter.circle(p, 6.0, rgb(calc_view::critical_color(c.kind)), Stroke::new(1.5, Color32::WHITE));
            if hover.is_some_and(|h| h.distance(p) < 9.0) {
                let txt = format!("{} at ({:.4}, {:.4})\nf = {:.6}", c.kind.name(), c.x, c.y, c.f);
                tooltip(&painter, p + Vec2::new(10.0, -10.0), &txt, t);
            }
        }
    }

    // P with direction / gradient arrows.
    if let Some(d) = inp.derivs {
        let p = to_screen(s.a, s.b);
        let unit = plot.width().min(plot.height()) * 0.16;
        if s.show_direction {
            let a = s.dir_angle.to_radians();
            // Screen y is flipped; the arrow shows the data direction (cos, sin)
            // scaled per axis.
            let dir = Vec2::new((a.cos() / (xr.1 - xr.0)) as f32, -(a.sin() / (yr.1 - yr.0)) as f32).normalized();
            arrow(&painter, p, dir, unit, Stroke::new(2.6, rgb(calc_view::C_DIR)));
        }
        if s.show_gradient && d.first_ok() {
            let g = Vec2::new((d.fx.v / (xr.1 - xr.0)) as f32, -(d.fy.v / (yr.1 - yr.0)) as f32);
            if g.length() > 1e-12 {
                arrow(&painter, p, g.normalized(), unit, Stroke::new(2.6, rgb(calc_view::C_GRAD)));
            }
        }
        if s.show_point && plot.expand(1.0).contains(p) {
            // A ringed marker so P stays distinct from red maxima.
            painter.circle(p, 9.0, Color32::TRANSPARENT, Stroke::new(2.0, rgb(calc_view::C_POINT)));
            painter.circle(p, 5.0, rgb(calc_view::C_POINT), Stroke::new(1.5, Color32::WHITE));
            let label = painter.layout_no_wrap("P".to_string(), FontId::proportional(13.0), Color32::WHITE);
            let r = Rect::from_min_size(p + Vec2::new(-26.0, 8.0), label.size() + Vec2::new(10.0, 4.0));
            painter.rect_filled(r, 7.0, rgb(calc_view::C_POINT));
            painter.galley(r.min + Vec2::new(5.0, 2.0), label, Color32::WHITE);
        }
    }

    // Hover read-out.
    if let (Some(h), Some(surf)) = (hover, inp.surface)
        && !resp.dragged()
    {
        let (x, y) = to_data(h);
        let f = surf.f(x, y);
        let txt = format!("x = {}\ny = {}\nf = {}", short(x), short(y), if f.is_finite() { short(f) } else { "undefined".into() });
        tooltip(&painter, h + Vec2::new(14.0, 14.0), &txt, t);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }

    if (resp.clicked() || resp.dragged())
        && let Some(pos) = resp.interact_pointer_pos()
    {
        return Some(to_data(pos));
    }
    None
}

fn short(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn arrow(painter: &egui::Painter, from: Pos2, dir: Vec2, len: f32, stroke: Stroke) {
    let tip = from + dir * len;
    painter.line_segment([from, tip], stroke);
    let head = (len * 0.3).clamp(4.0, 10.0);
    let side = Vec2::new(-dir.y, dir.x) * head * 0.55;
    let back = tip - dir * head;
    painter.add(Shape::convex_polygon(vec![tip, back + side, back - side], stroke.color, Stroke::NONE));
}

fn tooltip(painter: &egui::Painter, at: Pos2, text: &str, t: &Tokens) {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::monospace(12.0), t.text);
    let r = Rect::from_min_size(at, galley.size() + Vec2::new(12.0, 8.0));
    // Keep inside the clip rect.
    let clip = painter.clip_rect();
    let dx = (r.max.x - clip.max.x + 4.0).max(0.0);
    let dy = (r.max.y - clip.max.y + 4.0).max(0.0);
    let r = r.translate(Vec2::new(-dx, -dy));
    painter.rect(r, 6.0, t.overlay, Stroke::new(1.0, t.overlay_border), egui::StrokeKind::Inside);
    painter.galley(r.min + Vec2::new(6.0, 4.0), galley, t.text);
}
