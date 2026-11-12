//! A 2D view of the complex plane with square pixels: coordinate mapping,
//! pan / zoom and axes.

use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Stroke, Ui, pos2};
use num_complex::Complex64 as C64;

use crate::render::scene::{tick_label, ticks};
use crate::ui::theme::Tokens;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Plane {
    pub center: C64,
    /// Half of the extent of the shorter side.
    pub half: f64,
}

impl Plane {
    pub fn new(center: C64, half: f64) -> Self {
        Self { center, half }
    }

    fn scale(&self, rect: Rect) -> f64 {
        self.half / (rect.width().min(rect.height()) as f64 / 2.0)
    }

    /// Visible (Re, Im) ranges for this rect.
    pub fn ranges(&self, rect: Rect) -> ((f64, f64), (f64, f64)) {
        let s = self.scale(rect);
        let (hw, hh) = (rect.width() as f64 / 2.0 * s, rect.height() as f64 / 2.0 * s);
        ((self.center.re - hw, self.center.re + hw), (self.center.im - hh, self.center.im + hh))
    }

    pub fn to_screen(&self, rect: Rect, z: C64) -> Pos2 {
        let s = self.scale(rect);
        let c = rect.center();
        pos2(c.x + ((z.re - self.center.re) / s) as f32, c.y - ((z.im - self.center.im) / s) as f32)
    }

    pub fn from_screen(&self, rect: Rect, p: Pos2) -> C64 {
        let s = self.scale(rect);
        let c = rect.center();
        C64::new(self.center.re + (p.x - c.x) as f64 * s, self.center.im - (p.y - c.y) as f64 * s)
    }

    /// Plane units per screen point.
    pub fn units_per_point(&self, rect: Rect) -> f64 {
        self.scale(rect)
    }

    /// Right / middle drag pans, scroll zooms about the cursor. Returns true
    /// when the view changed.
    pub fn pan_zoom(&mut self, ui: &Ui, resp: &Response, rect: Rect) -> bool {
        let mut changed = false;
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            if d != egui::Vec2::ZERO {
                let s = self.scale(rect);
                self.center -= C64::new(d.x as f64 * s, -d.y as f64 * s);
                changed = true;
            }
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0
                && let Some(p) = resp.hover_pos()
            {
                let k = (-scroll as f64 * 0.0015).exp().clamp(0.8, 1.25);
                let anchor = self.from_screen(rect, p);
                let new_half = (self.half * k).clamp(1e-6, 1e6);
                let k = new_half / self.half;
                self.center = anchor + (self.center - anchor) * k;
                self.half = new_half;
                changed = true;
            }
        }
        changed
    }

    /// Axis lines through 0 and tick labels along the bottom and left edges.
    pub fn draw_axes(&self, ui: &Ui, rect: Rect, t: &Tokens, names: (&str, &str), on_dark_image: bool) {
        let p = ui.painter_at(rect);
        let ((x0, x1), (y0, y1)) = self.ranges(rect);
        let line = if on_dark_image { Color32::from_white_alpha(110) } else { t.faint.gamma_multiply(0.7) };
        let text = if on_dark_image { Color32::WHITE } else { t.muted };
        let shadow = Color32::from_black_alpha(if on_dark_image { 150 } else { 0 });
        if x0 < 0.0 && x1 > 0.0 {
            let x = self.to_screen(rect, C64::new(0.0, 0.0)).x;
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, line));
        }
        if y0 < 0.0 && y1 > 0.0 {
            let y = self.to_screen(rect, C64::new(0.0, 0.0)).y;
            p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, line));
        }
        let font = FontId::monospace(11.0);
        let label = |pos: Pos2, align: Align2, s: String| {
            if shadow.a() > 0 {
                p.text(pos + egui::vec2(1.0, 1.0), align, &s, font.clone(), shadow);
            }
            p.text(pos, align, s, font.clone(), text);
        };
        let tx = ticks(x0, x1, ((rect.width() / 90.0) as usize).max(3));
        let sx = if tx.len() > 1 { tx[1] - tx[0] } else { 1.0 };
        for &v in &tx {
            let x = self.to_screen(rect, C64::new(v, 0.0)).x;
            if x < rect.left() + 30.0 || x > rect.right() - 12.0 {
                continue;
            }
            p.line_segment([pos2(x, rect.bottom() - 5.0), pos2(x, rect.bottom())], Stroke::new(1.0, text));
            label(pos2(x, rect.bottom() - 7.0), Align2::CENTER_BOTTOM, tick_label(v, sx));
        }
        let ty = ticks(y0, y1, ((rect.height() / 80.0) as usize).max(3));
        let sy = if ty.len() > 1 { ty[1] - ty[0] } else { 1.0 };
        for &v in &ty {
            let y = self.to_screen(rect, C64::new(0.0, v)).y;
            if y < rect.top() + 12.0 || y > rect.bottom() - 24.0 {
                continue;
            }
            p.line_segment([pos2(rect.left(), y), pos2(rect.left() + 5.0, y)], Stroke::new(1.0, text));
            label(pos2(rect.left() + 8.0, y), Align2::LEFT_CENTER, tick_label(v, sy));
        }
        label(pos2(rect.right() - 8.0, rect.bottom() - 22.0), Align2::RIGHT_BOTTOM, names.0.to_string());
        label(pos2(rect.left() + 8.0, rect.top() + 8.0), Align2::LEFT_TOP, names.1.to_string());
    }
}
