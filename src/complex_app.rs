//! The Complex workspace: w = f(z) as a domain colouring, a 3D relief and
//! a conformal map, with zeros, poles, residues and contour integrals.

use std::time::Instant;

use egui::{Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use glam::DVec3;
use num_complex::Complex64 as C64;

use crate::app::{fmt_short, fmt_val, kv};
use crate::calc_view::{self, Theme};
use crate::complex::{CFunc, Contour, Kind, Special, c};
use crate::complex_view::{self, GridLine, Height, Style};
use crate::i18n::{self, tr};
use crate::render::scene::{Frame3, Scene, hex, line, polyline, sphere};
use crate::ui::canvas3d::{self, View3d};
use crate::ui::export::{self, Export};
use crate::ui::mathview;
use crate::ui::plane::Plane;
use crate::ui::theme::{Tokens, icon, tokens, with_icon};
use crate::ui::widgets::{badge, caption, card, eye, layer_bar, overlay_frame, segmented};

pub const DEFAULT_FUNCTION: &str = "(z^2 - 1)(z - 2 - i)^2 / (z^2 + 2 + 2i)";

const EXAMPLES: &[(&str, &str, &str)] = &[
    ("Zeros and poles (default)", "零点与极点（默认）", DEFAULT_FUNCTION),
    ("z²", "z²", "z^2"),
    ("Roots of unity", "单位根", "z^5 - 1"),
    ("1/z", "1/z", "1/z"),
    ("Two simple poles", "两个一阶极点", "1/(1 + z^2)"),
    ("Pole of order 3", "三阶极点", "exp(z)/z^3"),
    ("Möbius map", "莫比乌斯变换", "(z - i)/(z + i)"),
    ("Joukowski map", "儒科夫斯基变换", "z + 1/z"),
    ("Exponential", "指数函数", "exp(z)"),
    ("Sine", "正弦", "sin(z)"),
    ("Tangent", "正切", "tan(z)"),
    ("Logarithm (branch cut)", "对数（支割线）", "ln(z)"),
    ("Square root (branch cut)", "平方根（支割线）", "sqrt(z)"),
    ("Essential singularity", "本性奇点", "exp(1/z)"),
    ("Not holomorphic: conj z", "非全纯：共轭", "conj(z)"),
    ("Not holomorphic: |z|²", "非全纯：|z|²", "x^2 + y^2"),
    ("LaTeX: e^z / (z² + 1)", "LaTeX：e^z / (z² + 1)", "\\frac{e^{z}}{z^2 + 1}"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Colour,
    Relief,
    Mapping,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shade {
    Phase,
    Lightness,
}

pub struct ComplexWorkspace {
    src: String,
    func: Option<CFunc>,
    params: crate::ui::params::Params,
    error: Option<String>,
    mode: Mode,
    plane: Plane,
    wplane: Plane,
    z0: C64,
    show_z0: bool,
    style: Style,
    /// Fraction of the screen resolution used for the colouring.
    res: f32,
    // Zeros and poles.
    specials: Vec<Special>,
    spec_key: String,
    spec_ms: f64,
    show_specials: bool,
    // Contour integral over a circle.
    contour_c: C64,
    contour_r: f64,
    show_contour: bool,
    contour: Option<Contour>,
    contour_key: String,
    // Relief.
    height: Height,
    relief_n: usize,
    wire: bool,
    view: View3d,
    scene: Scene,
    frame3: Frame3,
    relief_zr: (f64, f64),
    relief_key: String,
    scene_version: u64,
    // Mapping.
    grid_n: usize,
    polar: bool,
    map_lines: Vec<GridLine>,
    map_images: Vec<Vec<C64>>,
    map_key: String,
    wfit_key: String,
    // Domain colouring texture.
    tex: Option<egui::TextureHandle>,
    tex_key: String,
    grid_step: f64,
    /// Visible ranges of the last 2D canvas (for zeros and poles).
    last_ranges: Option<((f64, f64), (f64, f64))>,
    interacting: bool,
    layers_open: bool,
    export: Export,
    toast: Option<(String, Instant)>,
    draw_ms: f64,
}

impl Default for ComplexWorkspace {
    fn default() -> Self {
        Self::new(DEFAULT_FUNCTION)
    }
}

/// `a + bi` with the usual shortcuts.
pub fn fmt_c(w: C64) -> String {
    if !w.is_finite() {
        return if w.re.is_nan() || w.im.is_nan() { tr("undefined", "无定义").into() } else { "∞".into() };
    }
    let w = crate::complex::clean(w);
    let im_abs = fmt_val(w.im.abs());
    let im_part = if im_abs == "1" { "i".to_string() } else { format!("{im_abs}i") };
    match (w.re == 0.0, w.im == 0.0) {
        (_, true) => fmt_val(w.re),
        (true, false) => {
            if w.im < 0.0 {
                format!("−{im_part}")
            } else {
                im_part
            }
        }
        _ => format!("{} {} {im_part}", fmt_val(w.re), if w.im < 0.0 { "−" } else { "+" }),
    }
}

fn fmt_cs(w: C64) -> String {
    let w = crate::complex::clean(w);
    if w.im == 0.0 {
        return fmt_short(w.re);
    }
    let im = fmt_short(w.im.abs());
    let im = if im == "1" { "i".to_string() } else { format!("{im}i") };
    if w.re == 0.0 {
        return if w.im < 0.0 { format!("−{im}") } else { im };
    }
    format!("{} {} {im}", fmt_short(w.re), if w.im < 0.0 { "−" } else { "+" })
}

fn deg(a: f64) -> String {
    format!("{}°", fmt_short(a.to_degrees()))
}

const C_Z0: u32 = 0xe03131;
const C_CONTOUR: u32 = 0xffffff;
const C_ZERO: u32 = 0x111111;
const C_POLE: u32 = 0xf8f9fa;

fn c32(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl ComplexWorkspace {
    pub fn new(src: &str) -> Self {
        let mut s = Self {
            src: src.to_string(),
            func: None,
            params: Default::default(),
            error: None,
            mode: Mode::Colour,
            plane: Plane::new(c(0.0, 0.0), 3.0),
            wplane: Plane::new(c(0.0, 0.0), 3.0),
            z0: c(1.0, 1.0),
            show_z0: false,
            style: Style::default(),
            res: 0.5,
            specials: vec![],
            spec_key: String::new(),
            spec_ms: 0.0,
            show_specials: false,
            contour_c: c(0.0, 0.0),
            contour_r: 1.5,
            show_contour: false,
            contour: None,
            contour_key: String::new(),
            height: Height::Modulus,
            relief_n: 161,
            wire: true,
            view: View3d::default(),
            scene: Scene::default(),
            frame3: Frame3::new(DVec3::splat(-1.0), DVec3::splat(1.0), false),
            relief_zr: (0.0, 1.0),
            relief_key: String::new(),
            scene_version: 0,
            grid_n: 10,
            polar: false,
            map_lines: vec![],
            map_images: vec![],
            map_key: String::new(),
            wfit_key: String::new(),
            tex: None,
            tex_key: String::new(),
            grid_step: 1.0,
            last_ranges: None,
            interacting: false,
            layers_open: false,
            export: Export::default(),
            toast: None,
            draw_ms: 0.0,
        };
        s.recompile();
        s
    }

    pub fn set_function(&mut self, src: &str) {
        self.src = src.to_string();
        self.recompile();
    }

    /// Re-run the parser so its messages follow the interface language.
    pub fn relocalize(&mut self) {
        self.recompile();
    }

    /// The inputs worth keeping across restarts.
    pub fn state(&self) -> crate::session::ComplexState {
        crate::session::ComplexState {
            src: self.src.clone(),
            center: (self.plane.center.re, self.plane.center.im),
            half: self.plane.half,
            params: self.params.state(),
        }
    }

    pub fn restore(&mut self, s: crate::session::ComplexState) {
        if s.center.0.is_finite() && s.center.1.is_finite() && s.half.is_finite() && s.half > 0.0 {
            self.plane = Plane::new(C64::new(s.center.0, s.center.1), s.half);
        }
        self.params.restore(&s.params);
        if !s.src.trim().is_empty() {
            self.set_function(&s.src);
        }
    }

    fn recompile(&mut self) {
        match CFunc::new(self.src.trim()) {
            Ok(mut f) => {
                self.params.sync(&f.params);
                f.set_params(&self.params.values());
                self.func = Some(f);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Zeros/poles and the contour integral, when their inputs changed.
    fn update_analysis(&mut self) {
        let Some(f) = &self.func else { return };
        let ranges = self.last_ranges.unwrap_or(((-3.0, 3.0), (-3.0, 3.0)));
        // Keyed on rounded ranges; not while the view is being dragged.
        let key = format!("{}|{:.4e},{:.4e},{:.4e},{:.4e}", f.source, ranges.0.0, ranges.0.1, ranges.1.0, ranges.1.1);
        if key != self.spec_key && !self.interacting {
            let t0 = Instant::now();
            self.specials = f.zeros_poles(ranges.0, ranges.1);
            self.spec_ms = t0.elapsed().as_secs_f64() * 1e3;
            self.spec_key = key;
        }
        let key = format!("{}|{:?}|{}", f.source, self.contour_c, self.contour_r);
        if key != self.contour_key {
            self.contour = (self.contour_r > 0.0).then(|| f.circle_integral(self.contour_c, self.contour_r));
            self.contour_key = key;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let t = tokens(ui.ctx());
        self.status_bar(ui, &t);
        egui::Panel::left("complex_controls")
            .resizable(true)
            .default_size(372.0)
            .min_size(320.0)
            .max_size(560.0)
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin { left: 12, right: 6, top: 12, bottom: 4 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.set_width(ui.available_width() - 8.0);
                    self.controls(ui);
                });
            });
        self.update_analysis();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin::same(10)))
            .show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                let t0 = Instant::now();
                match self.mode {
                    Mode::Colour => self.colour_canvas(ui, rect, &t),
                    Mode::Relief => self.relief_canvas(ui, frame, rect, &t),
                    Mode::Mapping => self.mapping_canvas(ui, rect, &t),
                }
                self.draw_ms = 0.8 * self.draw_ms + 0.2 * t0.elapsed().as_secs_f64() * 1e3;
                if let Some(msg) = self.export.step(ui, rect, "complex") {
                    self.toast = Some((msg, Instant::now()));
                }
                if self.export.overlays_visible() {
                    self.overlays(ui, rect, &t);
                }
            });
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let adapter = self.view.adapter();
        egui::Panel::bottom("complex_status")
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(14, 5)).stroke(Stroke::new(1.0, t.card_border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let small = |s: &str, c: Color32| RichText::new(s).size(12.0).color(c);
                    let hint = match self.mode {
                        Mode::Colour => tr("Click or drag to move z₀ · Right-drag to pan · Scroll to zoom · Double-click to reset", "单击或拖动移动 z₀ · 右键拖动平移 · 滚轮缩放 · 双击重置"),
                        Mode::Relief => tr("Drag to rotate · Right-drag to pan · Scroll to zoom · Double-click to reset", "拖动旋转 · 右键拖动平移 · 滚轮缩放 · 双击重置"),
                        Mode::Mapping => tr("Left: z-plane (click to move z₀) · Right: its image under f · Right-drag / scroll on either side", "左：z 平面（单击移动 z₀）· 右：在 f 下的像 · 两侧都可右键拖动、滚轮缩放"),
                    };
                    ui.label(small(hint, t.faint));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &self.toast {
                        Some((msg, at)) if at.elapsed().as_secs_f32() < 5.0 => {
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                            ui.label(small(msg, if export::is_ok(msg) { t.ok } else { t.error }));
                        }
                        _ => {
                            if self.mode == Mode::Relief && !adapter.is_empty() {
                                ui.label(small(&adapter, t.faint));
                                ui.label(small("·", t.faint));
                            }
                            let msg = if i18n::zh() { format!("绘制 {:.1} ms", self.draw_ms) } else { format!("drawn in {:.1} ms", self.draw_ms) };
                            ui.label(small(&msg, t.faint));
                        }
                    });
                });
            });
    }

    // ---- Sidebar ---------------------------------------------------------

    fn controls(&mut self, ui: &mut Ui) {
        let t = tokens(ui.ctx());
        let mut pick: Option<&str> = None;
        card(
            ui,
            "c_function",
            icon::FUNCTION,
            tr("Function", "函数"),
            true,
            |ui| {
                ui.menu_button(with_icon(icon::BOOK, tr("Examples", "示例")), |ui| {
                    ui.set_min_width(260.0);
                    for (en, zh, f) in EXAMPLES {
                        if ui.button(tr(en, zh)).on_hover_text(RichText::new(*f).monospace()).clicked() {
                            pick = Some(f);
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
                        .hint_text(tr("e.g. (z^2 - 1)/(z^2 + 1)  or  \\frac{e^z}{z}", "如 (z^2 - 1)/(z^2 + 1) 或 \\frac{e^z}{z}")),
                );
                if edit.changed() {
                    self.recompile();
                }
                caption(ui, tr("Variable z; x = Re z and y = Im z are also allowed; i is the imaginary unit.", "变量为 z；也可用 x = Re z、y = Im z；i 为虚数单位。"));
                if let Some(e) = &self.error {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(with_icon(icon::ALERT, e)).size(12.5).color(t.error));
                    });
                    caption(ui, tr("Showing the last valid function.", "下面显示的是上一个有效的函数。"));
                }
                if let Some(f) = &self.func {
                    let (pool, id) = f.expr(0);
                    egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::horizontal().id_salt("cf_preview").show(ui, |ui| {
                            if !mathview::show(ui, pool, id, &f.names(), "w", 17.0, t.text) {
                                ui.label(RichText::new(f.text(0)).monospace().size(12.5));
                            }
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        if f.holomorphic {
                            badge(ui, &with_icon(icon::CHECK, tr("holomorphic", "全纯")), t.ok)
                                .on_hover_text(tr("The Cauchy–Riemann equations hold wherever f is defined (checked at sample points).", "f 在有定义处满足柯西–黎曼方程（在若干采样点上检验）。"));
                        } else {
                            let why = f.reason.map(|r| format!("{} {r}", tr("uses", "用到了"))).unwrap_or_else(|| tr("Cauchy–Riemann fails", "不满足柯西–黎曼方程").to_string());
                            badge(ui, &with_icon(icon::ALERT, &format!("{} ({why})", tr("not holomorphic", "非全纯"))), t.warn)
                                .on_hover_text(tr("f is not complex-differentiable: f' does not exist, residues and the residue theorem do not apply.", "f 不是复可微的：f' 不存在，留数与留数定理都不适用。"));
                        }
                        if crate::expr::latex::looks_like_latex(&self.src) && self.error.is_none() {
                            badge(ui, &with_icon(icon::CHECK, "LaTeX"), t.ok);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button(with_icon(icon::COPY, "LaTeX")).on_hover_text(tr("Copy f as LaTeX", "复制 f 的 LaTeX")).clicked() {
                                ui.ctx().copy_text(f.latex(0));
                            }
                        });
                    });
                }
            },
        );
        if let Some(f) = pick {
            self.set_function(f);
        }
        let ch = self.params.ui(ui, "complex_params");
        if ch.changed
            && let Some(f) = &mut self.func
        {
            f.set_params(&self.params.values());
            // Everything cached per function is stale now.
            for key in [&mut self.spec_key, &mut self.contour_key, &mut self.relief_key, &mut self.map_key, &mut self.tex_key] {
                key.clear();
            }
        }

        // ---- z0 ----
        let ((x0, x1), (y0, y1)) = self.last_ranges.unwrap_or(((-3.0, 3.0), (-3.0, 3.0)));
        let mut z0 = self.z0;
        let mut show = self.show_z0;
        let mut reveal = false;
        card(
            ui,
            "c_point",
            icon::CROSSHAIRS,
            tr("Point z₀", "点 z₀"),
            false,
            |ui| {
                eye(ui, &mut show, tr("Show z₀ and its values in the view", "在视图中显示 z₀ 及其数值"));
            },
            |ui| {
                let mut moved = false;
                egui::Grid::new("c_point").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new("Re").monospace().color(t.muted));
                    moved |= ui.add(egui::Slider::new(&mut z0.re, x0..=x1).max_decimals(3).clamping(egui::SliderClamping::Never)).changed();
                    ui.end_row();
                    ui.label(RichText::new("Im").monospace().color(t.muted));
                    moved |= ui.add(egui::Slider::new(&mut z0.im, y0..=y1).max_decimals(3).clamping(egui::SliderClamping::Never)).changed();
                    ui.end_row();
                });
                if ui.button(with_icon(icon::RESTORE, tr("Origin", "原点"))).clicked() {
                    z0 = c(0.0, 0.0);
                    moved = true;
                }
                reveal = moved;
            },
        );
        self.z0 = z0;
        self.show_z0 = show || reveal;

        // ---- Values at z0 ----
        let local = self.func.as_ref().map(|f| (f.local(self.z0), f.holomorphic, f.has_symbolic_derivative()));
        let eps = 0.02 * self.plane.half;
        let angle = self.func.as_ref().map(|f| {
            let w0 = f.eval(self.z0);
            let a = f.eval(self.z0 + eps) - w0;
            let b = f.eval(self.z0 + c(0.0, eps)) - w0;
            (b / a).arg()
        });
        card(ui, "c_values", icon::SIGMA, tr("Values and derivative at z₀", "z₀ 处的值与导数"), false, |_| {}, |ui| {
            let Some((l, holo, symbolic)) = local else { return };
            kv(ui, &t, "z₀", "", &fmt_c(self.z0));
            kv(ui, &t, "f(z₀)", "", &fmt_c(l.f));
            kv(ui, &t, "|f|", tr("modulus", "模"), &fmt_val(l.f.norm()));
            kv(ui, &t, "arg f", tr("phase", "辐角"), &format!("{}  ({} rad)", deg(l.f.arg()), fmt_short(l.f.arg())));
            ui.separator();
            if holo {
                kv(ui, &t, "f'(z₀)", if symbolic { tr("exact", "精确值") } else { tr("numerical", "数值") }, &fmt_c(l.dz));
                kv(ui, &t, "|f'|", tr("local scale", "局部伸缩"), &fmt_val(l.dz.norm()));
                kv(ui, &t, "arg f'", tr("local rotation", "局部旋转"), &deg(l.dz.arg()));
                if let Some(a) = angle {
                    kv(ui, &t, tr("angle", "夹角"), tr("image of a right angle", "直角的像"), &deg(a));
                }
                caption(ui, tr("Near z₀, f acts like w ≈ f(z₀) + f'(z₀)(z − z₀): a rotation by arg f' and a scaling by |f'|, so angles are preserved (conformal) where f' ≠ 0.", "在 z₀ 附近 f(z) ≈ f(z₀) + f'(z₀)(z − z₀)：旋转 arg f'、伸缩 |f'|，所以 f' ≠ 0 处保角（共形）。"));
            } else {
                ui.label(RichText::new(with_icon(icon::ALERT, tr("f is not complex-differentiable, so f' does not exist.", "f 不是复可微的，f' 不存在。"))).size(12.5).color(t.warn));
                if let Some(a) = angle {
                    kv(ui, &t, tr("angle", "夹角"), tr("image of a right angle", "直角的像"), &deg(a));
                }
            }
            ui.separator();
            ui.label(RichText::new(tr("Cauchy–Riemann at z₀", "z₀ 处的柯西–黎曼方程")).strong().color(t.text));
            let (ux, vx, uy, vy) = (l.fx.re, l.fx.im, l.fy.re, l.fy.im);
            kv(ui, &t, "u_x  /  v_y", "", &format!("{}  /  {}", fmt_val(ux), fmt_val(vy)));
            kv(ui, &t, "u_y  /  −v_x", "", &format!("{}  /  {}", fmt_val(uy), fmt_val(-vx)));
            kv(ui, &t, "∂f/∂z̄", "= (f_x + i f_y)/2", &fmt_c(l.dzbar));
            if l.cauchy_riemann() {
                badge(ui, &with_icon(icon::CHECK, tr("u_x = v_y,  u_y = −v_x", "u_x = v_y，u_y = −v_x")), t.ok);
            } else {
                badge(ui, &with_icon(icon::ALERT, tr("Cauchy–Riemann fails here", "此处不满足柯西–黎曼方程")), t.error);
            }
            if !l.symbolic {
                caption(ui, tr("Derivatives from difference quotients (f uses a non-holomorphic operation).", "导数由差商求得（f 用到了非全纯运算）。"));
            }
        });

        // ---- Zeros and poles ----
        let specials = self.specials.clone();
        let mut goto = None;
        let mut show = self.show_specials;
        let holo = self.func.as_ref().is_some_and(|f| f.holomorphic);
        card(
            ui,
            "c_specials",
            icon::TARGET,
            tr("Zeros and poles", "零点与极点"),
            false,
            |ui| {
                eye(ui, &mut show, tr("Mark zeros and poles in the view", "在视图中标出零点与极点"));
                if !specials.is_empty() {
                    ui.label(RichText::new(specials.len().to_string()).size(12.0).color(t.faint));
                }
            },
            |ui| {
                caption(ui, tr("In the visible region, by the argument principle on a grid, refined by Newton's method. Click a row to move z₀ there.", "在可见区域内用网格上的辐角原理查找，再用牛顿法精化。单击一行把 z₀ 移过去。"));
                if specials.is_empty() {
                    caption(ui, tr("None found.", "没有找到。"));
                    return;
                }
                for s in specials.iter().take(16) {
                    let h = if s.residue.is_some() { 40.0 } else { 24.0 };
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
                    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr("Move z₀ here", "把 z₀ 移到这里"));
                    let p = ui.painter();
                    if resp.hovered() {
                        p.rect_filled(r, 6, t.accent_soft);
                    }
                    let y = r.top() + 12.0;
                    marker(p, pos2(r.min.x + 9.0, y), s.kind, 0.85);
                    let kind = match (s.kind, s.order) {
                        (Kind::Zero, 1) => tr("simple zero", "一阶零点").to_string(),
                        (Kind::Pole, 1) => tr("simple pole", "一阶极点").to_string(),
                        (Kind::Zero, n) => if i18n::zh() { format!("{n} 阶零点") } else { format!("zero of order {n}") },
                        (Kind::Pole, n) => if i18n::zh() { format!("{n} 阶极点") } else { format!("pole of order {n}") },
                    };
                    // Orientation-reversing points (non-holomorphic f).
                    let flipped = (s.kind == Kind::Zero) != (s.index > 0);
                    let kind = if flipped { format!("{kind}  ({} {})", tr("winding", "环绕数"), s.index.to_string().replace('-', "−")) } else { kind };
                    p.text(pos2(r.min.x + 22.0, y), Align2::LEFT_CENTER, kind, FontId::proportional(14.0), t.text);
                    p.text(pos2(r.max.x - 4.0, y), Align2::RIGHT_CENTER, fmt_cs(s.z), FontId::monospace(12.5), t.muted);
                    if let Some(res) = s.residue {
                        p.text(pos2(r.min.x + 22.0, y + 17.0), Align2::LEFT_CENTER, format!("{} = {}", tr("residue", "留数"), fmt_c(res)), FontId::monospace(12.0), t.accent_text);
                    }
                    if resp.clicked() {
                        goto = Some(s.z);
                    }
                }
                if specials.len() > 16 {
                    caption(ui, &format!("{} {}", tr("… and more:", "……另有"), specials.len() - 16));
                }
                if !holo && specials.iter().any(|s| s.kind == Kind::Pole) {
                    caption(ui, tr("f is not holomorphic, so no residues are given.", "f 非全纯，因此不给出留数。"));
                }
            },
        );
        self.show_specials = show;
        if let Some(z) = goto {
            self.z0 = z;
            self.show_z0 = true;
        }

        // ---- Contour integral ----
        let mut cc = self.contour_c;
        let mut r = self.contour_r;
        let mut show = self.show_contour;
        let res = self.contour;
        let inside: Vec<Special> = self.specials.iter().filter(|s| (s.z - cc).norm() < r).copied().collect();
        let z0 = self.z0;
        card(
            ui,
            "c_contour",
            icon::INTEGRAL,
            tr("Contour integral", "围道积分"),
            false,
            |ui| {
                eye(ui, &mut show, tr("Show the circle", "显示圆周"));
            },
            |ui| {
                caption(ui, tr("Counter-clockwise circle |z − c| = r.", "逆时针圆周 |z − c| = r。"));
                egui::Grid::new("c_contour").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new(tr("centre c", "圆心 c")).color(t.muted));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut cc.re).speed(0.01).max_decimals(4));
                        ui.label("+");
                        ui.add(egui::DragValue::new(&mut cc.im).speed(0.01).max_decimals(4));
                        ui.label("i");
                    });
                    ui.end_row();
                    ui.label(RichText::new(tr("radius r", "半径 r")).color(t.muted));
                    ui.add(egui::DragValue::new(&mut r).speed(0.01).range(1e-4..=1e4).max_decimals(4));
                    ui.end_row();
                });
                if ui.button(with_icon(icon::CROSSHAIRS, tr("Centre at z₀", "以 z₀ 为圆心"))).clicked() {
                    cc = z0;
                }
                let Some(res) = res else { return };
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("∮ f dz").size(16.0).color(t.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(fmt_c(res.integral)).monospace().size(15.0).strong().color(t.accent_text));
                    });
                });
                if res.singular {
                    ui.label(RichText::new(with_icon(icon::ALERT, tr("The circle passes through a point where f is undefined.", "圆周经过 f 无定义的点。"))).size(12.5).color(t.error));
                    return;
                }
                kv(ui, &t, tr("error", "误差"), tr("estimate", "估计"), &format!("± {}", crate::app::fmt_sci(res.error)));
                kv(ui, &t, "N − P", tr("winding of f(γ) about 0", "f(γ) 绕 0 的圈数"), &fmt_short(res.winding));
                let zs: i32 = inside.iter().filter(|s| s.kind == Kind::Zero).map(|s| s.order).sum();
                let ps: i32 = inside.iter().filter(|s| s.kind == Kind::Pole).map(|s| s.order).sum();
                kv(ui, &t, tr("inside", "圆内"), tr("zeros / poles with order", "零点 / 极点（计重数）"), &format!("{zs} / {ps}"));
                if holo && !res.jumps {
                    let sum: C64 = inside.iter().filter_map(|s| s.residue).sum();
                    let rhs = c(0.0, std::f64::consts::TAU) * sum;
                    kv(ui, &t, "2πi Σ Res", tr("residue theorem", "留数定理"), &fmt_c(rhs));
                    let ok = (rhs - res.integral).norm() <= 1e-6 * res.integral.norm().max(1.0);
                    let wn_ok = ((zs - ps) as f64 - res.winding).abs() < 1e-3;
                    ui.horizontal_wrapped(|ui| {
                        if ok {
                            badge(ui, &with_icon(icon::CHECK, tr("∮ f dz = 2πi Σ Res", "∮ f dz = 2πi Σ Res")), t.ok);
                        } else {
                            badge(ui, &with_icon(icon::ALERT, tr("differs: a singularity may lie outside the view", "不一致：可能有奇点不在视图内")), t.warn);
                        }
                        if wn_ok {
                            badge(ui, &with_icon(icon::CHECK, tr("argument principle", "辐角原理")), t.ok)
                                .on_hover_text(tr("(1/2πi) ∮ f'/f dz = N − P", "(1/2πi) ∮ f'/f dz = N − P"));
                        }
                    });
                } else if res.jumps {
                    ui.label(RichText::new(with_icon(icon::ALERT, tr("f jumps along the circle (branch cut or discontinuity): the residue theorem does not apply.", "f 沿圆周有跳跃（支割线或间断）：留数定理不适用。"))).size(12.5).color(t.warn));
                } else {
                    caption(ui, tr("f is not holomorphic: Cauchy's theorem does not apply, so ∮ f dz need not vanish.", "f 非全纯：柯西定理不适用，∮ f dz 不一定为 0。"));
                }
            },
        );
        if cc != self.contour_c || r != self.contour_r {
            show = true;
        }
        self.contour_c = cc;
        self.contour_r = r.max(1e-4);
        self.show_contour = show;

        // ---- Colouring ----
        let mut st = self.style;
        let mut res_scale = self.res;
        let step = self.grid_step;
        card(ui, "c_colour", icon::LAYERS, tr("Domain colouring", "定义域着色"), false, |_| {}, |ui| {
            caption(ui, tr("Colour shows arg f(z): red = 0, yellow = 60°, green = 120°, cyan = 180°, blue = 240°, magenta = 300°. Around a zero the colours run counter-clockwise in this order, around a pole clockwise.", "颜色表示 arg f(z)：红 0°、黄 60°、绿 120°、青 180°、蓝 240°、品红 300°。绕零点颜色按此顺序逆时针排列，绕极点则顺时针。"));
            let mut shade = if st.lightness { Shade::Lightness } else { Shade::Phase };
            segmented(
                ui,
                &mut shade,
                &[
                    (Shade::Phase, tr("Phase", "相位"), tr("Hue only", "只用色相")),
                    (Shade::Lightness, tr("Phase + |f|", "相位 + |f|"), tr("Zeros dark, poles light", "零点暗、极点亮")),
                ],
            );
            st.lightness = shade == Shade::Lightness;
            ui.checkbox(&mut st.modulus, tr("Modulus bands (|f| = 2ᵏ)", "模的分层（|f| = 2ᵏ）"));
            ui.checkbox(&mut st.phase_lines, tr("Phase bands (every 30°)", "辐角分层（每 30°）"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.fgrid, tr("Grid of Re f, Im f", "Re f、Im f 网格"));
                if st.fgrid {
                    ui.label(RichText::new(format!("{} {}", tr("step", "间距"), fmt_short(step))).size(12.0).color(t.faint));
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Resolution", "分辨率")).color(t.muted));
                ui.add(egui::Slider::new(&mut res_scale, 0.2..=1.0).max_decimals(2));
            });
        });
        self.style = st;
        self.res = res_scale;

        // ---- Relief ----
        let mut height = self.height;
        let mut n = self.relief_n;
        let mut wire = self.wire;
        card(ui, "c_relief", icon::CUBE, tr("Relief", "三维曲面"), false, |_| {}, |ui| {
            caption(ui, tr("Height over the z-plane, coloured by arg f.", "z 平面上的高度，按 arg f 着色。"));
            segmented(
                ui,
                &mut height,
                &[
                    (Height::Modulus, "|f|", tr("Modulus: zeros touch the floor, poles rise to infinity", "模：零点贴地，极点趋于无穷")),
                    (Height::LogModulus, "ln|f|", tr("Log-modulus: zeros and poles become symmetric", "对数模：零点和极点对称")),
                    (Height::Re, "Re f", tr("Real part (a harmonic function)", "实部（调和函数）")),
                    (Height::Im, "Im f", tr("Imaginary part (a harmonic function)", "虚部（调和函数）")),
                ],
            );
            ui.horizontal(|ui| {
                ui.checkbox(&mut wire, tr("Mesh lines", "网格线"));
                ui.label(RichText::new(tr("Resolution", "分辨率")).color(t.muted));
                ui.add(egui::Slider::new(&mut n, 41..=321).logarithmic(true));
            });
        });
        self.height = height;
        self.relief_n = n;
        self.wire = wire;

        // ---- Mapping ----
        let mut grid_n = self.grid_n;
        let mut polar = self.polar;
        let mut refit = false;
        card(ui, "c_mapping", icon::MAP, tr("Conformal map", "共形映射"), false, |_| {}, |ui| {
            caption(ui, tr("A grid in the z-plane and its image under f. Where f' ≠ 0 the image curves still cross at right angles.", "z 平面上的网格及其在 f 下的像。在 f' ≠ 0 处，像曲线仍以直角相交。"));
            segmented(ui, &mut polar, &[(false, tr("Cartesian", "直角坐标"), ""), (true, tr("Polar", "极坐标"), "")]);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Lines", "线数")).color(t.muted));
                ui.add(egui::Slider::new(&mut grid_n, 3..=24));
            });
            if ui.button(with_icon(icon::RESTORE, tr("Fit the w-plane", "w 平面自动缩放"))).clicked() {
                refit = true;
            }
        });
        self.grid_n = grid_n;
        self.polar = polar;
        if refit {
            self.wfit_key.clear();
        }

        // ---- View ----
        let mut pl = self.plane;
        card(ui, "c_view", icon::RULER, tr("View", "视图范围"), false, |_| {}, |ui| {
            egui::Grid::new("c_view").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new(tr("centre", "中心")).color(t.muted));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut pl.center.re).speed(pl.half * 0.005).max_decimals(4));
                    ui.label("+");
                    ui.add(egui::DragValue::new(&mut pl.center.im).speed(pl.half * 0.005).max_decimals(4));
                    ui.label("i");
                });
                ui.end_row();
                ui.label(RichText::new(tr("half-width", "半宽")).color(t.muted));
                let speed = pl.half * 0.005;
                ui.add(egui::DragValue::new(&mut pl.half).speed(speed).range(1e-6..=1e6).max_decimals(4));
                ui.end_row();
            });
            if ui.button(with_icon(icon::RESTORE, tr("Reset view", "重置视图"))).clicked() {
                pl = Plane::new(c(0.0, 0.0), 3.0);
            }
        });
        self.plane = pl;

        // ---- Derivative formula ----
        if let Some(f) = &self.func
            && f.has_symbolic_derivative()
            && f.holomorphic
        {
            card(ui, "c_formula", icon::INTEGRAL, tr("Derivative f'(z)", "导数 f'(z)"), false, |_| {}, |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("LaTeX").on_hover_text(tr("Copy as LaTeX", "复制为 LaTeX")).clicked() {
                            ui.ctx().copy_text(f.latex(1));
                        }
                        if ui.small_button(with_icon(icon::COPY, "")).on_hover_text(tr("Copy as plain text", "复制为纯文本")).clicked() {
                            ui.ctx().copy_text(f.text(1));
                        }
                    });
                });
                let (pool, id) = f.expr(1);
                egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::horizontal().id_salt("cf_deriv").show(ui, |ui| {
                        if !mathview::show(ui, pool, id, &f.names(), "f'", 15.0, t.text) {
                            ui.add(egui::Label::new(RichText::new(f.text(1)).monospace().size(12.5)).wrap());
                        }
                    });
                });
            });
        }
    }

    // ---- Canvases --------------------------------------------------------

    /// Left click / drag places z0; returns true if it moved.
    fn place_z0(&mut self, resp: &egui::Response, plane: &Plane, rect: Rect) -> bool {
        if (resp.clicked_by(egui::PointerButton::Primary) || resp.dragged_by(egui::PointerButton::Primary))
            && let Some(p) = resp.interact_pointer_pos()
        {
            self.z0 = plane.from_screen(rect, p);
            self.show_z0 = true;
            return true;
        }
        false
    }

    fn colour_canvas(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let moved = self.plane.pan_zoom(ui, &resp, rect);
        self.interacting = moved || resp.dragged_by(egui::PointerButton::Secondary);
        if resp.double_clicked() {
            self.plane = Plane::new(c(0.0, 0.0), 3.0);
        } else {
            let plane = self.plane;
            self.place_z0(&resp, &plane, rect);
        }
        self.last_ranges = Some(self.plane.ranges(rect));
        let Some(f) = &self.func else { return };
        let ppp = ui.ctx().pixels_per_point();
        let scale = if self.interacting { (self.res * 0.5).max(0.15) } else { self.res } * ppp;
        let (w, h) = (((rect.width() * scale) as usize).max(8), ((rect.height() * scale) as usize).max(8));
        let (re, im) = self.plane.ranges(rect);
        let key = format!("{}|{re:?}|{im:?}|{w}x{h}|{:?}", f.source, self.style);
        if key != self.tex_key {
            let ws = f.sample(re, im, w, h);
            self.grid_step = complex_view::auto_grid_step(&ws);
            let img = complex_view::image(&ws, w, h, &self.style, self.grid_step);
            match &mut self.tex {
                Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                None => self.tex = Some(ui.ctx().load_texture("domain_colouring", img, egui::TextureOptions::LINEAR)),
            }
            self.tex_key = key;
        }
        if self.interacting {
            // Sharpen once the view stops moving.
            ui.ctx().request_repaint();
        }
        if let Some(tex) = &self.tex {
            egui::Image::new((tex.id(), rect.size())).corner_radius(12).paint_at(ui, rect);
        }
        self.plane.draw_axes(ui, rect, t, ("Re z", "Im z"), true);
        let p = ui.painter_at(rect);
        let plane = self.plane;
        self.draw_marks(&p, rect, &plane);
        // Hover readout.
        if let Some(hp) = resp.hover_pos()
            && resp.hovered()
            && !resp.dragged()
        {
            let z = plane.from_screen(rect, hp);
            let w = f.eval(z);
            let txt = format!("z = {}\nf = {}\n|f| = {}\narg = {}", fmt_cs(z), fmt_cs(w), fmt_short(w.norm()), deg(w.arg()));
            tooltip_box(&p, rect, hp, &txt, t);
        }
        p.rect_stroke(rect, 12, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
    }

    /// z0, the contour circle and zeros/poles on a 2D plane.
    fn draw_marks(&self, p: &egui::Painter, rect: Rect, plane: &Plane) {
        if self.show_contour {
            let pts: Vec<Pos2> = (0..=180).map(|k| plane.to_screen(rect, self.contour_c + C64::from_polar(self.contour_r, std::f64::consts::TAU * k as f64 / 180.0))).collect();
            p.add(egui::Shape::line(pts.clone(), Stroke::new(4.0, Color32::from_black_alpha(110))));
            p.add(egui::Shape::line(pts, Stroke::new(2.0, c32(C_CONTOUR))));
            // Direction arrow at the top of the circle.
            let top = plane.to_screen(rect, self.contour_c + c(0.0, self.contour_r));
            p.add(egui::Shape::convex_polygon(vec![top + vec2(-6.0, 0.0), top + vec2(3.0, -5.0), top + vec2(3.0, 5.0)], c32(C_CONTOUR), Stroke::new(1.0, Color32::from_black_alpha(140))));
        }
        if self.show_specials {
            for s in &self.specials {
                let q = plane.to_screen(rect, s.z);
                if rect.contains(q) {
                    marker(p, q, s.kind, 1.0);
                    if s.order > 1 {
                        let txt = s.order.to_string();
                        p.text(q + vec2(9.0, -9.0) + vec2(1.0, 1.0), Align2::LEFT_BOTTOM, &txt, FontId::proportional(13.0), Color32::BLACK);
                        p.text(q + vec2(9.0, -9.0), Align2::LEFT_BOTTOM, txt, FontId::proportional(13.0), Color32::WHITE);
                    }
                }
            }
        }
        if self.show_z0 {
            point_marker(p, plane.to_screen(rect, self.z0), "z₀");
        }
    }

    fn relief_canvas(&mut self, ui: &mut Ui, frame: &mut eframe::Frame, rect: Rect, t: &Tokens) {
        self.interacting = false;
        let Some(f) = &self.func else { return };
        let half = self.plane.half;
        let center = self.plane.center;
        let key = format!("{}|{center:?}|{half}|{}|{:?}|{:?}|{}|{}|{}|{:?}|{}|{}|{:?}", f.source, self.relief_n, self.height, self.style, self.wire, self.show_z0, self.show_specials, self.z0, self.show_contour, self.contour_r, self.contour_c);
        if key != self.relief_key {
            let r = complex_view::relief(f, center, half, self.relief_n, self.height, &self.style, self.wire);
            let mut scene = Scene::default();
            let lift = |z: C64, w: C64| r.frame.map_xyz(z.re, z.im, self.height.of(w).clamp(r.zr.0, r.zr.1));
            if self.show_contour {
                let pts: Vec<Option<glam::Vec3>> = (0..=360)
                    .map(|k| {
                        let z = self.contour_c + C64::from_polar(self.contour_r, std::f64::consts::TAU * k as f64 / 360.0);
                        let w = f.eval(z);
                        w.is_finite().then(|| lift(z, w))
                    })
                    .collect();
                polyline(&mut scene.lines, &pts, hex(0xffffff, 1.0), 2.5);
            }
            if self.show_specials {
                for s in &self.specials {
                    if (s.z.re - center.re).abs() > half || (s.z.im - center.im).abs() > half {
                        continue;
                    }
                    let zc = if s.kind == Kind::Zero { r.zr.0 } else { r.zr.1 };
                    let zc = if self.height == Height::Modulus || self.height == Height::LogModulus { zc } else { (r.zr.0 + r.zr.1) / 2.0 };
                    let pos = r.frame.map_xyz(s.z.re, s.z.im, zc);
                    scene.meshes.push(sphere(pos, 0.03, hex(if s.kind == Kind::Zero { C_ZERO } else { C_POLE }, 1.0)));
                }
            }
            if self.show_z0 {
                let w = f.eval(self.z0);
                if w.is_finite() {
                    let top = lift(self.z0, w);
                    let floor = r.frame.map_xyz(self.z0.re, self.z0.im, r.zr.0);
                    scene.meshes.push(sphere(top, 0.035, hex(C_Z0, 1.0)));
                    scene.lines.push(line(floor, top, hex(C_Z0, 0.8), 1.5, true));
                }
            }
            scene.meshes.insert(0, r.mesh);
            self.scene = scene;
            self.frame3 = r.frame;
            self.relief_zr = r.zr;
            self.relief_key = key;
            self.scene_version += 1;
        }
        let resp = self.view.allocate(ui, rect);
        let _ = resp;
        let proj = self.view.projector(rect);
        let theme = if t.dark { Theme::Dark } else { Theme::Light };
        let settings = calc_view::Settings {
            xr: (center.re - half, center.re + half),
            yr: (center.im - half, center.im + half),
            zr: self.relief_zr,
            show_grid: false,
            show_ticks: true,
            theme,
            ..Default::default()
        };
        let hname = match self.height {
            Height::Modulus => "|f|",
            Height::LogModulus => "ln|f|",
            Height::Re => "Re f",
            Height::Im => "Im f",
        };
        let (deco, labels) = calc_view::decorations(&settings, &self.frame3, &self.view.camera, &proj, ["Re z", "Im z", hname]);
        let pal = theme.palette();
        if !self.view.render(ui, frame, rect, &self.scene, self.scene_version, deco, self.frame3.half, pal.background, pal.wire) {
            return;
        }
        canvas3d::paint_labels(&ui.painter_at(rect), rect, &proj, labels);
    }

    fn mapping_canvas(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let gap = 10.0;
        let zr = Rect::from_min_max(rect.min, pos2(rect.center().x - gap / 2.0, rect.max.y));
        let wr = Rect::from_min_max(pos2(rect.center().x + gap / 2.0, rect.min.y), rect.max);
        let bg = if t.dark { Color32::from_rgb(0x16, 0x18, 0x20) } else { Color32::from_rgb(0xfc, 0xfc, 0xfe) };
        let zresp = ui.allocate_rect(zr, Sense::click_and_drag());
        let wresp = ui.allocate_rect(wr, Sense::click_and_drag());
        let moved = self.plane.pan_zoom(ui, &zresp, zr);
        self.wplane.pan_zoom(ui, &wresp, wr);
        self.interacting = moved || zresp.dragged_by(egui::PointerButton::Secondary);
        if zresp.double_clicked() {
            self.plane = Plane::new(c(0.0, 0.0), 3.0);
        } else {
            let plane = self.plane;
            self.place_z0(&zresp, &plane, zr);
        }
        if wresp.double_clicked() {
            self.wfit_key.clear();
        }
        self.last_ranges = Some(self.plane.ranges(zr));
        let Some(f) = &self.func else { return };
        let (re, im) = self.plane.ranges(zr);
        let key = format!("{}|{re:?}|{im:?}|{}|{}", f.source, self.grid_n, self.polar);
        if key != self.map_key {
            self.map_lines = complex_view::grid_lines(re, im, self.grid_n, self.polar);
            let all: Vec<C64> = self.map_lines.iter().flat_map(|l| l.pts.iter().copied()).collect();
            let ws = f.eval_many(&all);
            let mut k = 0;
            self.map_images = self
                .map_lines
                .iter()
                .map(|l| {
                    let v = ws[k..k + l.pts.len()].to_vec();
                    k += l.pts.len();
                    v
                })
                .collect();
            self.map_key = key;
        }
        let fit_key = format!("{}|{}|{}", f.source, self.grid_n, self.polar);
        if fit_key != self.wfit_key {
            let all: Vec<C64> = self.map_images.iter().flatten().copied().collect();
            if let Some((cw, hw)) = complex_view::fit(&all) {
                self.wplane = Plane::new(cw, hw);
            }
            self.wfit_key = fit_key;
        }
        let p = ui.painter();
        for r in [zr, wr] {
            p.rect_filled(r, 12, bg);
        }
        self.plane.draw_axes(ui, zr, t, ("Re z", "Im z"), false);
        self.wplane.draw_axes(ui, wr, t, ("Re w", "Im w"), false);
        let pz = ui.painter_at(zr);
        let pw = ui.painter_at(wr);
        let plane = self.plane;
        let wplane = self.wplane;
        for (l, img) in self.map_lines.iter().zip(&self.map_images) {
            let col = complex_view::line_color(l.family, l.t, t.dark);
            let width = if l.axis { 2.4 } else { 1.3 };
            let zp: Vec<Option<Pos2>> = l.pts.iter().map(|&z| Some(plane.to_screen(zr, z))).collect();
            draw_path(&pz, &zp, Stroke::new(width, col), zr);
            let wp: Vec<Option<Pos2>> = img.iter().map(|&w| w.is_finite().then(|| wplane.to_screen(wr, w))).collect();
            draw_path(&pw, &wp, Stroke::new(width, col), wr);
        }
        // Contour circle and its image.
        if self.show_contour {
            let zs: Vec<C64> = (0..=360).map(|k| self.contour_c + C64::from_polar(self.contour_r, std::f64::consts::TAU * k as f64 / 360.0)).collect();
            let ws = f.eval_many(&zs);
            let ink = t.text;
            draw_path(&pz, &zs.iter().map(|&z| Some(plane.to_screen(zr, z))).collect::<Vec<_>>(), Stroke::new(2.5, ink), zr);
            draw_path(&pw, &ws.iter().map(|&w| w.is_finite().then(|| wplane.to_screen(wr, w))).collect::<Vec<_>>(), Stroke::new(2.5, ink), wr);
            let o = wplane.to_screen(wr, c(0.0, 0.0));
            pw.circle_stroke(o, 5.0, Stroke::new(1.5, ink));
        }
        // z0, a small right-angled cross and their images.
        if self.show_z0 {
            // About 30 px long in the z-plane; its image is scaled by |f'|.
            let eps = 30.0 * self.plane.units_per_point(zr);
            let w0 = f.eval(self.z0);
            for (dir, family) in [(c(1.0, 0.0), 1u8), (c(0.0, 1.0), 0u8)] {
                let col = complex_view::line_color(family, 0.5, t.dark);
                let zs: Vec<C64> = (0..=20).map(|k| self.z0 + dir * (eps * k as f64 / 20.0)).collect();
                let ws = f.eval_many(&zs);
                arrow(&pz, &zs.iter().map(|&z| plane.to_screen(zr, z)).collect::<Vec<_>>(), col);
                if ws.iter().all(|w| w.is_finite()) {
                    arrow(&pw, &ws.iter().map(|&w| wplane.to_screen(wr, w)).collect::<Vec<_>>(), col);
                }
            }
            point_marker(&pz, plane.to_screen(zr, self.z0), "z₀");
            if w0.is_finite() {
                point_marker(&pw, wplane.to_screen(wr, w0), "f(z₀)");
            }
        }
        if self.show_specials {
            for s in &self.specials {
                let q = plane.to_screen(zr, s.z);
                if zr.contains(q) {
                    marker(&pz, q, s.kind, 1.0);
                }
            }
        }
        let title = |p: &egui::Painter, r: Rect, s: &str| {
            let g = p.layout_no_wrap(s.to_string(), FontId::proportional(13.5), t.text);
            let at = pos2(r.left() + 52.0, r.top() + 8.0);
            p.rect_filled(Rect::from_min_size(at - vec2(6.0, 3.0), g.size() + vec2(12.0, 6.0)), 6, t.overlay);
            p.galley(at, g, t.text);
        };
        title(&pz, zr, tr("z-plane", "z 平面"));
        title(&pw, wr, tr("w = f(z)", "w = f(z) 平面"));
        for r in [zr, wr] {
            ui.painter().rect_stroke(r, 12, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        }
    }

    fn overlays(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let inner = rect.shrink(12.0);
        // Top-left: values at z0 (not over the split mapping view).
        if self.show_z0
            && self.mode != Mode::Mapping
            && let Some(f) = &self.func
        {
            let mut tl = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
            let w = f.eval(self.z0);
            overlay_frame(t).show(&mut tl, |ui| {
                egui::Grid::new("z0_vals").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
                    let row = |ui: &mut Ui, k: &str, v: String| {
                        ui.label(RichText::new(k).color(t.muted));
                        ui.label(RichText::new(v).monospace().color(t.text));
                        ui.end_row();
                    };
                    row(ui, "z₀", fmt_cs(self.z0));
                    row(ui, "f(z₀)", fmt_cs(w));
                    row(ui, "|f|", fmt_short(w.norm()));
                    row(ui, "arg f", deg(w.arg()));
                });
            });
        }
        // Top-right: mode switch, 3D controls, export.
        let mut top_right = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Min)));
        overlay_frame(t).inner_margin(egui::Margin::same(5)).show(&mut top_right, |ui| {
            ui.horizontal(|ui| {
                if ui.button(with_icon(icon::CAMERA, "")).on_hover_text(crate::ui::export::hint(tr("Save the view as PNG in", "把视图保存为 PNG，位置："))).clicked() {
                    self.export.start();
                }
                if self.mode == Mode::Relief {
                    self.view.projection_controls(ui);
                    ui.add_space(4.0);
                    self.view.preset_controls(ui, ["Re z", "Im z", "|f|"]);
                    ui.add_space(4.0);
                }
                segmented(
                    ui,
                    &mut self.mode,
                    &[
                        (Mode::Colour, &with_icon(icon::TARGET, tr("Colouring", "着色")), tr("Domain colouring: hue = arg f", "定义域着色：色相 = arg f")),
                        (Mode::Relief, &with_icon(icon::CUBE, tr("Relief", "曲面")), tr("3D relief coloured by arg f", "按 arg f 着色的三维曲面")),
                        (Mode::Mapping, &with_icon(icon::MAP, tr("Mapping", "映射")), tr("Conformal map: a grid and its image", "共形映射：网格及其像")),
                    ],
                );
            });
        });
        // Bottom-left: colour wheel.
        if self.mode != Mode::Mapping {
            colour_wheel(ui, pos2(inner.left() + 46.0, inner.bottom() - 46.0), &self.style, t);
        }
        let mut chips: Vec<(&mut bool, &str, &str, Option<Color32>, &str)> = vec![
            (&mut self.show_z0, tr("Point z₀", "点 z₀"), "z₀", Some(c32(C_Z0)), tr("The point z₀ and its image", "点 z₀ 及其像")),
            (&mut self.show_specials, tr("Zeros & poles", "零点与极点"), tr("Zeros", "零极点"), None, tr("Mark zeros and poles", "标出零点与极点")),
            (&mut self.show_contour, tr("Contour", "围道"), tr("Contour", "围道"), None, tr("The integration circle", "积分圆周")),
        ];
        if self.mode != Mode::Mapping {
            chips.push((&mut self.style.modulus, tr("|f| bands", "模分层"), "|f|", None, tr("Level curves of |f|", "|f| 的等值线")));
            chips.push((&mut self.style.phase_lines, tr("Phase bands", "辐角分层"), "arg", None, tr("Level curves of arg f", "arg f 的等值线")));
            chips.push((&mut self.style.fgrid, tr("Re/Im grid", "Re/Im 网格"), tr("Grid", "网格"), None, tr("Lines where Re f or Im f is a multiple of a step", "Re f 或 Im f 为间距整数倍的线")));
        }
        layer_bar(ui, inner, &mut self.layers_open, chips);
    }
}

/// Zero: open ring; pole: filled light square. Both outlined for contrast.
fn marker(p: &egui::Painter, q: Pos2, kind: Kind, s: f32) {
    match kind {
        Kind::Zero => {
            p.circle_stroke(q, 6.0 * s, Stroke::new(4.0 * s, Color32::from_black_alpha(160)));
            p.circle_stroke(q, 6.0 * s, Stroke::new(2.0 * s, Color32::WHITE));
        }
        Kind::Pole => {
            let r = Rect::from_center_size(q, vec2(10.0, 10.0) * s);
            p.rect(r, 2, c32(C_POLE), Stroke::new(1.5 * s, Color32::from_black_alpha(200)), egui::StrokeKind::Outside);
        }
    }
}

fn point_marker(p: &egui::Painter, q: Pos2, label: &str) {
    p.circle_filled(q, 6.0, Color32::WHITE);
    p.circle_filled(q, 4.5, c32(C_Z0));
    let galley = p.layout_no_wrap(label.to_string(), FontId::proportional(13.0), Color32::WHITE);
    let r = Rect::from_min_size(q + vec2(9.0, -24.0), galley.size() + vec2(10.0, 4.0));
    p.rect_filled(r, 7, c32(C_Z0));
    p.galley(r.min + vec2(5.0, 2.0), galley, Color32::WHITE);
}

/// Polyline that breaks at gaps and at jumps (poles, branch cuts).
fn draw_path(p: &egui::Painter, pts: &[Option<Pos2>], stroke: Stroke, rect: Rect) {
    let jump = rect.width().max(rect.height()) * 0.3;
    let far = rect.expand(rect.width().max(rect.height()) * 4.0);
    let mut run: Vec<Pos2> = Vec::new();
    let flush = |run: &mut Vec<Pos2>| {
        if run.len() > 1 {
            p.add(egui::Shape::line(std::mem::take(run), stroke));
        }
        run.clear();
    };
    for q in pts {
        match q {
            Some(q) if far.contains(*q) => {
                if let Some(last) = run.last()
                    && last.distance(*q) > jump
                {
                    flush(&mut run);
                }
                run.push(*q);
            }
            _ => flush(&mut run),
        }
    }
    flush(&mut run);
}

fn arrow(p: &egui::Painter, pts: &[Pos2], col: Color32) {
    if pts.len() < 2 {
        return;
    }
    p.add(egui::Shape::line(pts.to_vec(), Stroke::new(5.0, Color32::from_black_alpha(90))));
    p.add(egui::Shape::line(pts.to_vec(), Stroke::new(3.0, col)));
    let tip = pts[pts.len() - 1];
    let back = pts[pts.len() - 2];
    let d = (tip - back).normalized();
    if d.is_finite() {
        let n = vec2(-d.y, d.x);
        p.add(egui::Shape::convex_polygon(vec![tip + d * 4.0, tip - d * 7.0 + n * 5.0, tip - d * 7.0 - n * 5.0], col, Stroke::NONE));
    }
}

pub(crate) fn tooltip_box(p: &egui::Painter, rect: Rect, at: Pos2, txt: &str, t: &Tokens) {
    let galley = p.layout_no_wrap(txt.to_string(), FontId::monospace(12.0), t.text);
    let size = galley.size() + vec2(14.0, 10.0);
    let mut r = Rect::from_min_size(at + vec2(16.0, 12.0), size);
    if r.max.x > rect.max.x - 4.0 {
        r = r.translate(vec2(-size.x - 28.0, 0.0));
    }
    if r.max.y > rect.max.y - 4.0 {
        r = r.translate(vec2(0.0, -size.y - 20.0));
    }
    p.rect(r, 6, t.overlay, Stroke::new(1.0, t.overlay_border), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(7.0, 5.0), galley, t.text);
}

/// Hue legend: a ring coloured by arg with the main angles marked.
fn colour_wheel(ui: &Ui, c0: Pos2, st: &Style, t: &Tokens) {
    let p = ui.painter();
    let (r0, r1) = (22.0f32, 34.0f32);
    p.circle_filled(c0, r1 + 9.0, t.overlay);
    p.circle_stroke(c0, r1 + 9.0, Stroke::new(1.0, t.overlay_border));
    let mut mesh = egui::Mesh::default();
    let n = 72;
    let flat = Style { modulus: false, phase_lines: st.phase_lines, lightness: false, fgrid: false };
    for k in 0..=n {
        let a = std::f64::consts::TAU * k as f64 / n as f64;
        let col = complex_view::color(C64::from_polar(1.0, a), &flat);
        let col = Color32::from_rgb((col[0] * 255.0) as u8, (col[1] * 255.0) as u8, (col[2] * 255.0) as u8);
        let dir = vec2(a.cos() as f32, -a.sin() as f32);
        mesh.colored_vertex(c0 + dir * r0, col);
        mesh.colored_vertex(c0 + dir * r1, col);
        if k > 0 {
            let i = (2 * k) as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    p.add(mesh);
    let font = FontId::proportional(10.5);
    p.text(c0, Align2::CENTER_CENTER, "arg", font.clone(), t.muted);
    // Ticks at 0°, 90°, 180°, 270°.
    for a in [0.0f32, 90.0, 180.0, 270.0] {
        let r = a.to_radians();
        let d = vec2(r.cos(), -r.sin());
        p.line_segment([c0 + d * (r1 + 1.0), c0 + d * (r1 + 6.0)], Stroke::new(1.5, t.text));
    }
}
