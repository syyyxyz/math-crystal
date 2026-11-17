//! The Spectrum workspace: an absolutely integrable f(t), its Fourier
//! transform (magnitude and phase or real and imaginary parts),
//! band-limited reconstruction, Parseval and the uncertainty principle.

use std::time::Instant;

use egui::{Align, Color32, FontId, Layout, RichText, Stroke, Ui, vec2};
use egui_plot::{Line, LineStyle, Plot, PlotPoints, Span, VLine};
use num_complex::Complex64 as C64;

use crate::app::{fmt_sci, fmt_short, fmt_val, kv};
use crate::complex_app::fmt_c;
use crate::i18n::{self, tr};
use crate::spectrum::{Convention, Sampled, Signal, Spectrum, Support, Symmetry, symmetry};
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
    params: crate::ui::params::Params,
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
    /// Plot heights and which plots are unfolded: f(t), top, bottom.
    heights: [f32; 3],
    panel_open: [bool; 3],
    t0: f64,
    show_t0: bool,
    /// Pointer position over the plots last frame (ω in rad, t).
    hover_w: Option<f64>,
    hover_t: Option<f64>,
    /// Plot rect and marker x on screen, for grabbing the markers.
    marker_px: [Option<(egui::Rect, f32)>; 3],
    /// What ω0 was snapped to.
    snapped: Option<&'static str>,
    /// Marker being dragged: 0 = t0, 1 = ω0.
    grab: Option<usize>,
    /// Put Ω back to a quarter of the range for a new signal.
    recon_fresh: bool,
    // Sampling and the DFT.
    samp_on: bool,
    /// Sampling period.
    samp_ts: f64,
    /// N is whatever covers the time window (else `samp_n`).
    samp_auto_n: bool,
    samp_n: usize,
    samp: Option<Sampled>,
    samp_key: String,
    /// Pick a sampling period that suits a new signal.
    samp_fresh: bool,
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
const C_T0: Color32 = Color32::from_rgb(0x0c, 0xa6, 0x78);
const C_SAMP: Color32 = Color32::from_rgb(0xd6, 0x33, 0x6c);

impl SpectrumWorkspace {
    pub fn new(src: &str) -> Self {
        let mut s = Self {
            src: src.to_string(),
            sig: None,
            params: Default::default(),
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
            heights: [240.0, 300.0, 220.0],
            panel_open: [true; 3],
            t0: 0.0,
            show_t0: false,
            hover_w: None,
            hover_t: None,
            marker_px: [None; 3],
            snapped: None,
            grab: None,
            recon_fresh: true,
            samp_on: false,
            samp_ts: 0.2,
            samp_auto_n: true,
            samp_n: 32,
            samp: None,
            samp_key: String::new(),
            samp_fresh: true,
        };
        s.recompile();
        s
    }

    /// Turn the sampling layer on (used by the preview example).
    pub fn show_sampling(&mut self) {
        self.samp_on = true;
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
            Ok(mut s) => {
                self.params.sync(&s.params);
                s.set_params(&self.params.values());
                let changed = self.sig.as_ref().is_none_or(|o| o.source != s.source);
                self.sig = Some(s);
                self.error = None;
                if changed {
                    self.support = None;
                    self.reset_plots = true;
                    self.recon_fresh = true;
                    self.samp_fresh = true;
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
            if std::mem::take(&mut self.samp_fresh) {
                // Nyquist frequency at half the displayed range: some
                // aliasing to look at for most signals.
                self.samp_ts = std::f64::consts::TAU / sp.w_max;
            }
            self.samp_key.clear();
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

    /// Recompute the samples and their spectrum when their inputs changed.
    fn update_sampling(&mut self) {
        let (Some(sp), Some(sig), true) = (&self.spec, &self.sig, self.samp_on) else { return };
        let key = format!("{}|{}|{}|{}", self.spec_key, self.samp_ts, self.samp_auto_n, self.samp_n);
        if key == self.samp_key {
            return;
        }
        let ts = self.samp_ts;
        let (a, b) = self.window;
        // Samples sit at integer multiples of T_s, so t = 0 is one of them.
        let (n0, n) = if self.samp_auto_n {
            let (n0, n1) = ((a / ts).ceil() as i64, (b / ts).floor() as i64);
            (n0, ((n1 - n0 + 1).max(1) as usize).min(1 << 16))
        } else {
            let centre = ((a + b) / 2.0 / ts).round() as i64;
            (centre - self.samp_n as i64 / 2, self.samp_n)
        };
        self.samp = Some(sig.sampled(ts, n0, n, sp.w_max));
        self.samp_key = key;
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
        self.update_sampling();
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
                    ui.label(small(tr("Scroll: page · Ctrl+scroll: zoom the axis · Drag: pan · Double-click: reset · Click: pick a point (snaps to peaks/zeros, Alt: no snap) · Drag a marker to move it", "滚轮：滚动页面 · Ctrl+滚轮：缩放横轴 · 拖动：平移 · 双击：重置 · 单击：取点（吸附峰值/零点，按住 Alt 不吸附）· 拖动标记线可移动"), t.faint));
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
                            if !mathview::show(ui, pool, id, &sig.names(), "f(t)", 17.0, t.text) {
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
        let ch = self.params.ui(ui, "spectrum_params");
        if ch.changed
            && let Some(sig) = &mut self.sig
        {
            sig.set_params(&self.params.values());
            self.spec_key.clear();
            if self.auto_window {
                self.support = None;
            }
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

        // ---- Sampling and DFT ----
        let tau = std::f64::consts::TAU;
        let mut on = self.samp_on;
        let mut reveal = false;
        let mut ts = self.samp_ts;
        let mut auto_n = self.samp_auto_n;
        let mut n = self.samp_n;
        let count = self.samp.as_ref().filter(|_| self.samp_on).map(|s| s.x.len());
        let beyond = self.spec.as_ref().map(|sp| 1.0 - sp.energy_within(tau / ts / 2.0));
        let band99 = self.stats.as_ref().map(|s| s.band99);
        let (ts_lo, ts_hi) = (tau / (8.0 * self.w_max), tau / (self.w_max / 16.0));
        card(
            ui,
            "s_sampling",
            icon::DOT,
            tr("Sampling and DFT", "采样与 DFT"),
            false,
            |ui| {
                eye(ui, &mut on, tr("Show the samples and their spectrum", "显示采样点及其频谱"));
            },
            |ui| {
                caption(
                    ui,
                    tr(
                        "Sample f every Tₛ: the spectrum becomes copies of F repeated every ωₛ = 2π/Tₛ and added up. Where the copies overlap they cannot be told apart: aliasing.",
                        "每隔 Tₛ 对 f 采样：频谱变成 F 每隔 ωₛ = 2π/Tₛ 重复一次再相加。重复的部分一旦重叠就分不开了，这就是混叠。",
                    ),
                );
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Tₛ").color(t.muted));
                    // Edit a copy: the slider rounds what it shows.
                    let mut shown = ts;
                    if ui.add(egui::Slider::new(&mut shown, ts_lo..=ts_hi).logarithmic(true).max_decimals(4)).changed() {
                        ts = shown;
                        reveal = true;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Tₛ ÷ 2").on_hover_text(tr("Sample twice as fast", "采样快一倍")).clicked() {
                        ts /= 2.0;
                        reveal = true;
                    }
                    if ui.button("Tₛ × 2").on_hover_text(tr("Sample half as fast", "采样慢一倍")).clicked() {
                        ts *= 2.0;
                        reveal = true;
                    }
                    if let Some(b) = band99
                        && ui.button(tr("Fit 99% band", "刚好容纳 99% 带宽")).on_hover_text(tr("ωₛ/2 = the band holding 99% of the energy", "让 ωₛ/2 等于包含 99% 能量的带宽")).clicked()
                    {
                        ts = tau / (2.0 * b);
                        reveal = true;
                    }
                });
                let ws = tau / ts;
                match conv {
                    Convention::Angular => kv(ui, &t, "ωₛ", "= 2π/Tₛ", &fmt_val(ws)),
                    Convention::Hertz => kv(ui, &t, "νₛ", "= 1/Tₛ", &fmt_val(1.0 / ts)),
                }
                kv(ui, &t, tr("Nyquist", "奈奎斯特频率"), &format!("{w}ₛ/2"), &fmt_val(conv.from_omega(ws / 2.0)));
                if let Some(e) = beyond {
                    let e = e.max(0.0);
                    kv(ui, &t, tr("energy beyond it", "此频率以外的能量"), tr("folds back", "会折叠回来"), &format!("{}%", fmt_short(e * 100.0)));
                    if e < 1e-3 {
                        badge(ui, &with_icon(icon::CHECK, tr("almost no aliasing", "几乎没有混叠")), t.ok);
                    } else {
                        badge(ui, &with_icon(icon::ALERT, tr("aliasing: the copies overlap", "有混叠：相邻的重复频谱重叠")), t.warn);
                    }
                }
                ui.separator();
                ui.checkbox(&mut auto_n, tr("N samples cover the whole window", "N 个样本覆盖整个时间窗"));
                ui.add_enabled_ui(!auto_n, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("N").color(t.muted));
                        reveal |= ui.add(egui::Slider::new(&mut n, 2..=512).logarithmic(true)).changed();
                    });
                });
                if let Some(nn) = count {
                    kv(ui, &t, "N", tr("points in the DFT", "DFT 点数"), &nn.to_string());
                    kv(ui, &t, &format!("Δ{w}"), &format!("= {w}ₛ/N"), &fmt_val(conv.from_omega(ws / nn as f64)));
                    kv(ui, &t, tr("record length", "记录时长"), "N·Tₛ", &fmt_val(nn as f64 * ts));
                }
                caption(
                    ui,
                    tr(
                        "The dots are the N DFT values: the periodic spectrum read off every ωₛ/N. Fewer samples (a shorter record) put the dots further apart and smear the spectrum.",
                        "圆点就是 N 个 DFT 值：在这条周期频谱上每隔 ωₛ/N 读一个点。样本越少（记录越短），圆点越稀，频谱也被抹得越宽。",
                    ),
                );
            },
        );
        if auto_n != self.samp_auto_n {
            reveal = true;
        }
        self.samp_on = on || reveal;
        self.samp_ts = ts.clamp(ts_lo / 4.0, ts_hi * 4.0);
        self.samp_auto_n = auto_n;
        self.samp_n = n;

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
        let w = self.wname();
        // Toolbar (fixed above the scrolling plots).
        let mut fit = false;
        ui.horizontal(|ui| {
            overlay_frame(t).inner_margin(egui::Margin::same(5)).show(ui, |ui| {
                // Wraps onto a second row in a narrow window.
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    let mag = format!("|F({w})|, arg F");
                    let reim = format!("Re F({w}), Im F");
                    segmented(ui, &mut self.view, &[(View::MagPhase, &mag, tr("Magnitude and phase", "幅度与相位")), (View::ReIm, &reim, tr("Real and imaginary parts", "实部与虚部"))]);
                    ui.add_space(4.0);
                    if self.view == View::MagPhase {
                        segmented(ui, &mut self.db, &[(false, tr("Linear", "线性"), ""), (true, "dB", tr("20 log₁₀ |F|", "20 log₁₀ |F|"))]);
                        ui.add_space(4.0);
                    }
                    chip(ui, &mut self.show_t0, "t₀", Some(C_T0)).on_hover_text(tr("Time marker (click the f(t) plot)", "时间标记（单击 f(t) 图）"));
                    chip(ui, &mut self.show_w0, &format!("{w}₀"), Some(C_W0)).on_hover_text(tr("Frequency marker (click a spectrum)", "频率标记（单击频谱图）"));
                    chip(ui, &mut self.recon_on, tr("Reconstruction", "带限重建"), Some(C_RECON)).on_hover_text(tr("Band-limited f_Ω(t)", "带限信号 f_Ω(t)"));
                    chip(ui, &mut self.samp_on, tr("Sampling", "采样"), Some(C_SAMP)).on_hover_text(tr("Samples of f, their periodic spectrum and the DFT", "f 的采样点、采样后的周期频谱与 DFT"));
                    if ui.button(with_icon(icon::RULER, "")).on_hover_text(tr("Fit the open plots into the window", "让展开的图适应窗口高度")).clicked() {
                        fit = true;
                    }
                    if ui.button(with_icon(icon::RESTORE, "")).on_hover_text(tr("Reset the plot ranges", "重置坐标范围")).clicked() {
                        self.reset_plots = true;
                    }
                    if ui.button(with_icon(icon::CAMERA, "")).on_hover_text(crate::ui::export::hint(tr("Save the plots as PNG in", "把图保存为 PNG，位置："))).clicked() {
                        self.export.start();
                    }
                });
            });
        });
        ui.add_space(6.0);
        if self.spec.is_none() {
            return;
        }
        let view_rect = egui::Rect::from_min_max(ui.cursor().min, ui.max_rect().max);
        if fit {
            let n_open = self.panel_open.iter().filter(|o| **o).count();
            let free = view_rect.height() - 3.0 * PANEL_CHROME - 4.0;
            let weights = [0.32f32, 0.38, 0.30];
            let total: f32 = weights.iter().zip(&self.panel_open).filter(|(_, o)| **o).map(|(w, _)| w).sum();
            if n_open > 0 {
                let closed_h = (3 - n_open) as f32 * (PANEL_CLOSED - PANEL_CHROME);
                for k in 0..3 {
                    if self.panel_open[k] {
                        self.heights[k] = ((free - closed_h) * weights[k] / total).max(MIN_H);
                    }
                }
            }
        }
        if std::mem::take(&mut self.reset_plots) {
            self.plot_gen += 1;
        }
        let generation = self.plot_gen;

        // Local copies of the state the closures change.
        let mut heights = self.heights;
        let mut open = self.panel_open;
        let (mut w0, mut show_w0, mut t0, mut show_t0) = (self.w0, self.show_w0, self.t0, self.show_t0);
        let (prev_hw, prev_ht) = (self.hover_w, self.hover_t);
        let (mut hover_w, mut hover_t) = (None, None);
        let mut marker_px = self.marker_px;
        let mut snapped = self.snapped;
        let alt = ui.input(|i| i.modifiers.alt);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let near = |px: Option<(egui::Rect, f32)>, shown: bool| shown && matches!((pointer, px), (Some(p), Some((r, x))) if r.contains(p) && (p.x - x).abs() < 6.0);
        // A press next to a marker grabs it until the button is released.
        let (pressed, down) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down()));
        // Still applied on the release frame so the marker ends where the pointer did.
        let mut grab = self.grab;
        if pressed {
            if near(marker_px[0], show_t0) {
                grab = Some(0);
            } else if near(marker_px[1], show_w0) || near(marker_px[2], show_w0) {
                grab = Some(1);
            }
        }

        let sp = self.spec.as_ref().unwrap();
        let sig = self.sig.as_ref();
        let conv = self.conv;
        let view = self.view;
        let db = self.db;
        let recon = &self.recon;
        let recon_on = self.recon_on && !recon.is_empty();
        let complex_sig = self.stats.as_ref().is_some_and(|s| !s.sym.real);
        let max_mag = self.band.iter().map(|(_, v)| v.norm()).fold(0.0, f64::max).max(1e-300);
        let tiny = 1e-10 * max_mag;
        let clean = |v: C64| C64::new(if v.re.abs() < tiny { 0.0 } else { v.re }, if v.im.abs() < tiny { 0.0 } else { v.im });
        let floor_db = 20.0 * max_mag.log10() - 120.0;
        let top_q = move |v: C64| match (view, db) {
            (View::MagPhase, false) => v.norm(),
            (View::MagPhase, true) => (20.0 * v.norm().log10()).max(floor_db),
            (View::ReIm, _) => v.re,
        };
        let bot_q = move |v: C64| if view == View::MagPhase { v.arg() } else { v.im };
        let x_of = |om: f64| conv.from_omega(om);
        let band: Vec<(f64, C64)> = self.band.iter().map(|(om, v)| (*om, clean(*v))).collect();

        // Values for the readouts (exact, not from the drawn points).
        let f_at = |tt: f64| sig.map(|s| s.eval(tt)).unwrap_or(C64::new(f64::NAN, 0.0));
        let recon_at = |tt: f64| -> Option<C64> {
            if !recon_on {
                return None;
            }
            let k = (tt - sp.t[0]) / sp.h;
            if k < 0.0 || k > (recon.len() - 1) as f64 {
                return None;
            }
            let i = (k.floor() as usize).min(recon.len() - 2);
            let a = k - i as f64;
            Some(recon[i] * (1.0 - a) + recon[i + 1] * a)
        };
        let time_text = |tt: f64, label: &str| {
            let v = f_at(tt);
            let mut s = format!("{label} = {}   f = {}", fmt_short(tt), fmt_c(v));
            if let Some(r) = recon_at(tt) {
                s += &format!("   f_Ω = {}", fmt_val(r.re));
            }
            s
        };
        let freq_text = |om: f64, label: &str, bottom: bool| {
            let v = clean(sp.at(om));
            let x = fmt_short(x_of(om));
            match (view, bottom) {
                (View::MagPhase, false) => {
                    let m = v.norm();
                    let dbs = if m > 0.0 { format!("   {} dB", fmt_short(20.0 * m.log10())) } else { String::new() };
                    format!("{label} = {x}   |F| = {}{dbs}   F = {}", fmt_val(m), fmt_c(v))
                }
                (View::MagPhase, true) => format!("{label} = {x}   arg F = {}°", fmt_short(v.arg().to_degrees())),
                (View::ReIm, false) => format!("{label} = {x}   Re F = {}", fmt_val(v.re)),
                (View::ReIm, true) => format!("{label} = {x}   Im F = {}", fmt_val(v.im)),
            }
        };
        let w0_label = format!("{w}₀");
        let snap_note = snapped.map(|k| format!("   ({k})")).unwrap_or_default();
        // Headers show the picked points; hover values go next to the cursor.
        let readout_t = if show_t0 { time_text(t0, "t₀") } else { tr("click to pick t₀", "单击取点 t₀").to_string() };
        let readout_f = |bottom: bool| if show_w0 { freq_text(w0, &w0_label, bottom) + &snap_note } else { format!("{} {w}₀", tr("click to pick", "单击取点")) };
        let (readout_top, readout_bot) = (readout_f(false), readout_f(true));

        let decim = |n: usize| (n / 6000).max(1);
        let step = decim(sp.t.len());
        let pts = |f: &dyn Fn(&C64) -> f64, v: &[C64]| -> Vec<[f64; 2]> { sp.t.iter().zip(v).step_by(step).map(|(t, v)| [*t, f(v)]).collect() };
        let re_t = pts(&|v| v.re, &sp.f);
        let im_t = pts(&|v| v.im, &sp.f);
        let rc_t = if recon_on { pts(&|v| v.re, recon) } else { vec![] };
        let tw = self.window;
        let xr = x_of(self.w_max);
        let rw = recon_on.then(|| x_of(self.recon_w));
        let top: Vec<[f64; 2]> = band.iter().map(|(om, v)| [x_of(*om), top_q(*v)]).collect();
        let mut bottom_segs: Vec<Vec<[f64; 2]>> = Vec::new();
        if view == View::MagPhase {
            // Skip where |F| is negligible and break at wraps.
            let mut cur: Vec<[f64; 2]> = Vec::new();
            for (om, v) in &band {
                let ok = v.norm() > 1e-6 * max_mag;
                let a = v.arg();
                if !ok || cur.last().is_some_and(|p| (p[1] - a).abs() > std::f64::consts::PI) {
                    if cur.len() > 1 {
                        bottom_segs.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
                if ok {
                    cur.push([x_of(*om), a]);
                }
            }
            if cur.len() > 1 {
                bottom_segs.push(cur);
            }
        } else {
            bottom_segs.push(band.iter().map(|(om, v)| [x_of(*om), v.im]).collect());
        }
        // Sampled signal: stems in time, the periodic spectrum and DFT bins.
        let samp = self.samp.as_ref().filter(|_| self.samp_on);
        let samp_t: Vec<[f64; 2]> = samp.map(|s| s.t.iter().zip(&s.x).map(|(tt, v)| [*tt, v.re]).collect()).unwrap_or_default();
        let samp_top: Vec<[f64; 2]> = samp.map(|s| s.dtft.iter().map(|(om, v)| [x_of(*om), top_q(clean(*v))]).collect()).unwrap_or_default();
        let samp_bot: Vec<[f64; 2]> = samp.filter(|_| view == View::ReIm).map(|s| s.dtft.iter().map(|(om, v)| [x_of(*om), clean(*v).im]).collect()).unwrap_or_default();
        let bins_top: Vec<[f64; 2]> = samp.map(|s| s.bins.iter().map(|(om, v)| [x_of(*om), top_q(clean(*v))]).collect()).unwrap_or_default();
        let bins_bot: Vec<[f64; 2]> = samp.map(|s| s.bins.iter().filter(|(_, v)| view == View::ReIm || v.norm() > 1e-6 * max_mag).map(|(om, v)| [x_of(*om), bot_q(clean(*v))]).collect()).unwrap_or_default();
        let nyq = samp.map(|s| x_of(std::f64::consts::PI / s.ts));
        let samp_name = format!("Tₛ·X({w})");
        let top_name = match (view, db) {
            (View::MagPhase, false) => format!("|F({w})|"),
            (View::MagPhase, true) => format!("20 log₁₀|F({w})|"),
            (View::ReIm, _) => format!("Re F({w})"),
        };
        let bot_name = if view == View::MagPhase { format!("arg F({w})") } else { format!("Im F({w})") };
        let link = ui.scope_id().with(("spectrum_x", generation));
        let legend = || egui_plot::Legend::default().position(egui_plot::Corner::RightTop);
        let cross = Color32::from_gray(if t.dark { 170 } else { 110 }).gamma_multiply(0.7);

        egui::ScrollArea::vertical().id_salt("spectrum_scroll").scroll_source(egui::scroll_area::ScrollSource { scroll_bar: true, mouse_wheel: true, ..egui::scroll_area::ScrollSource::NONE }).auto_shrink(false).show(ui, |ui| {
            // ---- f(t) ----
            panel(ui, t, "f(t)", &readout_t, &mut heights[0], &mut open[0], |ui, h| {
                let dragging_marker = grab == Some(0);
                let r = Plot::new(("sig_t", generation))
                    .height(h)
                    .allow_scroll(false)
                    .allow_zoom([true, false])
                    .allow_drag(!dragging_marker)
                    .show_x(false)
                    .show_y(false)
                    .x_grid_spacer(spacer125)
                    .legend(legend())
                    .default_x_bounds(tw.0, tw.1)
                    .show(ui, |pui| {
                        pui.line(Line::new("Re f(t)", PlotPoints::from(re_t.clone())).color(C_RE).width(2.0));
                        if complex_sig {
                            pui.line(Line::new("Im f(t)", PlotPoints::from(im_t.clone())).color(C_IM).width(1.6).style(LineStyle::dashed_loose()));
                        }
                        if !rc_t.is_empty() {
                            pui.line(Line::new("f_Ω(t)", PlotPoints::from(rc_t.clone())).color(C_RECON).width(1.6));
                        }
                        if !samp_t.is_empty() {
                            let p = egui_plot::Points::new("f(nTₛ)", samp_t.clone()).radius(if samp_t.len() <= 400 { 3.0 } else { 1.6 }).color(C_SAMP);
                            pui.points(if samp_t.len() <= 400 { p.stems(0.0) } else { p });
                        }
                        let hovered = pui.response().hovered();
                        let coord = pui.pointer_coordinate();
                        let mark = |pui: &mut egui_plot::PlotUi, x: f64, color: Color32, dashed: bool| {
                            let mut l = VLine::new("", x).color(color).width(if dashed { 1.0 } else { 1.5 });
                            if dashed {
                                l = l.style(LineStyle::dashed_dense());
                            }
                            pui.vline(l.allow_hover(false));
                            let v = f_at(x);
                            let mut dots = vec![([x, v.re], C_RE)];
                            if complex_sig {
                                dots.push(([x, v.im], C_IM));
                            }
                            if let Some(r) = recon_at(x) {
                                dots.push(([x, r.re], C_RECON));
                            }
                            for (p, c) in dots {
                                if p[1].is_finite() {
                                    pui.points(egui_plot::Points::new("", vec![p]).radius(4.0).color(c).allow_hover(false));
                                }
                            }
                        };
                        if show_t0 {
                            mark(pui, t0, C_T0, false);
                        }
                        if hovered && let Some(c) = coord {
                            mark(pui, c.x, cross, true);
                        }
                        (coord, hovered)
                    });
                let (coord, hovered) = r.inner;
                if hovered
                    && let (Some(c), Some(p)) = (coord, pointer)
                {
                    crate::complex_app::tooltip_box(&ui.painter_at(r.response.rect), r.response.rect, p, &time_text(c.x, "t").replace("   ", "\n"), t);
                }
                if hovered {
                    hover_t = coord.map(|c| c.x);
                    if dragging_marker || near(marker_px[0], show_t0) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                }
                if let Some(c) = coord
                    && (r.response.clicked() || dragging_marker)
                {
                    t0 = c.x;
                    show_t0 = true;
                }
                marker_px[0] = Some((r.response.rect, r.transform.position_from_point_x(t0)));
            });

            // ---- Spectrum: top and bottom share the frequency axis ----
            for (k, (name, readout)) in [(top_name.clone(), &readout_top), (bot_name.clone(), &readout_bot)].into_iter().enumerate() {
                let idx = k + 1;
                let is_top = k == 0;
                let xw0 = x_of(w0);
                panel(ui, t, &name, readout, &mut heights[idx], &mut open[idx], |ui, h| {
                    let dragging_marker = grab == Some(1);
                    let mut plot = Plot::new((if is_top { "spec_top" } else { "spec_bottom" }, generation))
                        .height(h)
                        .allow_scroll(false)
                        .allow_zoom([true, false])
                        .allow_drag(!dragging_marker)
                        .link_axis(link, [true, false])
                        .show_x(false)
                        .show_y(false)
                        .x_grid_spacer(spacer125)
                        .legend(legend())
                        .default_x_bounds(-xr, xr);
                    if !is_top && view == View::MagPhase {
                        plot = plot.default_y_bounds(-3.3, 3.3);
                    }
                    let r = plot.show(ui, |pui| {
                        if let Some(rr) = rw {
                            pui.span(Span::new(format!("|{w}| ≤ Ω"), -rr..=rr).fill(C_RECON.gamma_multiply(0.08)).border_color(C_RECON.gamma_multiply(0.5)).border_width(1.0));
                        }
                        // The sampled spectrum goes underneath: where it agrees
                        // with F it shows as a halo, where it aliases it parts.
                        let (s_line, s_bins) = if is_top { (&samp_top, &bins_top) } else { (&samp_bot, &bins_bot) };
                        if !s_line.is_empty() {
                            pui.line(Line::new(samp_name.clone(), PlotPoints::from(s_line.clone())).color(C_SAMP.gamma_multiply(0.75)).width(4.0));
                        }
                        if let Some(nq) = nyq {
                            for x in [-nq, nq] {
                                pui.vline(VLine::new("", x).color(C_SAMP.gamma_multiply(0.8)).width(1.0).style(LineStyle::dashed_loose()).allow_hover(false));
                            }
                        }
                        if is_top {
                            pui.line(Line::new(name.clone(), PlotPoints::from(top.clone())).color(C_RE).width(2.0));
                        } else {
                            for seg in &bottom_segs {
                                pui.line(Line::new(name.clone(), PlotPoints::from(seg.clone())).color(C_IM).width(1.8));
                            }
                        }
                        if !s_bins.is_empty() {
                            let stems = s_bins.len() <= 400 && if is_top { !(view == View::MagPhase && db) } else { view == View::ReIm };
                            let p = egui_plot::Points::new("X[k]", s_bins.clone()).radius(3.0).color(C_SAMP);
                            pui.points(if stems { p.stems(0.0) } else { p });
                        }
                        let q = |v: C64| if is_top { top_q(v) } else { bot_q(v) };
                        let mark = |pui: &mut egui_plot::PlotUi, om: f64, color: Color32, dashed: bool| {
                            let x = x_of(om);
                            let mut l = VLine::new("", x).color(color).width(if dashed { 1.0 } else { 1.5 });
                            if dashed {
                                l = l.style(LineStyle::dashed_dense());
                            }
                            pui.vline(l.allow_hover(false));
                            let y = q(clean(sp.at(om)));
                            if y.is_finite() {
                                pui.points(egui_plot::Points::new("", vec![[x, y]]).radius(4.5).color(if is_top { C_RE } else { C_IM }).allow_hover(false));
                            }
                        };
                        if show_w0 {
                            mark(pui, w0, C_W0, false);
                        }
                        let hovered = pui.response().hovered();
                        let coord = pui.pointer_coordinate();
                        // Crosshair in both spectrum plots (the other one uses last frame's position).
                        let other = marker_px[if is_top { 2 } else { 1 }].map(|m| m.0);
                        let cross_om = if hovered {
                            coord.map(|c| conv.to_omega(c.x))
                        } else {
                            match (pointer, other) {
                                // Same x axis and width: the pointer's x maps to the same frequency.
                                (Some(p), Some(o)) if o.contains(p) => Some(conv.to_omega(pui.transform().value_from_position(egui::pos2(p.x, pui.transform().frame().center().y)).x)),
                                _ => None,
                            }
                        };
                        if let Some(om) = cross_om {
                            mark(pui, om, cross, true);
                        }
                        (coord, hovered)
                    });
                    let (coord, hovered) = r.inner;
                    if hovered
                        && let (Some(c), Some(p)) = (coord, pointer)
                    {
                        let txt = freq_text(conv.to_omega(c.x), w, false) + "\n" + &freq_text(conv.to_omega(c.x), w, true).split("   ").skip(1).collect::<Vec<_>>().join("   ");
                        let txt = if view == View::MagPhase { txt.replacen("   ", "\n", 3) } else { txt.replacen("   ", "\n", 1) };
                        crate::complex_app::tooltip_box(&ui.painter_at(r.response.rect), r.response.rect, p, &txt, t);
                    }
                    if hovered {
                        hover_w = coord.map(|c| conv.to_omega(c.x));
                        if dragging_marker || near(marker_px[idx], show_w0) {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                    }
                    if let Some(c) = coord {
                        if r.response.clicked() {
                            // Snap to a nearby peak, zero or valley of the top curve (Alt: no snapping).
                            let tol = 10.0 * r.transform.dvalue_dpos()[0].abs();
                            match (!alt).then(|| snap(sp, &band, conv, view, c.x, tol, max_mag)).flatten() {
                                Some((x, kind)) => {
                                    w0 = conv.to_omega(x);
                                    snapped = Some(kind);
                                }
                                None => {
                                    w0 = conv.to_omega(c.x);
                                    snapped = None;
                                }
                            }
                            show_w0 = true;
                        } else if dragging_marker && hovered {
                            w0 = conv.to_omega(c.x);
                            snapped = None;
                            show_w0 = true;
                        }
                    }
                    marker_px[idx] = Some((r.response.rect, r.transform.position_from_point_x(xw0)));
                });
            }
        });
        if hover_w != prev_hw || hover_t != prev_ht {
            ui.ctx().request_repaint();
        }
        self.heights = heights;
        self.panel_open = open;
        self.w0 = w0;
        self.show_w0 = show_w0;
        self.t0 = t0;
        self.show_t0 = show_t0;
        self.hover_w = hover_w;
        self.hover_t = hover_t;
        self.marker_px = marker_px;
        self.snapped = snapped;
        self.grab = if down { grab } else { None };
        if let Some(msg) = self.export.step(ui, view_rect, "spectrum") {
            self.toast = Some((msg, Instant::now()));
        }
    }
}

/// Grid steps from the 1–2–5 sequence (egui_plot's default jumps by 10,
/// which can leave a short axis with a single label).
fn spacer125(input: egui_plot::GridInput) -> Vec<egui_plot::GridMark> {
    let base = input.base_step_size.max(1e-300);
    let p = 10f64.powf(base.log10().floor());
    // Smallest 1/2/5 step above the minimum, then coarser steps that nest.
    let m = [1.0, 2.0, 5.0, 10.0].into_iter().find(|m| m * p >= base).unwrap_or(10.0);
    let s0 = m * p;
    let steps = match m as u32 {
        1 | 10 => [s0, 5.0 * s0, 10.0 * s0],
        2 => [s0, 5.0 * s0, 10.0 * s0],
        _ => [s0, 2.0 * s0, 10.0 * s0],
    };
    let (lo, hi) = input.bounds;
    let mut marks = Vec::new();
    let mut v = (lo / steps[0]).floor() * steps[0];
    while v <= hi && marks.len() < 2000 {
        let step = steps.iter().rev().find(|s| ((v / **s).round() * **s - v).abs() < 1e-9 * **s).copied().unwrap_or(steps[0]);
        marks.push(egui_plot::GridMark { value: v, step_size: step });
        v += steps[0];
    }
    marks
}

/// Header + body height + bottom grip, outside the panel frame.
const PANEL_CHROME: f32 = 66.0;
/// Height of a folded panel.
const PANEL_CLOSED: f32 = 50.0;
const MIN_H: f32 = 90.0;

/// A plot panel: a header (click to fold; readout on the right), the body
/// at `height`, and a grip below it to drag the height.
fn panel(ui: &mut Ui, t: &Tokens, title: &str, readout: &str, height: &mut f32, open: &mut bool, body: impl FnOnce(&mut Ui, f32)) {
    egui::Frame::new().fill(t.card).stroke(Stroke::new(1.0, t.card_border)).corner_radius(10).inner_margin(egui::Margin { left: 6, right: 6, top: 4, bottom: 6 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let row = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 26.0));
        let resp = ui.interact(row, ui.scope_id().with(("panel", title)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.hovered() {
            ui.painter().rect_filled(row, 6, t.accent_soft.gamma_multiply(0.6));
        }
        if resp.clicked() {
            *open = !*open;
        }
        ui.allocate_ui_with_layout(row.size(), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_size(row.size());
            ui.add_space(4.0);
            let arrow = if crate::ui::theme::icons_available() {
                if *open { icon::CHEVRON_DOWN } else { icon::CHEVRON_RIGHT }
            } else if *open {
                "▾"
            } else {
                "▸"
            };
            ui.add(egui::Label::new(RichText::new(arrow).color(t.faint)).selectable(false));
            ui.add(egui::Label::new(RichText::new(title).strong().color(t.text)).selectable(false));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(readout).monospace().size(12.5).color(t.accent_text)).selectable(false).truncate());
            });
        });
        if *open {
            ui.add_space(2.0);
            body(ui, *height);
        }
    });
    if *open {
        // Grip: drag to change the height.
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 12.0), egui::Sense::drag());
        let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text(tr("Drag to resize", "拖动调整高度"));
        if resp.dragged() {
            *height = (*height + resp.drag_delta().y).clamp(MIN_H, 1600.0);
        }
        let c = if resp.hovered() || resp.dragged() { t.accent } else { t.faint.gamma_multiply(0.6) };
        let mid = r.center();
        ui.painter().line_segment([mid - vec2(22.0, 0.0), mid + vec2(22.0, 0.0)], Stroke::new(3.0, c));
    } else {
        ui.add_space(6.0);
    }
}

/// Nearest local extremum of the top curve within `tol` of x, refined by a
/// golden-section search on the exact transform. Returns (x, kind).
#[allow(clippy::too_many_arguments)]
fn snap(sp: &Spectrum, band: &[(f64, C64)], conv: Convention, view: View, x: f64, tol: f64, max_mag: f64) -> Option<(f64, &'static str)> {
    let q = |v: C64| if view == View::MagPhase { v.norm() } else { v.re };
    let xs: Vec<f64> = band.iter().map(|(om, _)| conv.from_omega(*om)).collect();
    let qs: Vec<f64> = band.iter().map(|(_, v)| q(*v)).collect();
    let mut best: Option<(usize, bool)> = None;
    for i in 1..qs.len().saturating_sub(1) {
        let is_max = qs[i] >= qs[i - 1] && qs[i] > qs[i + 1];
        let is_min = qs[i] <= qs[i - 1] && qs[i] < qs[i + 1];
        if (is_max || is_min) && (xs[i] - x).abs() <= tol && best.is_none_or(|(b, _)| (xs[i] - x).abs() < (xs[b] - x).abs()) {
            best = Some((i, is_max));
        }
    }
    let (i, is_max) = best?;
    // Golden-section on [x_{i-1}, x_{i+1}] for the extremum of q(F(x)).
    let g = |x: f64| {
        let v = q(sp.at(conv.to_omega(x)));
        if is_max { -v } else { v }
    };
    let (mut a, mut b) = (xs[i - 1], xs[i + 1]);
    let phi = (5f64.sqrt() - 1.0) / 2.0;
    let (mut c, mut d) = (b - phi * (b - a), a + phi * (b - a));
    let (mut gc, mut gd) = (g(c), g(d));
    for _ in 0..60 {
        if gc < gd {
            b = d;
            d = c;
            gd = gc;
            c = b - phi * (b - a);
            gc = g(c);
        } else {
            a = c;
            c = d;
            gc = gd;
            d = a + phi * (b - a);
            gd = g(d);
        }
    }
    let xm = (a + b) / 2.0;
    let kind = if is_max {
        tr("peak", "峰值")
    } else if view == View::MagPhase && sp.at(conv.to_omega(xm)).norm() < 1e-6 * max_mag {
        tr("zero", "零点")
    } else {
        tr("valley", "谷值")
    };
    Some((xm, kind))
}
