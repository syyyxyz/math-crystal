//! egui application shell and the Calculus workspace UI.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2 as EVec2};
use glam::{Vec2, Vec3};

use crate::calc::{PointDerivs, Source, Surface, classify};
use crate::calc_view::{self, Coloring, LabelKind, Settings, Theme};
use crate::render::gpu::{Gpu3d, View};
use crate::render::scene::{Camera, Frame3, Projector, Scene};

pub const DEFAULT_FUNCTION: &str = "0.15*x^2 - 0.08*y^2 + 0.12*sin(x*y)";

const EXAMPLES: &[(&str, &str)] = &[
    ("Default (legacy demo)", DEFAULT_FUNCTION),
    ("Paraboloid", "x^2 + y^2"),
    ("Saddle", "x^2 - y^2"),
    ("Monkey saddle", "x^3 - 3*x*y^2"),
    ("Ripple", "sin(3*sqrt(x^2 + y^2)) / (1 + x^2 + y^2)"),
    ("Gaussian bump", "exp(-(x^2 + y^2))"),
    ("Peaks", "3(1-x)^2 exp(-x^2-(y+1)^2) - 10(x/5 - x^3 - y^5) exp(-x^2-y^2) - exp(-(x+1)^2-y^2)/3"),
    ("Mixed partials differ at 0", "if(x^2 + y^2 == 0, 0, x*y*(x^2 - y^2)/(x^2 + y^2))"),
    ("Cone (not differentiable at 0)", "sqrt(x^2 + y^2)"),
    ("Half domain", "sqrt(x) * cos(y)"),
    ("Egg crate", "sin(2x) * cos(2y) / 2"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Calculus,
}

pub struct MathCrystal {
    workspace: Workspace,
    calc: CalcWorkspace,
    dark_pending: bool,
}

impl MathCrystal {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Light);
        cc.egui_ctx.set_zoom_factor(1.1);
        let mut style = (*cc.egui_ctx.global_style()).clone();
        style.spacing.item_spacing = EVec2::new(8.0, 6.0);
        style.spacing.slider_width = 150.0;
        cc.egui_ctx.set_global_style(style);
        Self {
            workspace: Workspace::Calculus,
            calc: CalcWorkspace::new(DEFAULT_FUNCTION),
            dark_pending: false,
        }
    }
}

impl MathCrystal {
    pub fn calc_mut(&mut self) -> &mut CalcWorkspace {
        &mut self.calc
    }

    /// Switch to the dark theme (also used by the preview example).
    pub fn set_dark(&mut self) {
        self.calc.settings.theme = Theme::Dark;
        self.dark_pending = true;
    }
}

impl eframe::App for MathCrystal {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.dark_pending) {
            ui.ctx().set_theme(egui::Theme::Dark);
        }
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Math-crystal").strong());
                ui.separator();
                ui.selectable_value(&mut self.workspace, Workspace::Calculus, "Multivariable calculus");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dark = self.calc.settings.theme == Theme::Dark;
                    if ui.button(if dark { "Light theme" } else { "Dark theme" }).clicked() {
                        let t = if dark { Theme::Light } else { Theme::Dark };
                        self.calc.settings.theme = t;
                        ui.ctx().set_theme(if dark { egui::Theme::Light } else { egui::Theme::Dark });
                    }
                });
            });
        });
        match self.workspace {
            Workspace::Calculus => self.calc.ui(ui, frame),
        }
    }
}

/// GPU resources of one 3D canvas.
struct Canvas {
    gpu: Gpu3d,
    texture: Option<egui::TextureId>,
    /// What was rendered last: skip re-rendering when nothing changed.
    last: Option<(View, [u32; 2], u64)>,
}

pub struct CalcWorkspace {
    src: String,
    surface: Option<Surface>,
    error: Option<String>,
    settings: Settings,
    z_auto: bool,
    camera: Camera,
    canvas: Option<Canvas>,
    // Cached build results.
    built_for: Option<(String, Settings)>,
    scene: Scene,
    frame: Frame3,
    derivs: Option<PointDerivs>,
    z_samples: Vec<f64>,
    scene_version: u64,
    /// (src, a, b) the model sliders were last synced to.
    model_synced_for: Option<(String, f64, f64)>,
    show_formulas: bool,
}

impl CalcWorkspace {
    pub fn new(src: &str) -> Self {
        let mut s = Self {
            src: src.to_string(),
            surface: None,
            error: None,
            settings: Settings::default(),
            z_auto: true,
            camera: Camera::default(),
            canvas: None,
            built_for: None,
            scene: Scene::default(),
            frame: Frame3::new(glam::DVec3::splat(-1.0), glam::DVec3::splat(1.0), false),
            derivs: None,
            z_samples: vec![],
            scene_version: 0,
            model_synced_for: None,
            show_formulas: false,
        };
        s.recompile();
        s.update_auto_z();
        s
    }

    pub fn set_function(&mut self, src: &str) {
        self.src = src.to_string();
        self.recompile();
        self.update_auto_z();
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    fn recompile(&mut self) {
        match Surface::new(self.src.trim()) {
            Ok(surf) => {
                self.surface = Some(surf);
                self.error = None;
            }
            Err(e) => {
                // Keep showing the last valid surface while typing.
                self.error = Some(e.to_string());
            }
        }
    }

    fn update_auto_z(&mut self) {
        if !self.z_auto {
            return;
        }
        if let Some(surf) = &self.surface {
            let s = &self.settings;
            let z = surf.sample(s.xr, s.yr, 65);
            if let Some(zr) = calc_view::auto_z(&z) {
                self.settings.zr = zr;
            }
        }
    }

    /// Re-sync the model curvatures to the true second derivatives when the
    /// function or the point changes.
    fn sync_model(&mut self, force: bool) {
        let Some(surf) = &self.surface else { return };
        let key = (surf.source.clone(), self.settings.a, self.settings.b);
        if !force && self.model_synced_for.as_ref() == Some(&key) {
            return;
        }
        let d = surf.derivs(self.settings.a, self.settings.b);
        let s = &mut self.settings;
        let pick = |v: f64| if v.is_finite() { v } else { 0.0 };
        s.model_fxx = pick(d.fxx.v);
        s.model_fyy = pick(d.fyy.v);
        // The model uses one cross term; average the two orders.
        s.model_fxy = pick(if d.fxy.ok() && d.fyx.ok() { 0.5 * (d.fxy.v + d.fyx.v) } else { d.fxy.v });
        self.model_synced_for = Some(key);
    }

    fn rebuild_if_needed(&mut self) {
        let src = self.surface.as_ref().map(|s| s.source.clone()).unwrap_or_default();
        if self.built_for.as_ref().is_some_and(|(s, st)| *s == src && *st == self.settings) {
            return;
        }
        let b = calc_view::build(self.surface.as_ref(), &self.settings);
        self.scene = b.scene;
        self.frame = b.frame;
        self.derivs = b.derivs;
        self.z_samples = b.z_samples;
        self.scene_version += 1;
        self.built_for = Some((src, self.settings.clone()));
    }

    pub fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        egui::Panel::left("calc_controls")
            .resizable(true)
            .default_size(360.0)
            .min_size(300.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    self.controls(ui);
                    ui.add_space(12.0);
                });
            });
        // Derived state must be current before the canvas draws.
        self.sync_model(false);
        self.rebuild_if_needed();
        egui::CentralPanel::default().show(ui, |ui| {
            self.toolbar(ui);
            self.canvas(ui, frame);
        });
    }

    fn controls(&mut self, ui: &mut Ui) {
        section(ui, "Function  z = f(x, y)");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut self.src)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .hint_text("e.g. sin(x) * cos(y)"),
        );
        if edit.changed() {
            let old = self.surface.as_ref().map(|s| s.source.clone());
            self.recompile();
            if self.surface.as_ref().map(|s| s.source.clone()) != old {
                self.update_auto_z();
            }
        }
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(Color32::from_rgb(0xd0, 0x30, 0x30)));
        }
        ui.horizontal(|ui| {
            ui.menu_button("Examples", |ui| {
                for (name, f) in EXAMPLES {
                    if ui.button(*name).on_hover_text(*f).clicked() {
                        self.src = f.to_string();
                        self.recompile();
                        self.update_auto_z();
                        ui.close();
                    }
                }
            });
            ui.menu_button("Syntax help", |ui| {
                ui.set_max_width(420.0);
                ui.label("Operators: + - * / ^ (or **), implicit products like 2x, 3(x+1), |x|");
                ui.label("Comparisons: < <= > >= == != and, or, not; chains like 0 < x < 1");
                ui.label("Piecewise: if(cond, a, b)");
                ui.label("Constants: pi, e, tau");
                ui.separator();
                let names: Vec<&str> = crate::expr::parse::FUNCTIONS.iter().map(|f| f.name).filter(|n| !matches!(*n, "re" | "im" | "conj" | "arg")).collect();
                ui.label(format!("Functions: {}", names.join(", ")));
            });
        });

        ui.add_space(8.0);
        section(ui, "Point P(a, b)");
        let s = &mut self.settings;
        let (xr, yr) = (s.xr, s.yr);
        egui::Grid::new("point").num_columns(2).show(ui, |ui| {
            ui.label("a");
            ui.add(egui::Slider::new(&mut s.a, xr.0..=xr.1).max_decimals(3).clamping(egui::SliderClamping::Never));
            ui.end_row();
            ui.label("b");
            ui.add(egui::Slider::new(&mut s.b, yr.0..=yr.1).max_decimals(3).clamping(egui::SliderClamping::Never));
            ui.end_row();
        });
        ui.label(RichText::new("Shift+click the surface to move P.").small().weak());

        ui.add_space(8.0);
        section(ui, "Derivatives at P");
        self.readout(ui);

        ui.add_space(8.0);
        section(ui, "Taylor model at P");
        let s = &mut self.settings;
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.show_model, "Show quadratic model");
            egui::ComboBox::from_id_salt("model_col")
                .selected_text(coloring_name(s.model_coloring))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.model_coloring, Coloring::Solid, coloring_name(Coloring::Solid));
                    ui.selectable_value(&mut s.model_coloring, Coloring::Height, coloring_name(Coloring::Height));
                });
        });
        ui.label(RichText::new("Curvatures used by the model (drag to explore):").small().weak());
        egui::Grid::new("model").num_columns(2).show(ui, |ui| {
            for (name, v) in [("f_xx", &mut s.model_fxx), ("f_yy", &mut s.model_fyy), ("f_xy = f_yx", &mut s.model_fxy)] {
                ui.label(name);
                let lim = (v.abs() * 1.5).max(8.0);
                ui.add(egui::Slider::new(v, -lim..=lim).max_decimals(3).clamping(egui::SliderClamping::Never));
                ui.end_row();
            }
        });
        if ui.button("Reset model to f").clicked() {
            self.sync_model(true);
        }

        ui.add_space(8.0);
        section(ui, "Display");
        let s = &mut self.settings;
        ui.horizontal(|ui| {
            ui.checkbox(&mut s.show_surface, "Surface");
            egui::ComboBox::from_id_salt("surf_col")
                .selected_text(coloring_name(s.surface_coloring))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.surface_coloring, Coloring::Height, coloring_name(Coloring::Height));
                    ui.selectable_value(&mut s.surface_coloring, Coloring::Solid, coloring_name(Coloring::Solid));
                });
        });
        ui.checkbox(&mut s.wireframe, "Mesh lines");
        ui.checkbox(&mut s.show_point, "Contact point P");
        ui.checkbox(&mut s.show_sections, "Section curves through P");
        ui.checkbox(&mut s.show_gradient, "Steepest-ascent arrow (gradient)");
        ui.horizontal(|ui| {
            ui.label("Resolution");
            ui.add(egui::Slider::new(&mut s.resolution, 33..=401).logarithmic(true));
        });

        ui.add_space(8.0);
        section(ui, "Domain");
        let mut domain_changed = false;
        egui::Grid::new("domain").num_columns(3).show(ui, |ui| {
            for (name, r) in [("x", &mut s.xr), ("y", &mut s.yr)] {
                ui.label(name);
                domain_changed |= range_edit(ui, r);
                ui.end_row();
            }
            ui.label("z");
            ui.add_enabled_ui(!self.z_auto, |ui| {
                range_edit(ui, &mut s.zr);
            });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.z_auto, "Auto z range").changed() {
                domain_changed = true;
            }
            ui.checkbox(&mut self.settings.equal_aspect, "True proportions");
        });
        if ui.button("Reset domain").clicked() {
            self.settings.xr = (-2.0, 2.0);
            self.settings.yr = (-2.0, 2.0);
            self.z_auto = true;
            domain_changed = true;
        }
        if domain_changed {
            self.update_auto_z();
        }

        ui.add_space(8.0);
        egui::CollapsingHeader::new(RichText::new("Symbolic derivatives").strong())
            .default_open(self.show_formulas)
            .show(ui, |ui| {
                if let Some(surf) = &self.surface {
                    for (k, name) in [(1, "f_x"), (2, "f_y"), (3, "f_xx"), (4, "f_yy"), (5, "f_xy"), (6, "f_yx")] {
                        let text = surf.formula(k);
                        let shown = if text.len() > 600 { format!("{}…", &text[..600]) } else { text };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(format!("{name} =")).strong());
                            ui.label(RichText::new(shown).monospace());
                        });
                    }
                }
            });
    }

    fn readout(&self, ui: &mut Ui) {
        let Some(d) = &self.derivs else {
            ui.label("—");
            return;
        };
        if !d.f.is_finite() {
            ui.label(RichText::new("f is undefined at P.").color(Color32::from_rgb(0xd0, 0x30, 0x30)));
            return;
        }
        let mut any_limit = false;
        egui::Grid::new("readout").num_columns(3).striped(true).show(ui, |ui| {
            let mut row = |ui: &mut Ui, name: &str, v: f64, src: Option<Source>| {
                ui.label(name);
                ui.label(RichText::new(fmt_val(v)).monospace());
                match src {
                    Some(Source::Limit) => {
                        any_limit = true;
                        ui.label(RichText::new("limit").small().color(Color32::from_rgb(0xc0, 0x80, 0x10)))
                            .on_hover_text("The formula is undefined or disagrees at P; value from the limit definition.");
                    }
                    _ => {
                        ui.label("");
                    }
                }
                ui.end_row();
            };
            row(ui, "f", d.f, None);
            row(ui, "f_x", d.fx.v, Some(d.fx.src));
            row(ui, "f_y", d.fy.v, Some(d.fy.src));
            row(ui, "|grad f|", d.fx.v.hypot(d.fy.v), None);
            row(ui, "f_xx", d.fxx.v, Some(d.fxx.src));
            row(ui, "f_yy", d.fyy.v, Some(d.fyy.src));
            row(ui, "f_xy  (x, then y)", d.fxy.v, Some(d.fxy.src));
            row(ui, "f_yx  (y, then x)", d.fyx.v, Some(d.fyx.src));
        });
        match d.mixed_equal() {
            Some(true) => ui.label(RichText::new("f_xy = f_yx (Schwarz/Clairaut holds)").color(Color32::from_rgb(0x2b, 0x8a, 0x3e))),
            Some(false) => ui.label(
                RichText::new("f_xy != f_yx: mixed partials are not continuous at P").color(Color32::from_rgb(0xd0, 0x30, 0x30)),
            ),
            None => ui.label(RichText::new("Mixed partials cannot be determined at P").weak()),
        };
        if let Some(kind) = classify(d) {
            ui.label(RichText::new(format!("Critical point: {kind}")).strong());
        }
        if any_limit {
            ui.label(RichText::new("Values tagged 'limit' come from difference quotients.").small().weak());
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        let cam = &mut self.camera;
        ui.horizontal_wrapped(|ui| {
            if ui.button("3D").on_hover_text("Default 3D view").clicked() {
                let keep = (cam.ortho, cam.fov_deg);
                *cam = Camera::default();
                (cam.ortho, cam.fov_deg) = keep;
            }
            if ui.button("Front (xz)").on_hover_text("Look along +y").clicked() {
                cam.yaw = -std::f32::consts::FRAC_PI_2;
                cam.pitch = 0.0;
            }
            if ui.button("Side (yz)").on_hover_text("Look along -x").clicked() {
                cam.yaw = 0.0;
                cam.pitch = 0.0;
            }
            if ui.button("Top (xy)").clicked() {
                cam.yaw = -std::f32::consts::FRAC_PI_2;
                cam.pitch = 1.5705;
            }
            ui.separator();
            let mut persp = !cam.ortho;
            if ui.checkbox(&mut persp, "Perspective").changed() {
                cam.ortho = !persp;
            }
            ui.add_enabled(persp, egui::Slider::new(&mut cam.fov_deg, 10.0..=80.0).suffix("° FOV").max_decimals(0));
        });
        ui.horizontal_wrapped(|ui| {
            legend_check(ui, &mut s.show_tangent_x, "Tangent x", calc_view::C_TAN_X);
            legend_check(ui, &mut s.show_tangent_y, "Tangent y", calc_view::C_TAN_Y);
            legend_check(ui, &mut s.show_plane, "Tangent plane", calc_view::C_PLANE);
            ui.separator();
            ui.checkbox(&mut s.show_grid, "Grid");
            ui.checkbox(&mut s.show_ticks, "Axis values");
        });
        ui.add_space(4.0);
    }

    fn canvas(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let size_px = [((rect.width() * ppp).round() as u32).max(1), ((rect.height() * ppp).round() as u32).max(1)];

        // Interaction.
        if resp.dragged_by(egui::PointerButton::Primary) && !ui.input(|i| i.modifiers.shift) {
            let d = resp.drag_delta();
            self.camera.orbit(Vec2::new(d.x, d.y));
        }
        if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            let d = resp.drag_delta();
            self.camera.pan(Vec2::new(d.x, d.y), rect.height());
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.zoom((-scroll * 0.0025).exp());
            }
        }
        if resp.double_clicked() {
            let keep = (self.camera.ortho, self.camera.fov_deg);
            self.camera = Camera::default();
            (self.camera.ortho, self.camera.fov_deg) = keep;
        }
        let aspect = rect.width().max(1.0) / rect.height().max(1.0);
        let proj = Projector {
            vp: self.camera.view_proj(aspect),
            size: Vec2::new(rect.width(), rect.height()),
        };
        if resp.clicked()
            && ui.input(|i| i.modifiers.shift)
            && let Some(pos) = resp.interact_pointer_pos()
        {
            self.pick_point(&proj, Vec2::new(pos.x - rect.min.x, pos.y - rect.min.y));
            self.rebuild_if_needed();
        }

        let (deco, labels) = calc_view::decorations(&self.settings, &self.frame, &self.camera, &proj);
        let pal = self.settings.theme.palette();
        let view = View {
            camera: self.camera,
            clip_half: self.frame.half,
            background: pal.background,
            wire: pal.wire,
        };

        let Some(rs) = frame.wgpu_render_state() else {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "wgpu is not available", FontId::proportional(18.0), Color32::RED);
            return;
        };
        let canvas = self.canvas.get_or_insert_with(|| Canvas {
            gpu: Gpu3d::new(&rs.device),
            texture: None,
            last: None,
        });
        let key = (view, size_px, self.scene_version);
        if canvas.last.as_ref() != Some(&key) || canvas.texture.is_none() {
            if canvas.last.as_ref().map(|l| l.2) != Some(self.scene_version) {
                canvas.gpu.set_meshes(&rs.device, &self.scene.meshes);
            }
            let mut lines = self.scene.lines.clone();
            lines.extend(deco);
            canvas.gpu.set_lines(&rs.device, &lines);
            let recreated = canvas.gpu.render(&rs.device, &rs.queue, size_px, &view);
            let tex_view = canvas.gpu.view().unwrap();
            let mut renderer = rs.renderer.write();
            match canvas.texture {
                None => canvas.texture = Some(renderer.register_native_texture(&rs.device, tex_view, wgpu::FilterMode::Linear)),
                Some(id) if recreated => renderer.update_egui_texture_from_wgpu_texture(&rs.device, tex_view, wgpu::FilterMode::Linear, id),
                _ => {}
            }
            canvas.last = Some(key);
        }
        let painter = ui.painter_at(rect);
        if let Some(id) = canvas.texture {
            painter.image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        self.overlay(&painter, rect, &proj, labels);
    }

    /// Shift+click: move P to the surface sample nearest the cursor.
    fn pick_point(&mut self, proj: &Projector, at: Vec2) {
        let s = &self.settings;
        let n = (self.z_samples.len() as f64).sqrt().round() as usize;
        if n < 2 {
            return;
        }
        let mut best: Option<(f32, f64, f64)> = None;
        for (k, &z) in self.z_samples.iter().enumerate() {
            if !z.is_finite() {
                continue;
            }
            let x = crate::calc::lerp(s.xr, (k % n) as f64 / (n - 1) as f64);
            let y = crate::calc::lerp(s.yr, (k / n) as f64 / (n - 1) as f64);
            let w = self.frame.map_xyz(x, y, z);
            if w.abs().cmpgt(self.frame.half * 1.001).any() {
                continue;
            }
            if let Some(p) = proj.project(w) {
                let d = p.distance_squared(at);
                // Prefer points nearer the camera when several overlap.
                let depth = w.distance(self.camera.eye());
                let score = d + depth * 4.0;
                if d < 40.0 * 40.0 && best.is_none_or(|b| score < b.0) {
                    best = Some((score, x, y));
                }
            }
        }
        if let Some((_, x, y)) = best {
            let (xr, yr) = (self.settings.xr, self.settings.yr);
            self.settings.a = round_to(x, xr);
            self.settings.b = round_to(y, yr);
        }
    }

    fn overlay(&self, painter: &egui::Painter, rect: Rect, proj: &Projector, labels: Vec<calc_view::Label>) {
        let pal = self.settings.theme.palette();
        let text = Color32::from_rgb(pal.text[0], pal.text[1], pal.text[2]);
        let muted = Color32::from_rgb(pal.muted_text[0], pal.muted_text[1], pal.muted_text[2]);
        let to_screen = |w: Vec3| proj.project(w).map(|p| Pos2::new(rect.min.x + p.x, rect.min.y + p.y));
        let mut taken: Vec<Rect> = Vec::new();
        // Axis names first so tick labels give way to them.
        let mut labels = labels;
        labels.sort_by_key(|l| (l.kind != LabelKind::Axis) as u8);
        for l in labels {
            let Some(p) = to_screen(l.world) else { continue };
            let (font, color) = match l.kind {
                LabelKind::Axis => (FontId::proportional(17.0), text),
                _ => (FontId::proportional(12.5), muted),
            };
            let galley = painter.layout_no_wrap(l.text, font, color);
            let size = galley.size();
            let center = p + egui::vec2(l.push.x, l.push.y) * (size.x * 0.5 + 2.0).min(14.0);
            let r = Rect::from_center_size(center, size).expand(1.5);
            if !rect.contains_rect(r) || taken.iter().any(|t| t.intersects(r)) {
                continue;
            }
            taken.push(r);
            painter.galley(r.min + egui::vec2(1.5, 1.5), galley, color);
        }
        // Point label.
        if let Some(d) = &self.derivs
            && self.settings.show_point
            && d.f.is_finite()
        {
            let s = &self.settings;
            let w = self.frame.map_xyz(s.a, s.b, d.f);
            if w.abs().cmple(self.frame.half * 1.001).all()
                && let Some(p) = to_screen(w)
            {
                painter.text(
                    p + egui::vec2(10.0, -10.0),
                    Align2::LEFT_BOTTOM,
                    format!("P({}, {})", fmt_short(s.a), fmt_short(s.b)),
                    FontId::proportional(14.0),
                    Color32::from_rgb(0xe0, 0x31, 0x31),
                );
            }
        }
        // Slope readout (top-left) like the legacy floating panel.
        if let Some(d) = &self.derivs
            && d.first_ok()
        {
            let lines = [
                (format!("f_x(P) = {}", fmt_val(d.fx.v)), calc_view::C_TAN_X),
                (format!("f_y(P) = {}", fmt_val(d.fy.v)), calc_view::C_TAN_Y),
            ];
            let mut y = rect.min.y + 10.0;
            let bg = if self.settings.theme == Theme::Light { Color32::from_white_alpha(215) } else { Color32::from_black_alpha(170) };
            let galleys: Vec<_> = lines
                .into_iter()
                .map(|(t, c)| painter.layout_no_wrap(t, FontId::monospace(14.0), hex32(c)))
                .collect();
            let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
            let h: f32 = galleys.iter().map(|g| g.size().y + 2.0).sum();
            painter.rect(
                Rect::from_min_size(Pos2::new(rect.min.x + 8.0, y - 4.0), egui::vec2(w + 16.0, h + 8.0)),
                6.0,
                bg,
                Stroke::NONE,
                egui::StrokeKind::Inside,
            );
            for g in galleys {
                let gh = g.size().y;
                painter.galley(Pos2::new(rect.min.x + 16.0, y), g, text);
                y += gh + 2.0;
            }
        }
        painter.text(
            rect.left_bottom() + egui::vec2(10.0, -8.0),
            Align2::LEFT_BOTTOM,
            "drag: rotate · right-drag: pan · scroll: zoom · double-click: reset · shift+click: move P",
            FontId::proportional(11.5),
            muted,
        );
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).strong().size(15.0));
}

fn coloring_name(c: Coloring) -> &'static str {
    match c {
        Coloring::Height => "Height colours",
        Coloring::Solid => "Solid (translucent)",
    }
}

fn hex32(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn legend_check(ui: &mut Ui, v: &mut bool, label: &str, color: u32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 4.0), Sense::hover());
    ui.painter().rect_filled(r, 1.0, hex32(color));
    ui.checkbox(v, label);
}

/// Edit a (lo, hi) range; returns true when changed. Keeps lo < hi.
fn range_edit(ui: &mut Ui, r: &mut (f64, f64)) -> bool {
    let speed = ((r.1 - r.0).abs() * 0.005).max(1e-4);
    let mut changed = ui.add(egui::DragValue::new(&mut r.0).speed(speed).max_decimals(4)).changed();
    changed |= ui.add(egui::DragValue::new(&mut r.1).speed(speed).max_decimals(4)).changed();
    if r.1 <= r.0 {
        r.1 = r.0 + 1e-3;
    }
    changed
}

fn round_to(v: f64, r: (f64, f64)) -> f64 {
    let step = 10f64.powf(((r.1 - r.0) / 2000.0).log10().ceil());
    (v / step).round() * step
}

pub fn fmt_val(v: f64) -> String {
    if !v.is_finite() {
        return "undefined".into();
    }
    if v != 0.0 && (v.abs() >= 1e6 || v.abs() < 1e-4) {
        return format!("{v:.4e}");
    }
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn fmt_short(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}
