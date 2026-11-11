//! egui application shell and the Calculus workspace UI.

use std::time::Instant;

use egui::{Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2 as EVec2};
use glam::{Vec2, Vec3};

use crate::calc::{PointDerivs, Source, Surface, classify};
use crate::calc_view::{self, Coloring, LabelKind, Settings, Theme};
use crate::render::gpu::{Gpu3d, View, write_png};
use crate::render::scene::{Camera, Frame3, Projector, Scene};
use crate::ui::theme::{self, Tokens, icon, tokens, with_icon};
use crate::ui::widgets::{badge, caption, card, chip, overlay_frame, segmented};

pub const DEFAULT_FUNCTION: &str = "0.15*x^2 - 0.08*y^2 + 0.12*sin(x*y)";

const EXAMPLES: &[(&str, &str)] = &[
    ("Gentle saddle (default)", DEFAULT_FUNCTION),
    ("Paraboloid", "x^2 + y^2"),
    ("Saddle", "x^2 - y^2"),
    ("Monkey saddle", "x^3 - 3*x*y^2"),
    ("Ripple", "sin(3*sqrt(x^2 + y^2)) / (1 + x^2 + y^2)"),
    ("Gaussian bump", "exp(-(x^2 + y^2))"),
    ("Peaks", "3(1-x)^2 exp(-x^2-(y+1)^2) - 10(x/5 - x^3 - y^5) exp(-x^2-y^2) - exp(-(x+1)^2-y^2)/3"),
    ("Mixed partials differ at 0", "if(x^2 + y^2 == 0, 0, x*y*(x^2 - y^2)/(x^2 + y^2))"),
    ("Cone (no derivative at 0)", "sqrt(x^2 + y^2)"),
    ("Half domain", "sqrt(x) * cos(y)"),
    ("Egg crate", "sin(2x) * cos(2y) / 2"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Calculus,
    Complex,
    Spectrum,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ViewPreset {
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

pub struct MathCrystal {
    workspace: Workspace,
    calc: CalcWorkspace,
    logo: Option<egui::TextureHandle>,
    show_help: bool,
}

impl MathCrystal {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        cc.egui_ctx.set_theme(egui::Theme::Light);
        let (w, h, rgba) = crate::ui::icon_rgba();
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
            egui::TextureOptions::LINEAR,
        );
        Self {
            workspace: Workspace::Calculus,
            calc: CalcWorkspace::new(DEFAULT_FUNCTION),
            logo: Some(logo),
            show_help: false,
        }
    }

    pub fn calc_mut(&mut self) -> &mut CalcWorkspace {
        &mut self.calc
    }

    /// Switch to the dark theme (also used by the preview example).
    pub fn set_dark(&mut self, ctx: &egui::Context) {
        ctx.set_theme(egui::Theme::Dark);
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let t = tokens(ui.ctx());
        egui::Panel::top("top")
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(14, 8)).stroke(Stroke::new(1.0, t.card_border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.add(egui::Image::new((logo.id(), EVec2::splat(26.0))));
                    }
                    ui.label(RichText::new("Math-crystal").size(17.0).strong().color(t.text));
                    ui.add_space(14.0);
                    segmented(
                        ui,
                        &mut self.workspace,
                        &[
                            (Workspace::Calculus, &with_icon(icon::FUNCTION, "Calculus"), "Functions of two variables"),
                            (Workspace::Complex, &with_icon(icon::TARGET, "Complex"), "Complex functions (coming next)"),
                            (Workspace::Spectrum, &with_icon(icon::SINE, "Spectrum"), "Fourier spectra (coming next)"),
                        ],
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let dark = t.dark;
                        let label = if dark { with_icon(icon::SUN, "Light") } else { with_icon(icon::MOON, "Dark") };
                        if ui.button(label).on_hover_text("Switch colour theme").clicked() {
                            ui.ctx().set_theme(if dark { egui::Theme::Light } else { egui::Theme::Dark });
                        }
                        if ui.button(with_icon(icon::HELP, "Help")).on_hover_text("Controls and syntax (F1)").clicked() {
                            self.show_help = !self.show_help;
                        }
                    });
                });
            });
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_help;
        egui::Window::new(with_icon(icon::HELP, "Help"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let t = tokens(ui.ctx());
                let row = |ui: &mut Ui, k: &str, v: &str| {
                    ui.label(RichText::new(k).monospace().color(t.accent_text));
                    ui.label(v);
                    ui.end_row();
                };
                ui.label(RichText::new("3D view").strong());
                egui::Grid::new("help_mouse").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    row(ui, "Drag", "Rotate");
                    row(ui, "Right / middle drag", "Pan");
                    row(ui, "Scroll", "Zoom");
                    row(ui, "Double-click", "Reset the view");
                    row(ui, "Shift + click", "Move P to the surface point under the cursor");
                    row(ui, "Ctrl + / Ctrl -", "Scale the whole interface");
                });
                ui.add_space(8.0);
                ui.label(RichText::new("Expression syntax").strong());
                egui::Grid::new("help_syntax").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    row(ui, "+ - * / ^", "Arithmetic; ** also works for powers");
                    row(ui, "2x  3(x+1)", "Implicit multiplication");
                    row(ui, "|x|", "Absolute value");
                    row(ui, "if(c, a, b)", "Piecewise: a where c holds, else b");
                    row(ui, "< <= == != and or", "Comparisons; chains like 0 < x < 1");
                    row(ui, "pi  e  tau", "Constants");
                });
                ui.add_space(6.0);
                let names: Vec<&str> = crate::expr::parse::FUNCTIONS
                    .iter()
                    .map(|f| f.name)
                    .filter(|n| !matches!(*n, "re" | "im" | "conj" | "arg"))
                    .collect();
                ui.label(RichText::new(names.join("  ")).monospace().size(12.5).color(t.muted));
            });
        self.show_help = open;
    }
}

impl eframe::App for MathCrystal {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        if ui.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_help = !self.show_help;
        }
        self.top_bar(ui);
        let ctx = ui.ctx().clone();
        match self.workspace {
            Workspace::Calculus => self.calc.ui(ui, frame),
            Workspace::Complex | Workspace::Spectrum => coming_soon(ui, self.workspace == Workspace::Complex),
        }
        self.help_window(&ctx);
    }
}

fn coming_soon(ui: &mut Ui, complex: bool) {
    let t = tokens(ui.ctx());
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(t.window))
        .show(ui, |ui| {
            ui.centered_and_justified(|ui| {
                let (ic, title, body) = if complex {
                    (icon::TARGET, "Complex functions", "Domain colouring, conformal grids and |f(z)| relief are planned for phase 3.")
                } else {
                    (icon::SINE, "Fourier spectrum", "Spectra of absolutely integrable signals are planned for phase 4.")
                };
                ui.label(
                    RichText::new(format!("{}\n\n{body}", with_icon(ic, title)))
                        .size(16.0)
                        .color(t.muted),
                );
            });
        });
}

/// GPU resources of one 3D canvas.
struct Canvas {
    gpu: Gpu3d,
    texture: Option<egui::TextureId>,
    /// What was rendered last: skip re-rendering when nothing changed.
    last: Option<(View, [u32; 2], u64)>,
    adapter: String,
}

pub struct CalcWorkspace {
    src: String,
    surface: Option<Surface>,
    error: Option<String>,
    settings: Settings,
    z_auto: bool,
    camera: Camera,
    preset: ViewPreset,
    canvas: Option<Canvas>,
    // Cached build results.
    built_for: Option<(String, Settings)>,
    scene: Scene,
    frame: Frame3,
    derivs: Option<PointDerivs>,
    z_samples: Vec<f64>,
    scene_version: u64,
    build_ms: f64,
    /// (src, a, b) the model sliders were last synced to.
    model_synced_for: Option<(String, f64, f64)>,
    toast: Option<(String, Instant)>,
    want_export: bool,
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
            preset: ViewPreset::Iso,
            canvas: None,
            built_for: None,
            scene: Scene::default(),
            frame: Frame3::new(glam::DVec3::splat(-1.0), glam::DVec3::splat(1.0), false),
            derivs: None,
            z_samples: vec![],
            scene_version: 0,
            build_ms: 0.0,
            model_synced_for: None,
            toast: None,
            want_export: false,
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
            // Keep showing the last valid surface while typing.
            Err(e) => self.error = Some(e.to_string()),
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
        // The model has one cross term; use the mean of both orders.
        s.model_fxy = pick(if d.fxy.ok() && d.fyx.ok() { 0.5 * (d.fxy.v + d.fyx.v) } else { d.fxy.v });
        self.model_synced_for = Some(key);
    }

    fn rebuild_if_needed(&mut self) {
        let src = self.surface.as_ref().map(|s| s.source.clone()).unwrap_or_default();
        if self.built_for.as_ref().is_some_and(|(s, st)| *s == src && *st == self.settings) {
            return;
        }
        let t0 = Instant::now();
        let b = calc_view::build(self.surface.as_ref(), &self.settings);
        self.build_ms = t0.elapsed().as_secs_f64() * 1e3;
        self.scene = b.scene;
        self.frame = b.frame;
        self.derivs = b.derivs;
        self.z_samples = b.z_samples;
        self.scene_version += 1;
        self.built_for = Some((src, self.settings.clone()));
    }

    pub fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let t = tokens(ui.ctx());
        self.settings.theme = if t.dark { Theme::Dark } else { Theme::Light };
        self.status_bar(ui, &t);
        egui::Panel::left("calc_controls")
            .resizable(true)
            .default_size(372.0)
            .min_size(320.0)
            .max_size(560.0)
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin { left: 12, right: 6, top: 12, bottom: 4 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    // Leave room for the scrollbar.
                    ui.set_width(ui.available_width() - 8.0);
                    self.controls(ui);
                });
            });
        // Derived state must be current before the canvas draws.
        self.sync_model(false);
        self.rebuild_if_needed();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin::same(10)))
            .show(ui, |ui| self.canvas(ui, frame));
    }

    fn controls(&mut self, ui: &mut Ui) {
        let t = tokens(ui.ctx());
        // ---- Function -------------------------------------------------
        let mut examples_pick: Option<&str> = None;
        card(
            ui,
            "function",
            icon::FUNCTION,
            "Function",
            None,
            |ui| {
                ui.menu_button(with_icon(icon::BOOK, "Examples"), |ui| {
                    ui.set_min_width(260.0);
                    for (name, f) in EXAMPLES {
                        if ui.button(*name).on_hover_text(RichText::new(*f).monospace()).clicked() {
                            examples_pick = Some(f);
                            ui.close();
                        }
                    }
                });
            },
            |ui| {
                ui.label(RichText::new("z = f(x, y)").size(12.5).color(t.muted));
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut self.src)
                        .font(FontId::monospace(15.0))
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 7))
                        .hint_text("e.g. sin(x) * cos(y)"),
                );
                if edit.changed() {
                    let old = self.surface.as_ref().map(|s| s.source.clone());
                    self.recompile();
                    if self.surface.as_ref().map(|s| s.source.clone()) != old {
                        self.update_auto_z();
                    }
                }
                match &self.error {
                    Some(e) => {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(with_icon(icon::ALERT, e)).size(12.5).color(t.error));
                        });
                        caption(ui, "Showing the last valid function.");
                    }
                    None => {
                        ui.label(RichText::new(with_icon(icon::CHECK, "Ready: exact symbolic derivatives")).size(12.5).color(t.ok));
                    }
                }
            },
        );
        if let Some(f) = examples_pick {
            self.set_function(f);
        }

        // ---- Point ---------------------------------------------------
        let s = &mut self.settings;
        let (xr, yr) = (s.xr, s.yr);
        card(ui, "point", icon::CROSSHAIRS, "Point P(a, b)", None, |_| {}, |ui| {
            egui::Grid::new("point").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new("a").monospace().color(t.muted));
                ui.add(egui::Slider::new(&mut s.a, xr.0..=xr.1).max_decimals(3).clamping(egui::SliderClamping::Never));
                ui.end_row();
                ui.label(RichText::new("b").monospace().color(t.muted));
                ui.add(egui::Slider::new(&mut s.b, yr.0..=yr.1).max_decimals(3).clamping(egui::SliderClamping::Never));
                ui.end_row();
            });
            caption(ui, "Tip: Shift + click the surface to place P.");
        });

        // ---- Derivatives ---------------------------------------------
        let derivs = self.derivs;
        card(ui, "derivs", icon::SIGMA, "Derivatives at P", Some(true), |_| {}, |ui| readout(ui, &t, derivs.as_ref()));

        // ---- Taylor model ---------------------------------------------
        let mut reset = false;
        let s = &mut self.settings;
        card(
            ui,
            "model",
            icon::CURVE,
            "Quadratic model",
            Some(true),
            |ui| {
                ui.checkbox(&mut s.show_model, "Show");
            },
            |ui| {
                caption(ui, "Second-order Taylor model at P. Drag the curvatures to explore.");
                egui::Grid::new("model").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    for (name, v) in [("∂²f/∂x²", &mut s.model_fxx), ("∂²f/∂y²", &mut s.model_fyy), ("∂²f/∂x∂y", &mut s.model_fxy)] {
                        ui.label(RichText::new(name).color(t.muted));
                        let lim = (v.abs() * 1.5).max(8.0);
                        ui.add(egui::Slider::new(v, -lim..=lim).max_decimals(3).clamping(egui::SliderClamping::Never));
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(with_icon(icon::RESTORE, "Reset to f")).clicked() {
                        reset = true;
                    }
                    segmented(ui, &mut s.model_coloring, &[(Coloring::Solid, "Solid", ""), (Coloring::Height, "Height", "")]);
                });
            },
        );
        if reset {
            self.sync_model(true);
        }

        // ---- Appearance ----------------------------------------------
        let s = &mut self.settings;
        card(ui, "appearance", icon::LAYERS, "Surface", Some(true), |ui| {
            ui.checkbox(&mut s.show_surface, "Show");
        }, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Colour").color(t.muted));
                segmented(ui, &mut s.surface_coloring, &[(Coloring::Height, "Height", "Colour by height"), (Coloring::Solid, "Solid", "Translucent single colour")]);
            });
            ui.checkbox(&mut s.wireframe, "Mesh lines");
            ui.horizontal(|ui| {
                ui.label(RichText::new("Resolution").color(t.muted));
                ui.add(egui::Slider::new(&mut s.resolution, 33..=401).logarithmic(true));
            });
        });

        // ---- Domain --------------------------------------------------
        let mut domain_changed = false;
        let mut z_auto = self.z_auto;
        let s = &mut self.settings;
        card(ui, "domain", icon::RULER, "Domain", Some(false), |_| {}, |ui| {
            egui::Grid::new("domain").num_columns(4).spacing([8.0, 6.0]).show(ui, |ui| {
                for (name, r) in [("x", &mut s.xr), ("y", &mut s.yr)] {
                    ui.label(RichText::new(name).monospace().color(t.muted));
                    domain_changed |= range_edit(ui, r);
                    ui.end_row();
                }
                ui.label(RichText::new("z").monospace().color(t.muted));
                ui.add_enabled_ui(!z_auto, |ui| {
                    range_edit(ui, &mut s.zr);
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                domain_changed |= ui.checkbox(&mut z_auto, "Auto z").changed();
                ui.checkbox(&mut s.equal_aspect, "True proportions");
            });
            if ui.button(with_icon(icon::RESTORE, "Reset domain")).clicked() {
                s.xr = (-2.0, 2.0);
                s.yr = (-2.0, 2.0);
                z_auto = true;
                domain_changed = true;
            }
        });
        self.z_auto = z_auto;
        if domain_changed {
            self.update_auto_z();
        }

        // ---- Formulas ------------------------------------------------
        let surface = self.surface.as_ref();
        card(ui, "formulas", icon::INTEGRAL, "Symbolic derivatives", Some(false), |_| {}, |ui| {
            let Some(surf) = surface else { return };
            for (k, name) in [(1, "∂f/∂x"), (2, "∂f/∂y"), (3, "∂²f/∂x²"), (4, "∂²f/∂y²"), (5, "∂²f/∂y∂x"), (6, "∂²f/∂x∂y")] {
                let text = surf.formula(k);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).strong().color(t.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button(with_icon(icon::COPY, "")).on_hover_text("Copy").clicked() {
                            ui.ctx().copy_text(text.clone());
                        }
                    });
                });
                let shown = if text.chars().count() > 600 { format!("{}…", text.chars().take(600).collect::<String>()) } else { text };
                egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(shown).monospace().size(12.5));
                });
            }
        });
    }

    fn canvas(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let t = tokens(ui.ctx());
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        let size_px = [((rect.width() * ppp).round() as u32).max(1), ((rect.height() * ppp).round() as u32).max(1)];

        // Camera interaction.
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
                self.camera.zoom((-scroll * 0.0025).exp());
            }
        }
        if resp.double_clicked() {
            self.apply_preset(ViewPreset::Iso);
        }
        let aspect = rect.width().max(1.0) / rect.height().max(1.0);
        let proj = Projector {
            vp: self.camera.view_proj(aspect),
            size: Vec2::new(rect.width(), rect.height()),
        };
        if resp.clicked()
            && shift
            && let Some(pos) = resp.interact_pointer_pos()
        {
            self.pick_point(&proj, Vec2::new(pos.x - rect.min.x, pos.y - rect.min.y));
            self.sync_model(false);
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
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, "wgpu is not available", FontId::proportional(18.0), t.error);
            return;
        };
        let canvas = self.canvas.get_or_insert_with(|| Canvas {
            gpu: Gpu3d::new(&rs.device),
            texture: None,
            last: None,
            adapter: rs.adapter.get_info().name,
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
        if std::mem::take(&mut self.want_export) {
            self.toast = Some((export_png(canvas, rs), Instant::now()));
        }
        if let Some(id) = canvas.texture {
            egui::Image::new(egui::load::SizedTexture::new(id, rect.size()))
                .corner_radius(12)
                .paint_at(ui, rect);
        }
        ui.painter().rect_stroke(rect, 12, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        let painter = ui.painter_at(rect);
        self.labels(&painter, rect, &proj, labels);
        self.overlays(ui, rect, &t);
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let adapter = self.canvas.as_ref().map(|c| c.adapter.clone()).unwrap_or_default();
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(14, 5)).stroke(Stroke::new(1.0, t.card_border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let small = |s: &str, c: Color32| RichText::new(s).size(12.0).color(c);
                    ui.label(small("Drag to rotate · Right-drag to pan · Scroll to zoom · Shift+click to move P · F1 help", t.faint));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match &self.toast {
                            Some((msg, at)) if at.elapsed().as_secs_f32() < 5.0 => {
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                                let c = if msg.starts_with("Saved") { t.ok } else { t.error };
                                ui.label(small(msg, c));
                            }
                            _ => {
                                if !adapter.is_empty() {
                                    ui.label(small(&adapter, t.faint));
                                    ui.label(small("·", t.faint));
                                }
                                ui.label(small(&format!("{0}×{0} grid in {1:.1} ms", self.settings.resolution, self.build_ms), t.faint));
                            }
                        }
                    });
                });
            });
    }

    fn apply_preset(&mut self, p: ViewPreset) {
        let keep = (self.camera.ortho, self.camera.fov_deg, self.camera.dist);
        let cam = &mut self.camera;
        match p {
            ViewPreset::Iso | ViewPreset::Custom => {
                *cam = Camera::default();
                (cam.ortho, cam.fov_deg) = (keep.0, keep.1);
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

    fn overlays(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let inner = rect.shrink(12.0);
        // Top-left: values at P.
        let mut tl = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
        if let Some(d) = &self.derivs {
            overlay_frame(t).show(&mut tl, |ui| {
                let s = &self.settings;
                ui.label(RichText::new(format!("P = ({}, {})", fmt_short(s.a), fmt_short(s.b))).strong().color(t.text));
                if d.f.is_finite() {
                    egui::Grid::new("p_vals").num_columns(3).spacing([6.0, 3.0]).show(ui, |ui| {
                        let row = |ui: &mut Ui, sw: Option<u32>, name: &str, v: f64| {
                            match sw {
                                Some(c) => {
                                    let (r, _) = ui.allocate_exact_size(EVec2::new(8.0, 8.0), Sense::hover());
                                    ui.painter().circle_filled(r.center(), 4.0, hex32(c));
                                }
                                None => {
                                    ui.label("");
                                }
                            }
                            ui.label(RichText::new(name).color(t.muted));
                            ui.label(RichText::new(fmt_val(v)).monospace().color(t.text));
                            ui.end_row();
                        };
                        row(ui, None, "f(P)", d.f);
                        if d.fx.ok() {
                            row(ui, Some(calc_view::C_TAN_X), "∂f/∂x", d.fx.v);
                        }
                        if d.fy.ok() {
                            row(ui, Some(calc_view::C_TAN_Y), "∂f/∂y", d.fy.v);
                        }
                    });
                } else {
                    ui.label(RichText::new("f is undefined here").color(t.error));
                }
            });
        }

        // Top-right: view presets, projection, export.
        let mut tr = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Min)));
        overlay_frame(t).inner_margin(egui::Margin::same(5)).show(&mut tr, |ui| {
            // The parent lays out right-to-left, so add items right-most first.
            ui.horizontal(|ui| {
                if ui.button(with_icon(icon::CAMERA, "")).on_hover_text("Save the 3D view as PNG in ~/Pictures/Math-crystal").clicked() {
                    self.want_export = true;
                }
                if !self.camera.ortho {
                    ui.add(egui::Slider::new(&mut self.camera.fov_deg, 10.0..=80.0).suffix("°").max_decimals(0))
                        .on_hover_text("Field of view");
                }
                let mut proj = if self.camera.ortho { Projection::Ortho } else { Projection::Persp };
                if segmented(ui, &mut proj, &[(Projection::Ortho, "Ortho", "Orthographic projection"), (Projection::Persp, "Persp", "Perspective projection")]) {
                    self.camera.ortho = proj == Projection::Ortho;
                }
                ui.add_space(4.0);
                let mut p = self.preset;
                if segmented(
                    ui,
                    &mut p,
                    &[
                        (ViewPreset::Iso, &with_icon(icon::CUBE, "3D"), "Default 3D view (double-click the canvas)"),
                        (ViewPreset::Front, "Front", "Look along +y: the x-z plane"),
                        (ViewPreset::Side, "Side", "Look along -x: the y-z plane"),
                        (ViewPreset::Top, "Top", "Look down: the x-y plane"),
                    ],
                ) {
                    self.apply_preset(p);
                }
            });
        });

        // Bottom: layer chips centred.
        let s = &mut self.settings;
        let chips_w = 640.0f32.min(inner.width());
        let bottom = Rect::from_center_size(
            Pos2::new(inner.center().x, inner.max.y - 20.0),
            EVec2::new(chips_w, 40.0),
        );
        let mut bc = ui.new_child(UiBuilder::new().max_rect(bottom).layout(Layout::left_to_right(Align::Center)));
        let resp = overlay_frame(t).inner_margin(egui::Margin::symmetric(8, 6)).show(&mut bc, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            chip(ui, &mut s.show_point, "Point P", Some(hex32(calc_view::C_POINT)));
            chip(ui, &mut s.show_tangent_x, "Tangent x", Some(hex32(calc_view::C_TAN_X)));
            chip(ui, &mut s.show_tangent_y, "Tangent y", Some(hex32(calc_view::C_TAN_Y)));
            chip(ui, &mut s.show_plane, "Tangent plane", Some(hex32(calc_view::C_PLANE)));
            chip(ui, &mut s.show_sections, "Sections", None);
            chip(ui, &mut s.show_gradient, "Gradient", Some(hex32(calc_view::C_GRAD)));
            chip(ui, &mut s.show_grid, "Grid", None);
            chip(ui, &mut s.show_ticks, "Ticks", None);
        });
        let _ = resp;

    }

    /// Shift+click: move P to the surface sample nearest the cursor.
    fn pick_point(&mut self, proj: &Projector, at: Vec2) {
        let s = &self.settings;
        let n = (self.z_samples.len() as f64).sqrt().round() as usize;
        if n < 2 {
            return;
        }
        let eye = self.camera.eye();
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
                let score = d + w.distance(eye) * 4.0;
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

    fn labels(&self, painter: &egui::Painter, rect: Rect, proj: &Projector, labels: Vec<calc_view::Label>) {
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
        // Point label with a soft background pill.
        if let Some(d) = &self.derivs
            && self.settings.show_point
            && d.f.is_finite()
        {
            let s = &self.settings;
            let w = self.frame.map_xyz(s.a, s.b, d.f);
            if w.abs().cmple(self.frame.half * 1.001).all()
                && let Some(p) = to_screen(w)
            {
                let galley = painter.layout_no_wrap("P".to_string(), FontId::proportional(13.0), Color32::WHITE);
                let r = Rect::from_min_size(p + egui::vec2(9.0, -24.0), galley.size() + egui::vec2(10.0, 4.0));
                painter.rect_filled(r, 7, hex32(calc_view::C_POINT));
                painter.galley(r.min + egui::vec2(5.0, 2.0), galley, Color32::WHITE);
            }
        }
    }
}

fn readout(ui: &mut Ui, t: &Tokens, d: Option<&PointDerivs>) {
    let Some(d) = d else {
        ui.label("—");
        return;
    };
    if !d.f.is_finite() {
        ui.label(RichText::new(with_icon(icon::ALERT, "f is undefined at P")).color(t.error));
        return;
    }
    let mut any_limit = false;
    let mut row = |ui: &mut Ui, name: &str, hint: &str, v: f64, src: Option<Source>| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).color(t.text));
            if !hint.is_empty() {
                ui.label(RichText::new(hint).size(11.0).color(t.faint));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(fmt_val(v)).monospace().size(14.0).color(if v.is_finite() { t.text } else { t.faint }));
                if src == Some(Source::Limit) {
                    any_limit = true;
                    badge(ui, "limit", t.warn)
                        .on_hover_text("The formula is undefined or disagrees at P, so this value comes from the limit definition.");
                }
            });
        });
    };
    row(ui, "f", "", d.f, None);
    row(ui, "∂f/∂x", "f_x", d.fx.v, Some(d.fx.src));
    row(ui, "∂f/∂y", "f_y", d.fy.v, Some(d.fy.src));
    row(ui, "|∇f|", "gradient length", d.fx.v.hypot(d.fy.v), None);
    ui.separator();
    row(ui, "∂²f/∂x²", "f_xx", d.fxx.v, Some(d.fxx.src));
    row(ui, "∂²f/∂y²", "f_yy", d.fyy.v, Some(d.fyy.src));
    row(ui, "∂²f/∂y∂x", "f_xy: x first", d.fxy.v, Some(d.fxy.src));
    row(ui, "∂²f/∂x∂y", "f_yx: y first", d.fyx.v, Some(d.fyx.src));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        match d.mixed_equal() {
            Some(true) => badge(ui, &with_icon(icon::CHECK, "f_xy = f_yx"), t.ok).on_hover_text("Schwarz/Clairaut: the mixed partials agree."),
            Some(false) => badge(ui, &with_icon(icon::ALERT, "f_xy ≠ f_yx"), t.error)
                .on_hover_text("The mixed partials differ, so they are not continuous at P."),
            None => badge(ui, "mixed partials undefined", t.faint),
        };
        if let Some(kind) = classify(d) {
            let color = if kind.contains("not apply") || kind.contains("inconclusive") { t.warn } else { t.accent_text };
            let short = if kind.contains("not apply") { "test n/a (not C²)" } else { kind };
            badge(ui, &format!("critical: {short}"), color).on_hover_text(kind);
        }
    });
    if any_limit {
        caption(ui, "'limit' values come from difference quotients of the exact derivatives.");
    }
}

fn hex32(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Edit a (lo, hi) range; returns true when changed. Keeps lo < hi.
fn range_edit(ui: &mut Ui, r: &mut (f64, f64)) -> bool {
    let speed = ((r.1 - r.0).abs() * 0.005).max(1e-4);
    let mut changed = ui.add(egui::DragValue::new(&mut r.0).speed(speed).max_decimals(4)).changed();
    ui.label("…");
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
    if s == "-0" { "0".into() } else { s.replace('-', "−") }
}

fn fmt_short(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.replace('-', "−") }
}

/// Save the last rendered 3D view; returns a status message.
fn export_png(canvas: &Canvas, rs: &eframe::egui_wgpu::RenderState) -> String {
    let Some((w, h, px)) = canvas.gpu.read_rgba(&rs.device, &rs.queue) else {
        return "Export failed: could not read the image".into();
    };
    let dir = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Pictures/Math-crystal");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return format!("Export failed: {e}");
    }
    let path = dir.join(format!("surface-{}.png", timestamp()));
    match write_png(&path, w, h, &px) {
        Ok(()) => format!("Saved {}", path.display()),
        Err(e) => format!("Export failed: {e}"),
    }
}

/// Local-ish timestamp YYYYmmdd-HHMMSS (UTC; no timezone database needed).
fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}
