//! Shared 3D canvas: camera interaction, offscreen wgpu rendering shown as
//! an egui texture, view presets and text labels.

use egui::{Align2, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2 as EVec2};
use glam::{Vec2, Vec3};

use crate::calc_view::{Label, LabelKind};
use crate::i18n::tr;
use crate::render::gpu::{Gpu3d, View};
use crate::render::scene::{Camera, LineInstance, Projector, Scene};
use crate::ui::theme::{icon, tokens, with_icon};
use crate::ui::widgets::segmented;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewPreset {
    Iso,
    Front,
    Side,
    Top,
    Custom,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Projection {
    Ortho,
    Persp,
}

/// GPU resources of one canvas.
struct Gpu {
    gpu: Gpu3d,
    texture: Option<egui::TextureId>,
    /// What was rendered last: skip re-rendering when nothing changed.
    last: Option<(View, [u32; 2], u64)>,
    adapter: String,
}

pub struct View3d {
    pub camera: Camera,
    pub preset: ViewPreset,
    gpu: Option<Gpu>,
}

impl Default for View3d {
    fn default() -> Self {
        Self { camera: Camera::default(), preset: ViewPreset::Iso, gpu: None }
    }
}

impl View3d {
    pub fn adapter(&self) -> String {
        self.gpu.as_ref().map(|g| g.adapter.clone()).unwrap_or_default()
    }

    /// Allocate the canvas rect and apply orbit / pan / zoom / reset.
    /// Primary drag with Shift held is left to the caller.
    pub fn allocate(&mut self, ui: &mut Ui, rect: Rect) -> Response {
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let shift = ui.input(|i| i.modifiers.shift);
        if resp.dragged_by(egui::PointerButton::Primary) && !shift {
            let d = resp.drag_delta();
            if d != EVec2::ZERO {
                self.camera.orbit(Vec2::new(d.x, d.y));
                self.preset = ViewPreset::Custom;
            }
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            self.camera.pan(Vec2::new(d.x, d.y), rect.height());
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.zoom((-scroll * 0.0012).exp().clamp(0.8, 1.25));
            }
        }
        if resp.double_clicked() {
            self.apply_preset(ViewPreset::Iso);
        }
        resp
    }

    pub fn projector(&self, rect: Rect) -> Projector {
        let aspect = rect.width().max(1.0) / rect.height().max(1.0);
        Projector { vp: self.camera.view_proj(aspect), size: Vec2::new(rect.width(), rect.height()) }
    }

    /// Render `scene` (plus camera-dependent `extra` lines) into `rect`.
    /// Returns false when wgpu is unavailable.
    #[allow(clippy::too_many_arguments)]
    pub fn render(&mut self, ui: &mut Ui, frame: &mut eframe::Frame, rect: Rect, scene: &Scene, version: u64, extra: Vec<LineInstance>, clip_half: Vec3, background: [f32; 4], wire: [f32; 3]) -> bool {
        let t = tokens(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let size_px = [((rect.width() * ppp).round() as u32).max(1), ((rect.height() * ppp).round() as u32).max(1)];
        let view = View { camera: self.camera, clip_half, background, wire };
        let Some(rs) = frame.wgpu_render_state() else {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, tr("wgpu is not available", "wgpu 不可用"), FontId::proportional(18.0), t.error);
            return false;
        };
        let g = self.gpu.get_or_insert_with(|| Gpu { gpu: Gpu3d::new(&rs.device), texture: None, last: None, adapter: rs.adapter.get_info().name });
        let key = (view, size_px, version);
        if g.last.as_ref() != Some(&key) || g.texture.is_none() {
            if g.last.as_ref().map(|l| l.2) != Some(version) {
                g.gpu.set_meshes(&rs.device, &scene.meshes);
            }
            let mut lines = scene.lines.clone();
            lines.extend(extra);
            g.gpu.set_lines(&rs.device, &lines);
            let recreated = g.gpu.render(&rs.device, &rs.queue, size_px, &view);
            let tex_view = g.gpu.view().unwrap();
            let mut renderer = rs.renderer.write();
            match g.texture {
                None => g.texture = Some(renderer.register_native_texture(&rs.device, tex_view, wgpu::FilterMode::Linear)),
                Some(id) if recreated => renderer.update_egui_texture_from_wgpu_texture(&rs.device, tex_view, wgpu::FilterMode::Linear, id),
                _ => {}
            }
            g.last = Some(key);
        }
        if let Some(id) = g.texture {
            egui::Image::new(egui::load::SizedTexture::new(id, rect.size())).corner_radius(12).paint_at(ui, rect);
        }
        ui.painter().rect_stroke(rect, 12, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        true
    }

    pub fn apply_preset(&mut self, p: ViewPreset) {
        let keep = (self.camera.ortho, self.camera.fov_deg);
        let cam = &mut self.camera;
        match p {
            ViewPreset::Iso | ViewPreset::Custom => {
                *cam = Camera::default();
                (cam.ortho, cam.fov_deg) = keep;
            }
            ViewPreset::Front => {
                cam.yaw = -std::f32::consts::FRAC_PI_2;
                cam.pitch = 0.0;
            }
            ViewPreset::Side => {
                cam.yaw = 0.0;
                cam.pitch = 0.0;
            }
            ViewPreset::Top => {
                cam.yaw = -std::f32::consts::FRAC_PI_2;
                cam.pitch = 1.5705;
            }
        }
        self.preset = if p == ViewPreset::Custom { ViewPreset::Iso } else { p };
    }

    /// Field of view (perspective only) and projection; for a right-to-left row.
    pub fn projection_controls(&mut self, ui: &mut Ui) {
        if !self.camera.ortho {
            ui.add(egui::Slider::new(&mut self.camera.fov_deg, 10.0..=80.0).suffix("°").max_decimals(0)).on_hover_text(tr("Field of view", "视场角"));
        }
        let mut proj = if self.camera.ortho { Projection::Ortho } else { Projection::Persp };
        if segmented(
            ui,
            &mut proj,
            &[(Projection::Ortho, tr("Ortho", "正交"), tr("Orthographic projection", "正交投影")), (Projection::Persp, tr("Persp", "透视"), tr("Perspective projection", "透视投影"))],
        ) {
            self.camera.ortho = proj == Projection::Ortho;
        }
    }

    /// 3D / Front / Side / Top presets. `names` are the horizontal axis names.
    pub fn preset_controls(&mut self, ui: &mut Ui, names: [&str; 3]) {
        let [x, y, z] = names;
        let front = format!("{} {x}-{z}", tr("Look along the second axis: the plane", "沿第二个坐标轴看：平面"));
        let side = format!("{} {y}-{z}", tr("Look along the first axis: the plane", "沿第一个坐标轴看：平面"));
        let top = format!("{} {x}-{y}", tr("Look down: the plane", "向下看：平面"));
        let mut p = self.preset;
        if segmented(
            ui,
            &mut p,
            &[
                (ViewPreset::Iso, &with_icon(icon::CUBE, "3D"), tr("Default 3D view (double-click the canvas)", "默认 3D 视角（双击画布）")),
                (ViewPreset::Front, tr("Front", "正视"), &front),
                (ViewPreset::Side, tr("Side", "侧视"), &side),
                (ViewPreset::Top, tr("Top", "俯视"), &top),
            ],
        ) {
            self.apply_preset(p);
        }
    }
}

/// Paint axis names and tick labels, skipping overlapping ones.
pub fn paint_labels(painter: &egui::Painter, rect: Rect, proj: &Projector, labels: Vec<Label>) {
    let t = tokens(painter.ctx());
    let to_screen = |w: Vec3| proj.project(w).map(|p| Pos2::new(rect.min.x + p.x, rect.min.y + p.y));
    let mut taken: Vec<Rect> = Vec::new();
    let mut labels = labels;
    // Axis names first so tick labels give way to them.
    labels.sort_by_key(|l| (l.kind != LabelKind::Axis) as u8);
    for l in labels {
        let Some(p) = to_screen(l.world) else { continue };
        let (font, color) = match l.kind {
            LabelKind::Axis => (FontId::new(17.0, egui::FontFamily::Proportional), t.text),
            _ => (FontId::monospace(11.5), t.muted),
        };
        let galley = painter.layout_no_wrap(l.text, font, color);
        let size = galley.size();
        let center = p + egui::vec2(l.push.x, l.push.y) * (size.x * 0.5 + 2.0).min(14.0);
        let r = Rect::from_center_size(center, size).expand(1.5);
        if !rect.shrink(4.0).contains_rect(r) || taken.iter().any(|q| q.intersects(r)) {
            continue;
        }
        taken.push(r);
        painter.galley(r.min + egui::vec2(1.5, 1.5), galley, color);
    }
}
