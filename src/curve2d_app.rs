//! The Plane curves workspace: up to six curves in the plane (graphs,
//! implicit, polar, parametric, regions), their special points and
//! intersections, a point on a curve with its tangent and circle of
//! curvature, and integrals: areas and arc lengths.

use std::time::Instant;

use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, UiBuilder, pos2, vec2};

use crate::app::{kv, range_edit};
use crate::curve2d::features::{self, ASYMPTOTES, Asym, EXTREMA, Features, INFLECTIONS, Kind, Line3, SINGULAR, Touch, ZEROS};
use crate::curve2d::integral::{Integral, integrate};
use crate::curve2d::nice;
use crate::curve2d::region::{self, RegionDraw};
use crate::curve2d::sample::{self, Event};
use crate::curve2d::trace::{Grid, trace};
use crate::curve2d::{Def, Form, Frame2};
use crate::i18n::{self, tr};
use crate::render::scene::{tick_label, ticks};
use crate::space::object::COLORS;
use crate::ui::export::{self, Export};
use crate::ui::params::Params;
use crate::ui::theme::{Tokens, icon, tokens, with_icon};
use crate::ui::widgets::{badge, caption, card, eye, layer_bar, overlay_frame, segmented};

/// Most curves at once.
pub const MAX_CURVES: usize = 6;
const CIRCLED: [&str; 6] = ["①", "②", "③", "④", "⑤", "⑥"];
/// Implicit curves are traced on cells of this many points.
const TRACE_CELL: f64 = 2.5;

const EXAMPLES: &[(&str, &str, &str)] = &[
    ("Parabola", "抛物线", "y = x^2 - 2"),
    ("Sine", "正弦曲线", "y = sin(x)"),
    ("Exponential", "指数函数", "y = e^x"),
    ("Logarithm", "对数函数", "y = ln(x)"),
    ("Hyperbola y = 1/x", "反比例函数", "y = 1/x"),
    ("Tangent", "正切函数", "y = tan(x)"),
    ("Semicircle", "半圆", "y = sqrt(4 - x^2)"),
    ("Absolute value", "绝对值", "y = |x| - 1"),
    ("Floor (jumps)", "取整函数（跳跃）", "y = floor(x)"),
    ("Cubic", "三次函数", "y = x^3 - 3x"),
    ("Rational with asymptotes", "有渐近线的分式函数", "y = (x^2 + 1)/(x - 1)"),
    ("Piecewise", "分段函数", "y = if(x < 0, -x, x^2)"),
    ("Parabola x = y²", "抛物线 x = y²", "x = y^2 - 1"),
    ("Circle", "圆", "x^2 + y^2 = 4"),
    ("Ellipse", "椭圆", "x^2/9 + y^2/4 = 1"),
    ("Hyperbola", "双曲线", "x^2 - y^2 = 1"),
    ("Folium of Descartes", "笛卡尔叶形线", "x^3 + y^3 = 3x y"),
    ("Cardioid", "心形线", "r = 1 + cos(θ)"),
    ("Rose", "玫瑰线", "r = 2sin(3θ)"),
    ("Lemniscate", "双纽线", "r^2 = 4cos(2θ)"),
    ("Spiral of Archimedes", "阿基米德螺线", "r = θ/2"),
    ("Cycloid", "摆线", "(t - sin(t), 1 - cos(t))"),
    ("Astroid", "星形线", "(2cos(t)^3, 2sin(t)^3)"),
    ("Lissajous", "利萨如图形", "(2sin(3t), 2sin(2t))"),
    ("Disc (region)", "圆盘（区域）", "x^2 + y^2 <= 4"),
    ("Region between two curves", "两曲线之间的区域", "x^2 <= y <= sqrt(x)"),
    ("Polar region", "极坐标区域", "r <= 2cos(θ)"),
    ("With parameters a, b", "带参数 a、b", "y = a sin(b x)"),
    ("LaTeX", "LaTeX 写法", "y = \\frac{1}{1 + x^2}"),
];

/// A ready-made picture.
struct Scene {
    en: &'static str,
    zh: &'static str,
    /// (formula, marks, range of t or θ)
    curves: Vec<(&'static str, u8, Option<(f64, f64)>)>,
    pi_ticks: bool,
    polar_grid: bool,
    /// Show the asymptotes.
    asym: bool,
    /// A point off the curves with the tangents through it.
    ext: Option<[f64; 2]>,
    view: Option<View>,
    /// Point on curve k at parameter u, with tangent and curvature circle.
    pick: Option<(usize, f64)>,
    /// Integral over [a, b] of curve k (against curve m).
    integral: Option<(usize, Option<usize>, IntKind, f64, f64)>,
}

fn scenes() -> Vec<Scene> {
    use std::f64::consts::{PI, TAU};
    let s = |en, zh, curves| Scene { en, zh, curves, pi_ticks: false, polar_grid: false, asym: false, ext: None, view: None, pick: None, integral: None };
    let all = ZEROS | EXTREMA | INFLECTIONS | ASYMPTOTES;
    vec![
        Scene { pi_ticks: true, ..s("sin x and cos x", "sin x 与 cos x", vec![("y = sin(x)", 0, None), ("y = cos(x)", 0, None)]) },
        Scene {
            view: Some(View { cx: 1.0, cy: 1.5, half: 8.0, aspect: 1.0 }),
            asym: true,
            ..s("Sketching a graph: extrema, inflection, asymptotes", "函数作图：极值、拐点与渐近线", vec![("y = (x^2 + 1)/(x - 1)", all, None)])
        },
        s("Cubic: zeros, extrema, inflection point", "三次函数：零点、极值与拐点", vec![("y = x^3/3 - x", ZEROS | EXTREMA | INFLECTIONS, None)]),
        Scene { pi_ticks: true, ..s("Parameters: y = a sin(bx + c)", "参数：y = a sin(bx + c)", vec![("y = a sin(b x + c)", 0, None), ("y = sin(x)", 0, None)]) },
        Scene {
            pick: Some((0, 1.0)),
            ..s("Tangent, normal and circle of curvature", "切线、法线与曲率圆", vec![("y = x^2/2", 0, None)])
        },
        Scene { asym: true, ..s("Folium of Descartes: tangents, a node, its asymptote", "笛卡尔叶形线：切线、奇点与渐近线", vec![("x^3 + y^3 = 3x y", ZEROS | EXTREMA | SINGULAR, None)]) },
        Scene { asym: true, ..s("Hyperbola and its asymptotes", "双曲线及其渐近线", vec![("x^2/4 - y^2 = 1", ZEROS, None)]) },
        Scene {
            ext: Some([0.0, -1.0]),
            ..s("Tangents from a point outside the curve", "过曲线外一点作切线", vec![("y = x^2", 0, None)])
        },
        Scene {
            ext: Some([3.0, 1.0]),
            ..s("Tangents from a point to a circle", "过圆外一点作圆的切线", vec![("x^2 + y^2 = 4", 0, None)])
        },
        s("Circle and ellipse: intersections", "圆与椭圆的交点", vec![("x^2 + y^2 = 4", 0, None), ("x^2/9 + y^2 = 1", 0, None)]),
        Scene { polar_grid: true, ..s("Cardioid", "心形线", vec![("r = 1 + cos(θ)", ZEROS | EXTREMA, None)]) },
        Scene { polar_grid: true, ..s("Rose r = 2cos 3θ", "三叶玫瑰线", vec![("r = 2cos(3θ)", 0, Some((0.0, PI)))]) },
        Scene { polar_grid: true, ..s("Lemniscate r² = 4cos 2θ", "双纽线 r² = 4cos 2θ", vec![("r^2 = 4cos(2θ)", 0, None)]) },
        Scene { polar_grid: true, ..s("Spiral of Archimedes", "阿基米德螺线", vec![("r = θ/2", 0, Some((0.0, 6.0 * PI)))]) },
        Scene {
            view: Some(View { cx: 6.3, cy: 1.5, half: 7.5, aspect: 1.0 }),
            ..s("Cycloid: cusps", "摆线与尖点", vec![("(t - sin(t), 1 - cos(t))", EXTREMA | SINGULAR, Some((-0.5, 4.0 * PI + 0.5)))])
        },
        s("Astroid", "星形线", vec![("(2cos(t)^3, 2sin(t)^3)", SINGULAR, Some((0.0, TAU)))]),
        s("Lissajous figure", "利萨如图形", vec![("(2sin(3t), 2sin(2t))", 0, Some((0.0, TAU)))]),
        Scene {
            view: Some(View { cx: 0.6, cy: 0.5, half: 1.6, aspect: 1.0 }),
            integral: Some((0, Some(1), IntKind::Between, 0.0, 1.0)),
            ..s("Area between y = √x and y = x²", "y = √x 与 y = x² 围成的面积", vec![("y = sqrt(x)", 0, None), ("y = x^2", 0, None)])
        },
        Scene {
            polar_grid: true,
            integral: Some((0, None, IntKind::Area, 0.0, TAU)),
            ..s("Area inside a cardioid", "心形线围成的面积", vec![("r = 2(1 + cos(θ))", 0, None)])
        },
        s("Region D: x² + y² ≤ 4, y ≥ x", "区域 D：x² + y² ≤ 4，y ≥ x", vec![("x^2 + y^2 <= 4, y >= x", 0, None)]),
        Scene { polar_grid: true, ..s("Polar region r ≤ 2cos θ", "极坐标区域 r ≤ 2cos θ", vec![("r <= 2cos(θ)", 0, None)]) },
        s("Strict inequalities are dashed", "严格不等式的边界画成虚线", vec![("y > x^2 - 2 and y < x", 0, None)]),
    ]
}

/// What part of the plane is shown.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct View {
    pub cx: f64,
    pub cy: f64,
    /// Half of the visible x range.
    pub half: f64,
    /// Data units of y per data unit of x on screen (1: equal scales).
    pub aspect: f64,
}

impl View {
    pub const HOME: View = View { cx: 0.0, cy: 0.0, half: 6.0, aspect: 1.0 };

    pub fn frame(&self, rect: Rect) -> Frame2 {
        let kx = rect.width().max(1.0) as f64 / (2.0 * self.half);
        let ky = kx / self.aspect;
        let hh = rect.height().max(1.0) as f64 / 2.0 / ky;
        Frame2 { x0: self.cx - self.half, x1: self.cx + self.half, y0: self.cy - hh, y1: self.cy + hh, kx, ky }
    }
}

fn to_screen(rect: Rect, f: &Frame2, x: f64, y: f64) -> Pos2 {
    pos2(rect.left() + f.sx(x) as f32, rect.top() + f.sy(y) as f32)
}

fn from_screen(rect: Rect, f: &Frame2, p: Pos2) -> [f64; 2] {
    [f.x0 + (p.x - rect.left()) as f64 / f.kx, f.y1 - (p.y - rect.top()) as f64 / f.ky]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
enum IntKind {
    /// ∫ f(x) dx.
    Signed,
    /// ∫ (f − g) dx.
    Between,
    /// Arc length.
    Arc,
    /// Polar sector area or the area swept by a parametric curve.
    Area,
}

/// Drawing data of one curve for a window a little larger than the view.
#[derive(Default)]
struct Cache {
    key: String,
    window: Option<Frame2>,
    lines: Vec<Line3>,
    events: Vec<Event>,
    region: Option<RegionDraw>,
    tex: Option<egui::TextureHandle>,
    version: u64,
    feats: Option<(u8, Features)>,
    asym: Option<Vec<Asym>>,
}

pub struct CurveItem {
    pub uid: u32,
    pub src: String,
    pub def: Option<Def>,
    pub error: Option<String>,
    pub color: usize,
    pub visible: bool,
    /// Range of t or θ.
    pub range: (f64, f64),
    /// Special points shown (bits of `curve2d::features`).
    pub marks: u8,
    cache: Cache,
    /// Area of a region in the view: (key, value).
    area: Option<(String, f64)>,
}

impl CurveItem {
    pub fn new(src: &str, color: usize) -> Self {
        let mut c = Self {
            uid: 0,
            src: src.to_string(),
            def: None,
            error: None,
            color,
            visible: true,
            range: (0.0, std::f64::consts::TAU),
            marks: 0,
            cache: Cache::default(),
            area: None,
        };
        c.recompile();
        c
    }

    /// Parse again; the last valid definition stays when the text has an error.
    pub fn recompile(&mut self) {
        match Def::new(&self.src) {
            Ok(d) => {
                self.def = Some(d);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.cache.key.clear();
    }

    fn form(&self) -> Option<Form> {
        self.def.as_ref().map(|d| d.form)
    }

    fn is_curve(&self) -> bool {
        self.form().is_some_and(|f| f != Form::Region)
    }

    /// Bring the drawing data up to date for the visible window.
    fn update(&mut self, vis: &Frame2, pkey: &str) {
        let Some(def) = &self.def else {
            self.cache = Cache::default();
            return;
        };
        let key = format!("{}|{:?}|{pkey}", self.src, self.range);
        let c = &mut self.cache;
        let fresh = c.key == key
            && c.window.is_some_and(|w| w.contains(vis) && (0.75..1.33).contains(&(vis.kx / w.kx)) && (0.75..1.33).contains(&(vis.ky / w.ky)));
        if fresh {
            return;
        }
        let w = vis.grown(0.25);
        let path = |us: &[f64]| def.path(us);
        let mut lines: Vec<Line3> = Vec::new();
        let mut events = Vec::new();
        let mut region = None;
        let to3 = |s: sample::Sampled, lines: &mut Vec<Line3>, events: &mut Vec<Event>| {
            lines.extend(s.lines.into_iter().map(|l| l.into_iter().map(|p| [p.x, p.y, p.u]).collect()));
            *events = s.events;
        };
        let [ww, wh] = w.size();
        match def.form {
            Form::FnX => to3(sample::sample(&path, w.x0, w.x1, ww as usize, &w), &mut lines, &mut events),
            Form::FnY => to3(sample::sample(&path, w.y0, w.y1, wh as usize, &w), &mut lines, &mut events),
            Form::Param | Form::Polar => to3(sample::sample(&path, self.range.0, self.range.1, 1500, &w), &mut lines, &mut events),
            Form::Implicit => {
                let (nu, nv) = ((ww / TRACE_CELL).ceil() as usize, (wh / TRACE_CELL).ceil() as usize);
                // Offsets keep grid nodes off round numbers such as the origin.
                let g = Grid { u0: w.x0 - 0.137 * TRACE_CELL / w.kx, v0: w.y0 - 0.071 * TRACE_CELL / w.ky, du: TRACE_CELL / w.kx, dv: TRACE_CELL / w.ky, nu: nu + 1, nv: nv + 1 };
                let f = |u: &[f64], v: &[f64]| def.field(u, v);
                lines = trace(&f, &g).into_iter().map(|l| l.into_iter().map(|p| [p[0], p[1], f64::NAN]).collect()).collect();
            }
            Form::PolarImplicit => {
                // Trace in the (θ, r) plane, then map to (r cos θ, r sin θ).
                let corners = [(w.x0, w.y0), (w.x0, w.y1), (w.x1, w.y0), (w.x1, w.y1)];
                let rmax = corners.iter().map(|(x, y)| x.hypot(*y)).fold(0.0, f64::max);
                let k = w.kx.min(w.ky);
                let nr = ((2.0 * rmax * k / TRACE_CELL) as usize).clamp(40, 900);
                let span = self.range.1 - self.range.0;
                let nt = ((720.0 * span / std::f64::consts::TAU) as usize).clamp(90, (400_000 / nr).max(90));
                let g = Grid { u0: self.range.0, v0: -rmax * 1.0007, du: span / nt as f64, dv: 2.0 * rmax * 1.0007 / nr as f64, nu: nt, nv: nr };
                let f = |u: &[f64], v: &[f64]| def.field(u, v);
                lines = trace(&f, &g).into_iter().map(|l| l.into_iter().map(|p| [p[1] * p[0].cos(), p[1] * p[0].sin(), f64::NAN]).collect()).collect();
            }
            Form::Region => region = Some(region::draw(def, &w, 2.0)),
        }
        *c = Cache { key, window: Some(w), lines, events, region, tex: None, version: c.version + 1, feats: None, asym: None };
    }

    /// Special points for the current drawing (computed when first asked).
    fn features(&mut self) -> Option<&Features> {
        let (def, w) = (self.def.as_ref()?, self.cache.window?);
        let marks = self.marks;
        if marks == 0 {
            return None;
        }
        if self.cache.feats.as_ref().is_none_or(|f| f.0 != marks) {
            let fe = match def.form {
                Form::FnX => features::path_features(def, w.x0, w.x1, &w, marks),
                Form::FnY => features::path_features(def, w.y0, w.y1, &w, marks),
                Form::Param | Form::Polar => features::path_features(def, self.range.0, self.range.1, &w, marks),
                Form::Implicit => {
                    let lines: Vec<Vec<[f64; 2]>> = self.cache.lines.iter().map(|l| l.iter().map(|p| [p[0], p[1]]).collect()).collect();
                    features::field_features(def, &lines, &w, marks)
                }
                _ => Features::default(),
            };
            self.cache.feats = Some((marks, fe));
        }
        self.cache.feats.as_ref().map(|f| &f.1)
    }

    /// Asymptotes of the curve (computed when first asked).
    fn asymptotes(&mut self) -> &[Asym] {
        if self.cache.asym.is_none() {
            let a = match (&self.def, self.cache.window) {
                (Some(def), Some(w)) => match def.form {
                    Form::FnX => features::path_asymptotes(def, &self.cache.events, w.x1 - w.x0),
                    Form::FnY => features::path_asymptotes(def, &self.cache.events, w.y1 - w.y0),
                    Form::Param | Form::Polar => features::path_asymptotes(def, &self.cache.events, self.range.1 - self.range.0),
                    Form::Implicit => features::field_asymptotes(def, &w),
                    _ => vec![],
                },
                _ => vec![],
            };
            // A straight line is not its own asymptote: drop lines that the
            // drawn curve lies on everywhere.
            let pts: Vec<[f64; 3]> = self.cache.lines.iter().flat_map(|l| l.iter().step_by((l.len() / 8).max(1)).copied()).collect();
            let on = |a: &Asym, p: &[f64; 3]| match *a {
                Asym::Vertical(x) => (p[0] - x).abs() <= 1e-9 * (1.0 + x.abs()),
                Asym::Line { k, b } => (k * p[0] + b - p[1]).abs() <= 1e-9 * (1.0 + p[1].abs() + (k * p[0]).abs()),
            };
            let a = a.into_iter().filter(|a| pts.len() < 3 || !pts.iter().all(|p| on(a, p))).collect();
            self.cache.asym = Some(a);
        }
        self.cache.asym.as_deref().unwrap_or(&[])
    }

    /// The parameter range searched for tangents through a point.
    fn search_range(&self) -> (f64, f64) {
        match (self.form(), self.cache.window) {
            (Some(Form::FnX), Some(w)) => (w.x0, w.x1),
            (Some(Form::FnY), Some(w)) => (w.y0, w.y1),
            _ => self.range,
        }
    }
}

/// A chosen point on a curve.
#[derive(Clone, Copy, Debug)]
struct Pick {
    uid: u32,
    /// Curve parameter (NaN for implicit curves).
    u: f64,
    x: f64,
    y: f64,
}

/// Geometry at the chosen point.
struct PickInfo {
    p: [f64; 2],
    /// Unit tangent (NaN when unknown).
    t: [f64; 2],
    slope: f64,
    /// Signed curvature and the centre of curvature.
    kappa: f64,
    center: Option<[f64; 2]>,
}

struct IntegralState {
    curve: Option<u32>,
    other: Option<u32>,
    kind: IntKind,
    a: f64,
    b: f64,
    show: bool,
    /// (key, main result, area / second result)
    result: Option<(String, Integral, Option<Integral>)>,
}

/// Saved view of the Plane workspace beyond the curves and the window:
/// layers, the chosen point, the point Q and the integral. Curves are
/// referred to by their position in the list.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct PlaneView {
    show_grid: bool,
    polar_grid: bool,
    pi_ticks: bool,
    show_asym: bool,
    hover_tangent: bool,
    show_marks: bool,
    show_inter: bool,
    show_labels: bool,
    show_legend: bool,
    /// Point Q with the tangents through it.
    ext: Option<[f64; 2]>,
    /// Chosen point: (curve, parameter, x, y).
    pick: Option<(usize, f64, f64, f64)>,
    show_tangent: bool,
    show_normal: bool,
    show_circle: bool,
    /// Integral: curve, second curve, kind, a, b, shown.
    int_curve: Option<usize>,
    int_other: Option<usize>,
    int_kind: IntKind,
    int_a: f64,
    int_b: f64,
    int_show: bool,
}

impl Default for PlaneView {
    fn default() -> Self {
        PlaneWorkspace::new().view_state()
    }
}

pub struct PlaneWorkspace {
    curves: Vec<CurveItem>,
    next_uid: u32,
    fold_below: u32,
    params: Params,
    view: View,
    equal: bool,
    show_grid: bool,
    polar_grid: bool,
    pi_ticks: bool,
    show_asym: bool,
    /// Tangent at the curve point under the cursor.
    hover_tangent: bool,
    /// A point with the tangents through it, and whether a click places it.
    ext: Option<[f64; 2]>,
    ext_mode: bool,
    dragging_ext: bool,
    /// Tangents through `ext`: (key, list of (curve index, touch)).
    ext_tangents: (String, Vec<(usize, Touch)>),
    show_marks: bool,
    show_inter: bool,
    show_labels: bool,
    show_legend: bool,
    layers_open: bool,
    pick: Option<Pick>,
    show_tangent: bool,
    show_normal: bool,
    show_circle: bool,
    dragging_pick: bool,
    integral: IntegralState,
    /// Intersections: (key, list of (curve a, curve b, point)).
    inter: (String, Vec<(usize, usize, [f64; 2])>),
    last_rect: Rect,
    export: Export,
    toast: Option<(String, Instant)>,
    draw_ms: f64,
    hover: Option<[f64; 2]>,
}

impl Default for PlaneWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

fn c32(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Formula text with superscripts and dots, for display only.
fn pretty(src: &str) -> String {
    if crate::expr::latex::looks_like_latex(src) {
        return src.to_string();
    }
    src.replace("^2", "²").replace("^3", "³").replace("**2", "²").replace('*', "·").replace("sqrt", "√").replace("theta", "θ")
}

/// `y = kx + b` through p (vertical when the slope is infinite).
fn line_eq(p: [f64; 2], slope: f64) -> String {
    if !slope.is_finite() || slope.abs() > 1e12 {
        return format!("x = {}", nice::short(p[0]));
    }
    let k = if slope.abs() < 1e-12 { 0.0 } else { slope };
    let b = p[1] - k * p[0];
    let b = if b.abs() < 1e-12 * (1.0 + p[1].abs()) { 0.0 } else { b };
    let kx = match nice::short(k).as_str() {
        "0" => String::new(),
        "1" => "x".into(),
        "−1" => "−x".into(),
        // A fraction: x/2, −3x/4, 3πx/4 (not 1/2x, which reads as 1/(2x)).
        s if s.contains('/') => {
            let (num, den) = s.rsplit_once('/').unwrap();
            let num = match num {
                "1" => "",
                "−1" => "−",
                n => n,
            };
            format!("{num}x/{den}")
        }
        s => format!("{s}x"),
    };
    let bs = nice::short(b.abs());
    match (kx.is_empty(), b == 0.0) {
        (true, _) => format!("y = {}", nice::short(b)),
        (false, true) => format!("y = {kx}"),
        (false, false) => format!("y = {kx} {} {bs}", if b < 0.0 { "−" } else { "+" }),
    }
}

/// A number as the parser reads it, exact when simple: `3pi/4`,
/// `sqrt(2)/2`, `-5`, else ten decimals.
fn src_num(v: f64) -> String {
    if v.abs() < 1e-12 {
        return "0".into();
    }
    let r = v.round();
    if (v - r).abs() < 1e-9 * v.abs().max(1.0) {
        return format!("{}", r as i64);
    }
    if let Some((_, f)) = nice::find(v, 1e-9 * v.abs().max(1.0)) {
        let mut out = String::new();
        let mut chars = f.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                'π' => out.push_str("pi"),
                '−' => out.push('-'),
                '√' => {
                    out.push_str("sqrt(");
                    while let Some(d) = chars.peek().copied().filter(|d| d.is_ascii_digit()) {
                        out.push(d);
                        chars.next();
                    }
                    out.push(')');
                }
                c => out.push(c),
            }
        }
        return out;
    }
    let s = format!("{v:.10}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Source text of the line through p with direction d, for a new curve.
fn line_src(p: [f64; 2], d: [f64; 2]) -> String {
    if d[0].abs() <= 1e-12 * d[1].abs() {
        return format!("x = {}", src_num(p[0]));
    }
    let k = d[1] / d[0];
    let b = p[1] - k * p[0];
    let kx = match src_num(k).as_str() {
        "0" => String::new(),
        "1" => "x".into(),
        "-1" => "-x".into(),
        s => format!("{s}*x"),
    };
    let bs = src_num(b);
    match (kx.is_empty(), bs.as_str()) {
        (true, _) => format!("y = {bs}"),
        (false, "0") => format!("y = {kx}"),
        (false, _) if bs.starts_with('-') => format!("y = {kx} - {}", &bs[1..]),
        (false, _) => format!("y = {kx} + {bs}"),
    }
}

/// Clip a polyline (screen coordinates in f64) to a rectangle; returns the
/// visible runs.
fn clip_runs(pts: &[[f64; 2]], r: Rect) -> Vec<Vec<Pos2>> {
    let (x0, x1, y0, y1) = (r.left() as f64, r.right() as f64, r.top() as f64, r.bottom() as f64);
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    let p32 = |p: [f64; 2]| pos2(p[0] as f32, p[1] as f32);
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        // Liang–Barsky.
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut ok = true;
        for (p, q) in [(-dx, a[0] - x0), (dx, x1 - a[0]), (-dy, a[1] - y0), (dy, y1 - a[1])] {
            if p == 0.0 {
                if q < 0.0 {
                    ok = false;
                    break;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        if !ok || t0 > t1 || !t0.is_finite() || !t1.is_finite() {
            if cur.len() > 1 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
            continue;
        }
        let pa = [a[0] + t0 * dx, a[1] + t0 * dy];
        let pb = [a[0] + t1 * dx, a[1] + t1 * dy];
        if t0 > 0.0 || cur.is_empty() {
            if cur.len() > 1 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
            cur.push(p32(pa));
        }
        cur.push(p32(pb));
        if t1 < 1.0 {
            out.push(std::mem::take(&mut cur));
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

fn pill(p: &egui::Painter, pos: Pos2, align: Align2, text: String, color: Color32, bg: Color32) -> Rect {
    let galley = p.layout_no_wrap(text, FontId::monospace(11.5), color);
    let r = align.anchor_size(pos, galley.size()).expand2(vec2(3.0, 1.0));
    p.rect_filled(r, CornerRadius::same(3), bg);
    p.galley(r.min + vec2(3.0, 1.0), galley, color);
    r
}

fn tooltip(p: &egui::Painter, at: Pos2, text: &str, t: &Tokens, clip: Rect) {
    let galley = p.layout_no_wrap(text.to_string(), FontId::proportional(12.5), t.text);
    let size = galley.size() + vec2(12.0, 8.0);
    let mut r = Rect::from_min_size(at + vec2(14.0, 14.0), size);
    if r.right() > clip.right() - 4.0 {
        r = r.translate(vec2(-size.x - 28.0, 0.0));
    }
    if r.bottom() > clip.bottom() - 4.0 {
        r = r.translate(vec2(0.0, -size.y - 28.0));
    }
    p.rect(r, 6.0, t.overlay, Stroke::new(1.0, t.overlay_border), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(6.0, 4.0), galley, t.text);
}

impl PlaneWorkspace {
    pub fn new() -> Self {
        let mut s = Self {
            curves: Vec::new(),
            next_uid: 1,
            fold_below: 0,
            params: Params::default(),
            view: View::HOME,
            equal: true,
            show_grid: true,
            polar_grid: false,
            pi_ticks: false,
            show_asym: false,
            hover_tangent: false,
            ext: None,
            ext_mode: false,
            dragging_ext: false,
            ext_tangents: (String::new(), Vec::new()),
            show_marks: true,
            show_inter: true,
            show_labels: false,
            show_legend: true,
            layers_open: false,
            pick: None,
            show_tangent: true,
            show_normal: false,
            show_circle: true,
            dragging_pick: false,
            integral: IntegralState { curve: None, other: None, kind: IntKind::Signed, a: 0.0, b: 1.0, show: false, result: None },
            inter: (String::new(), Vec::new()),
            last_rect: Rect::from_min_size(Pos2::ZERO, vec2(900.0, 700.0)),
            export: Export::default(),
            toast: None,
            draw_ms: 0.0,
            hover: None,
        };
        s.set_curves(vec![CurveItem::new("y = sin(x)", 0), CurveItem::new("y = x^2/4 - 1", 1)]);
        s.fold_below = s.next_uid;
        s
    }

    pub fn state(&self) -> crate::session::PlaneState {
        use crate::session::{PlaneCurveState, PlaneState};
        PlaneState {
            curves: self.curves.iter().map(|c| PlaneCurveState { src: c.src.clone(), color: c.color, visible: c.visible, range: c.range, marks: c.marks }).collect(),
            center: (self.view.cx, self.view.cy),
            half: self.view.half,
            aspect: self.view.aspect,
            equal: self.equal,
            params: self.params.state(),
        }
    }

    pub fn view_state(&self) -> PlaneView {
        let idx = |uid: Option<u32>| uid.and_then(|u| self.index_of(u));
        let it = &self.integral;
        PlaneView {
            show_grid: self.show_grid,
            polar_grid: self.polar_grid,
            pi_ticks: self.pi_ticks,
            show_asym: self.show_asym,
            hover_tangent: self.hover_tangent,
            show_marks: self.show_marks,
            show_inter: self.show_inter,
            show_labels: self.show_labels,
            show_legend: self.show_legend,
            ext: self.ext,
            pick: self.pick.and_then(|p| Some((self.index_of(p.uid)?, p.u, p.x, p.y))),
            show_tangent: self.show_tangent,
            show_normal: self.show_normal,
            show_circle: self.show_circle,
            int_curve: idx(it.curve),
            int_other: idx(it.other),
            int_kind: it.kind,
            int_a: it.a,
            int_b: it.b,
            int_show: it.show,
        }
    }

    /// Put back a saved view; call after [`Self::restore`] so that curve
    /// positions refer to the restored curves.
    pub fn restore_view(&mut self, v: PlaneView) {
        self.show_grid = v.show_grid;
        self.polar_grid = v.polar_grid && !v.show_grid;
        self.pi_ticks = v.pi_ticks;
        self.show_asym = v.show_asym;
        self.hover_tangent = v.hover_tangent;
        self.show_marks = v.show_marks;
        self.show_inter = v.show_inter;
        self.show_labels = v.show_labels;
        self.show_legend = v.show_legend;
        self.show_tangent = v.show_tangent;
        self.show_normal = v.show_normal;
        self.show_circle = v.show_circle;
        let finite = |p: &[f64]| p.iter().all(|x| x.is_finite() && x.abs() < 1e12);
        self.ext = v.ext.filter(|q| finite(q));
        self.pick = v.pick.and_then(|(k, u, x, y)| {
            let c = self.curves.get(k)?;
            // A path point is found again from u, an implicit one from (x, y).
            (u.is_finite() || finite(&[x, y])).then_some(Pick { uid: c.uid, u, x, y })
        });
        self.settle_pick();
        let uid = |k: Option<usize>| k.and_then(|k| self.curves.get(k)).map(|c| c.uid);
        let it = &mut self.integral;
        it.curve = uid(v.int_curve);
        it.other = uid(v.int_other);
        it.kind = v.int_kind;
        if finite(&[v.int_a, v.int_b]) {
            (it.a, it.b) = (v.int_a, v.int_b);
        }
        it.show = v.int_show && it.curve.is_some();
        it.result = None;
    }

    pub fn restore(&mut self, s: crate::session::PlaneState) {
        use crate::session::range_ok;
        let v = View { cx: s.center.0, cy: s.center.1, half: s.half, aspect: s.aspect };
        if [v.cx, v.cy, v.half, v.aspect].iter().all(|x| x.is_finite()) && v.half > 1e-9 && v.half < 1e9 && v.aspect > 1e-6 && v.aspect < 1e6 {
            self.view = v;
            self.equal = s.equal;
            if self.equal {
                self.view.aspect = 1.0;
            }
        }
        let curves: Vec<CurveItem> = s
            .curves
            .into_iter()
            .map(|c| {
                let mut it = CurveItem::new(&c.src, c.color % COLORS.len());
                it.visible = c.visible;
                if range_ok(c.range) {
                    it.range = c.range;
                }
                it.marks = c.marks & (ZEROS | EXTREMA | INFLECTIONS | ASYMPTOTES | SINGULAR);
                it
            })
            .collect();
        self.params.restore(&s.params);
        self.set_curves(curves);
        self.fold_below = self.next_uid;
    }

    fn set_curves(&mut self, curves: Vec<CurveItem>) {
        self.curves = curves.into_iter().take(MAX_CURVES).collect();
        for c in &mut self.curves {
            c.uid = self.next_uid;
            self.next_uid += 1;
        }
        self.pick = None;
        self.integral.curve = None;
        self.integral.other = None;
        self.integral.result = None;
        self.sync_params();
    }

    /// Load example `k` (also used by the preview example).
    pub fn load_scene(&mut self, k: usize) {
        let Some(sc) = scenes().into_iter().nth(k) else { return };
        let curves = sc
            .curves
            .iter()
            .enumerate()
            .map(|(i, (src, marks, range))| {
                let mut c = CurveItem::new(src, i % COLORS.len());
                c.marks = *marks;
                if let Some(r) = range {
                    c.range = *r;
                }
                c
            })
            .collect();
        self.set_curves(curves);
        self.pi_ticks = sc.pi_ticks;
        self.polar_grid = sc.polar_grid;
        self.show_grid = !sc.polar_grid;
        self.show_asym = sc.asym;
        self.ext = sc.ext;
        self.ext_mode = false;
        self.view = sc.view.unwrap_or(View::HOME);
        self.equal = (self.view.aspect - 1.0).abs() < 1e-12;
        if let Some((k, u)) = sc.pick
            && let Some(c) = self.curves.get(k)
        {
            self.pick = Some(Pick { uid: c.uid, u, x: f64::NAN, y: f64::NAN });
            self.show_tangent = true;
            self.show_circle = true;
        }
        if let Some((k, m, kind, a, b)) = sc.integral {
            self.integral.curve = self.curves.get(k).map(|c| c.uid);
            self.integral.other = m.and_then(|m| self.curves.get(m)).map(|c| c.uid);
            (self.integral.kind, self.integral.a, self.integral.b, self.integral.show) = (kind, a, b, true);
        }
    }

    /// Set the formula of the first curve (preview example).
    pub fn set_function(&mut self, src: &str) {
        if let Some(c) = self.curves.first_mut() {
            c.src = src.to_string();
            c.recompile();
        } else {
            self.set_curves(vec![CurveItem::new(src, 0)]);
        }
        self.sync_params();
    }

    pub fn relocalize(&mut self) {
        for c in &mut self.curves {
            c.recompile();
        }
        self.sync_params();
    }

    fn sync_params(&mut self) {
        let mut names: Vec<String> = Vec::new();
        for c in &self.curves {
            for p in c.def.as_ref().map(|d| d.params.clone()).unwrap_or_default() {
                if !names.contains(&p) {
                    names.push(p);
                }
            }
        }
        self.params.sync(&names);
        self.push_params();
    }

    fn push_params(&mut self) {
        let named: Vec<(String, f64)> = self.params.items.iter().map(|p| (p.name.clone(), p.value)).collect();
        for c in &mut self.curves {
            if let Some(d) = &mut c.def {
                d.set_params(&named);
            }
        }
    }

    fn params_key(&self) -> String {
        self.params.items.iter().map(|p| format!("{}={};", p.name, p.value)).collect()
    }

    fn index_of(&self, uid: u32) -> Option<usize> {
        self.curves.iter().position(|c| c.uid == uid)
    }

    pub fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let t = tokens(ui.ctx());
        self.status_bar(ui, &t);
        egui::Panel::left("plane_controls")
            .resizable(true)
            .default_size(380.0)
            .min_size(320.0)
            .max_size(580.0)
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin { left: 12, right: 6, top: 12, bottom: 4 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.set_width(ui.available_width() - 8.0);
                    self.controls(ui, &t);
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin::same(10)))
            .show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                let t0 = Instant::now();
                self.canvas(ui, rect, &t);
                self.draw_ms = 0.8 * self.draw_ms + 0.2 * t0.elapsed().as_secs_f64() * 1e3;
                if let Some(msg) = self.export.step(ui, rect, "plane") {
                    self.toast = Some((msg, Instant::now()));
                }
                if self.export.overlays_visible() {
                    self.overlays(ui, rect, &t);
                }
            });
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        egui::Panel::bottom("plane_status")
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(14, 5)).stroke(Stroke::new(1.0, t.card_border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let small = |s: &str, c: Color32| RichText::new(s).size(12.0).color(c);
                    ui.label(small(
                        tr(
                            "Drag to pan · Scroll to zoom at the cursor · Double-click to reset · Click a curve to pick a point · Shift-click: tangents through a point",
                            "拖动平移 · 滚轮以光标为中心缩放 · 双击复位 · 单击曲线选点 · Shift+单击：过该点作切线",
                        ),
                        t.faint,
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &self.toast {
                        Some((msg, at)) if at.elapsed().as_secs_f32() < 5.0 => {
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                            ui.label(small(msg, if export::is_ok(msg) { t.ok } else { t.error }));
                        }
                        _ => {
                            let msg = if i18n::zh() { format!("绘制 {:.1} ms", self.draw_ms) } else { format!("drawn in {:.1} ms", self.draw_ms) };
                            ui.label(small(&msg, t.faint));
                            if let Some([x, y]) = self.hover {
                                let f = self.view.frame(self.last_rect);
                                let d = |k: f64| (-(1.0 / k).log10()).ceil().clamp(0.0, 10.0) as usize;
                                ui.label(small("·", t.faint));
                                ui.label(small(&format!("x = {x:.*}   y = {y:.*}", d(f.kx), d(f.ky)), t.muted));
                            }
                        }
                    });
                });
            });
    }

    // ---- Sidebar ---------------------------------------------------------

    fn controls(&mut self, ui: &mut Ui, t: &Tokens) {
        let mut load: Option<usize> = None;
        let mut add = false;
        let count = self.curves.len();
        card(
            ui,
            "p_scene",
            icon::CURVE,
            tr("Curves", "图形"),
            false,
            |ui| {
                ui.menu_button(with_icon(icon::BOOK, tr("Examples", "示例")), |ui| {
                    ui.set_min_width(300.0);
                    for (k, sc) in scenes().iter().enumerate() {
                        if ui.button(tr(sc.en, sc.zh)).clicked() {
                            load = Some(k);
                            ui.close();
                        }
                    }
                });
            },
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(count < MAX_CURVES, egui::Button::new(format!("+ {}", tr("Curve", "曲线")))).clicked() {
                        add = true;
                    }
                });
                caption(ui, tr(
                    "y = f(x), x = g(y), any equation in x and y, r = f(θ), (x(t), y(t)), or inequalities for a region.",
                    "可以写 y = f(x)、x = g(y)、x 与 y 的任意方程、r = f(θ)、(x(t), y(t))，或用不等式表示区域。",
                ));
                caption(
                    ui,
                    &if i18n::zh() { format!("同时最多 {MAX_CURVES} 条（现有 {count} 条）。") } else { format!("Up to {MAX_CURVES} at a time ({count} now).") },
                );
            },
        );
        if let Some(k) = load {
            self.load_scene(k);
        }
        if add {
            let used: Vec<usize> = self.curves.iter().map(|c| c.color).collect();
            let color = (0..COLORS.len()).find(|c| !used.contains(c)).unwrap_or(0);
            let mut c = CurveItem::new("y = x", color);
            c.uid = self.next_uid;
            self.next_uid += 1;
            self.curves.push(c);
            self.sync_params();
        }

        let mut remove = None;
        let mut resync = false;
        let mut pick_feature: Option<(u32, Kind, [f64; 2], f64)> = None;
        let vis = self.view.frame(self.last_rect);
        for k in 0..self.curves.len() {
            let open = self.curves[k].uid >= self.fold_below;
            let (gone, changed) = curve_card(ui, t, k, &mut self.curves[k], open, &vis, self.show_asym, &mut pick_feature);
            if gone {
                remove = Some(k);
            }
            resync |= changed;
        }
        if let Some(k) = remove {
            let uid = self.curves[k].uid;
            self.curves.remove(k);
            if self.pick.is_some_and(|p| p.uid == uid) {
                self.pick = None;
            }
            resync = true;
        }
        if resync {
            self.sync_params();
        }
        if let Some((uid, _, p, u)) = pick_feature {
            self.pick = Some(Pick { uid, u, x: p[0], y: p[1] });
        }
        let ch = self.params.ui(ui, "plane_params");
        if ch.changed {
            self.push_params();
        }
        self.pick_card(ui, t);
        self.ext_card(ui, t);
        self.inter_card(ui, t);
        self.integral_card(ui, t);
        self.view_card(ui, t);
    }

    fn pick_card(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(pick) = self.pick else { return };
        let Some(k) = self.index_of(pick.uid) else {
            self.pick = None;
            return;
        };
        let info = self.pick_info();
        let mut clear = false;
        let mut u = pick.u;
        let (mut tan, mut nor, mut circ) = (self.show_tangent, self.show_normal, self.show_circle);
        let mut add_line: Option<String> = None;
        let c = &self.curves[k];
        let form = c.form();
        card(
            ui,
            "p_pick",
            icon::CROSSHAIRS,
            tr("Point on the curve", "曲线上的点"),
            true,
            |ui| {
                if ui.add(egui::Button::new(RichText::new("✕").size(13.0).color(t.muted)).frame(false)).on_hover_text(tr("Remove the point", "取消选点")).clicked() {
                    clear = true;
                }
            },
            |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 5.0, c32(COLORS[c.color]));
                    ui.label(RichText::new(CIRCLED[k.min(5)]).size(13.0).color(t.text));
                    ui.label(RichText::new(pretty(&c.src)).monospace().size(13.0).color(t.text));
                });
                if let Some(form) = form
                    && form.is_path()
                    && let Some(d) = &c.def
                {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} =", d.var)).monospace().color(t.text));
                        let speed = (self.view.half * 0.002).max(1e-4);
                        ui.add(egui::DragValue::new(&mut u).speed(speed).max_decimals(6)).on_hover_text(tr("Move the point along the curve", "沿曲线移动这个点"));
                    });
                }
                let Some(info) = &info else {
                    caption(ui, tr("The curve is not defined here.", "曲线在这里没有定义。"));
                    return;
                };
                let [x, y] = info.p;
                kv(ui, t, tr("Point", "坐标"), "", &format!("({}, {})", nice::short(x), nice::short(y)));
                if form == Some(Form::Polar) {
                    let r = d_polar_r(c.def.as_ref(), pick.u);
                    kv(ui, t, "r, θ", "", &format!("{}, {}", nice::short(r), nice::short(pick.u)));
                }
                if info.t[0].is_finite() {
                    let slope = if info.slope.is_finite() && info.slope.abs() < 1e12 { nice::show(info.slope) } else { tr("∞ (vertical)", "∞（竖直）").to_string() };
                    kv(ui, t, tr("Slope", "斜率"), "dy/dx", &slope);
                    kv(ui, t, tr("Tangent", "切线"), "", &line_eq(info.p, info.slope));
                    let ns = if info.slope.abs() < 1e-12 { f64::INFINITY } else { -1.0 / info.slope };
                    kv(ui, t, tr("Normal", "法线"), "", &line_eq(info.p, ns));
                    if info.kappa.is_finite() {
                        kv(ui, t, tr("Curvature", "曲率"), "κ", &nice::show(info.kappa.abs()));
                        if info.kappa.abs() > 1e-12 {
                            kv(ui, t, tr("Radius of curvature", "曲率半径"), "R = 1/κ", &nice::show(1.0 / info.kappa.abs()));
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.toggle_value(&mut tan, tr("Tangent", "切线"));
                        ui.toggle_value(&mut nor, tr("Normal", "法线"));
                        ui.toggle_value(&mut circ, tr("Circle of curvature", "曲率圆"));
                    });
                    ui.horizontal_wrapped(|ui| {
                        let full = self.curves.len() >= MAX_CURVES;
                        let tip = if full { tr("The list is full (6 curves)", "曲线已满（6 条）") } else { tr("Keep this line as a curve of its own", "把这条直线作为一条曲线保留下来") };
                        if ui.add_enabled(!full, egui::Button::new(format!("+ {}", tr("Keep tangent", "保留切线")))).on_hover_text(tip).on_disabled_hover_text(tip).clicked() {
                            add_line = Some(line_src(info.p, info.t));
                        }
                        if ui.add_enabled(!full, egui::Button::new(format!("+ {}", tr("Keep normal", "保留法线")))).on_hover_text(tip).on_disabled_hover_text(tip).clicked() {
                            add_line = Some(line_src(info.p, [-info.t[1], info.t[0]]));
                        }
                    });
                }
                caption(ui, tr("Drag the point along the curve, or click another curve.", "可以沿曲线拖动这个点，或单击另一条曲线。"));
            },
        );
        (self.show_tangent, self.show_normal, self.show_circle) = (tan, nor, circ);
        if let Some(src) = add_line {
            self.add_curve(&src);
        }
        if clear {
            self.pick = None;
        } else if let Some(p) = &mut self.pick
            && u != pick.u
        {
            p.u = u;
        }
    }

    /// Add a curve (a tangent or normal line); false when the list is full.
    fn add_curve(&mut self, src: &str) -> bool {
        if self.curves.len() >= MAX_CURVES {
            return false;
        }
        let used: Vec<usize> = self.curves.iter().map(|c| c.color).collect();
        let color = (0..COLORS.len()).find(|c| !used.contains(c)).unwrap_or(0);
        let mut c = CurveItem::new(src, color);
        c.uid = self.next_uid;
        self.next_uid += 1;
        self.curves.push(c);
        self.sync_params();
        true
    }

    fn update_ext_tangents(&mut self) {
        let Some(q) = self.ext else {
            self.ext_tangents = (String::new(), Vec::new());
            return;
        };
        let mut key = format!("{q:?}");
        for c in &self.curves {
            key += &format!("|{}:{}:{}:{:?}", c.uid, c.visible, c.cache.version, c.range);
        }
        if key == self.ext_tangents.0 {
            return;
        }
        let mut out = Vec::new();
        for (k, c) in self.curves.iter().enumerate() {
            if !c.visible || !c.is_curve() {
                continue;
            }
            let (Some(d), Some(w)) = (&c.def, c.cache.window) else { continue };
            let (u0, u1) = c.search_range();
            let lines: Vec<Vec<[f64; 2]>> = if d.form == Form::Implicit { c.cache.lines.iter().map(|l| l.iter().map(|p| [p[0], p[1]]).collect()).collect() } else { Vec::new() };
            for tc in features::tangents_from(d, q, u0, u1, &lines, &w) {
                out.push((k, tc));
            }
        }
        self.ext_tangents = (key, out);
    }

    fn ext_card(&mut self, ui: &mut Ui, t: &Tokens) {
        let Some(mut q) = self.ext else { return };
        let mut clear = false;
        let mut add: Option<String> = None;
        let full = self.curves.len() >= MAX_CURVES;
        let list = self.ext_tangents.1.clone();
        let curves = &self.curves;
        card(
            ui,
            "p_ext",
            icon::CROSSHAIRS,
            tr("Tangents through a point", "过点作切线"),
            true,
            |ui| {
                if ui.add(egui::Button::new(RichText::new("✕").size(13.0).color(t.muted)).frame(false)).on_hover_text(tr("Remove the point", "去掉这个点")).clicked() {
                    clear = true;
                }
            },
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Q = (").monospace().color(t.text));
                    let speed = (self.view.half * 0.002).max(1e-4);
                    ui.add(egui::DragValue::new(&mut q[0]).speed(speed).max_decimals(4));
                    ui.label(RichText::new(",").monospace().color(t.text));
                    ui.add(egui::DragValue::new(&mut q[1]).speed(speed).max_decimals(4));
                    ui.label(RichText::new(")").monospace().color(t.text));
                });
                if list.is_empty() {
                    caption(ui, tr(
                        "No tangent passes through this point near the view (for example, the point is inside a closed curve).",
                        "视图附近没有经过这个点的切线（例如点在封闭曲线内部）。",
                    ));
                }
                for (k, tc) in list.iter().take(12) {
                    let c = &curves[*k];
                    let slope = tc.dir[1] / tc.dir[0];
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.0, c32(COLORS[c.color]));
                        ui.label(RichText::new(CIRCLED[(*k).min(5)]).size(12.5).color(t.muted));
                        ui.label(RichText::new(tr("touches at", "切点")).size(12.5).color(t.muted));
                        ui.label(RichText::new(format!("({}, {})", nice::short(tc.p[0]), nice::short(tc.p[1]))).monospace().size(13.0).color(t.text));
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.label(RichText::new(line_eq(tc.p, slope)).monospace().size(13.0).color(t.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add_enabled(!full, egui::Button::new("+").small()).on_hover_text(tr("Add this line to the curves", "把这条切线加入曲线列表")).clicked() {
                                add = Some(line_src(tc.p, tc.dir));
                            }
                        });
                    });
                }
                if list.len() > 12 {
                    caption(ui, &if i18n::zh() { format!("……还有 {} 条", list.len() - 12) } else { format!("… and {} more", list.len() - 12) });
                }
                caption(ui, tr("Shift-click the view to move Q, or drag it.", "按住 Shift 单击画布可移动 Q，也可以直接拖动它。"));
            },
        );
        if clear {
            self.ext = None;
        } else {
            self.ext = Some(q);
        }
        if let Some(src) = add {
            self.add_curve(&src);
        }
    }

    fn inter_card(&mut self, ui: &mut Ui, t: &Tokens) {
        if !self.show_inter || self.inter.1.is_empty() {
            return;
        }
        let list = self.inter.1.clone();
        card(ui, "p_inter", icon::TARGET, &format!("{} ({})", tr("Intersections", "交点"), list.len()), false, |_| {}, |ui| {
            for (a, b, p) in list.iter().take(16) {
                kv(ui, t, &format!("{}∩{}", CIRCLED[(*a).min(5)], CIRCLED[(*b).min(5)]), "", &format!("({}, {})", nice::short(p[0]), nice::short(p[1])));
            }
            if list.len() > 16 {
                caption(ui, &if i18n::zh() { format!("……还有 {} 个", list.len() - 16) } else { format!("… and {} more", list.len() - 16) });
            }
            caption(ui, tr("Crossings inside the view. Points where curves only touch may be missed.", "只列出视图内穿过的交点；仅相切的点可能找不到。"));
        });
    }

    fn integral_card(&mut self, ui: &mut Ui, t: &Tokens) {
        // Curves that can be integrated.
        let paths: Vec<(u32, usize, Form)> = self.curves.iter().enumerate().filter_map(|(k, c)| c.form().filter(|f| f.is_path()).map(|f| (c.uid, k, f))).collect();
        let st = &mut self.integral;
        if st.curve.is_none_or(|u| !paths.iter().any(|p| p.0 == u)) {
            st.curve = paths.first().map(|p| p.0);
        }
        let label = |k: usize, c: &CurveItem| format!("{} {}", CIRCLED[k.min(5)], pretty(&c.src));
        let mut show = st.show;
        let curves = &self.curves;
        let mut reset_range: Option<(f64, f64)> = None;
        let mut from_inter = false;
        card(
            ui,
            "p_integral",
            icon::INTEGRAL,
            tr("Integrals and areas", "积分与面积"),
            false,
            |ui| {
                eye(ui, &mut show, tr("Shade in the view", "在视图中显示"));
            },
            |ui| {
                let Some(uid) = st.curve else {
                    caption(ui, tr("Add a curve y = f(x), r = f(θ) or (x(t), y(t)) first.", "请先添加 y = f(x)、r = f(θ) 或 (x(t), y(t)) 形式的曲线。"));
                    return;
                };
                let (_, k, form) = *paths.iter().find(|p| p.0 == uid).unwrap();
                ui.horizontal(|ui| {
                    ui.label(tr("Curve", "曲线"));
                    egui::ComboBox::from_id_salt("p_int_curve").selected_text(label(k, &curves[k])).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                        for (u, k, _) in &paths {
                            if ui.selectable_label(st.curve == Some(*u), label(*k, &curves[*k])).clicked() {
                                st.curve = Some(*u);
                                if curves[*k].form().is_some_and(|f| f.has_range()) {
                                    reset_range = Some(curves[*k].range);
                                }
                            }
                        }
                    });
                });
                let kinds: Vec<(IntKind, &str, &str)> = match form {
                    Form::FnX => vec![
                        (IntKind::Signed, tr("Integral", "定积分"), "∫ f(x) dx"),
                        (IntKind::Between, tr("Between curves", "两曲线之间"), "∫ [f(x) − g(x)] dx"),
                        (IntKind::Arc, tr("Arc length", "弧长"), "∫ √(1 + f′²) dx"),
                    ],
                    Form::Polar => vec![(IntKind::Area, tr("Area", "面积"), "½ ∫ r² dθ"), (IntKind::Arc, tr("Arc length", "弧长"), "∫ √(r² + r′²) dθ")],
                    Form::Param => vec![(IntKind::Area, tr("Area", "面积"), "½ ∫ (x y′ − y x′) dt"), (IntKind::Arc, tr("Arc length", "弧长"), "∫ √(x′² + y′²) dt")],
                    _ => vec![(IntKind::Arc, tr("Arc length", "弧长"), "∫ √(1 + g′²) dy")],
                };
                if !kinds.iter().any(|x| x.0 == st.kind) {
                    st.kind = kinds[0].0;
                }
                ui.horizontal(|ui| {
                    segmented(ui, &mut st.kind, &kinds);
                });
                let others: Vec<(u32, usize)> = paths.iter().filter(|p| p.2 == Form::FnX && p.0 != uid).map(|p| (p.0, p.1)).collect();
                if st.kind == IntKind::Between {
                    if st.other.is_none_or(|o| !others.iter().any(|p| p.0 == o)) {
                        st.other = others.first().map(|p| p.0);
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr("and", "与"));
                        let text = st.other.and_then(|o| others.iter().find(|p| p.0 == o)).map_or(tr("(no other y = f(x))", "（没有别的 y = f(x)）").to_string(), |p| label(p.1, &curves[p.1]));
                        egui::ComboBox::from_id_salt("p_int_other").selected_text(text).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                            for (u, k) in &others {
                                if ui.selectable_label(st.other == Some(*u), label(*k, &curves[*k])).clicked() {
                                    st.other = Some(*u);
                                }
                            }
                        });
                    });
                }
                let var = match form {
                    Form::FnX => "x",
                    Form::FnY => "y",
                    Form::Polar => "θ",
                    _ => "t",
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{var} ∈")).monospace().color(t.text));
                    let mut r = (st.a, st.b);
                    if range_edit(ui, &mut r) {
                        (st.a, st.b) = r;
                    }
                    if form.has_range() && ui.small_button(tr("Whole curve", "整条曲线")).clicked() {
                        reset_range = Some(curves[k].range);
                    }
                    if st.kind == IntKind::Between && ui.small_button(tr("Between crossings", "以交点为界")).on_hover_text(tr("From the first to the last crossing in the view", "从视图内第一个交点到最后一个交点")).clicked() {
                        from_inter = true;
                    }
                });
                match &st.result {
                    Some((_, main, second)) => {
                        let formula = kinds.iter().find(|x| x.0 == st.kind).map_or("", |x| x.2);
                        let bounds = format!("{var} ∈ [{}, {}]", nice::short(st.a), nice::short(st.b));
                        egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(format!("{formula},  {bounds}")).monospace().size(12.5).color(t.muted));
                            ui.label(RichText::new(format!("= {}", nice::show(main.value))).monospace().size(15.0).color(t.text));
                        });
                        if let Some(s) = second {
                            let name = if st.kind == IntKind::Signed { tr("Area ∫|f| dx", "面积 ∫|f| dx") } else { tr("Area ∫|f − g| dx", "面积 ∫|f − g| dx") };
                            kv(ui, t, name, "", &nice::show(s.value));
                        }
                        if !main.finite {
                            ui.label(RichText::new(with_icon(icon::ALERT, tr("The integrand is undefined somewhere in the interval.", "积分区间内被积函数有无定义的点。"))).size(12.5).color(t.warn));
                        } else if !main.converged {
                            ui.label(RichText::new(with_icon(icon::ALERT, tr("Did not converge: the integral may diverge.", "没有收敛：这个积分可能发散。"))).size(12.5).color(t.warn));
                        }
                    }
                    None => caption(ui, "…"),
                }
            },
        );
        st.show = show;
        if let Some(r) = reset_range {
            (st.a, st.b) = r;
        }
        let (cu, ou) = (st.curve, st.other);
        if from_inter
            && let (Some(a), Some(b)) = (cu.and_then(|u| self.index_of(u)), ou.and_then(|u| self.index_of(u)))
        {
            let mut xs: Vec<f64> = self.inter.1.iter().filter(|(p, q, _)| (*p == a && *q == b) || (*p == b && *q == a)).map(|(_, _, p)| p[0]).collect();
            xs.sort_by(|a, b| a.total_cmp(b));
            if xs.len() >= 2 {
                let st = &mut self.integral;
                (st.a, st.b) = (xs[0], xs[xs.len() - 1]);
            }
        }
        self.compute_integral();
    }

    fn compute_integral(&mut self) {
        let st = &self.integral;
        let Some(k) = st.curve.and_then(|u| self.index_of(u)) else {
            self.integral.result = None;
            return;
        };
        let other = st.other.and_then(|u| self.index_of(u));
        let key = format!("{}|{:?}|{:?}|{}|{}|{}", self.curves[k].src, other.map(|o| &self.curves[o].src), st.kind, st.a, st.b, self.params_key());
        if st.result.as_ref().is_some_and(|r| r.0 == key) {
            return;
        }
        let Some(d) = &self.curves[k].def else { return };
        let g = other.and_then(|o| self.curves[o].def.as_ref());
        let (a, b, kind) = (st.a, st.b, st.kind);
        let rel = 1e-10;
        let res: Option<(Integral, Option<Integral>)> = match kind {
            IntKind::Signed => {
                let h = |u: &[f64]| d.path(u)[1].clone();
                let ha = |u: &[f64]| d.path(u)[1].iter().map(|v| v.abs()).collect();
                Some((integrate(&h, a, b, rel), Some(integrate(&ha, a, b, rel))))
            }
            IntKind::Between => g.map(|g| {
                let diff = move |u: &[f64]| -> Vec<f64> {
                    let (f1, f2) = (d.path(u)[1].clone(), g.path(u)[1].clone());
                    f1.iter().zip(&f2).map(|(p, q)| p - q).collect()
                };
                let absd = |u: &[f64]| diff(u).iter().map(|v| v.abs()).collect();
                (integrate(&diff, a, b, rel), Some(integrate(&absd, a, b, rel)))
            }),
            IntKind::Arc => {
                let h = |u: &[f64]| {
                    let [dx, dy] = d.path_d1(u);
                    dx.iter().zip(&dy).map(|(p, q)| p.hypot(*q)).collect()
                };
                Some((integrate(&h, a, b, rel), None))
            }
            IntKind::Area => {
                let h = |u: &[f64]| -> Vec<f64> {
                    let [x, y] = d.path(u);
                    let [dx, dy] = d.path_d1(u);
                    (0..u.len()).map(|i| 0.5 * (x[i] * dy[i] - y[i] * dx[i])).collect()
                };
                Some((integrate(&h, a, b, rel), None))
            }
        };
        self.integral.result = res.map(|(m, s)| (key, m, s));
    }

    fn view_card(&mut self, ui: &mut Ui, t: &Tokens) {
        let f = self.view.frame(self.last_rect);
        let (mut xr, mut yr) = ((f.x0, f.x1), (f.y0, f.y1));
        let mut equal = self.equal;
        let mut reset = false;
        let (mut cx, mut cy) = (false, false);
        card(
            ui,
            "p_view",
            icon::RULER,
            tr("View", "视图范围"),
            false,
            |ui| {
                if ui.small_button(with_icon(icon::RESTORE, "")).on_hover_text(tr("Back to the default view", "恢复默认视图")).clicked() {
                    reset = true;
                }
            },
            |ui| {
                egui::Grid::new("p_view_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new("x").monospace().color(t.text));
                    ui.horizontal(|ui| cx = range_edit(ui, &mut xr));
                    ui.end_row();
                    ui.label(RichText::new("y").monospace().color(t.text));
                    ui.horizontal(|ui| cy = range_edit(ui, &mut yr));
                    ui.end_row();
                });
                ui.checkbox(&mut equal, tr("Equal scale on both axes", "两轴等比例"));
                caption(ui, tr("With equal scales circles look round; the other range follows the window.", "等比例时圆看起来是圆的；另一个范围随窗口大小而定。"));
            },
        );
        let (w, h) = (self.last_rect.width().max(1.0) as f64, self.last_rect.height().max(1.0) as f64);
        if cx {
            self.view.cx = 0.5 * (xr.0 + xr.1);
            self.view.half = 0.5 * (xr.1 - xr.0);
            if !self.equal {
                // Keep the y range.
                self.view.aspect = ((f.y1 - f.y0) / h) / ((xr.1 - xr.0) / w);
            }
        }
        if cy {
            self.view.cy = 0.5 * (yr.0 + yr.1);
            if self.equal {
                self.view.half = 0.5 * (yr.1 - yr.0) * w / h;
            } else {
                self.view.aspect = ((yr.1 - yr.0) / h) / ((f.x1 - f.x0) / w);
            }
        }
        if equal != self.equal {
            self.equal = equal;
            if equal {
                self.view.aspect = 1.0;
            }
        }
        if reset {
            self.view = View::HOME;
            self.equal = true;
        }
    }

    // ---- Canvas ----------------------------------------------------------

    fn pick_info(&self) -> Option<PickInfo> {
        let pick = self.pick?;
        let c = &self.curves[self.index_of(pick.uid)?];
        let d = c.def.as_ref()?;
        if d.form.is_path() {
            let [x, y, dx, dy, ddx, ddy] = d.path_at(pick.u);
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            let sp = dx.hypot(dy);
            let ok = sp.is_finite() && sp > 0.0;
            let tt = if ok { [dx / sp, dy / sp] } else { [f64::NAN; 2] };
            let kappa = if ok { (dx * ddy - dy * ddx) / (sp * sp * sp) } else { f64::NAN };
            let center = (kappa.is_finite() && kappa.abs() > 1e-12).then(|| [x - tt[1] / kappa, y + tt[0] / kappa]);
            return Some(PickInfo { p: [x, y], t: tt, slope: dy / dx, kappa, center });
        }
        if d.form == Form::Implicit {
            let [_, fx, fy, fxx, fxy, fyy] = d.field_hess(pick.x, pick.y);
            let g = fx.hypot(fy);
            let ok = g.is_finite() && g > 0.0;
            let tt = if ok { [fy / g, -fx / g] } else { [f64::NAN; 2] };
            let k = (fy * fy * fxx - 2.0 * fx * fy * fxy + fx * fx * fyy) / (g * g * g);
            let center = (ok && k.is_finite() && k.abs() > 1e-12).then(|| [pick.x - fx / g / k, pick.y - fy / g / k]);
            return Some(PickInfo { p: [pick.x, pick.y], t: tt, slope: -fx / fy, kappa: k, center });
        }
        pick.x.is_finite().then_some(PickInfo { p: [pick.x, pick.y], t: [f64::NAN; 2], slope: f64::NAN, kappa: f64::NAN, center: None })
    }

    /// Keep the chosen point on its curve after the curve changed.
    fn settle_pick(&mut self) {
        let Some(pick) = &mut self.pick else { return };
        let Some(c) = self.curves.iter().find(|c| c.uid == pick.uid) else { return };
        let Some(d) = &c.def else { return };
        if d.form.is_path() {
            let [x, y] = d.path(&[pick.u]);
            (pick.x, pick.y) = (x[0], y[0]);
        } else if d.form == Form::Implicit && pick.x.is_finite() {
            for _ in 0..4 {
                let [f, fx, fy] = d.field_grad(&[pick.x], &[pick.y]);
                let g2 = fx[0] * fx[0] + fy[0] * fy[0];
                if !(g2 > 0.0 && f[0].is_finite()) {
                    break;
                }
                pick.x -= f[0] * fx[0] / g2;
                pick.y -= f[0] * fy[0] / g2;
            }
        }
    }

    /// Nearest point on a drawn curve: (curve index, point, parameter, distance in points).
    fn nearest(&self, rect: Rect, f: &Frame2, at: Pos2, only: Option<u32>) -> Option<(usize, [f64; 2], f64, f32)> {
        let mut best: Option<(usize, [f64; 2], f64, f32)> = None;
        for (k, c) in self.curves.iter().enumerate() {
            if !c.visible || !c.is_curve() || only.is_some_and(|u| u != c.uid) {
                continue;
            }
            for l in &c.cache.lines {
                let mut prev: Option<(Pos2, [f64; 3])> = None;
                for p in l {
                    let s = to_screen(rect, f, p[0], p[1]);
                    if let Some((ps, pp)) = prev {
                        let d = s - ps;
                        let len2 = d.length_sq();
                        let tt = if len2 > 0.0 { ((at - ps).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
                        let q = ps + d * tt;
                        let dist = q.distance(at);
                        if dist.is_finite() && best.is_none_or(|b| dist < b.3) {
                            let tt = tt as f64;
                            best = Some((k, [pp[0] + tt * (p[0] - pp[0]), pp[1] + tt * (p[1] - pp[1])], pp[2] + tt * (p[2] - pp[2]), dist));
                        }
                    }
                    prev = Some((s, *p));
                }
            }
        }
        best
    }

    fn canvas(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        self.last_rect = rect;
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let fr0 = self.view.frame(rect);
        // ---- interaction ----
        if resp.drag_started_by(egui::PointerButton::Primary) {
            // Where the button went down (the drag starts a few points later).
            let start = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
            self.dragging_ext = match (start, self.ext) {
                (Some(s), Some(q)) => to_screen(rect, &fr0, q[0], q[1]).distance(s) < 12.0,
                _ => false,
            };
            self.dragging_pick = !self.dragging_ext
                && match (start, self.pick_info()) {
                    (Some(s), Some(info)) => to_screen(rect, &fr0, info.p[0], info.p[1]).distance(s) < 12.0,
                    _ => false,
                };
        }
        if resp.drag_stopped() {
            self.dragging_pick = false;
            self.dragging_ext = false;
        }
        if resp.dragged() {
            if self.dragging_ext && resp.dragged_by(egui::PointerButton::Primary) {
                if let Some(at) = resp.interact_pointer_pos() {
                    self.ext = Some(from_screen(rect, &fr0, at));
                }
            } else if self.dragging_pick && resp.dragged_by(egui::PointerButton::Primary) {
                if let (Some(at), Some(pick)) = (resp.interact_pointer_pos(), self.pick)
                    && let Some((_, p, u, _)) = self.nearest(rect, &fr0, at, Some(pick.uid))
                {
                    self.pick = Some(Pick { u, x: p[0], y: p[1], ..pick });
                }
            } else {
                let d = resp.drag_delta();
                self.view.cx -= d.x as f64 / fr0.kx;
                self.view.cy += d.y as f64 / fr0.ky;
            }
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0
                && let Some(p) = resp.hover_pos()
            {
                let k = (-scroll as f64 * 0.0015).exp().clamp(0.8, 1.25);
                let a = from_screen(rect, &fr0, p);
                let half = (self.view.half * k).clamp(1e-9, 1e9);
                let k = half / self.view.half;
                self.view.cx = a[0] + (self.view.cx - a[0]) * k;
                self.view.cy = a[1] + (self.view.cy - a[1]) * k;
                self.view.half = half;
            }
        }
        if resp.double_clicked() {
            self.view = View { aspect: if self.equal { 1.0 } else { self.view.aspect }, ..View::HOME };
        }
        let fr = self.view.frame(rect);
        // ---- data ----
        let pkey = self.params_key();
        for c in &mut self.curves {
            if c.visible || c.marks != 0 {
                c.update(&fr, &pkey);
            }
        }
        for c in &mut self.curves {
            if c.visible {
                if self.show_marks {
                    c.features();
                }
                if self.show_asym {
                    c.asymptotes();
                }
            }
        }
        self.settle_pick();
        self.update_ext_tangents();
        self.update_intersections(&fr);
        let shift = ui.input(|i| i.modifiers.shift);
        if resp.clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            if shift || self.ext_mode {
                self.ext = Some(from_screen(rect, &fr, at));
                self.update_ext_tangents();
            } else if let Some((uid, p, u)) = self.marker_at(rect, &fr, at) {
                self.pick = Some(Pick { uid, u, x: p[0], y: p[1] });
            } else if let Some((k, p, u, dist)) = self.nearest(rect, &fr, at, None)
                && dist < 9.0
            {
                self.pick = Some(Pick { uid: self.curves[k].uid, u, x: p[0], y: p[1] });
            }
            self.settle_pick();
        }
        self.hover = resp.hover_pos().map(|p| from_screen(rect, &fr, p));

        // ---- drawing ----
        let p = ui.painter_at(rect);
        let bg = if t.dark { Color32::from_rgb(0x15, 0x17, 0x1e) } else { Color32::WHITE };
        p.rect(rect, CornerRadius::same(10), bg, Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
        let clip = rect.shrink(1.0);
        let p = ui.painter_at(clip);
        self.draw_grid(&p, rect, &fr, t);
        // Regions.
        for c in &mut self.curves {
            if !c.visible {
                continue;
            }
            let Some(rd) = &c.cache.region else { continue };
            let col = c32(COLORS[c.color]);
            if c.cache.tex.is_none() {
                let mut img = egui::ColorImage::filled([rd.nu, rd.nv], Color32::TRANSPARENT);
                let fill = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), if t.dark { 80 } else { 64 });
                for j in 0..rd.nv {
                    for i in 0..rd.nu {
                        if rd.mask[j * rd.nu + i] {
                            img.pixels[(rd.nv - 1 - j) * rd.nu + i] = fill;
                        }
                    }
                }
                c.cache.tex = Some(ui.ctx().load_texture(format!("region{}", c.uid), img, egui::TextureOptions::LINEAR));
            }
            if let Some(tex) = &c.cache.tex {
                let w = rd.window;
                let r = Rect::from_min_max(to_screen(rect, &fr, w.x0, w.y1), to_screen(rect, &fr, w.x1, w.y0));
                p.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        self.draw_integral(&p, rect, &fr, t);
        self.draw_axes(&p, rect, &fr, t, bg);
        // Asymptotes.
        if self.show_asym {
            for c in &self.curves {
                let Some(list) = c.cache.asym.as_ref().filter(|_| c.visible) else { continue };
                let col = c32(COLORS[c.color]).gamma_multiply(0.75);
                for a in list {
                    let pts = match *a {
                        Asym::Vertical(x) => [[fr.sx(x), -10.0], [fr.sx(x), fr.size()[1] + 10.0]],
                        Asym::Line { k, b } => {
                            let (x0, x1) = (fr.x0 - 1.0 * (fr.x1 - fr.x0), fr.x1 + 1.0 * (fr.x1 - fr.x0));
                            [[fr.sx(x0), fr.sy(k * x0 + b)], [fr.sx(x1), fr.sy(k * x1 + b)]]
                        }
                    };
                    let pts = pts.map(|q| [q[0] + rect.left() as f64, q[1] + rect.top() as f64]);
                    for run in clip_runs(&pts, clip) {
                        p.extend(Shape::dashed_line(&run, Stroke::new(1.4, col), 7.0, 5.0));
                    }
                }
            }
        }
        // Curves and region boundaries.
        for c in &self.curves {
            if !c.visible {
                continue;
            }
            let col = c32(COLORS[c.color]);
            let to_s = |q: &[f64]| [rect.left() as f64 + fr.sx(q[0]), rect.top() as f64 + fr.sy(q[1])];
            for l in &c.cache.lines {
                let pts: Vec<[f64; 2]> = l.iter().map(|q| to_s(q)).collect();
                for run in clip_runs(&pts, clip.expand(4.0)) {
                    p.add(Shape::line(run, Stroke::new(2.5, col)));
                }
            }
            if let Some(rd) = &c.cache.region {
                for (lines, strict) in &rd.bounds {
                    for l in lines {
                        let pts: Vec<[f64; 2]> = l.iter().map(|q| to_s(q)).collect();
                        for run in clip_runs(&pts, clip.expand(4.0)) {
                            if *strict {
                                p.extend(Shape::dashed_line(&run, Stroke::new(2.0, col), 8.0, 5.0));
                            } else {
                                p.add(Shape::line(run, Stroke::new(2.0, col)));
                            }
                        }
                    }
                }
            }
        }
        self.draw_pick(&p, rect, &fr, t);
        self.draw_ext(&p, rect, &fr, t, bg);
        let hover = resp.hover_pos().filter(|_| !resp.dragged());
        let tip = self.draw_markers(&p, rect, &fr, t, bg, hover);
        // Hover: the nearest curve point, else nothing.
        if let Some(h) = hover {
            if let Some(text) = tip {
                tooltip(&p, h, &text, t, clip);
            } else if let Some((k, q, u, dist)) = self.nearest(rect, &fr, h, None)
                && dist < 9.0
            {
                let c = &self.curves[k];
                let s = to_screen(rect, &fr, q[0], q[1]);
                p.circle(s, 4.0, c32(COLORS[c.color]), Stroke::new(1.5, bg));
                let mut text = format!("x = {}\ny = {}", nice::short(q[0]), nice::short(q[1]));
                if let Some(d) = &c.def
                    && matches!(d.form, Form::Param | Form::Polar)
                    && u.is_finite()
                {
                    text += &format!("\n{} = {}", d.var, nice::short(u));
                }
                if self.hover_tangent
                    && let Some(d) = &c.def
                {
                    let dir = if d.form.is_path() {
                        let pp = d.path_at(u);
                        [pp[2], pp[3]]
                    } else if d.form == Form::Implicit {
                        let [_, fx, fy] = d.field_grad(&[q[0]], &[q[1]]);
                        [fy[0], -fx[0]]
                    } else {
                        [f64::NAN; 2]
                    };
                    let l = dir[0].hypot(dir[1]);
                    if l > 0.0 && l.is_finite() {
                        let dir = [dir[0] / l, dir[1] / l];
                        let big = 4.0 * (fr.x1 - fr.x0 + fr.y1 - fr.y0);
                        let sc = |x: f64, y: f64| [rect.left() as f64 + fr.sx(x), rect.top() as f64 + fr.sy(y)];
                        let ends = [sc(q[0] - dir[0] * big, q[1] - dir[1] * big), sc(q[0] + dir[0] * big, q[1] + dir[1] * big)];
                        let ink = if t.dark { Color32::from_white_alpha(150) } else { Color32::from_black_alpha(140) };
                        for run in clip_runs(&ends, clip) {
                            p.add(Shape::line(run, Stroke::new(1.3, ink)));
                        }
                        let k = dir[1] / dir[0];
                        text += &format!("\n{} {}", tr("slope", "斜率"), if k.is_finite() && k.abs() < 1e12 { nice::short(k) } else { "∞".into() });
                    }
                }
                tooltip(&p, s, &text, t, clip);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    }

    fn update_intersections(&mut self, fr: &Frame2) {
        if !self.show_inter {
            return;
        }
        let ids: Vec<usize> = (0..self.curves.len()).filter(|&k| self.curves[k].visible && self.curves[k].is_curve()).collect();
        let key: String = ids.iter().map(|&k| format!("{}:{};", self.curves[k].uid, self.curves[k].cache.version)).collect();
        if key == self.inter.0 {
            return;
        }
        let mut out = Vec::new();
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                let (ca, cb) = (&self.curves[a], &self.curves[b]);
                let (Some(da), Some(db), Some(w)) = (&ca.def, &cb.def, ca.cache.window) else { continue };
                for p in features::intersections(da, &ca.cache.lines, db, &cb.cache.lines, &w, 60) {
                    out.push((a, b, p));
                }
            }
        }
        // Those in the view first (the card lists these).
        let inside = |p: &[f64; 2]| p[0] >= fr.x0 && p[0] <= fr.x1 && p[1] >= fr.y0 && p[1] <= fr.y1;
        out.sort_by_key(|(_, _, p)| !inside(p));
        self.inter = (key, out);
    }

    /// A special point or intersection under the cursor: (curve uid, point, parameter).
    fn marker_at(&self, rect: Rect, fr: &Frame2, at: Pos2) -> Option<(u32, [f64; 2], f64)> {
        if self.show_marks {
            for c in &self.curves {
                let Some((_, fe)) = c.cache.feats.as_ref().filter(|_| c.visible) else { continue };
                for q in &fe.points {
                    if to_screen(rect, fr, q.x, q.y).distance(at) < 7.0 {
                        let u = if c.def.as_ref().is_some_and(|d| d.form == Form::FnX) { q.x } else if c.def.as_ref().is_some_and(|d| d.form == Form::FnY) { q.y } else { q.u };
                        return Some((c.uid, [q.x, q.y], u));
                    }
                }
            }
        }
        None
    }

    fn draw_grid(&self, p: &egui::Painter, rect: Rect, f: &Frame2, t: &Tokens) {
        if !self.show_grid && !self.polar_grid {
            return;
        }
        let minor = if t.dark { Color32::from_white_alpha(9) } else { Color32::from_black_alpha(9) };
        let major = if t.dark { Color32::from_white_alpha(22) } else { Color32::from_black_alpha(20) };
        let tx = ticks(f.x0, f.x1, ((rect.width() / 90.0) as usize).max(3));
        let ty = ticks(f.y0, f.y1, ((rect.height() / 70.0) as usize).max(3));
        if self.polar_grid {
            // Circles at the x tick step; rays every 30° from the first circle out.
            let step = if tx.len() > 1 { tx[1] - tx[0] } else { 1.0 };
            let corners = [(f.x0, f.y0), (f.x0, f.y1), (f.x1, f.y0), (f.x1, f.y1)];
            let rmax = corners.iter().map(|(x, y)| x.hypot(*y)).fold(0.0, f64::max);
            let inside = f.x0 <= 0.0 && f.x1 >= 0.0 && f.y0 <= 0.0 && f.y1 >= 0.0;
            let rmin = if inside { 0.0 } else { corners.iter().map(|(x, y)| x.hypot(*y)).fold(f64::INFINITY, f64::min).min(f.x0.abs().min(f.x1.abs()).max(f.y0.abs().min(f.y1.abs()))) };
            let o = to_screen(rect, f, 0.0, 0.0);
            let k0 = (rmin / step).floor().max(1.0) as i64;
            let k1 = (rmax / step).ceil() as i64;
            if k1 - k0 < 300 {
                for k in k0..=k1 {
                    let r = k as f64 * step;
                    let n = 180;
                    let pts: Vec<Pos2> = (0..=n).map(|i| {
                        let a = std::f64::consts::TAU * i as f64 / n as f64;
                        to_screen(rect, f, r * a.cos(), r * a.sin())
                    }).collect();
                    p.add(Shape::line(pts, Stroke::new(1.0, major)));
                }
            }
            let _ = minor;
            for i in 0..12 {
                let a = std::f64::consts::TAU * i as f64 / 12.0;
                let start = to_screen(rect, f, step * a.cos(), step * a.sin());
                let end = to_screen(rect, f, 2.0 * rmax * a.cos(), 2.0 * rmax * a.sin());
                p.line_segment([start, end], Stroke::new(1.0, major));
            }
            if inside {
                let lr = (rect.width().min(rect.height()) * 0.42) as f64;
                for i in (0..12).filter(|i| i % 3 != 0) {
                    let a = std::f64::consts::TAU * i as f64 / 12.0;
                    let at = o + vec2((lr * a.cos()) as f32, -(lr * a.sin()) as f32);
                    let label = nice::closed_form(a).unwrap_or_default();
                    pill(p, at, Align2::CENTER_CENTER, label, t.muted, Color32::TRANSPARENT);
                }
            }
            return;
        }
        let sub = |v: &[f64]| -> f64 {
            if v.len() < 2 {
                return 0.0;
            }
            let s = v[1] - v[0];
            let m = s / 10f64.powf(s.log10().floor());
            if (m - 2.0).abs() < 1e-9 { s / 4.0 } else { s / 5.0 }
        };
        let (mx, my) = (sub(&tx), sub(&ty));
        if mx > 0.0 && !self.pi_ticks && (f.x1 - f.x0) / mx < 600.0 {
            let mut x = (f.x0 / mx).ceil() * mx;
            while x <= f.x1 {
                let s = to_screen(rect, f, x, 0.0).x;
                p.line_segment([pos2(s, rect.top()), pos2(s, rect.bottom())], Stroke::new(1.0, minor));
                x += mx;
            }
        }
        if my > 0.0 && (f.y1 - f.y0) / my < 600.0 {
            let mut y = (f.y0 / my).ceil() * my;
            while y <= f.y1 {
                let s = to_screen(rect, f, 0.0, y).y;
                p.line_segment([pos2(rect.left(), s), pos2(rect.right(), s)], Stroke::new(1.0, minor));
                y += my;
            }
        }
        let xs: Vec<f64> = if self.pi_ticks { nice::pi_ticks(f.x0, f.x1, ((rect.width() / 70.0) as usize).max(3)).into_iter().map(|v| v.0).collect() } else { tx };
        for x in xs {
            let s = to_screen(rect, f, x, 0.0).x;
            p.line_segment([pos2(s, rect.top()), pos2(s, rect.bottom())], Stroke::new(1.0, major));
        }
        for y in ty {
            let s = to_screen(rect, f, 0.0, y).y;
            p.line_segment([pos2(rect.left(), s), pos2(rect.right(), s)], Stroke::new(1.0, major));
        }
    }

    fn draw_axes(&self, p: &egui::Painter, rect: Rect, f: &Frame2, t: &Tokens, bg: Color32) {
        let axis = if t.dark { Color32::from_white_alpha(150) } else { Color32::from_black_alpha(150) };
        let o = to_screen(rect, f, 0.0, 0.0);
        let label_bg = bg.gamma_multiply(0.85);
        // Where the axes (or the labels standing in for them) sit.
        let ax_y = o.y.clamp(rect.top() + 4.0, rect.bottom() - 20.0);
        let ax_x = o.x.clamp(rect.left() + 34.0, rect.right() - 4.0);
        let x_vis = o.y >= rect.top() && o.y <= rect.bottom();
        let y_vis = o.x >= rect.left() && o.x <= rect.right();
        if x_vis {
            p.line_segment([pos2(rect.left(), o.y), pos2(rect.right(), o.y)], Stroke::new(1.3, axis));
            let tip = pos2(rect.right() - 3.0, o.y);
            p.add(Shape::convex_polygon(vec![tip, tip + vec2(-9.0, -4.0), tip + vec2(-9.0, 4.0)], axis, Stroke::NONE));
            p.text(tip + vec2(-4.0, -8.0), Align2::RIGHT_BOTTOM, "x", FontId::proportional(16.0), t.text);
        }
        if y_vis {
            p.line_segment([pos2(o.x, rect.top()), pos2(o.x, rect.bottom())], Stroke::new(1.3, axis));
            let tip = pos2(o.x, rect.top() + 3.0);
            p.add(Shape::convex_polygon(vec![tip, tip + vec2(-4.0, 9.0), tip + vec2(4.0, 9.0)], axis, Stroke::NONE));
            p.text(tip + vec2(9.0, 3.0), Align2::LEFT_TOP, "y", FontId::proportional(16.0), t.text);
        }
        if x_vis && y_vis {
            p.text(o + vec2(-5.0, 4.0), Align2::RIGHT_TOP, "O", FontId::proportional(13.0), t.muted);
        }
        // Tick labels along the x-axis.
        let xt: Vec<(f64, String)> = if self.pi_ticks {
            nice::pi_ticks(f.x0, f.x1, ((rect.width() / 70.0) as usize).max(3))
        } else {
            let tx = ticks(f.x0, f.x1, ((rect.width() / 90.0) as usize).max(3));
            let s = if tx.len() > 1 { tx[1] - tx[0] } else { 1.0 };
            tx.into_iter().map(|v| (v, tick_label(v, s).replace('-', "−"))).collect()
        };
        for (v, label) in xt {
            let x = to_screen(rect, f, v, 0.0).x;
            if v == 0.0 || x < rect.left() + 12.0 || x > rect.right() - 24.0 {
                continue;
            }
            if x_vis {
                p.line_segment([pos2(x, o.y - 3.0), pos2(x, o.y + 3.0)], Stroke::new(1.2, axis));
            }
            pill(p, pos2(x, ax_y + 5.0), Align2::CENTER_TOP, label, t.muted, label_bg);
        }
        let ty = ticks(f.y0, f.y1, ((rect.height() / 70.0) as usize).max(3));
        let sy = if ty.len() > 1 { ty[1] - ty[0] } else { 1.0 };
        for v in ty {
            let y = to_screen(rect, f, 0.0, v).y;
            if v == 0.0 || y < rect.top() + 24.0 || y > rect.bottom() - 12.0 {
                continue;
            }
            if y_vis {
                p.line_segment([pos2(o.x - 3.0, y), pos2(o.x + 3.0, y)], Stroke::new(1.2, axis));
            }
            pill(p, pos2(ax_x - 6.0, y), Align2::RIGHT_CENTER, tick_label(v, sy).replace('-', "−"), t.muted, label_bg);
        }
    }

    /// Markers for special points and intersections; returns the tooltip
    /// of the one under the cursor.
    fn draw_markers(&self, p: &egui::Painter, rect: Rect, f: &Frame2, t: &Tokens, bg: Color32, hover: Option<Pos2>) -> Option<String> {
        let mut tip = None;
        let coord = |x: f64, y: f64| format!("({}, {})", nice::short(x), nice::short(y));
        if self.show_marks {
            for c in &self.curves {
                let Some((_, fe)) = c.cache.feats.as_ref().filter(|_| c.visible) else { continue };
                let col = c32(COLORS[c.color]);
                for q in &fe.points {
                    let s = to_screen(rect, f, q.x, q.y);
                    if !rect.expand(6.0).contains(s) {
                        continue;
                    }
                    p.circle(s, 4.5, col, Stroke::new(1.5, bg));
                    if self.show_labels {
                        pill(p, s + vec2(7.0, -6.0), Align2::LEFT_BOTTOM, coord(q.x, q.y), t.text, bg.gamma_multiply(0.8));
                    }
                    if hover.is_some_and(|h| h.distance(s) < 7.0) {
                        tip = Some(format!("{}\nx = {}\ny = {}", q.kind.name(), nice::show(q.x), nice::show(q.y)));
                    }
                }
            }
        }
        if self.show_asym && hover.is_some() && tip.is_none() {
            for c in &self.curves {
                let Some(list) = c.cache.asym.as_ref().filter(|_| c.visible) else { continue };
                {
                    for a in list {
                        let h = hover.unwrap();
                        let d = match *a {
                            Asym::Vertical(x) => (to_screen(rect, f, x, 0.0).x - h.x).abs(),
                            Asym::Line { k, b } => {
                                let q = from_screen(rect, f, h);
                                ((k * q[0] + b - q[1]) * f.ky / (1.0 + k * k * f.ky * f.ky / (f.kx * f.kx)).sqrt()).abs() as f32
                            }
                        };
                        if d < 5.0 {
                            let eq = match *a {
                                Asym::Vertical(x) => format!("x = {}", nice::short(x)),
                                Asym::Line { k, b } => line_eq([0.0, b], k),
                            };
                            tip = Some(format!("{}\n{eq}", tr("asymptote", "渐近线")));
                        }
                    }
                }
            }
        }
        if self.show_inter {
            for (a, b, q) in &self.inter.1 {
                let s = to_screen(rect, f, q[0], q[1]);
                if !rect.expand(6.0).contains(s) {
                    continue;
                }
                p.circle(s, 4.5, bg, Stroke::new(2.0, t.text));
                if self.show_labels {
                    pill(p, s + vec2(7.0, -6.0), Align2::LEFT_BOTTOM, coord(q[0], q[1]), t.text, bg.gamma_multiply(0.8));
                }
                if hover.is_some_and(|h| h.distance(s) < 7.0) {
                    tip = Some(format!("{} {}∩{}\nx = {}\ny = {}", tr("intersection", "交点"), CIRCLED[(*a).min(5)], CIRCLED[(*b).min(5)], nice::show(q[0]), nice::show(q[1])));
                }
            }
        }
        tip
    }

    fn draw_pick(&self, p: &egui::Painter, rect: Rect, f: &Frame2, t: &Tokens) {
        let Some(info) = self.pick_info() else { return };
        let Some(c) = self.pick.and_then(|pk| self.index_of(pk.uid)).map(|k| &self.curves[k]) else { return };
        let col = c32(COLORS[c.color]);
        let s = to_screen(rect, f, info.p[0], info.p[1]);
        let line_through = |dir: [f64; 2]| -> Vec<Vec<Pos2>> {
            let big = 4.0 * (f.x1 - f.x0 + f.y1 - f.y0);
            let a = [info.p[0] - dir[0] * big, info.p[1] - dir[1] * big];
            let b = [info.p[0] + dir[0] * big, info.p[1] + dir[1] * big];
            let sc = |q: [f64; 2]| [rect.left() as f64 + f.sx(q[0]), rect.top() as f64 + f.sy(q[1])];
            clip_runs(&[sc(a), sc(b)], rect)
        };
        let ink = if t.dark { Color32::from_rgb(0xe6, 0xe9, 0xf0) } else { Color32::from_rgb(0x30, 0x34, 0x40) };
        if info.t[0].is_finite() {
            if self.show_tangent {
                for run in line_through(info.t) {
                    p.add(Shape::line(run, Stroke::new(1.6, ink.gamma_multiply(0.85))));
                }
            }
            if self.show_normal {
                for run in line_through([-info.t[1], info.t[0]]) {
                    p.extend(Shape::dashed_line(&run, Stroke::new(1.4, ink.gamma_multiply(0.7)), 6.0, 4.0));
                }
            }
            if self.show_circle
                && let Some(cn) = info.center
            {
                let r = 1.0 / info.kappa.abs();
                let n = 240;
                let pts: Vec<[f64; 2]> = (0..=n)
                    .map(|i| {
                        let a = std::f64::consts::TAU * i as f64 / n as f64;
                        [rect.left() as f64 + f.sx(cn[0] + r * a.cos()), rect.top() as f64 + f.sy(cn[1] + r * a.sin())]
                    })
                    .collect();
                let circ = Color32::from_rgb(0xe0, 0x31, 0x31);
                for run in clip_runs(&pts, rect) {
                    p.extend(Shape::dashed_line(&run, Stroke::new(1.5, circ.gamma_multiply(0.85)), 6.0, 4.0));
                }
                let cs = to_screen(rect, f, cn[0], cn[1]);
                if rect.contains(cs) {
                    p.circle_filled(cs, 3.0, circ);
                    p.line_segment([cs, s], Stroke::new(1.0, circ.gamma_multiply(0.5)));
                }
            }
        }
        p.circle(s, 8.0, Color32::TRANSPARENT, Stroke::new(2.0, col));
        p.circle(s, 4.5, col, Stroke::new(1.5, Color32::WHITE));
        // Label beside the point, on the left near the right edge.
        let (at, align) = if s.x + 150.0 > rect.right() { (s + vec2(-10.0, 8.0), Align2::RIGHT_TOP) } else { (s + vec2(10.0, 8.0), Align2::LEFT_TOP) };
        pill(p, at, align, format!("({}, {})", nice::short(info.p[0]), nice::short(info.p[1])), t.text, t.overlay);
    }

    fn draw_ext(&self, p: &egui::Painter, rect: Rect, f: &Frame2, t: &Tokens, bg: Color32) {
        let Some(q) = self.ext else { return };
        let sc = |x: f64, y: f64| [rect.left() as f64 + f.sx(x), rect.top() as f64 + f.sy(y)];
        let big = 4.0 * (f.x1 - f.x0 + f.y1 - f.y0);
        for (k, tc) in &self.ext_tangents.1 {
            let col = c32(COLORS[self.curves.get(*k).map_or(0, |c| c.color)]);
            let (a, d) = (tc.p, tc.dir);
            let ends = [sc(a[0] - d[0] * big, a[1] - d[1] * big), sc(a[0] + d[0] * big, a[1] + d[1] * big)];
            for run in clip_runs(&ends, rect) {
                p.add(Shape::line(run, Stroke::new(1.5, col.gamma_multiply(0.8))));
            }
            let s = to_screen(rect, f, a[0], a[1]);
            p.circle(s, 4.5, bg, Stroke::new(2.0, col));
            if self.show_labels {
                pill(p, s + vec2(7.0, -6.0), Align2::LEFT_BOTTOM, format!("({}, {})", nice::short(a[0]), nice::short(a[1])), t.text, bg.gamma_multiply(0.8));
            }
        }
        let s = to_screen(rect, f, q[0], q[1]);
        let ink = if t.dark { Color32::from_rgb(0xe6, 0xe9, 0xf0) } else { Color32::from_rgb(0x30, 0x34, 0x40) };
        let r = 6.0;
        p.add(Shape::convex_polygon(vec![s + vec2(0.0, -r), s + vec2(r, 0.0), s + vec2(0.0, r), s + vec2(-r, 0.0)], ink, Stroke::new(1.5, bg)));
        p.text(s + vec2(9.0, -9.0), Align2::LEFT_BOTTOM, "Q", FontId::proportional(14.0), ink);
    }

    fn draw_integral(&self, p: &egui::Painter, rect: Rect, f: &Frame2, _t: &Tokens) {
        let st = &self.integral;
        if !st.show {
            return;
        }
        let Some(k) = st.curve.and_then(|u| self.index_of(u)) else { return };
        let Some(d) = &self.curves[k].def else { return };
        let col = c32(COLORS[self.curves[k].color]);
        let fill = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 70);
        let neg = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 34);
        let (a, b) = (st.a, st.b);
        let n = 800;
        let us: Vec<f64> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
        let lim = 1e5f32;
        let s = |x: f64, y: f64| {
            let q = to_screen(rect, f, x, y);
            pos2(q.x.clamp(-lim, lim), q.y.clamp(-lim, lim))
        };
        let mut mesh = egui::Mesh::default();
        let tri = |m: &mut egui::Mesh, a: Pos2, b: Pos2, c: Pos2, color: Color32| {
            let i = m.vertices.len() as u32;
            m.colored_vertex(a, color);
            m.colored_vertex(b, color);
            m.colored_vertex(c, color);
            m.add_triangle(i, i + 1, i + 2);
        };
        match st.kind {
            IntKind::Signed | IntKind::Between => {
                let top = d.path(&us)[1].clone();
                let bot = match (st.kind, st.other.and_then(|u| self.index_of(u)).and_then(|o| self.curves[o].def.as_ref())) {
                    (IntKind::Between, Some(g)) => g.path(&us)[1].clone(),
                    (IntKind::Between, None) => return,
                    _ => vec![0.0; us.len()],
                };
                for i in 0..n {
                    let (x0, x1) = (us[i], us[i + 1]);
                    let (h0, h1) = (top[i] - bot[i], top[i + 1] - bot[i + 1]);
                    if !(h0.is_finite() && h1.is_finite() && bot[i].is_finite() && bot[i + 1].is_finite()) {
                        continue;
                    }
                    let color_of = |h: f64| if h >= 0.0 || st.kind == IntKind::Between { fill } else { neg };
                    if h0 * h1 < 0.0 {
                        let tc = h0 / (h0 - h1);
                        let xc = x0 + tc * (x1 - x0);
                        let bc = bot[i] + tc * (bot[i + 1] - bot[i]);
                        tri(&mut mesh, s(x0, bot[i]), s(x0, top[i]), s(xc, bc), color_of(h0));
                        tri(&mut mesh, s(xc, bc), s(x1, top[i + 1]), s(x1, bot[i + 1]), color_of(h1));
                    } else {
                        let c = color_of(h0 + h1);
                        tri(&mut mesh, s(x0, bot[i]), s(x0, top[i]), s(x1, top[i + 1]), c);
                        tri(&mut mesh, s(x0, bot[i]), s(x1, top[i + 1]), s(x1, bot[i + 1]), c);
                    }
                }
                // Edges at a and b.
                for (x, i) in [(a, 0), (b, n)] {
                    if top[i].is_finite() && bot[i].is_finite() {
                        p.line_segment([s(x, bot[i]), s(x, top[i])], Stroke::new(1.2, col));
                    }
                }
            }
            IntKind::Area => {
                let [x, y] = d.path(&us);
                let closed = (x[0] - x[n]).hypot(y[0] - y[n]) < 1e-6 * (1.0 + x[0].abs() + y[0].abs());
                // A closed parametric curve is shaded from its centroid (the
                // value does not depend on the centre for closed curves).
                let ok: Vec<usize> = (0..=n).filter(|&i| x[i].is_finite() && y[i].is_finite()).collect();
                let c = if closed && d.form == Form::Param && !ok.is_empty() {
                    let m = ok.len() as f64;
                    [ok.iter().map(|&i| x[i]).sum::<f64>() / m, ok.iter().map(|&i| y[i]).sum::<f64>() / m]
                } else {
                    [0.0, 0.0]
                };
                for i in 0..n {
                    if x[i].is_finite() && y[i].is_finite() && x[i + 1].is_finite() && y[i + 1].is_finite() {
                        tri(&mut mesh, s(c[0], c[1]), s(x[i], y[i]), s(x[i + 1], y[i + 1]), fill);
                    }
                }
            }
            IntKind::Arc => {
                let [x, y] = d.path(&us);
                let pts: Vec<[f64; 2]> = (0..=n).filter(|&i| x[i].is_finite() && y[i].is_finite()).map(|i| [rect.left() as f64 + f.sx(x[i]), rect.top() as f64 + f.sy(y[i])]).collect();
                for run in clip_runs(&pts, rect) {
                    p.add(Shape::line(run, Stroke::new(8.0, fill)));
                }
            }
        }
        if !mesh.is_empty() {
            p.add(mesh);
        }
    }

    fn overlays(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let inner = rect.shrink(12.0);
        let mut top_right = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Min)));
        let mut fit = false;
        let toolbar = overlay_frame(t).inner_margin(egui::Margin::same(5)).show(&mut top_right, |ui| {
            ui.horizontal(|ui| {
                if ui.button(with_icon(icon::CAMERA, "")).on_hover_text(export::hint(tr("Save the view as PNG in", "把视图保存为 PNG，位置："))).clicked() {
                    self.export.start();
                }
                if ui.button(with_icon(icon::RESTORE, tr("Reset", "复位"))).on_hover_text(tr("Default view (double-click)", "默认视图（双击画布）")).clicked() {
                    self.view = View { aspect: if self.equal { 1.0 } else { self.view.aspect }, ..View::HOME };
                }
                if ui.button(with_icon(icon::TARGET, tr("Fit", "适应"))).on_hover_text(tr("Fit the view to the curves", "调整视图以显示全部曲线")).clicked() {
                    fit = true;
                }
                if ui
                    .selectable_label(self.ext_mode, with_icon(icon::CROSSHAIRS, tr("Tangent from a point", "过点作切线")))
                    .on_hover_text(tr("While on, a click places the point Q and draws the tangents through it (Shift-click works any time)", "打开后，单击画布放置点 Q，画出过 Q 的切线（随时也可以按住 Shift 单击）"))
                    .clicked()
                {
                    self.ext_mode = !self.ext_mode;
                }
            });
        });
        if fit {
            self.fit();
        }
        let legend_w = (toolbar.response.rect.left() - inner.left() - 12.0).max(120.0);
        if self.show_legend && !self.curves.is_empty() {
            let mut tl = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
            overlay_frame(t).show(&mut tl, |ui| {
                ui.set_max_width(legend_w - 20.0);
                ui.spacing_mut().item_spacing.y = 3.0;
                for (k, c) in self.curves.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        let col = c32(COLORS[c.color]);
                        ui.painter().circle_filled(r.center(), 5.0, if c.visible { col } else { col.gamma_multiply(0.3) });
                        ui.label(RichText::new(CIRCLED[k.min(5)]).size(13.0).color(if c.visible { t.text } else { t.faint }));
                        let text = if c.src.trim().is_empty() { "—".to_string() } else { pretty(&c.src) };
                        ui.add(egui::Label::new(RichText::new(text).monospace().size(12.5).color(if c.visible { t.text } else { t.faint })).truncate());
                    });
                }
            });
        }
        let (mut grid, mut polar) = (self.show_grid, self.polar_grid);
        let chips: Vec<(&mut bool, &str, &str, Option<Color32>, &str)> = vec![
            (&mut self.show_marks, tr("Special points", "特征点"), tr("Points", "特征点"), None, tr("Zeros, extrema … chosen in each curve's card", "零点、极值等（在各曲线卡片中选择）")),
            (&mut self.show_inter, tr("Intersections", "交点"), tr("Crossings", "交点"), None, tr("Where the curves cross", "曲线之间的交点")),
            (&mut self.show_labels, tr("Coordinates", "坐标标签"), tr("Labels", "标签"), None, tr("Write the coordinates next to the points", "在点旁边标出坐标")),
            (&mut self.show_asym, tr("Asymptotes", "渐近线"), tr("Asym.", "渐近线"), None, tr("Vertical, horizontal and oblique asymptotes of all curves", "所有曲线的竖直、水平与斜渐近线")),
            (&mut self.hover_tangent, tr("Tangent at cursor", "跟随切线"), tr("Follow", "跟随"), None, tr("Show the tangent where the cursor is on a curve", "鼠标移到曲线上时显示该点的切线")),
            (&mut grid, tr("Grid", "直角网格"), tr("Grid", "网格"), None, tr("Rectangular grid", "直角坐标网格")),
            (&mut polar, tr("Polar grid", "极坐标网格"), tr("Polar", "极坐标"), None, tr("Circles and rays around the origin (instead of the rectangular grid)", "以原点为中心的圆与射线（代替直角网格）")),
            (&mut self.pi_ticks, tr("π ticks", "π 刻度"), "π", None, tr("x-axis ticks in multiples of π", "x 轴刻度用 π 的倍数")),
            (&mut self.show_legend, tr("Legend", "图例"), tr("Legend", "图例"), None, tr("List of the curves", "曲线列表")),
        ];
        layer_bar(ui, inner, &mut self.layers_open, chips);
        // One grid at a time.
        if polar && !self.polar_grid {
            (self.polar_grid, self.show_grid) = (true, false);
        } else if grid && !self.show_grid {
            (self.show_grid, self.polar_grid) = (true, false);
        } else {
            (self.show_grid, self.polar_grid) = (grid, polar);
        }
    }

    /// Fit the view around the visible curves.
    fn fit(&mut self) {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let f = self.view.frame(self.last_rect);
        for c in &self.curves {
            if !c.visible {
                continue;
            }
            match c.form() {
                Some(Form::FnX) => {
                    for l in &c.cache.lines {
                        for q in l.iter().filter(|q| q[0] >= f.x0 && q[0] <= f.x1) {
                            xs.push(q[0]);
                            ys.push(q[1]);
                        }
                    }
                }
                Some(Form::Region) => {
                    if let Some(rd) = &c.cache.region {
                        for (lines, _) in &rd.bounds {
                            for l in lines {
                                for q in l {
                                    xs.push(q[0]);
                                    ys.push(q[1]);
                                }
                            }
                        }
                    }
                }
                Some(_) => {
                    for l in &c.cache.lines {
                        for q in l {
                            xs.push(q[0]);
                            ys.push(q[1]);
                        }
                    }
                }
                None => {}
            }
        }
        if xs.len() < 2 {
            return;
        }
        // Robust ranges: drop the outer 2% (poles).
        let range = |mut v: Vec<f64>| {
            v.retain(|x| x.is_finite());
            v.sort_by(|a, b| a.total_cmp(b));
            let n = v.len();
            if n == 0 {
                return None;
            }
            let (lo, hi) = (v[n / 50], v[(n - 1) - n / 50]);
            Some((lo, hi))
        };
        let (Some(xr), Some(yr)) = (range(xs), range(ys)) else { return };
        let (w, h) = (self.last_rect.width().max(1.0) as f64, self.last_rect.height().max(1.0) as f64);
        let pad = |r: (f64, f64)| {
            let s = (r.1 - r.0).max(1e-6);
            (r.0 - 0.1 * s, r.1 + 0.1 * s)
        };
        let (xr, yr) = (pad(xr), pad(yr));
        self.view.cx = 0.5 * (xr.0 + xr.1);
        self.view.cy = 0.5 * (yr.0 + yr.1);
        if self.equal {
            self.view.half = (0.5 * (xr.1 - xr.0)).max(0.5 * (yr.1 - yr.0) * w / h);
        } else {
            self.view.half = 0.5 * (xr.1 - xr.0);
            self.view.aspect = ((yr.1 - yr.0) / h) / ((xr.1 - xr.0) / w);
        }
    }
}

fn d_polar_r(d: Option<&Def>, u: f64) -> f64 {
    d.and_then(|d| d.polar_r(&[u])).map_or(f64::NAN, |r| r[0][0])
}

/// One curve's card. Returns (removed, parameters may have changed).
#[allow(clippy::too_many_arguments)]
fn curve_card(ui: &mut Ui, t: &Tokens, k: usize, c: &mut CurveItem, default_open: bool, vis: &Frame2, show_asym: bool, pick: &mut Option<(u32, Kind, [f64; 2], f64)>) -> (bool, bool) {
    let mut visible = c.visible;
    let mut remove = false;
    let mut changed = false;
    let title = if c.src.trim().is_empty() { format!("{} {}", CIRCLED[k.min(5)], tr("new curve", "新曲线")) } else { format!("{} {}", CIRCLED[k.min(5)], pretty(&c.src)) };
    let title: String = if title.chars().count() > 30 { title.chars().take(29).collect::<String>() + "…" } else { title };
    card(
        ui,
        &format!("p_curve_{}", c.uid),
        icon::CURVE,
        &title,
        default_open,
        |ui| {
            if ui.add(egui::Button::new(RichText::new("✕").size(13.0).color(t.muted)).frame(false)).on_hover_text(tr("Remove this curve", "删除这条曲线")).clicked() {
                remove = true;
            }
            eye(ui, &mut visible, tr("Show in the view", "在视图中显示"));
        },
        |ui| {
            let mut pick_ex: Option<&str> = None;
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.menu_button(with_icon(icon::BOOK, ""), |ui| {
                    ui.set_min_width(300.0);
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        for (en, zh, f) in EXAMPLES {
                            if ui.button(tr(en, zh)).on_hover_text(RichText::new(*f).monospace()).clicked() {
                                pick_ex = Some(f);
                                ui.close();
                            }
                        }
                    });
                })
                .response
                .on_hover_text(tr("Examples", "示例"));
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut c.src)
                        .font(FontId::monospace(14.5))
                        .desired_width(ui.available_width())
                        .margin(egui::Margin::symmetric(8, 6))
                        .hint_text(tr("e.g. y = x^2", "如 y = x^2")),
                );
                if edit.changed() {
                    c.recompile();
                    changed = true;
                }
            });
            if let Some(f) = pick_ex {
                c.src = f.to_string();
                c.recompile();
                changed = true;
                if let Some(d) = &c.def
                    && d.form.has_range()
                {
                    c.range = d.default_range();
                }
            }
            if let Some(e) = &c.error {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(with_icon(icon::ALERT, e)).size(12.5).color(t.error));
                });
                if c.def.is_some() {
                    caption(ui, tr("Showing the last valid curve.", "下面显示的是上一条有效的曲线。"));
                }
            }
            let Some(form) = c.form() else { return };
            ui.horizontal_wrapped(|ui| {
                badge(ui, form.name(), t.ok);
            });
            if form.has_range() {
                let var = c.def.as_ref().map_or("t", |d| d.var);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{var} ∈")).monospace().color(t.text));
                    range_edit(ui, &mut c.range);
                    if ui.small_button("0 … 2π").clicked() {
                        c.range = (0.0, std::f64::consts::TAU);
                    }
                });
            }
            // Which special points to mark.
            let groups: Vec<(u8, &str, &str)> = match form {
                Form::FnX => vec![
                    (ZEROS, tr("Zeros", "零点"), tr("Zeros and the y-intercept", "零点与 y 轴截距")),
                    (EXTREMA, tr("Extrema", "极值"), tr("Local maxima and minima", "极大值与极小值")),
                    (INFLECTIONS, tr("Inflection", "拐点"), tr("Where the concavity changes", "凹凸性改变的点")),
                ],
                Form::FnY | Form::Param | Form::Polar => vec![
                    (ZEROS, tr("Axis crossings", "与坐标轴交点"), ""),
                    (EXTREMA, tr("Horizontal / vertical tangents", "水平 / 竖直切线"), ""),
                    (INFLECTIONS, tr("Inflection", "拐点"), ""),
                    (SINGULAR, tr("Cusps", "尖点"), tr("Points where the curve stops: x′ = y′ = 0", "x′ = y′ = 0 的点")),
                ],
                Form::Implicit => vec![
                    (ZEROS, tr("Axis crossings", "与坐标轴交点"), ""),
                    (EXTREMA, tr("Horizontal / vertical tangents", "水平 / 竖直切线"), ""),
                    (SINGULAR, tr("Singular points", "奇点"), tr("F = F_x = F_y = 0: crossings, cusps", "F = F_x = F_y = 0：自交点、尖点等")),
                ],
                _ => vec![],
            };
            if !groups.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr("Mark", "标注")).size(12.5).color(t.muted));
                    for (bit, label, tip) in &groups {
                        let mut on = c.marks & bit != 0;
                        let r = ui.toggle_value(&mut on, *label);
                        let r = if tip.is_empty() { r } else { r.on_hover_text(*tip) };
                        if r.changed() {
                            c.marks ^= bit;
                        }
                    }
                });
            }
            if form == Form::Region {
                region_area(ui, t, c, vis);
            }
            // The points found, to read their values.
            if let Some((_, fe)) = &c.cache.feats
                && c.marks != 0
            {
                let in_view: Vec<_> = fe.points.iter().filter(|q| q.x >= vis.x0 && q.x <= vis.x1 && q.y >= vis.y0 && q.y <= vis.y1).collect();
                for q in in_view.iter().take(10) {
                    let r = ui
                        .horizontal(|ui| {
                            ui.label(RichText::new(q.kind.name()).size(12.5).color(t.muted));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add(egui::Label::new(RichText::new(format!("({}, {})", nice::short(q.x), nice::short(q.y))).monospace().size(13.0).color(t.text)).sense(Sense::click()))
                            })
                            .inner
                        })
                        .inner;
                    if r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tr("Pick this point", "选取这个点")).clicked() {
                        let u = match form {
                            Form::FnX => q.x,
                            Form::FnY => q.y,
                            _ => q.u,
                        };
                        *pick = Some((c.uid, q.kind, [q.x, q.y], u));
                    }
                }
                if in_view.len() > 10 {
                    caption(ui, &if i18n::zh() { format!("……视图内还有 {} 个", in_view.len() - 10) } else { format!("… {} more in the view", in_view.len() - 10) });
                }
                if fe.truncated {
                    caption(ui, tr("Too many points: only some are shown.", "点太多，只显示了一部分。"));
                }
            }
            if show_asym {
                let list: Vec<Asym> = c.cache.asym.as_deref().unwrap_or(&[]).iter().copied().filter(|a| !matches!(a, Asym::Vertical(x) if *x < vis.x0 || *x > vis.x1)).collect();
                for a in list {
                    let eq = match a {
                        Asym::Vertical(x) => format!("x = {}", nice::short(x)),
                        Asym::Line { k, b } => line_eq([0.0, b], k),
                    };
                    kv(ui, t, tr("Asymptote", "渐近线"), "", &eq);
                }
            }
            appearance(ui, t, c);
        },
    );
    c.visible = visible;
    (remove, changed)
}

fn region_area(ui: &mut Ui, t: &Tokens, c: &mut CurveItem, vis: &Frame2) {
    let Some(d) = &c.def else { return };
    let key = format!("{}|{}|{}|{}|{}|{}", c.cache.key, vis.x0, vis.x1, vis.y0, vis.y1, c.cache.version);
    if c.area.as_ref().is_none_or(|a| a.0 != key) {
        c.area = Some((key, region::area(d, (vis.x0, vis.x1), (vis.y0, vis.y1), 500)));
    }
    if let Some((_, a)) = &c.area {
        kv(ui, t, tr("Area in the view", "视图内的面积"), "≈", &nice::short(*a));
        caption(ui, tr(
            "Counted on a fine grid over the visible part only. Strict inequalities are dashed.",
            "在细网格上数出，只算视图内可见的部分。严格不等式的边界画成虚线。",
        ));
    }
}

fn appearance(ui: &mut Ui, t: &Tokens, c: &mut CurveItem) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (k, rgb) in COLORS.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
            ui.painter().circle_filled(r.center(), 6.5, c32(*rgb));
            if c.color == k {
                ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, t.text));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && c.color != k {
                c.color = k;
                c.cache.tex = None;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_equations() {
        assert_eq!(line_eq([1.0, 2.0], 2.0), "y = 2x");
        assert_eq!(line_eq([1.0, 1.0], 2.0), "y = 2x − 1");
        assert_eq!(line_eq([2.0, 5.0], f64::INFINITY), "x = 2");
        assert_eq!(line_eq([0.0, 3.0], 0.0), "y = 3");
        assert_eq!(line_eq([1.0, 0.5], -1.0), "y = −x + 3/2");
        assert_eq!(line_eq([0.0, 0.0], 0.5), "y = x/2");
        assert_eq!(line_eq([0.0, 1.0], -0.75), "y = −3x/4 + 1");
    }

    #[test]
    fn kept_lines_parse_back() {
        let pi = std::f64::consts::PI;
        for (p, d) in [([1.0, 1.0], [1.0, 2.0]), ([0.5, 3f64.sqrt() / 2.0], [-3f64.sqrt() / 2.0, 0.5]), ([2.0, 5.0], [0.0, 1.0]), ([pi / 2.0, 1.0], [1.0, 0.0]), ([0.3, 0.7], [0.6, -0.8])] {
            let src = line_src(p, d);
            let def = Def::new(&src).unwrap_or_else(|e| panic!("{src}: {e}"));
            let k = d[1] / d[0];
            match def.form {
                Form::FnX => {
                    let y = def.path(&[p[0], p[0] + 1.0])[1].clone();
                    assert!((y[0] - p[1]).abs() < 1e-9 && (y[1] - y[0] - k).abs() < 1e-9, "{src}");
                }
                Form::FnY => assert!(d[0] == 0.0 && (def.path(&[0.0])[0][0] - p[0]).abs() < 1e-12, "{src}"),
                f => panic!("{src}: {f:?}"),
            }
        }
        assert_eq!(line_src([1.0, 1.0], [1.0, 2.0]), "y = 2*x - 1");
        assert_eq!(src_num(-3.0 * pi / 4.0), "-3pi/4");
        assert_eq!(src_num(3f64.sqrt() / 3.0), "sqrt(3)/3");
    }

    #[test]
    fn clipping_keeps_the_inside() {
        let r = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        let runs = clip_runs(&[[-50.0, 50.0], [50.0, 50.0], [150.0, 50.0]], r);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].first().unwrap().x, 0.0);
        assert_eq!(runs[0].last().unwrap().x, 100.0);
        let runs = clip_runs(&[[50.0, -1e30], [50.0, 1e30]], r);
        assert_eq!(runs.len(), 1);
    }

    /// Every prefix of a formula as it is typed (tan(x) once hung the
    /// intersection search on its huge values near the poles).
    #[test]
    fn typing_partial_formulas() {
        let mut w = PlaneWorkspace::new();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(626.0, 606.0));
        let f = w.view.frame(rect);
        let full = "y = tan(x)";
        for n in 0..=full.len() {
            w.curves[0].src = full[..n].to_string();
            w.curves[0].recompile();
            w.curves[0].marks = ZEROS | EXTREMA | INFLECTIONS | ASYMPTOTES;
            let pk = w.params_key();
            for c in &mut w.curves {
                c.update(&f, &pk);
                c.features();
            }
            w.update_intersections(&f);
        }
        // tan x meets y = x²/4 − 1 more than twice in the window.
        assert!(w.inter.1.len() >= 4, "{}", w.inter.1.len());
        let a = w.curves[0].asymptotes();
        assert!(a.len() >= 4, "{a:?}");
    }

    #[test]
    fn scenes_parse() {
        for (k, sc) in scenes().iter().enumerate() {
            for (src, _, _) in &sc.curves {
                assert!(Def::new(src).is_ok(), "scene {k}: {src}");
            }
        }
        for (_, _, src) in EXAMPLES {
            assert!(Def::new(src).is_ok(), "{src}");
        }
        let mut w = PlaneWorkspace::new();
        for k in 0..scenes().len() {
            w.load_scene(k);
            assert!(!w.curves.is_empty());
        }
    }
}
