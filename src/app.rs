//! egui application shell and the Calculus workspace UI.

use std::time::Instant;

use egui::{Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2 as EVec2};
use glam::{Vec2, Vec3};

use crate::analysis::{self, Critical, Integral, Region};
use crate::calc::{PointDerivs, Source, Surface, classify};
use crate::map_view::{self, MapCache, MapInput};
use crate::calc_view::{self, Coloring, LabelKind, Settings, Theme};
use crate::render::gpu::{Gpu3d, View, write_png};
use crate::render::scene::{Camera, Frame3, Projector, Scene};
use crate::ui::theme::{self, Tokens, icon, tokens, with_icon};
use crate::i18n::{self, tr};
use crate::ui::mathview;
use crate::ui::widgets::{badge, caption, card, chip, eye, overlay_frame, segmented};

/// Resolution of the region indicator grid used for outlines.
const REGION_N: usize = 161;

pub const DEFAULT_FUNCTION: &str = "0.15*x^2 - 0.08*y^2 + 0.12*sin(x*y)";

const EXAMPLES: &[(&str, &str, &str)] = &[
    ("Gentle saddle (default)", "平缓鞍面（默认）", DEFAULT_FUNCTION),
    ("Paraboloid", "抛物面", "x^2 + y^2"),
    ("Saddle", "鞍面", "x^2 - y^2"),
    ("Monkey saddle", "猴鞍面", "x^3 - 3*x*y^2"),
    ("Ripple", "水波", "sin(3*sqrt(x^2 + y^2)) / (1 + x^2 + y^2)"),
    ("Gaussian bump", "高斯曲面", "exp(-(x^2 + y^2))"),
    ("Peaks", "Peaks 函数", "3(1-x)^2 exp(-x^2-(y+1)^2) - 10(x/5 - x^3 - y^5) exp(-x^2-y^2) - exp(-(x+1)^2-y^2)/3"),
    ("Mixed partials differ at 0", "原点处混合偏导不相等", "if(x^2 + y^2 == 0, 0, x*y*(x^2 - y^2)/(x^2 + y^2))"),
    ("Same, written in LaTeX", "同上，LaTeX 写法", "\\begin{cases} 0 & x^2 + y^2 = 0 \\\\ \\frac{xy(x^2 - y^2)}{x^2 + y^2} & \\text{otherwise} \\end{cases}"),
    ("Cone (no derivative at 0)", "圆锥（原点不可导）", "sqrt(x^2 + y^2)"),
    ("Half domain", "半平面定义域", "sqrt(x) * cos(y)"),
    ("Egg crate", "蛋托面", "sin(2x) * cos(2y) / 2"),
    ("LaTeX: fraction and root", "LaTeX：分式与根式", "\\frac{\\sin(\\pi x)}{1 + \\sqrt{x^2 + y^2}}"),
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
enum CanvasMode {
    Surface,
    Map,
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
        let stored = |k: &str| cc.storage.and_then(|s| s.get_string(k));
        i18n::set_zh(match stored("lang").as_deref() {
            Some("zh") => true,
            Some("en") => false,
            _ => i18n::system_prefers_zh(),
        });
        let dark = stored("theme").as_deref() == Some("dark");
        cc.egui_ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
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
                            (Workspace::Calculus, &with_icon(icon::FUNCTION, tr("Calculus", "多元微积分")), tr("Functions of two variables", "二元函数")),
                            (Workspace::Complex, &with_icon(icon::TARGET, tr("Complex", "复变函数")), tr("Complex functions (coming next)", "复变函数（即将推出）")),
                            (Workspace::Spectrum, &with_icon(icon::SINE, tr("Spectrum", "频谱")), tr("Fourier spectra (coming next)", "傅里叶频谱（即将推出）")),
                        ],
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let dark = t.dark;
                        let label = if dark { with_icon(icon::SUN, tr("Light", "浅色")) } else { with_icon(icon::MOON, tr("Dark", "深色")) };
                        if ui.button(label).on_hover_text(tr("Switch colour theme", "切换配色")).clicked() {
                            ui.ctx().set_theme(if dark { egui::Theme::Light } else { egui::Theme::Dark });
                        }
                        if ui.button(with_icon(icon::HELP, tr("Help", "帮助"))).on_hover_text(tr("Controls and syntax (F1)", "操作与语法（F1）")).clicked() {
                            self.show_help = !self.show_help;
                        }
                        let other = if i18n::zh() { "English" } else { "中文" };
                        if ui.button(with_icon(icon::TRANSLATE, other)).on_hover_text(tr("Interface language", "界面语言")).clicked() {
                            i18n::set_zh(!i18n::zh());
                            // Error messages come from the parser: re-run it.
                            self.calc.relocalize();
                        }
                    });
                });
            });
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_help;
        egui::Window::new(with_icon(icon::HELP, tr("Help", "帮助")))
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
                ui.label(RichText::new(tr("3D view", "3D 视图")).strong());
                egui::Grid::new("help_mouse").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    row(ui, tr("Drag", "拖动"), tr("Rotate", "旋转"));
                    row(ui, tr("Right / middle drag", "右键 / 中键拖动"), tr("Pan", "平移"));
                    row(ui, tr("Scroll", "滚轮"), tr("Zoom", "缩放"));
                    row(ui, tr("Double-click", "双击"), tr("Reset the view", "重置视角"));
                    row(ui, tr("Shift + click", "Shift + 单击"), tr("Move P to the surface point under the cursor", "把 P 移到光标下的曲面点"));
                    row(ui, "Ctrl + / Ctrl -", tr("Scale the whole interface", "缩放整个界面"));
                });
                ui.add_space(8.0);
                ui.label(RichText::new(tr("Expression syntax", "表达式语法")).strong());
                egui::Grid::new("help_syntax").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    row(ui, "+ - * / ^", tr("Arithmetic; ** also works for powers", "四则运算；乘方也可写 **"));
                    row(ui, "2x  3(x+1)", tr("Implicit multiplication", "省略乘号"));
                    row(ui, "|x|", tr("Absolute value", "绝对值"));
                    row(ui, "if(c, a, b)", tr("Piecewise: a where c holds, else b", "分段：满足 c 时取 a，否则取 b"));
                    row(ui, "< <= == != and or", tr("Comparisons; chains like 0 < x < 1", "比较运算；可连写 0 < x < 1"));
                    row(ui, "pi  e  tau", tr("Constants", "常数"));
                });
                ui.add_space(8.0);
                ui.label(RichText::new(tr("LaTeX input", "LaTeX 输入")).strong());
                egui::Grid::new("help_latex").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    row(ui, "\\frac{a}{b}  \\sqrt{x}  \\sqrt[3]{x}", tr("Fractions and roots", "分式与根式"));
                    row(ui, "e^{-x^2}  x^{10}", tr("Powers (braces for more than one token)", "乘方（多于一个字符要加花括号）"));
                    row(ui, "\\sin x  \\sin^2(x)  \\ln  \\log_2", tr("Functions; parentheses are optional", "函数；括号可省略"));
                    row(ui, "xy  \\cdot  \\times", tr("Adjacent letters multiply", "相邻字母表示相乘"));
                    row(ui, "\\left( \\right)  \\pi  \\le  \\ne", tr("Delimiters, constants, comparisons", "括号、常数、比较"));
                    row(ui, "\\begin{cases} … \\end{cases}", tr("Piecewise, with \\text{otherwise}", "分段函数，可用 \\text{otherwise}"));
                });
                caption(ui, tr("Input containing \\ or { is read as LaTeX. A leading z = or f(x,y) = is ignored.", "含有 \\ 或 { 的输入按 LaTeX 解析。开头的 z = 或 f(x,y) = 会被忽略。"));
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
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("lang", (if i18n::zh() { "zh" } else { "en" }).into());
        let dark = self.calc.settings_mut().theme == Theme::Dark;
        storage.set_string("theme", (if dark { "dark" } else { "light" }).into());
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        if ui.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_help = !self.show_help;
        }
        if self.show_help && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_help = false;
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
                    (icon::TARGET, tr("Complex functions", "复变函数"), tr("Domain colouring, conformal grids and |f(z)| relief are planned for phase 3.", "定义域着色、保角网格和 |f(z)| 曲面计划在第三阶段完成。"))
                } else {
                    (icon::SINE, tr("Fourier spectrum", "傅里叶频谱"), tr("Spectra of absolutely integrable signals are planned for phase 4.", "绝对可积信号的频谱计划在第四阶段完成。"))
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
    built_for: Option<(String, Settings, u64)>,
    scene: Scene,
    frame: Frame3,
    derivs: Option<PointDerivs>,
    z_samples: Vec<f64>,
    scene_version: u64,
    build_ms: f64,
    /// (src, a, b) the model sliders were last synced to.
    model_synced_for: Option<(String, f64, f64)>,
    toast: Option<(String, Instant)>,
    /// Frames left in an export: overlays are hidden while > 0.
    export_frames: u8,
    export_result: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// A screenshot was requested and has not arrived yet.
    export_waiting: bool,
    mode: CanvasMode,
    map_visited: bool,
    map_cache: MapCache,
    // Analysis caches, recomputed only when their inputs change.
    criticals: Vec<Critical>,
    crit_key: Option<(String, (f64, f64), (f64, f64))>,
    region_src: String,
    use_region: bool,
    region: Option<Region>,
    region_error: Option<String>,
    region_grid: Vec<f64>,
    integral: Option<Integral>,
    int_key: Option<(String, (f64, f64), (f64, f64), String, bool)>,
    int_ms: f64,
    analysis_version: u64,
    /// Plain clicks on the canvas place P.
    pick_mode: bool,
    /// The layer chips below the canvas are unfolded.
    layers_open: bool,
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
            export_frames: 0,
            export_result: Default::default(),
            export_waiting: false,
            mode: CanvasMode::Surface,
            map_visited: false,
            map_cache: MapCache::default(),
            criticals: vec![],
            crit_key: None,
            region_src: "x^2 + y^2 <= 1".into(),
            use_region: false,
            region: None,
            region_error: None,
            region_grid: vec![],
            integral: None,
            int_key: None,
            int_ms: 0.0,
            analysis_version: 0,
            pick_mode: false,
            layers_open: false,
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

    /// Re-run the parsers so their messages follow the interface language.
    pub fn relocalize(&mut self) {
        self.recompile();
        self.int_key = None;
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

    /// Recompute critical points and the double integral when their inputs
    /// changed (not on every move of P).
    fn update_analysis(&mut self) {
        let Some(surf) = &self.surface else { return };
        let s = &self.settings;
        let ck = (surf.source.clone(), s.xr, s.yr);
        if self.crit_key.as_ref() != Some(&ck) {
            self.criticals = analysis::critical_points(surf, s.xr, s.yr, 24);
            self.crit_key = Some(ck);
            self.analysis_version += 1;
        }
        let ik = (surf.source.clone(), s.xr, s.yr, self.region_src.clone(), self.use_region);
        if self.int_key.as_ref() != Some(&ik) {
            let t0 = Instant::now();
            self.region = None;
            self.region_error = None;
            self.region_grid.clear();
            if self.use_region {
                match Region::new(self.region_src.trim()) {
                    Ok(r) => {
                        self.region_grid = r.boundary_field(s.xr, s.yr, REGION_N);
                        self.region = Some(r);
                    }
                    Err(e) => self.region_error = Some(e.to_string()),
                }
            }
            self.integral = if self.use_region && self.region.is_none() {
                None
            } else {
                Some(analysis::integrate(surf, s.xr, s.yr, self.region.as_ref()))
            };
            self.int_ms = t0.elapsed().as_secs_f64() * 1e3;
            self.int_key = Some(ik);
            self.analysis_version += 1;
        }
    }

    fn rebuild_if_needed(&mut self) {
        self.update_analysis();
        let src = self.surface.as_ref().map(|s| s.source.clone()).unwrap_or_default();
        if self
            .built_for
            .as_ref()
            .is_some_and(|(s, st, v)| *s == src && *st == self.settings && *v == self.analysis_version)
        {
            return;
        }
        let t0 = Instant::now();
        let mut b = calc_view::build(self.surface.as_ref(), &self.settings);
        if let Some(surf) = &self.surface {
            let region = (!self.region_grid.is_empty()).then_some((&self.region_grid[..], REGION_N));
            calc_view::analysis_layers(surf, &self.settings, &b.frame, &self.criticals, region, &mut b.scene);
        }
        self.build_ms = t0.elapsed().as_secs_f64() * 1e3;
        self.scene = b.scene;
        self.frame = b.frame;
        self.derivs = b.derivs;
        self.z_samples = b.z_samples;
        self.scene_version += 1;
        self.built_for = Some((src, self.settings.clone(), self.analysis_version));
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
        // Derived state must be current before the canvas draws. The sidebar
        // was drawn from the previous build, so repaint once more if it moved.
        let version = self.scene_version;
        self.sync_model(false);
        self.rebuild_if_needed();
        if self.scene_version != version {
            ui.ctx().request_repaint();
        }
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
            tr("Function", "函数"),
            true,
            |ui| {
                ui.menu_button(with_icon(icon::BOOK, tr("Examples", "示例")), |ui| {
                    ui.set_min_width(260.0);
                    for (en, zh, f) in EXAMPLES {
                        if ui.button(tr(en, zh)).on_hover_text(RichText::new(*f).monospace()).clicked() {
                            examples_pick = Some(f);
                            ui.close();
                        }
                    }
                });
            },
            |ui| {
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut self.src)
                        .font(FontId::monospace(15.0))
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 7))
                        .hint_text(tr("e.g. sin(x) * cos(y)  or  \\frac{x}{1+y^2}", "如 sin(x) * cos(y) 或 \\frac{x}{1+y^2}")),
                );
                if edit.changed() {
                    let old = self.surface.as_ref().map(|s| s.source.clone());
                    self.recompile();
                    if self.surface.as_ref().map(|s| s.source.clone()) != old {
                        self.update_auto_z();
                    }
                }
                let latex = crate::expr::latex::looks_like_latex(&self.src);
                let read_as = latex.then(|| crate::expr::latex::to_plain(&self.src).ok()).flatten();
                if let Some(e) = &self.error {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(with_icon(icon::ALERT, e)).size(12.5).color(t.error));
                    });
                    if let Some(r) = &read_as {
                        caption(ui, &format!("{} {r}", tr("LaTeX read as:", "LaTeX 转写为:")));
                    }
                    caption(ui, tr("Showing the last valid function.", "下面显示的是上一个有效的函数。"));
                }
                // Typeset preview of the function in use.
                if let Some(surf) = &self.surface {
                    let (pool, id) = surf.expr(0);
                    egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::horizontal().id_salt("f_preview").show(ui, |ui| {
                            if !mathview::show(ui, pool, id, &crate::calc::VARS, "z", 17.0, t.text) {
                                ui.label(RichText::new(surf.formula(0)).monospace().size(12.5));
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        if latex && self.error.is_none() {
                            let tip = read_as.clone().unwrap_or_default();
                            badge(ui, &with_icon(icon::CHECK, tr("LaTeX", "LaTeX")), t.ok).on_hover_text(format!("{} {tip}", tr("Read as:", "转写为:")));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button(with_icon(icon::COPY, "LaTeX")).on_hover_text(tr("Copy f as LaTeX", "复制 f 的 LaTeX")).clicked() {
                                ui.ctx().copy_text(surf.formula_latex(0));
                            }
                        });
                    });
                }
            },
        );
        if let Some(f) = examples_pick {
            self.set_function(f);
        }

        // ---- Point ---------------------------------------------------
        let s = &mut self.settings;
        let (xr, yr) = (s.xr, s.yr);
        let mut reveal = false;
        card(
            ui,
            "point",
            icon::CROSSHAIRS,
            tr("Point P(a, b)", "点 P(a, b)"),
            false,
            |ui| {
                eye(ui, &mut s.show_point, tr("Show P and its values in the view", "在视图中显示 P 及其数值"));
            },
            |ui| {
                let mut moved = false;
                egui::Grid::new("point").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new("a").monospace().color(t.muted));
                    moved |= ui.add(egui::Slider::new(&mut s.a, xr.0..=xr.1).max_decimals(3).clamping(egui::SliderClamping::Never)).changed();
                    ui.end_row();
                    ui.label(RichText::new("b").monospace().color(t.muted));
                    moved |= ui.add(egui::Slider::new(&mut s.b, yr.0..=yr.1).max_decimals(3).clamping(egui::SliderClamping::Never)).changed();
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    let label = with_icon(icon::CROSSHAIRS, tr("Pick on surface", "在曲面上选点"));
                    if ui
                        .selectable_label(self.pick_mode, label)
                        .on_hover_text(tr("When on, clicking the 3D surface moves P there (Shift+click always works).", "开启后，单击 3D 曲面即可移动 P（Shift+单击始终可用）。"))
                        .clicked()
                    {
                        self.pick_mode = !self.pick_mode;
                        reveal |= self.pick_mode;
                    }
                    if ui.button(with_icon(icon::RESTORE, tr("Origin", "原点"))).on_hover_text(tr("Move P to (0, 0)", "把 P 移到 (0, 0)")).clicked() {
                        s.a = 0.0;
                        s.b = 0.0;
                        moved = true;
                    }
                });
                // Moving P is pointless while it is hidden.
                reveal |= moved;
            },
        );
        self.settings.show_point |= reveal;

        // ---- Derivatives ---------------------------------------------
        let derivs = self.derivs;
        card(ui, "derivs", icon::SIGMA, tr("Derivatives at P", "P 点的导数"), false, |_| {}, |ui| readout(ui, &t, derivs.as_ref()));

        // ---- Directional derivative ------------------------------------
        let derivs = self.derivs;
        let s = &mut self.settings;
        card(
            ui,
            "direction",
            icon::COMPASS,
            tr("Directional derivative", "方向导数"),
            false,
            |ui| {
                eye(ui, &mut s.show_direction, tr("Show the direction u at P", "在 P 点显示方向 u"));
            },
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("angle θ", "角度 θ")).color(t.muted));
                    // Edit a copy: the slider rounds what it shows, and exact
                    // angles from the buttons must survive.
                    let mut shown = s.dir_angle;
                    if ui.add(egui::Slider::new(&mut shown, -180.0..=180.0).suffix("°").max_decimals(1)).changed() {
                        s.dir_angle = shown;
                    }
                });
                let Some(d) = derivs.filter(|d| d.first_ok()) else {
                    caption(ui, tr("Needs f and its gradient at P.", "需要 f 在 P 点有定义且可求梯度。"));
                    return;
                };
                let a = s.dir_angle.to_radians();
                let (c, sn) = (a.cos(), a.sin());
                let du = d.fx.v * c + d.fy.v * sn;
                let g = d.fx.v.hypot(d.fy.v);
                kv(ui, &t, "u", tr("unit vector", "单位向量"), &format!("({}, {})", fmt_short(c), fmt_short(sn)));
                kv(ui, &t, "D_u f", "= ∇f · u", &fmt_val(du));
                kv(ui, &t, tr("max rate", "最大变化率"), "= |∇f|", &fmt_val(g));
                ui.horizontal_wrapped(|ui| {
                    let grad_angle = d.fy.v.atan2(d.fx.v).to_degrees();
                    if ui.add_enabled(g > 0.0, egui::Button::new(tr("Along ∇f", "沿 ∇f"))).on_hover_text(tr("Steepest ascent: D_u f = |∇f|", "最速上升方向：D_u f = |∇f|")).clicked() {
                        s.dir_angle = grad_angle;
                    }
                    if ui.add_enabled(g > 0.0, egui::Button::new(tr("Along level curve", "沿等值线"))).on_hover_text(tr("Perpendicular to ∇f: D_u f = 0", "与 ∇f 垂直：D_u f = 0")).clicked() {
                        s.dir_angle = wrap_deg(grad_angle + 90.0);
                    }
                    if ui.button("x").on_hover_text(tr("u = (1, 0): D_u f = f_x", "u = (1, 0)：D_u f = f_x")).clicked() {
                        s.dir_angle = 0.0;
                    }
                    if ui.button("y").on_hover_text(tr("u = (0, 1): D_u f = f_y", "u = (0, 1)：D_u f = f_y")).clicked() {
                        s.dir_angle = 90.0;
                    }
                });
            },
        );

        // ---- Critical points -------------------------------------------
        let crit = self.criticals.clone();
        let mut goto: Option<(f64, f64)> = None;
        let s = &mut self.settings;
        card(
            ui,
            "criticals",
            icon::TARGET,
            tr("Critical points", "临界点"),
            false,
            |ui| {
                eye(ui, &mut s.show_criticals, tr("Show critical points in the view", "在视图中显示临界点"));
                if !crit.is_empty() {
                    ui.label(RichText::new(crit.len().to_string()).size(12.0).color(t.faint));
                }
            },
            |ui| {
                if crit.is_empty() {
                    caption(ui, tr("No critical points (∇f = 0) found in the domain.", "定义域内没有找到临界点（∇f = 0）。"));
                    return;
                }
                let msg = if i18n::zh() {
                    format!("用牛顿法在 ∇f = 0 上找到 {} 个。单击一行把 P 移过去。", crit.len())
                } else {
                    format!("{} found in the domain by Newton's method on ∇f = 0. Click one to move P there.", crit.len())
                };
                caption(ui, &msg);
                for c in crit.iter().take(12) {
                    // Whole row is one click target (labels would swallow clicks).
                    let (r, resp) = ui.allocate_exact_size(EVec2::new(ui.available_width(), 24.0), Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr("Move P here", "把 P 移到这里"));
                    let p = ui.painter();
                    if resp.hovered() {
                        p.rect_filled(r, 6, t.accent_soft);
                    }
                    p.circle_filled(egui::pos2(r.min.x + 8.0, r.center().y), 5.0, hex32(calc_view::critical_color(c.kind)));
                    p.text(egui::pos2(r.min.x + 20.0, r.center().y), Align2::LEFT_CENTER, c.kind.name(), FontId::proportional(14.0), t.text);
                    p.text(
                        egui::pos2(r.max.x - 4.0, r.center().y),
                        Align2::RIGHT_CENTER,
                        format!("({}, {})   f = {}", fmt_short(c.x), fmt_short(c.y), fmt_val(c.f)),
                        FontId::monospace(12.5),
                        t.muted,
                    );
                    if resp.clicked() {
                        goto = Some((c.x, c.y));
                    }
                }
                if crit.len() > 12 {
                    caption(ui, &format!("{} {}", tr("… and more:", "……另有"), crit.len() - 12));
                }
            },
        );
        if let Some((x, y)) = goto {
            self.settings.a = x;
            self.settings.b = y;
            self.settings.show_point = true;
        }

        // ---- Double integral -------------------------------------------
        let s = &mut self.settings;
        let mut use_region = self.use_region;
        let region_src = &mut self.region_src;
        let region_error = self.region_error.clone();
        let integral = self.integral;
        let int_ms = self.int_ms;
        card(
            ui,
            "integral",
            icon::INTEGRAL,
            tr("Double integral", "二重积分"),
            false,
            |ui| {
                eye(ui, &mut s.show_region, tr("Show the integration region", "显示积分区域"));
            },
            |ui| {
                segmented(
                    ui,
                    &mut use_region,
                    &[
                        (false, tr("Whole domain", "整个定义域"), tr("Integrate over the x and y ranges", "在 x、y 范围上积分")),
                        (true, tr("Region", "区域"), tr("Integrate where a condition holds", "在满足条件的区域上积分")),
                    ],
                );
                if use_region {
                    ui.add(
                        egui::TextEdit::singleline(region_src)
                            .font(FontId::monospace(14.0))
                            .desired_width(f32::INFINITY)
                            .hint_text(tr("e.g. x^2 + y^2 <= 1", "如 x^2 + y^2 <= 1")),
                    );
                    if let Some(e) = &region_error {
                        ui.label(RichText::new(with_icon(icon::ALERT, e)).size(12.5).color(t.error));
                    }
                }
                if let Some(r) = integral {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("∬ f dA").size(16.0).color(t.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(fmt_val(r.value)).monospace().size(16.0).strong().color(t.accent_text));
                        });
                    });
                    kv(ui, &t, tr("error", "误差"), tr("estimate", "估计"), &format!("± {}", fmt_sci(r.error)));
                    kv(ui, &t, tr("area", "面积"), tr("of the region", "积分区域"), &fmt_val(r.area));
                    if r.area > 0.0 {
                        kv(ui, &t, tr("mean of f", "f 的平均值"), tr("= ∬f / area", "= ∬f / 面积"), &fmt_val(r.value / r.area));
                    }
                    if r.undefined_fraction > 0.0 {
                        let msg = if i18n::zh() {
                            format!("f 在区域的 {:.1}% 上无定义，这部分已跳过。", r.undefined_fraction * 100.0)
                        } else {
                            format!("f is undefined on {:.1}% of the region; those parts were skipped.", r.undefined_fraction * 100.0)
                        };
                        ui.label(RichText::new(with_icon(icon::ALERT, &msg)).size(12.5).color(t.warn));
                    }
                    caption(ui, &format!("{}, {int_ms:.0} ms.", tr("Composite Gauss–Legendre", "复合 Gauss–Legendre 求积")));
                }
            },
        );
        self.use_region = use_region;

        // ---- Taylor model ---------------------------------------------
        let mut reset = false;
        let s = &mut self.settings;
        card(
            ui,
            "model",
            icon::CURVE,
            tr("Quadratic model", "二阶泰勒模型"),
            false,
            |ui| {
                if eye(ui, &mut s.show_model, tr("Show the Taylor model surface", "显示泰勒模型曲面")) && s.show_model {
                    s.show_point = true;
                }
            },
            |ui| {
                caption(ui, tr("Second-order Taylor model at P. Drag the curvatures to explore.", "P 点处的二阶泰勒近似。拖动曲率系数可以观察变化。"));
                egui::Grid::new("model").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    for (name, v) in [("∂²f/∂x²", &mut s.model_fxx), ("∂²f/∂y²", &mut s.model_fyy), ("∂²f/∂x∂y", &mut s.model_fxy)] {
                        ui.label(RichText::new(name).color(t.muted));
                        let lim = (v.abs() * 1.5).max(8.0);
                        ui.add(egui::Slider::new(v, -lim..=lim).max_decimals(3).clamping(egui::SliderClamping::Never));
                        ui.end_row();
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button(with_icon(icon::RESTORE, tr("Reset to f", "重置为 f 的值"))).clicked() {
                        reset = true;
                    }
                    segmented(ui, &mut s.model_coloring, &[(Coloring::Solid, tr("Solid", "单色"), ""), (Coloring::Height, tr("Height", "高度"), "")]);
                });
            },
        );
        if reset {
            self.sync_model(true);
        }

        // ---- Appearance ----------------------------------------------
        let s = &mut self.settings;
        card(
            ui,
            "appearance",
            icon::LAYERS,
            tr("Surface", "曲面"),
            false,
            |ui| {
                eye(ui, &mut s.show_surface, tr("Show the surface", "显示曲面"));
            },
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Colour", "配色")).color(t.muted));
                    segmented(
                        ui,
                        &mut s.surface_coloring,
                        &[
                            (Coloring::Height, tr("Height", "按高度"), tr("Colour by height", "按高度着色")),
                            (Coloring::Solid, tr("Solid", "单色"), tr("Translucent single colour", "半透明单色")),
                        ],
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut s.wireframe, tr("Mesh lines", "网格线"));
                    ui.checkbox(&mut s.show_sections, tr("Sections through P", "过 P 的截线"));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut s.show_grid, tr("Box grid", "包围盒网格"));
                    ui.checkbox(&mut s.show_ticks, tr("Axis values", "坐标刻度"));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Resolution", "分辨率")).color(t.muted));
                    ui.add(egui::Slider::new(&mut s.resolution, 33..=401).logarithmic(true));
                });
            },
        );

        // ---- Domain --------------------------------------------------
        let mut domain_changed = false;
        let mut z_auto = self.z_auto;
        let s = &mut self.settings;
        card(ui, "domain", icon::RULER, tr("Domain", "定义域"), false, |_| {}, |ui| {
            egui::Grid::new("domain").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                for (name, r) in [("x", &mut s.xr), ("y", &mut s.yr)] {
                    ui.label(RichText::new(name).monospace().color(t.muted));
                    ui.horizontal(|ui| domain_changed |= range_edit(ui, r));
                    ui.end_row();
                }
                ui.label(RichText::new("z").monospace().color(t.muted));
                ui.add_enabled_ui(!z_auto, |ui| {
                    ui.horizontal(|ui| range_edit(ui, &mut s.zr));
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                domain_changed |= ui.checkbox(&mut z_auto, tr("Auto z", "自动 z 范围")).changed();
                ui.checkbox(&mut s.equal_aspect, tr("True proportions", "等比例"));
            });
            if ui.button(with_icon(icon::RESTORE, tr("Reset domain", "重置定义域"))).clicked() {
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
        card(ui, "formulas", icon::INTEGRAL, tr("Symbolic derivatives", "符号导数"), false, |_| {}, |ui| {
            let Some(surf) = surface else { return };
            for (k, name) in [(1, "∂f/∂x"), (2, "∂f/∂y"), (3, "∂²f/∂x²"), (4, "∂²f/∂y²"), (5, "∂²f/∂y∂x"), (6, "∂²f/∂x∂y")] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).strong().color(t.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("LaTeX").on_hover_text(tr("Copy as LaTeX", "复制为 LaTeX")).clicked() {
                            ui.ctx().copy_text(surf.formula_latex(k));
                        }
                        if ui.small_button(with_icon(icon::COPY, "")).on_hover_text(tr("Copy as plain text", "复制为纯文本")).clicked() {
                            ui.ctx().copy_text(surf.formula(k));
                        }
                    });
                });
                let (pool, id) = surf.expr(k);
                egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::horizontal().id_salt(("formula", k)).show(ui, |ui| {
                        if !mathview::show(ui, pool, id, &crate::calc::VARS, "", 15.0, t.text) {
                            let text = surf.formula(k);
                            let shown = if text.chars().count() > 600 { format!("{}…", text.chars().take(600).collect::<String>()) } else { text };
                            ui.add(egui::Label::new(RichText::new(shown).monospace().size(12.5)).wrap());
                        }
                    });
                });
            }
        });
    }

    fn canvas(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        if self.mode == CanvasMode::Map {
            self.map_canvas(ui);
            return;
        }
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
                self.camera.zoom((-scroll * 0.0012).exp().clamp(0.8, 1.25));
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
        if self.pick_mode && resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if resp.clicked()
            && (shift || self.pick_mode)
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
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, tr("wgpu is not available", "wgpu 不可用"), FontId::proportional(18.0), t.error);
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
        if let Some(id) = canvas.texture {
            egui::Image::new(egui::load::SizedTexture::new(id, rect.size()))
                .corner_radius(12)
                .paint_at(ui, rect);
        }
        ui.painter().rect_stroke(rect, 12, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        let painter = ui.painter_at(rect);
        self.labels(&painter, rect, &proj, labels);
        self.export_step(ui, rect);
        if let Some(msg) = self.export_result.lock().ok().and_then(|mut r| r.take()) {
            self.toast = Some((msg, Instant::now()));
            self.export_waiting = false;
        }
        // Overlays stay hidden until the screenshot has been taken.
        if self.export_frames == 0 && !self.export_waiting {
            self.overlays(ui, rect, &t);
        }
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let adapter = self.canvas.as_ref().map(|c| c.adapter.clone()).unwrap_or_default();
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(14, 5)).stroke(Stroke::new(1.0, t.card_border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let small = |s: &str, c: Color32| RichText::new(s).size(12.0).color(c);
                    let hint = if self.mode == CanvasMode::Map {
                        tr("Click or drag on the map to move P · Hover for values · F1 help", "在地图上单击或拖动移动 P · 悬停查看数值 · F1 帮助")
                    } else {
                        tr("Drag to rotate · Right-drag to pan · Scroll to zoom · Shift+click to move P · F1 help", "拖动旋转 · 右键拖动平移 · 滚轮缩放 · Shift+单击移动 P · F1 帮助")
                    };
                    ui.label(small(hint, t.faint));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match &self.toast {
                            Some((msg, at)) if at.elapsed().as_secs_f32() < 5.0 => {
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                                let c = if msg.starts_with("Saved") || msg.starts_with("已保存") { t.ok } else { t.error };
                                ui.label(small(msg, c));
                            }
                            _ => {
                                if !adapter.is_empty() {
                                    ui.label(small(&adapter, t.faint));
                                    ui.label(small("·", t.faint));
                                }
                                let msg = if i18n::zh() {
                                    format!("{0}×{0} 网格，用时 {1:.1} ms", self.settings.resolution, self.build_ms)
                                } else {
                                    format!("{0}×{0} grid in {1:.1} ms", self.settings.resolution, self.build_ms)
                                };
                                ui.label(small(&msg, t.faint));
                            }
                        }
                    });
                });
            });
    }

    fn map_canvas(&mut self, ui: &mut Ui) {
        let t = tokens(ui.ctx());
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let bg = if t.dark { Color32::from_rgb(0x16, 0x18, 0x20) } else { Color32::from_rgb(0xfc, 0xfc, 0xfe) };
        ui.painter().rect_filled(rect, 12, bg);
        let n = (self.z_samples.len() as f64).sqrt().round() as usize;
        let region = (!self.region_grid.is_empty()).then_some((&self.region_grid[..], REGION_N));
        let inp = MapInput {
            surface: self.surface.as_ref(),
            settings: &self.settings,
            z: &self.z_samples,
            n,
            criticals: &self.criticals,
            region,
            derivs: self.derivs.as_ref(),
            version: self.scene_version,
        };
        let moved = map_view::draw(ui, rect, &inp, &mut self.map_cache, &t);
        if let Some((x, y)) = moved {
            self.settings.show_point = true;
            let (xr, yr) = (self.settings.xr, self.settings.yr);
            self.settings.a = round_to(x, xr);
            self.settings.b = round_to(y, yr);
            self.sync_model(false);
            self.rebuild_if_needed();
            ui.ctx().request_repaint();
        }
        ui.painter().rect_stroke(rect, 12, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        self.export_step(ui, rect);
        if let Some(msg) = self.export_result.lock().ok().and_then(|mut r| r.take()) {
            self.toast = Some((msg, Instant::now()));
            self.export_waiting = false;
        }
        if self.export_frames == 0 && !self.export_waiting {
            self.overlays(ui, rect, &t);
        }
    }

    /// Drive a PNG export: wait a frame with overlays hidden, then ask for a
    /// window screenshot and crop it to the canvas (3D view plus labels).
    fn export_step(&mut self, ui: &mut Ui, rect: Rect) {
        if self.export_frames == 0 {
            return;
        }
        self.export_frames -= 1;
        if self.export_frames == 1 {
            self.export_waiting = true;
            let ppp = ui.ctx().pixels_per_point();
            let slot = self.export_result.clone();
            let ctx = ui.ctx().clone();
            ui.ctx().request_screenshot(move |image| {
                let msg = save_screenshot(&image, rect, ppp);
                if let Ok(mut s) = slot.lock() {
                    *s = Some(msg);
                }
                ctx.request_repaint();
            });
        }
        ui.ctx().request_repaint();
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
        if let Some(d) = self.derivs.as_ref().filter(|_| self.settings.show_point) {
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
                    ui.label(RichText::new(tr("f is undefined here", "f 在此处无定义")).color(t.error));
                }
            });
        }

        // Top-right: view presets, projection, export.
        let mut top_right = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Min)));
        overlay_frame(t).inner_margin(egui::Margin::same(5)).show(&mut top_right, |ui| {
            // The parent lays out right-to-left, so add items right-most first.
            ui.horizontal(|ui| {
                if ui.button(with_icon(icon::CAMERA, "")).on_hover_text(tr("Save the view as PNG in ~/Pictures/Math-crystal", "把视图保存为 PNG（~/Pictures/Math-crystal）")).clicked() {
                    // Hide the overlays, then capture the canvas with its labels.
                    self.export_frames = 3;
                }
                let map = self.mode == CanvasMode::Map;
                if !map && !self.camera.ortho {
                    ui.add(egui::Slider::new(&mut self.camera.fov_deg, 10.0..=80.0).suffix("°").max_decimals(0))
                        .on_hover_text(tr("Field of view", "视场角"));
                }
                let mut proj = if self.camera.ortho { Projection::Ortho } else { Projection::Persp };
                if !map && segmented(ui, &mut proj, &[(Projection::Ortho, tr("Ortho", "正交"), tr("Orthographic projection", "正交投影")), (Projection::Persp, tr("Persp", "透视"), tr("Perspective projection", "透视投影"))]) {
                    self.camera.ortho = proj == Projection::Ortho;
                }
                ui.add_space(4.0);
                let mut p = self.preset;
                if !map && segmented(
                    ui,
                    &mut p,
                    &[
                        (ViewPreset::Iso, &with_icon(icon::CUBE, "3D"), tr("Default 3D view (double-click the canvas)", "默认 3D 视角（双击画布）")),
                        (ViewPreset::Front, tr("Front", "正视"), tr("Look along +y: the x-z plane", "沿 +y 方向看：x-z 平面")),
                        (ViewPreset::Side, tr("Side", "侧视"), tr("Look along -x: the y-z plane", "沿 -x 方向看：y-z 平面")),
                        (ViewPreset::Top, tr("Top", "俯视"), tr("Look down: the x-y plane", "向下看：x-y 平面")),
                    ],
                ) {
                    self.apply_preset(p);
                }
                ui.add_space(4.0);
                let mut m = self.mode;
                if segmented(
                    ui,
                    &mut m,
                    &[
                        (CanvasMode::Surface, &with_icon(icon::CUBE, tr("Surface", "曲面")), tr("3D surface view", "3D 曲面视图")),
                        (CanvasMode::Map, &with_icon(icon::MAP, tr("Map", "等高线图")), tr("2D contour map: click or drag to move P", "2D 等高线图：单击或拖动移动 P")),
                    ],
                ) {
                    if m == CanvasMode::Map && !self.map_visited {
                        // Level curves are what a map is for.
                        self.settings.show_contours = true;
                        self.map_visited = true;
                    }
                    self.mode = m;
                }
            });
        });

        // Bottom: layer chips. Folded into one "Layers" button by default;
        // unfolded they sit in one centred row and shorten their labels when
        // the canvas is too narrow.
        let map = self.mode == CanvasMode::Map;
        let s = &mut self.settings;
        type Chip<'a> = (&'a mut bool, &'a str, &'a str, Option<u32>, &'a str);
        let mut chips: Vec<Chip> = Vec::new();
        chips.push((&mut s.show_point, tr("Point P", "点 P"), "P", Some(calc_view::C_POINT), tr("The point P(a, b)", "点 P(a, b)")));
        if !map {
            chips.push((&mut s.show_tangent_x, tr("Tangent x", "x 切线"), tr("Tan x", "x 切线"), Some(calc_view::C_TAN_X), tr("Tangent line in the x direction", "沿 x 方向的切线")));
            chips.push((&mut s.show_tangent_y, tr("Tangent y", "y 切线"), tr("Tan y", "y 切线"), Some(calc_view::C_TAN_Y), tr("Tangent line in the y direction", "沿 y 方向的切线")));
            chips.push((&mut s.show_plane, tr("Plane", "切平面"), tr("Plane", "切面"), Some(calc_view::C_PLANE), tr("Tangent plane at P", "P 点的切平面")));
            chips.push((&mut s.show_model, tr("Taylor model", "泰勒模型"), tr("Model", "模型"), Some(calc_view::C_MODEL), tr("Second-order Taylor model at P", "P 点的二阶泰勒近似曲面")));
        }
        chips.push((&mut s.show_direction, tr("Direction u", "方向 u"), "u", Some(calc_view::C_DIR), tr("Directional derivative along u", "沿 u 的方向导数")));
        chips.push((&mut s.show_gradient, tr("∇f at P", "P 点 ∇f"), "∇f", Some(calc_view::C_GRAD), tr("Steepest-ascent direction at P", "P 点的最速上升方向")));
        chips.push((&mut s.show_contours, tr("Contours", "等值线"), tr("Levels", "等值线"), None, tr("Level curves f = c", "等值线 f = c")));
        chips.push((&mut s.show_field, tr("Gradient field", "梯度场"), tr("Field", "梯度场"), None, tr("∇f across the domain", "整个定义域上的 ∇f")));
        chips.push((&mut s.show_criticals, tr("Critical points", "临界点"), tr("Critical", "临界点"), Some(calc_view::C_SADDLE), tr("Points where ∇f = 0", "∇f = 0 的点")));
        chips.push((&mut s.show_region, tr("Region", "积分区域"), tr("Region", "区域"), Some(calc_view::C_REGION), tr("Integration region (Double integral card)", "积分区域（见“二重积分”卡片）")));
        let active = chips.iter().filter(|c| *c.0).count();
        let toggle_label = if self.layers_open {
            with_icon(icon::CHEVRON_DOWN, tr("Layers", "图层"))
        } else if active > 0 {
            format!("{}  {active}", with_icon(icon::LAYERS, tr("Layers", "图层")))
        } else {
            with_icon(icon::LAYERS, tr("Layers", "图层"))
        };
        let measure = |ui: &Ui, text: &str, sw: bool| {
            ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.0), Color32::WHITE).size().x + 22.0 + if sw { 14.0 } else { 0.0 }
        };
        let toggle_w = measure(ui, &toggle_label, false) + 6.0;
        let width_of = |short: bool| {
            toggle_w + chips.iter().map(|c| measure(ui, if short { c.2 } else { c.1 }, c.3.is_some()) + 6.0).sum::<f32>() + 10.0
        };
        let short = width_of(false) > inner.width() - 8.0;
        let w = if self.layers_open { width_of(short).min(inner.width()) } else { toggle_w + 10.0 };
        let bottom = Rect::from_center_size(Pos2::new(inner.center().x, inner.max.y - 20.0), EVec2::new(w + 2.0, 40.0));
        let mut bc = ui.new_child(UiBuilder::new().max_rect(bottom).layout(Layout::left_to_right(Align::Center)));
        let mut open = self.layers_open;
        overlay_frame(t).inner_margin(egui::Margin::symmetric(8, 6)).show(&mut bc, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let mut shown = open;
            chip(ui, &mut shown, &toggle_label, None).on_hover_text(if open { tr("Hide the layer list", "收起图层列表") } else { tr("Show or hide parts of the drawing", "显示或隐藏图中的元素") });
            if shown != open {
                open = shown;
                return;
            }
            if !open {
                return;
            }
            for (v, full, brief, color, tip) in chips {
                let label = if short { brief } else { full };
                chip(ui, v, label, color.map(hex32)).on_hover_text(if short { format!("{full}: {tip}") } else { tip.to_string() });
            }
        });
        self.layers_open = open;
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
            self.settings.show_point = true;
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
        ui.label(RichText::new(with_icon(icon::ALERT, tr("f is undefined at P", "f 在 P 点无定义"))).color(t.error));
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
                    badge(ui, tr("limit", "极限"), t.warn)
                        .on_hover_text(tr("The formula is undefined or disagrees at P, so this value comes from the limit definition.", "公式在 P 点无定义或与实际不符，此值由极限定义求得。"));
                }
            });
        });
    };
    row(ui, "f", "", d.f, None);
    row(ui, "∂f/∂x", "f_x", d.fx.v, Some(d.fx.src));
    row(ui, "∂f/∂y", "f_y", d.fy.v, Some(d.fy.src));
    row(ui, "|∇f|", tr("gradient length", "梯度长度"), d.fx.v.hypot(d.fy.v), None);
    ui.separator();
    row(ui, "∂²f/∂x²", "f_xx", d.fxx.v, Some(d.fxx.src));
    row(ui, "∂²f/∂y²", "f_yy", d.fyy.v, Some(d.fyy.src));
    row(ui, "∂²f/∂y∂x", tr("f_xy: x first", "f_xy：先对 x"), d.fxy.v, Some(d.fxy.src));
    row(ui, "∂²f/∂x∂y", tr("f_yx: y first", "f_yx：先对 y"), d.fyx.v, Some(d.fyx.src));
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        match d.mixed_equal() {
            Some(true) => badge(ui, &with_icon(icon::CHECK, "f_xy = f_yx"), t.ok).on_hover_text(tr("Schwarz/Clairaut: the mixed partials agree.", "Schwarz/Clairaut 定理：混合偏导相等。")),
            Some(false) => badge(ui, &with_icon(icon::ALERT, "f_xy ≠ f_yx"), t.error)
                .on_hover_text(tr("The mixed partials differ, so they are not continuous at P.", "混合偏导不相等，说明它们在 P 点不连续。")),
            None => badge(ui, tr("mixed partials undefined", "混合偏导无定义"), t.faint),
        };
        if let Some(kind) = classify(d) {
            let color = if kind.contains("not apply") || kind.contains("inconclusive") { t.warn } else { t.accent_text };
            let long = match kind {
                k if k.contains("not apply") => tr(k, "临界点，但二阶导数判别法不适用（f 在此处不是 C²）"),
                "local minimum" => tr(kind, "局部极小值"),
                "local maximum" => tr(kind, "局部极大值"),
                "saddle point" => tr(kind, "鞍点"),
                k => tr(k, "退化（判别法无法判断）"),
            };
            let short = if kind.contains("not apply") { tr("test n/a (not C²)", "判别法不适用（非 C²）") } else { long };
            badge(ui, &format!("{} {short}", tr("critical:", "临界点:")), color).on_hover_text(long);
        }
    });
    if any_limit {
        caption(ui, tr("'limit' values come from difference quotients of the exact derivatives.", "标为“极限”的值由精确导数的差商求得。"));
    }
}

/// Label / hint on the left, value on the right.
fn kv(ui: &mut Ui, t: &Tokens, name: &str, hint: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).color(t.text));
        if !hint.is_empty() {
            ui.label(RichText::new(hint).size(11.0).color(t.faint));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).monospace().size(14.0).color(t.text));
        });
    });
}

fn wrap_deg(a: f64) -> f64 {
    let w = (a + 180.0).rem_euclid(360.0) - 180.0;
    if w == -180.0 { 180.0 } else { w }
}

fn fmt_sci(v: f64) -> String {
    if v == 0.0 { "0".into() } else { format!("{v:.1e}") }
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
        return crate::i18n::tr("undefined", "无定义").into();
    }
    // Rounding noise such as 3e-15 at a critical point.
    if v != 0.0 && v.abs() < 1e-12 {
        return "≈ 0".into();
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

/// Crop a window screenshot to the canvas and save it; returns a status message.
fn save_screenshot(image: &egui::ColorImage, rect: Rect, ppp: f32) -> String {
    let crop = image.region(&rect.shrink(1.0), Some(ppp));
    let [w, h] = crop.size;
    let rgba: Vec<u8> = crop.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let dir = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default().join("Pictures/Math-crystal");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return format!("{} {e}", tr("Export failed:", "导出失败:"));
    }
    let path = dir.join(format!("surface-{}.png", timestamp()));
    match write_png(&path, w as u32, h as u32, &rgba) {
        Ok(()) => format!("{} {}", tr("Saved", "已保存"), path.display()),
        Err(e) => format!("{} {e}", tr("Export failed:", "导出失败:")),
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
