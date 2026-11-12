//! The Spectrum workspace: an absolutely integrable f(t), its Fourier
//! transform (magnitude and phase or real and imaginary parts),
//! band-limited reconstruction, Parseval and the uncertainty principle.

use std::time::Instant;

use egui::{Align, Color32, FontId, Layout, RichText, Stroke, Ui, vec2};
use egui_plot::{HoverPosition, Line, LineStyle, Plot, PlotPoints, Span, VLine};
use num_complex::Complex64 as C64;

use crate::app::{fmt_sci, fmt_short, fmt_val, kv};
use crate::complex_app::fmt_c;
use crate::i18n::{self, tr};
use crate::spectrum::{Convention, Signal, Spectrum, Support, Symmetry, symmetry};
use crate::ui::export::{self, Export};
use crate::ui::mathview;
use crate::ui::theme::{Tokens, icon, tokens, with_icon};
use crate::ui::widgets::{badge, caption, card, chip, eye, overlay_frame, segmented};

pub const DEFAULT_FUNCTION: &str = "rect(t)";

const EXAMPLES: &[(&str, &str, &str)] = &[
    ("Rectangular pulse (default)", "矩形脉冲（默认）", DEFAULT_FUNCTION),
    ("Triangular pulse", "三角脉冲", "tri(t)"),
    ("Gaussian", "高斯函数", "exp(-t^2/2)"),
    ("Two-sided exponential", "双边指数", "exp(-|t|)"),
    ("One-sided exponential", "单边指数", "step(t) exp(-t)"),
    ("Lorentzian 1/(1+t²)", "洛伦兹函数 1/(1+t²)", "1/(1 + t^2)"),
    ("Shifted pulse: linear phase", "平移脉冲：线性相位", "rect(t - 2)"),
    ("Two pulses: interference", "两个脉冲：干涉", "rect(t - 2) + rect(t + 2)"),
    ("Modulated Gaussian", "调制高斯脉冲", "exp(-t^2/2) cos(8t)"),
    ("Damped oscillation", "阻尼振荡", "step(t) exp(-t/2) sin(6t)"),
    ("Odd: t e^{−t²}", "奇函数：t e^{−t²}", "t exp(-t^2)"),
    ("Complex: e^{5it − t²}", "复信号：e^{5it − t²}", "exp(5i t - t^2)"),
    ("Chirp", "线性调频", "exp(-t^2/8) cos(t^2)"),
    ("Not integrable: sin t / t", "不绝对可积：sin t / t", "sin(t)/t"),
    ("LaTeX: e^{−t²} cos 5t", "LaTeX：e^{−t²} cos 5t", "e^{-t^2}\\cos(5t)"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    MagPhase,
    ReIm,
}

struct Stats {
    energy_t: f64,
    energy_w: f64,
    dt: f64,
    dw: Option<f64>,
    band99: f64,
    sym: Symmetry,
    integral: C64,
}

pub struct SpectrumWorkspace {
    src: String,
    sig: Option<Signal>,
    error: Option<String>,
    conv: Convention,
    auto_window: bool,
    window: (f64, f64),
    support: Option<Support>,
    auto_w: bool,
    /// Displayed |ω| range in rad.
    w_max: f64,
    spec: Option<Spectrum>,
    /// Display band, cached: (ω, F(ω)).
    band: Vec<(f64, C64)>,
    stats: Option<Stats>,
    spec_key: String,
    compute_ms: f64,
    view: View,
    db: bool,
    /// ω0 in rad.
    w0: f64,
    show_w0: bool,
    recon_on: bool,
    /// Ω in rad.
    recon_w: f64,
    recon: Vec<C64>,
    recon_key: String,
    export: Export,
    toast: Option<(String, Instant)>,
    reset_plots: bool,
    /// Derived state changed after the sidebar was drawn: repaint once more.
    dirty: bool,
    /// Plot ids and the x-axis link include this; bumped on reset so the
    /// plots forget their old bounds.
    plot_gen: u32,
    /// Put Ω back to a quarter of the range for a new signal.
    recon_fresh: bool,
}

impl Default for SpectrumWorkspace {
    fn default() -> Self {
        Self::new(DEFAULT_FUNCTION)
    }
}

const C_RE: Color32 = Color32::from_rgb(0x1c, 0x7e, 0xd6);
const C_IM: Color32 = Color32::from_rgb(0xf0, 0x8c, 0x00);
const C_RECON: Color32 = Color32::from_rgb(0xe0, 0x31, 0x31);
const C_W0: Color32 = Color32::from_rgb(0x70, 0x48, 0xe8);

impl SpectrumWorkspace {
    pub fn new(src: &str) -> Self {
        let mut s = Self {
            src: src.to_string(),
            sig: None,
            error: None,
            conv: Convention::Angular,
            auto_window: true,
            window: (-1.0, 1.0),
            support: None,
            auto_w: true,
            w_max: 40.0,
            spec: None,
            band: vec![],
            stats: None,
            spec_key: String::new(),
            compute_ms: 0.0,
            view: View::MagPhase,
            db: false,
            w0: 1.0,
            show_w0: false,
            recon_on: false,
            recon_w: 10.0,
            recon: vec![],
            recon_key: String::new(),
            export: Export::default(),
            toast: None,
            reset_plots: true,
            dirty: false,
            plot_gen: 0,
            recon_fresh: true,
        };
        s.recompile();
        s
    }

    pub fn set_function(&mut self, src: &str) {
        self.src = src.to_string();
        self.recompile();
    }

    pub fn relocalize(&mut self) {
        self.recompile();
    }

    fn recompile(&mut self) {
        match Signal::new(self.src.trim()) {
            Ok(s) => {
                let changed = self.sig.as_ref().is_none_or(|o| o.source != s.source);
                self.sig = Some(s);
                self.error = None;
                if changed {
                    self.support = None;
                    self.reset_plots = true;
                    self.recon_fresh = true;
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn update(&mut self) {
        let Some(sig) = &self.sig else { return };
        if self.support.is_none() {
            let sup = sig.support();
            if self.auto_window {
                self.window = (sup.t0, sup.t1);
            }
            self.support = Some(sup);
        }
        let key = format!("{}|{:?}|{}|{}", sig.source, self.window, self.auto_w, if self.auto_w { 0.0 } else { self.w_max });
        if key != self.spec_key {
            let t0 = Instant::now();
            let sp = sig.spectrum(self.window.0, self.window.1, (!self.auto_w).then_some(self.w_max));
            self.w_max = sp.w_max;
            self.band = sp.band(sp.w_max);
            let (dt, dw) = sp.spreads();
            let t_max = self.window.0.abs().max(self.window.1.abs());
            self.stats = Some(Stats {
                energy_t: sp.energy_time(),
                energy_w: sp.energy_freq(),
                dt,
                dw,
                band99: sp.energy_radius(0.99),
                sym: symmetry(sig, t_max),
                integral: sp.at(0.0),
            });
            if std::mem::take(&mut self.recon_fresh) || self.recon_w > sp.w_max {
                self.recon_w = sp.w_max / 4.0;
            }
            self.reset_plots = true;
            self.spec = Some(sp);
            self.compute_ms = t0.elapsed().as_secs_f64() * 1e3;
            self.spec_key = key;
            self.recon_key.clear();
            self.dirty = true;
        }
        if self.recon_on
            && let Some(sp) = &self.spec
        {
            let key = format!("{}|{}", self.spec_key, self.recon_w);
            if key != self.recon_key {
                self.recon = sp.band_limited(self.recon_w);
                self.recon_key = key;
            }
        }
    }

    /// Frequency-axis symbol and unit.
    fn wname(&self) -> &'static str {
        match self.conv {
            Convention::Angular => "ω",
            Convention::Hertz => "ν",
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let t = tokens(ui.ctx());
        self.status_bar(ui, &t);
        egui::Panel::left("spectrum_controls")
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
        self.update();
        if std::mem::take(&mut self.dirty) {
            ui.ctx().request_repaint();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin::same(10)))
            .show(ui, |ui| self.plots(ui, &t));
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        egui::Panel::bottom("spectrum_status")
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(14, 5)).stroke(Stroke::new(1.0, t.card_border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let small = |s: &str, c: Color32| RichText::new(s).size(12.0).color(c);
                    ui.label(small(tr("Drag to pan · Scroll to zoom along the axis · Double-click to reset · Click a spectrum to set ω₀", "拖动平移 · 滚轮沿横轴缩放 · 双击重置 · 单击频谱设置 ω₀"), t.faint));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &self.toast {
                        Some((msg, at)) if at.elapsed().as_secs_f32() < 5.0 => {
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                            ui.label(small(msg, if export::is_ok(msg) { t.ok } else { t.error }));
                        }
                        _ => {
                            if let Some(sp) = &self.spec {
                                let msg = if i18n::zh() {
                                    format!("{} 个采样，FFT {}，用时 {:.1} ms", sp.t.len(), sp.n, self.compute_ms)
                                } else {
                                    format!("{} samples, FFT size {}, {:.1} ms", sp.t.len(), sp.n, self.compute_ms)
                                };
                                ui.label(small(&msg, t.faint));
                            }
                        }
                    });
                });
            });
    }

    // ---- Sidebar ---------------------------------------------------------

    fn controls(&mut self, ui: &mut Ui) {
        let t = tokens(ui.ctx());
        let w = self.wname();
        let mut pick: Option<&str> = None;
        card(
            ui,
            "s_function",
            icon::FUNCTION,
            tr("Signal", "信号"),
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
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut self.src)
                            .font(FontId::monospace(15.0))
                            .desired_rows(1)
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(8, 7))
                            .hint_text(tr("e.g. exp(-t^2) cos(5t)  or  \\frac{1}{1+t^2}", "如 exp(-t^2) cos(5t) 或 \\frac{1}{1+t^2}")),
                    )
                    .changed()
                {
                    self.recompile();
                }
                caption(ui, tr("Variable t; i is allowed for complex signals.", "变量为 t；复信号可以用 i。"));
                if let Some(e) = &self.error {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(with_icon(icon::ALERT, e)).size(12.5).color(t.error));
                    });
                    caption(ui, tr("Showing the last valid signal.", "下面显示的是上一个有效的信号。"));
                }
                if let Some(sig) = &self.sig {
                    let (pool, id) = sig.expr();
                    egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::horizontal().id_salt("sf_preview").show(ui, |ui| {
                            if !mathview::show(ui, pool, id, &crate::spectrum::VARS, "f(t)", 17.0, t.text) {
                                ui.label(RichText::new(&sig.source).monospace().size(12.5));
                            }
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        match &self.support {
                            Some(sup) if sup.integrable => {
                                let l1 = self.spec.as_ref().map_or(sup.l1, |sp| sp.l1());
                                badge(ui, &with_icon(icon::CHECK, &format!("{}  ∫|f| dt = {}", tr("absolutely integrable", "绝对可积"), fmt_short(l1))), t.ok);
                            }
                            Some(_) => {
                                badge(ui, &with_icon(icon::ALERT, tr("not absolutely integrable", "不绝对可积")), t.warn)
                                    .on_hover_text(tr("|f| decays too slowly for ∫|f| dt to converge. The spectrum shown is that of f cut off to the window.", "|f| 衰减太慢，∫|f| dt 不收敛。图中是把 f 截断到时间窗口后的频谱。"));
                            }
                            None => {}
                        }
                        if crate::expr::latex::looks_like_latex(&self.src) && self.error.is_none() {
                            badge(ui, &with_icon(icon::CHECK, "LaTeX"), t.ok);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button(with_icon(icon::COPY, "LaTeX")).on_hover_text(tr("Copy f as LaTeX", "复制 f 的 LaTeX")).clicked() {
                                ui.ctx().copy_text(sig.latex());
                            }
                        });
                    });
                    if let Some(sup) = &self.support
                        && !sup.integrable
                    {
                        ui.label(
                            RichText::new(with_icon(
                                icon::ALERT,
                                &format!("{} [{}, {}].", tr("Its Fourier integral does not converge; showing the transform of f cut off to", "其傅里叶积分不收敛；下面是把 f 截断到"), fmt_short(self.window.0), fmt_short(self.window.1)),
                            ))
                            .size(12.5)
                            .color(t.warn),
                        );
                    }
                }
            },
        );
        if let Some(f) = pick {
            self.set_function(f);
        }

        // ---- Transform convention ----
        let mut conv = self.conv;
        card(ui, "s_transform", icon::SINE, tr("Fourier transform", "傅里叶变换"), false, |_| {}, |ui| {
            segmented(
                ui,
                &mut conv,
                &[
                    (Convention::Angular, tr("ω  (angular)", "ω（角频率）"), tr("F(ω) = ∫ f(t) e^(−iωt) dt", "F(ω) = ∫ f(t) e^(−iωt) dt")),
                    (Convention::Hertz, tr("ν  (cycles)", "ν（频率）"), tr("F(ν) = ∫ f(t) e^(−2πiνt) dt", "F(ν) = ∫ f(t) e^(−2πiνt) dt")),
                ],
            );
            let (fwd, inv) = match conv {
                Convention::Angular => ("F(ω) = ∫ f(t) e^(−iωt) dt", "f(t) = (1/2π) ∫ F(ω) e^(iωt) dω"),
                Convention::Hertz => ("F(ν) = ∫ f(t) e^(−2πiνt) dt", "f(t) = ∫ F(ν) e^(2πiνt) dν"),
            };
            for s in [fwd, inv] {
                ui.label(RichText::new(s).monospace().size(13.0).color(t.text));
            }
            caption(ui, tr("Integrals over the whole real line. Computed with the trapezoid rule and a zero-padded FFT.", "积分区间为整个实轴。用梯形公式和补零 FFT 计算。"));
        });
        if conv != self.conv {
            self.conv = conv;
            self.reset_plots = true;
        }

        // ---- Value at ω0 ----
        let wlim = self.conv.from_omega(self.w_max);
        let mut x0 = self.conv.from_omega(self.w0);
        let mut show = self.show_w0;
        let mut reveal = false;
        let val = self.spec.as_ref().map(|sp| sp.at(self.w0));
        card(
            ui,
            "s_point",
            icon::CROSSHAIRS,
            &format!("{} {w}₀", tr("Value at", "频率点")),
            false,
            |ui| {
                eye(ui, &mut show, tr("Mark it on the spectrum", "在频谱上标出"));
            },
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{w}₀")).color(t.muted));
                    reveal |= ui.add(egui::Slider::new(&mut x0, -wlim..=wlim).max_decimals(3).clamping(egui::SliderClamping::Never)).changed();
                });
                if ui.button(format!("{w}₀ = 0")).on_hover_text(tr("F(0) = ∫ f(t) dt", "F(0) = ∫ f(t) dt")).clicked() {
                    x0 = 0.0;
                    reveal = true;
                }
                if let Some(v) = val {
                    kv(ui, &t, &format!("F({w}₀)"), "", &fmt_c(v));
                    kv(ui, &t, "|F|", tr("amplitude", "幅度"), &fmt_val(v.norm()));
                    kv(ui, &t, "arg F", tr("phase", "相位"), &format!("{}°", fmt_short(v.arg().to_degrees())));
                    if x0 == 0.0 {
                        caption(ui, tr("At zero frequency F is the area under f.", "零频处 F 等于 f 下方的面积。"));
                    }
                }
            },
        );
        self.w0 = self.conv.to_omega(x0);
        self.show_w0 = show || reveal;

        // ---- Properties ----
        let conv = self.conv;
        if let Some(st) = &self.stats {
            card(ui, "s_props", icon::SIGMA, tr("Properties", "性质"), false, |_| {}, |ui| {
                ui.label(RichText::new(tr("Parseval", "帕塞瓦尔定理")).strong().color(t.text));
                kv(ui, &t, "∫|f|² dt", "", &fmt_val(st.energy_t));
                let rhs = match conv {
                    Convention::Angular => "(1/2π)∫|F|² dω",
                    Convention::Hertz => "∫|F|² dν",
                };
                kv(ui, &t, rhs, "", &fmt_val(st.energy_w));
                kv(ui, &t, tr("99% energy", "99% 能量"), &format!("|{w}| ≤"), &fmt_val(conv.from_omega(st.band99)));
                kv(ui, &t, "F(0)", "= ∫ f dt", &fmt_c(st.integral));
                ui.separator();
                ui.label(RichText::new(tr("Symmetry", "对称性")).strong().color(t.text));
                ui.horizontal_wrapped(|ui| {
                    if st.sym.real {
                        badge(ui, tr("f real ⇒ F(−ω) = F(ω)*", "f 为实 ⇒ F(−ω) = F(ω)*"), t.accent_text)
                            .on_hover_text(tr("|F| is even and arg F is odd.", "|F| 为偶函数，arg F 为奇函数。"));
                    }
                    if st.sym.even && st.sym.real {
                        badge(ui, tr("f even ⇒ F real", "f 为偶 ⇒ F 为实"), t.accent_text);
                    } else if st.sym.odd && st.sym.real {
                        badge(ui, tr("f odd ⇒ F imaginary", "f 为奇 ⇒ F 为纯虚"), t.accent_text);
                    }
                    if !st.sym.real {
                        badge(ui, tr("f complex: |F| need not be even", "f 为复信号：|F| 不必是偶函数"), t.warn);
                    }
                });
                ui.separator();
                ui.label(RichText::new(tr("Uncertainty principle", "不确定性原理")).strong().color(t.text));
                kv(ui, &t, "Δt", tr("RMS duration", "均方根时宽"), &fmt_val(st.dt));
                match st.dw {
                    Some(dw) => {
                        let (dx, bound) = match conv {
                            Convention::Angular => (dw, 0.5),
                            Convention::Hertz => (dw / std::f64::consts::TAU, 1.0 / (4.0 * std::f64::consts::PI)),
                        };
                        kv(ui, &t, &format!("Δ{w}"), tr("RMS bandwidth", "均方根带宽"), &fmt_val(dx));
                        kv(ui, &t, &format!("Δt·Δ{w}"), &format!("≥ {}", if conv == Convention::Angular { "1/2" } else { "1/4π" }), &fmt_val(st.dt * dx));
                        let tight = (st.dt * dx / bound - 1.0).abs() < 1e-3;
                        if tight {
                            badge(ui, &with_icon(icon::CHECK, tr("equality: Gaussian", "取等号：高斯函数")), t.ok);
                        }
                    }
                    None => {
                        kv(ui, &t, &format!("Δ{w}"), tr("RMS bandwidth", "均方根带宽"), "∞");
                        caption(ui, tr("|F|² decays too slowly (f has a jump), so the RMS bandwidth is infinite.", "|F|² 衰减太慢（f 有跳变），均方根带宽为无穷大。"));
                    }
                }
            });
        }

        // ---- Reconstruction ----
        let mut on = self.recon_on;
        let mut reveal = false;
        let mut omega = conv.from_omega(self.recon_w);
        let lim = conv.from_omega(self.w_max);
        let frac = self.spec.as_ref().map(|sp| sp.energy_within(self.recon_w));
        let err = (self.recon_on && !self.recon.is_empty()).then(|| {
            let sp = self.spec.as_ref().unwrap();
            let e = self.recon.iter().zip(&sp.f).map(|(a, b)| (a - b).norm()).fold(0.0, f64::max);
            let peak = sp.f.iter().map(|v| v.re).fold(f64::MIN, f64::max);
            let over = self.recon.iter().map(|v| v.re).fold(f64::MIN, f64::max) - peak;
            (e, over, peak)
        });
        card(
            ui,
            "s_recon",
            icon::CURVE,
            tr("Band-limited reconstruction", "带限重建"),
            false,
            |ui| {
                eye(ui, &mut on, tr("Show the reconstruction", "显示重建信号"));
            },
            |ui| {
                caption(
                    ui,
                    match conv {
                        Convention::Angular => tr("f_Ω(t) = (1/2π) ∫_{|ω|≤Ω} F(ω) e^(iωt) dω: keep only the frequencies up to Ω.", "f_Ω(t) = (1/2π) ∫_{|ω|≤Ω} F(ω) e^(iωt) dω：只保留 Ω 以内的频率。"),
                        Convention::Hertz => tr("f_Ω(t) = ∫_{|ν|≤Ω} F(ν) e^(2πiνt) dν: keep only the frequencies up to Ω.", "f_Ω(t) = ∫_{|ν|≤Ω} F(ν) e^(2πiνt) dν：只保留 Ω 以内的频率。"),
                    },
                );
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ω").color(t.muted));
                    if ui.add(egui::Slider::new(&mut omega, 0.0..=lim).max_decimals(3)).changed() {
                        reveal = true;
                    }
                });
                if let Some(fr) = frac {
                    kv(ui, &t, tr("energy kept", "保留能量"), "", &format!("{}%", fmt_short(fr * 100.0)));
                }
                if let Some((e, over, peak)) = err {
                    kv(ui, &t, "max |f_Ω − f|", "", &fmt_val(e));
                    if peak > 0.0 && over > 0.0 {
                        kv(ui, &t, tr("overshoot", "过冲"), "", &format!("{} ({}%)", fmt_val(over), fmt_short(over / peak * 100.0)));
                    }
                    caption(ui, tr("At a jump the overshoot stays near 9% of the jump however large Ω gets: the Gibbs phenomenon.", "在跳变处，无论 Ω 多大，过冲都保持在跳变高度的约 9%：吉布斯现象。"));
                }
            },
        );
        self.recon_on = on || reveal;
        self.recon_w = conv.to_omega(omega);

        // ---- Window and range ----
        let mut auto_window = self.auto_window;
        let mut win = self.window;
        let mut auto_w = self.auto_w;
        let mut wm = conv.from_omega(self.w_max);
        let sup = self.support;
        let info = self.spec.as_ref().map(|sp| (sp.h, sp.t.len(), sp.n, conv.from_omega(sp.nyquist()), conv.from_omega(sp.d_omega())));
        card(ui, "s_window", icon::RULER, tr("Window and sampling", "时间窗与采样"), false, |_| {}, |ui| {
            ui.checkbox(&mut auto_window, tr("Automatic time window", "自动时间窗"));
            ui.add_enabled_ui(!auto_window, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("t").monospace().color(t.muted));
                    crate::app::range_edit(ui, &mut win);
                });
            });
            ui.checkbox(&mut auto_w, tr("Automatic frequency range", "自动频率范围"));
            ui.add_enabled_ui(!auto_w, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("|{w}| ≤")).monospace().color(t.muted));
                    let speed = wm * 0.01;
                    ui.add(egui::DragValue::new(&mut wm).speed(speed).range(1e-4..=1e5).max_decimals(4));
                });
            });
            if let Some((h, nt, n, nyq, dw)) = info {
                kv(ui, &t, tr("step", "步长"), "h", &fmt_val(h));
                kv(ui, &t, tr("samples", "采样数"), "", &nt.to_string());
                kv(ui, &t, "FFT", tr("size, zero-padded", "点数（补零）"), &n.to_string());
                kv(ui, &t, "Nyquist", &format!("π/h → {w}"), &fmt_val(nyq));
                kv(ui, &t, &format!("Δ{w}"), tr("frequency step", "频率间隔"), &fmt_val(dw));
            }
            if let Some(sup) = sup
                && sup.integrable
            {
                kv(ui, &t, tr("tail", "截断"), tr("∫|f| outside, about", "窗外 ∫|f| 约为"), &fmt_sci(sup.tail));
            }
        });
        if auto_window != self.auto_window || win != self.window {
            self.auto_window = auto_window;
            self.window = win;
            if auto_window {
                self.support = None;
            }
            self.reset_plots = true;
        }
        let wm = conv.to_omega(wm);
        if auto_w != self.auto_w || (!auto_w && (wm - self.w_max).abs() > 1e-12) {
            self.auto_w = auto_w;
            self.w_max = wm;
            self.reset_plots = true;
        }
    }

    // ---- Plots -----------------------------------------------------------

    fn plots(&mut self, ui: &mut Ui, t: &Tokens) {
        let rect = ui.available_rect_before_wrap();
        // Toolbar.
        let w = self.wname();
        ui.horizontal(|ui| {
            overlay_frame(t).inner_margin(egui::Margin::same(5)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mag = format!("|F({w})|, arg F");
                    let reim = format!("Re F({w}), Im F");
                    segmented(ui, &mut self.view, &[(View::MagPhase, &mag, tr("Magnitude and phase", "幅度与相位")), (View::ReIm, &reim, tr("Real and imaginary parts", "实部与虚部"))]);
                    ui.add_space(4.0);
                    if self.view == View::MagPhase {
                        segmented(ui, &mut self.db, &[(false, tr("Linear", "线性"), ""), (true, "dB", tr("20 log₁₀ |F|", "20 log₁₀ |F|"))]);
                        ui.add_space(4.0);
                    }
                    chip(ui, &mut self.show_w0, &format!("{w}₀"), Some(C_W0)).on_hover_text(tr("Frequency marker", "频率标记"));
                    chip(ui, &mut self.recon_on, tr("Reconstruction", "带限重建"), Some(C_RECON)).on_hover_text(tr("Band-limited f_Ω(t)", "带限信号 f_Ω(t)"));
                    if ui.button(with_icon(icon::RESTORE, "")).on_hover_text(tr("Reset the plot ranges", "重置坐标范围")).clicked() {
                        self.reset_plots = true;
                    }
                    if ui.button(with_icon(icon::CAMERA, "")).on_hover_text(tr("Save the plots as PNG in ~/Pictures/Math-crystal", "把图保存为 PNG（~/Pictures/Math-crystal）")).clicked() {
                        self.export.start();
                    }
                });
            });
        });
        ui.add_space(6.0);
        let Some(sp) = &self.spec else { return };
        let plots_rect = egui::Rect::from_min_max(ui.cursor().min, rect.max);
        // Frames add margins and item spacing on top of the set heights.
        let avail = ui.available_height() - 3.0 * ui.spacing().item_spacing.y - 6.0;
        let gap = 6.0;
        let h_time = ((avail - 2.0 * gap) * 0.32).max(80.0);
        let h_top = ((avail - 2.0 * gap) * 0.38).max(80.0);
        let h_bot = (avail - 2.0 * gap - h_time - h_top).max(70.0);
        if std::mem::take(&mut self.reset_plots) {
            self.plot_gen += 1;
        }
        let generation = self.plot_gen;
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        let zoom_x = |pui: &mut egui_plot::PlotUi| {
            if pui.response().hovered() && scroll != 0.0 {
                pui.zoom_bounds_around_hovered(vec2((scroll * 0.0015).exp(), 1.0));
                pui.set_auto_bounds([false, true]);
            }
        };
        let frame = |ui: &mut Ui, h: f32, add: &mut dyn FnMut(&mut Ui)| {
            egui::Frame::new().fill(t.card).stroke(Stroke::new(1.0, t.card_border)).corner_radius(10).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.set_height(h - 12.0);
                add(ui);
            });
        };

        // ---- f(t) ----
        let decim = |n: usize| (n / 6000).max(1);
        let step = decim(sp.t.len());
        let re: Vec<[f64; 2]> = sp.t.iter().zip(&sp.f).step_by(step).map(|(t, v)| [*t, v.re]).collect();
        let complex_sig = self.stats.as_ref().is_some_and(|s| !s.sym.real);
        let im: Vec<[f64; 2]> = sp.t.iter().zip(&sp.f).step_by(step).map(|(t, v)| [*t, v.im]).collect();
        let recon: Vec<[f64; 2]> = if self.recon_on { sp.t.iter().zip(&self.recon).step_by(step).map(|(t, v)| [*t, v.re]).collect() } else { vec![] };
        let tw = (self.window.0, self.window.1);
        frame(ui, h_time, &mut |ui| {
            let mut plot = Plot::new(("sig_t", generation))
                .allow_scroll(false)
                .allow_zoom([true, false])
                .legend(egui_plot::Legend::default().position(egui_plot::Corner::RightTop))
                .label_formatter(|pos: &HoverPosition| {
                    let (name, p) = match pos {
                        HoverPosition::NearDataPoint { plot_name, position, .. } => (*plot_name, position),
                        HoverPosition::Elsewhere { position } => ("", position),
                    };
                    Some(format!("{name}\nt = {}\ny = {}", fmt_short(p.x), fmt_val(p.y)))
                });
            plot = plot.default_x_bounds(tw.0, tw.1);
            plot.show(ui, |pui| {
                zoom_x(pui);
                pui.line(Line::new("Re f(t)", PlotPoints::from(re.clone())).color(C_RE).width(2.0));
                if complex_sig {
                    pui.line(Line::new("Im f(t)", PlotPoints::from(im.clone())).color(C_IM).width(1.6).style(LineStyle::dashed_loose()));
                }
                if !recon.is_empty() {
                    pui.line(Line::new("f_Ω(t)", PlotPoints::from(recon.clone())).color(C_RECON).width(1.6));
                }
            });
        });
        ui.add_space(gap);

        // ---- Spectrum ----
        let conv = self.conv;
        let x = |w: f64| conv.from_omega(w);
        let max_mag = self.band.iter().map(|(_, v)| v.norm()).fold(0.0, f64::max).max(1e-300);
        // Rounding noise in a real or imaginary F would make its phase flip
        // between +π and −π: drop parts below 1e-10 of the peak.
        let tiny = 1e-10 * max_mag;
        let band: Vec<(f64, C64)> = self.band.iter().map(|(w, v)| (*w, C64::new(if v.re.abs() < tiny { 0.0 } else { v.re }, if v.im.abs() < tiny { 0.0 } else { v.im }))).collect();
        let band = &band;
        let floor_db = 20.0 * max_mag.log10() - 120.0;
        let (top_name, top): (String, Vec<[f64; 2]>) = match (self.view, self.db) {
            (View::MagPhase, false) => (format!("|F({w})|"), band.iter().map(|(om, v)| [x(*om), v.norm()]).collect()),
            (View::MagPhase, true) => (format!("20 log₁₀|F({w})|"), band.iter().map(|(om, v)| [x(*om), (20.0 * v.norm().log10()).max(floor_db)]).collect()),
            (View::ReIm, _) => (format!("Re F({w})"), band.iter().map(|(om, v)| [x(*om), v.re]).collect()),
        };
        // Phase in segments: skip where |F| is negligible and break at wraps.
        let mut phase_segs: Vec<Vec<[f64; 2]>> = Vec::new();
        let im_part: Vec<[f64; 2]>;
        if self.view == View::MagPhase {
            let mut cur: Vec<[f64; 2]> = Vec::new();
            for (om, v) in band {
                let ok = v.norm() > 1e-6 * max_mag;
                let a = v.arg();
                if !ok || cur.last().is_some_and(|p| (p[1] - a).abs() > std::f64::consts::PI) {
                    if cur.len() > 1 {
                        phase_segs.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
                if ok {
                    cur.push([x(*om), a]);
                }
            }
            if cur.len() > 1 {
                phase_segs.push(cur);
            }
            im_part = vec![];
        } else {
            im_part = band.iter().map(|(om, v)| [x(*om), v.im]).collect();
        }
        let xr = x(self.w_max);
        let w0 = self.show_w0.then(|| x(self.w0));
        let rw = self.recon_on.then(|| x(self.recon_w));
        let link = ui.scope_id().with(("spectrum_x", generation));
        let mut clicked: Option<f64> = None;
        let fmt_label = move |pos: &HoverPosition| {
            let (name, p) = match pos {
                HoverPosition::NearDataPoint { plot_name, position, .. } => (*plot_name, position),
                HoverPosition::Elsewhere { position } => ("", position),
            };
            Some(format!("{name}\n{w} = {}\ny = {}", fmt_short(p.x), fmt_val(p.y)))
        };
        let add_marks = |pui: &mut egui_plot::PlotUi| {
            if let Some(r) = rw {
                pui.span(Span::new(format!("|{w}| ≤ Ω"), -r..=r).fill(C_RECON.gamma_multiply(0.08)).border_color(C_RECON.gamma_multiply(0.5)).border_width(1.0));
            }
            if let Some(x0) = w0 {
                pui.vline(VLine::new(format!("{w}₀"), x0).color(C_W0).width(1.5));
            }
        };
        frame(ui, h_top, &mut |ui| {
            let mut plot = Plot::new(("spec_top", generation)).allow_scroll(false).allow_zoom([true, false]).link_axis(link, [true, false]).label_formatter(fmt_label).legend(egui_plot::Legend::default().position(egui_plot::Corner::RightTop));
            plot = plot.default_x_bounds(-xr, xr);
            let r = plot.show(ui, |pui| {
                zoom_x(pui);
                add_marks(pui);
                pui.line(Line::new(top_name.clone(), PlotPoints::from(top.clone())).color(C_RE).width(2.0));
                pui.pointer_coordinate()
            });
            if r.response.clicked()
                && let Some(p) = r.inner
            {
                clicked = Some(p.x);
            }
        });
        ui.add_space(gap);
        frame(ui, h_bot, &mut |ui| {
            let name = if self.view == View::MagPhase { format!("arg F({w})") } else { format!("Im F({w})") };
            let mut plot = Plot::new(("spec_bottom", generation)).allow_scroll(false).allow_zoom([true, false]).link_axis(link, [true, false]).label_formatter(fmt_label).legend(egui_plot::Legend::default().position(egui_plot::Corner::RightTop));
            plot = plot.default_x_bounds(-xr, xr);
            if self.view == View::MagPhase {
                plot = plot.default_y_bounds(-3.3, 3.3);
            }
            let r = plot.show(ui, |pui| {
                zoom_x(pui);
                add_marks(pui);
                if self.view == View::MagPhase {
                    // One legend entry: same name for every segment.
                    for seg in &phase_segs {
                        pui.line(Line::new(name.clone(), PlotPoints::from(seg.clone())).color(C_IM).width(1.8));
                    }
                } else {
                    pui.line(Line::new(name.clone(), PlotPoints::from(im_part.clone())).color(C_IM).width(2.0));
                }
                pui.pointer_coordinate()
            });
            if r.response.clicked()
                && let Some(p) = r.inner
            {
                clicked = Some(p.x);
            }
        });
        if let Some(xc) = clicked {
            self.w0 = conv.to_omega(xc);
            self.show_w0 = true;
        }
        if let Some(msg) = self.export.step(ui, plots_rect, "spectrum") {
            self.toast = Some((msg, Instant::now()));
        }
    }
}
