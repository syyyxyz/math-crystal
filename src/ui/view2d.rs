//! Front / side / top views as 2D drawings: coordinate axes along the
//! canvas edges with ticks that follow the zoom, a faint grid, the range of
//! the coordinate along the line of sight, a readout under the cursor, and
//! the outer outlines of the surfaces (see [`crate::outline`]).

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Ui, pos2, vec2};
use glam::DVec3;

use crate::outline::{Seg, ViewPlane};

/// A traced outline: polylines in data coordinates.
pub type Lines = Vec<Vec<[f64; 2]>>;
use crate::render::scene::{Frame3, tick_label, ticks};
use crate::ui::canvas3d::View3d;
use crate::ui::theme::Tokens;

/// Outline cells in screen points.
const CELL_POINTS: f64 = 1.5;

/// The linear map between a coordinate plane and the canvas of a 2D view.
pub struct Axes2 {
    pub rect: Rect,
    /// `[horizontal, vertical, depth]` as indices of x, y, z.
    pub axes: [usize; 3],
    /// Screen position of data (0, 0) and points per data unit (the
    /// vertical one is negative: up on screen).
    origin: [f64; 2],
    scale: [f64; 2],
    lo: DVec3,
    hi: DVec3,
}

impl Axes2 {
    /// None unless `view` shows a 2D drawing.
    pub fn new(view: &View3d, frame: &Frame3, rect: Rect) -> Option<Self> {
        let axes = view.plane_axes()?;
        let [u, v, _] = axes;
        let proj = view.projector(rect);
        let mid = (frame.lo + frame.hi) * 0.5;
        let span = frame.hi - frame.lo;
        let at = |du: f64, dv: f64| {
            let mut p = mid;
            p[u] += du;
            p[v] += dv;
            proj.project(frame.map(p))
        };
        let (p0, pu, pv) = (at(0.0, 0.0)?, at(span[u], 0.0)?, at(0.0, span[v])?);
        let scale = [(pu.x - p0.x) as f64 / span[u], (pv.y - p0.y) as f64 / span[v]];
        if !(scale[0].abs() > 1e-12 && scale[1].abs() > 1e-12 && scale.iter().all(|s| s.is_finite())) {
            return None;
        }
        let origin = [rect.min.x as f64 + p0.x as f64 - mid[u] * scale[0], rect.min.y as f64 + p0.y as f64 - mid[v] * scale[1]];
        Some(Self { rect, axes, origin, scale, lo: frame.lo, hi: frame.hi })
    }

    pub fn to_screen(&self, u: f64, v: f64) -> Pos2 {
        pos2((self.origin[0] + u * self.scale[0]) as f32, (self.origin[1] + v * self.scale[1]) as f32)
    }

    pub fn from_screen(&self, p: Pos2) -> [f64; 2] {
        [(p.x as f64 - self.origin[0]) / self.scale[0], (p.y as f64 - self.origin[1]) / self.scale[1]]
    }

    /// Visible `[(u0, u1), (v0, v1)]`.
    pub fn visible(&self) -> [(f64, f64); 2] {
        let a = self.from_screen(self.rect.left_bottom());
        let b = self.from_screen(self.rect.right_top());
        [(a[0].min(b[0]), a[0].max(b[0])), (a[1].min(b[1]), a[1].max(b[1]))]
    }

    /// Data units per screen point along each axis.
    pub fn units_per_point(&self) -> [f64; 2] {
        [1.0 / self.scale[0].abs(), 1.0 / self.scale[1].abs()]
    }

    /// Decimals that resolve one screen point along axis `k` (0 or 1).
    fn decimals(&self, k: usize) -> usize {
        (-self.units_per_point()[k].log10()).ceil().clamp(0.0, 12.0) as usize
    }

    /// A value along axis `k` (0 or 1), to the precision of a screen point.
    pub fn fmt(&self, k: usize, x: f64) -> String {
        let s = format!("{x:.*}", self.decimals(k));
        if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') { s[1..].to_string() } else { s }
    }

    /// Grid, ticks and labels along the bottom and left edges, axis names,
    /// the range along the line of sight and the cursor readout. `extra`
    /// adds a line to the readout (e.g. the function value).
    pub fn draw(&self, ui: &Ui, t: &Tokens, names: [&str; 3], hover: Option<Pos2>, extra: Option<&dyn Fn(f64, f64) -> Option<String>>) {
        let rect = self.rect;
        let p = ui.painter_at(rect);
        let [(u0, u1), (v0, v1)] = self.visible();
        let grid = if t.dark { Color32::from_white_alpha(22) } else { Color32::from_black_alpha(18) };
        let zero = if t.dark { Color32::from_white_alpha(70) } else { Color32::from_black_alpha(60) };
        let tu = ticks(u0, u1, ((rect.width() / 70.0) as usize).max(3));
        let tv = ticks(v0, v1, ((rect.height() / 50.0) as usize).max(3));
        let su = if tu.len() > 1 { tu[1] - tu[0] } else { 1.0 };
        let sv = if tv.len() > 1 { tv[1] - tv[0] } else { 1.0 };
        for &u in &tu {
            let x = self.to_screen(u, 0.0).x;
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, if u == 0.0 { zero } else { grid }));
        }
        for &v in &tv {
            let y = self.to_screen(0.0, v).y;
            p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, if v == 0.0 { zero } else { grid }));
        }

        let font = FontId::monospace(11.5);
        let pill = |pos: Pos2, align: Align2, text: String, color: Color32| {
            let galley = p.layout_no_wrap(text, font.clone(), color);
            let r = align.anchor_size(pos, galley.size()).expand2(vec2(3.0, 1.0));
            p.rect_filled(r, CornerRadius::same(3), t.overlay.gamma_multiply(0.85));
            p.galley(r.min + vec2(3.0, 1.0), galley, color);
            r
        };
        for &u in &tu {
            let x = self.to_screen(u, 0.0).x;
            if x < rect.left() + 40.0 || x > rect.right() - 28.0 {
                continue;
            }
            p.line_segment([pos2(x, rect.bottom() - 5.0), pos2(x, rect.bottom())], Stroke::new(1.0, t.muted));
            pill(pos2(x, rect.bottom() - 8.0), Align2::CENTER_BOTTOM, tick_label(u, su), t.muted);
        }
        let mut left: Vec<f32> = Vec::new();
        for &v in &tv {
            let y = self.to_screen(0.0, v).y;
            if y < rect.top() + 14.0 || y > rect.bottom() - 30.0 {
                continue;
            }
            p.line_segment([pos2(rect.left(), y), pos2(rect.left() + 5.0, y)], Stroke::new(1.0, t.muted));
            pill(pos2(rect.left() + 8.0, y), Align2::LEFT_CENTER, tick_label(v, sv), t.muted);
            left.push(y);
        }
        // Axis names: horizontal at the right end, vertical in the gap
        // between the left labels nearest the middle.
        let [nu, nv, nw] = [names[self.axes[0]], names[self.axes[1]], names[self.axes[2]]];
        let name_font = FontId::proportional(16.0);
        p.text(pos2(rect.right() - 10.0, rect.bottom() - 10.0), Align2::RIGHT_BOTTOM, nu, name_font.clone(), t.text);
        left.sort_by(|a, b| a.total_cmp(b));
        let mid = rect.center().y;
        let gap = left.windows(2).map(|w| (w[0] + w[1]) / 2.0).min_by(|a, b| (a - mid).abs().total_cmp(&(b - mid).abs())).unwrap_or(mid);
        p.text(pos2(rect.left() + 10.0, gap), Align2::LEFT_CENTER, nv, name_font, t.text);

        // Range along the line of sight.
        let w = self.axes[2];
        let range = format!("{nw} ∈ [{}, {}]", crate::app::fmt_short(self.lo[w]), crate::app::fmt_short(self.hi[w]));
        let along = if crate::i18n::zh() { format!("沿 {nw} 方向看 · {range}") } else { format!("Looking along {nw} · {range}") };
        let galley = p.layout_no_wrap(along, FontId::proportional(12.5), t.muted);
        let r = Align2::LEFT_BOTTOM.anchor_size(pos2(rect.left() + 8.0, rect.bottom() - 30.0), galley.size()).expand2(vec2(4.0, 2.0));
        p.rect_filled(r, CornerRadius::same(4), t.overlay.gamma_multiply(0.85));
        p.galley(r.min + vec2(4.0, 2.0), galley, t.muted);

        // Readout under the cursor.
        if let Some(hp) = hover.filter(|h| rect.contains(*h)) {
            let [u, v] = self.from_screen(hp);
            let mut text = format!("{nu} = {}   {nv} = {}", self.fmt(0, u), self.fmt(1, v));
            if let Some(more) = extra.and_then(|f| f(u, v)) {
                text += &format!("\n{more}");
            }
            let galley = p.layout(text, FontId::monospace(12.0), t.text, 400.0);
            let size = galley.size();
            let mut at = hp + vec2(16.0, 16.0);
            if at.x + size.x + 8.0 > rect.right() {
                at.x = hp.x - 16.0 - size.x;
            }
            if at.y + size.y + 6.0 > rect.bottom() {
                at.y = hp.y - 16.0 - size.y;
            }
            let r = Rect::from_min_size(at, size).expand2(vec2(5.0, 3.0));
            p.rect_filled(r, CornerRadius::same(5), t.overlay);
            p.rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, t.overlay_border), egui::StrokeKind::Inside);
            p.galley(at, galley, t.text);
            p.circle_stroke(hp, 3.0, Stroke::new(1.0, t.text));
        }
    }
}

/// Outlines of several surfaces, traced for a window a little larger than
/// the view and kept while panning and zooming stay within it.
#[derive(Default)]
pub struct OutlineCache {
    key: Option<(u64, [usize; 3], [u64; 6])>,
    window: [(f64, f64); 2],
    cell: [f64; 2],
    /// Polylines in data coordinates and their colour.
    pub parts: Vec<(Lines, Color32)>,
}

impl OutlineCache {
    /// Trace again when the geometry (`version`), the view or the zoom
    /// changed, or the view left the traced window. `trace` gets the plane
    /// to trace and returns each surface's segments with its colour.
    pub fn update(&mut self, version: u64, a: &Axes2, trace: impl FnOnce(&ViewPlane) -> Vec<(Vec<Seg>, Color32)>) {
        let upp = a.units_per_point();
        let cell = [upp[0] * CELL_POINTS, upp[1] * CELL_POINTS];
        let vis = a.visible();
        let b = |d: DVec3| d.to_array().map(f64::to_bits);
        let [l0, l1, l2] = b(a.lo);
        let [h0, h1, h2] = b(a.hi);
        let key = (version, a.axes, [l0, l1, l2, h0, h1, h2]);
        let inside = |r: (f64, f64), w: (f64, f64)| r.0 >= w.0 && r.1 <= w.1;
        let zoom_ok = (0..2).all(|k| (0.7..1.4).contains(&(cell[k] / self.cell[k])));
        if self.key == Some(key) && zoom_ok && inside(vis[0], self.window[0]) && inside(vis[1], self.window[1]) {
            return;
        }
        let grow = |r: (f64, f64)| {
            let m = (r.1 - r.0) * 0.5;
            (r.0 - m, r.1 + m)
        };
        let window = [grow(vis[0]), grow(vis[1])];
        let plane = ViewPlane { axes: a.axes, lo: a.lo, hi: a.hi, window, cell };
        self.parts = trace(&plane).into_iter().map(|(segs, c)| (crate::outline::polylines(&segs), c)).collect();
        (self.key, self.window, self.cell) = (Some(key), window, cell);
    }

    pub fn paint(&self, ui: &Ui, a: &Axes2, width: f32) {
        let p = ui.painter_at(a.rect);
        for (lines, color) in &self.parts {
            let stroke = Stroke::new(width, *color);
            for l in lines {
                let pts: Vec<Pos2> = l.iter().map(|q| a.to_screen(q[0], q[1])).collect();
                p.add(egui::Shape::line(pts, stroke));
            }
        }
    }
}
