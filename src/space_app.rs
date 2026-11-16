//! The Space workspace: up to three surfaces, lines / curves and solids in
//! one box, for looking at how they meet.

use std::time::Instant;

use egui::{Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, vec2};
use glam::DVec3;

use crate::app::{fmt_short, kv, range_edit};
use crate::calc_view::{self, Theme};
use crate::i18n::{self, tr};
use crate::render::scene::{Frame3, Scene, hex};
use crate::space::object::{COLORS, DEFAULT_CURVE, Kind, Object};
use crate::space::quadric::{Class, Rhs};
use crate::space::solid::{Solid, SolidKind};
use crate::space::surface::SurfaceKind;
use crate::space_inter::Originals;
use crate::space_view::{self, Lines};
use crate::ui::canvas3d::{self, View3d};
use crate::ui::export::{self, Export};
use crate::ui::params::Params;
use crate::ui::theme::{Tokens, icon, tokens, with_icon};
use crate::ui::widgets::{badge, caption, card, eye, layer_bar, overlay_frame, segmented};

/// Most objects shown at once.
pub const MAX_OBJECTS: usize = 3;

const SURFACES: &[(&str, &str, &str)] = &[
    ("Sphere", "球面", "x^2 + y^2 + z^2 = 4"),
    ("Ellipsoid", "椭球面", "x^2/4 + y^2/2.25 + z^2 = 1"),
    ("Hyperboloid of one sheet", "单叶双曲面", "x^2 + y^2 - z^2 = 1"),
    ("Hyperboloid of two sheets", "双叶双曲面", "x^2 + y^2 - z^2 = -1"),
    ("Cone", "圆锥面", "x^2 + y^2 = z^2"),
    ("Paraboloid", "旋转抛物面", "z = x^2 + y^2"),
    ("Saddle", "马鞍面", "z = x^2 - y^2"),
    ("Saddle z = xy", "马鞍面 z = xy", "z = x*y"),
    ("Cylinder", "圆柱面", "x^2 + y^2 = 1"),
    ("Parabolic cylinder", "抛物柱面", "y = x^2"),
    ("Hyperbolic cylinder", "双曲柱面", "x^2 - y^2 = 1"),
    ("Plane", "平面", "x + y + z = 1"),
    ("Torus (not a quadric)", "圆环面（非二次曲面）", "(sqrt(x^2 + y^2) - 2)^2 + z^2 = 0.64"),
    ("With a parameter a", "带参数 a", "x^2 + y^2 - z^2 = a"),
    ("LaTeX", "LaTeX 写法", "\\frac{x^2}{4} + \\frac{y^2}{9} = z"),
];

const CURVES: &[(&str, &str, &str)] = &[
    ("Line, symmetric form", "直线（对称式）", "(x - 1)/2 = (y + 1)/3 = z/1"),
    ("Line, parametric form", "直线（参数式）", "(1 + 2t, -1 + 3t, t)"),
    ("Line, two planes", "直线（一般式）", "x + y + z = 1, x - y = 0"),
    ("Line with a zero direction number", "方向数含 0 的直线", "(x - 1)/2 = (y - 1)/0 = z/1"),
    ("Helix", "圆柱螺线", "(cos(t), sin(t), t/4)"),
    ("Circle", "空间圆", "(2cos(t), 2sin(t), 1)"),
    ("Viviani's curve", "维维亚尼曲线", "(1 + cos(t), sin(t), 2sin(t/2))"),
];

/// Ready-made scenes: (en, zh, objects).
fn scenes() -> Vec<(&'static str, &'static str, Vec<Object>)> {
    let s = |src: &str, c: usize| Object::new(Kind::Surface, src, c);
    let l = |src: &str, c: usize| Object::new(Kind::Curve, src, c);
    let solid = |kind: SolidKind, f: &dyn Fn(&mut Solid), c: usize| {
        let mut so = Solid::preset(kind);
        f(&mut so);
        Object::solid(so, c)
    };
    vec![
        ("Sphere and cylinder (Viviani)", "球面与圆柱面（维维亚尼曲线）", vec![s("x^2 + y^2 + z^2 = 4", 0), s("x^2 + y^2 = 2x", 1)]),
        ("Cone and a plane: conic sections (slide k)", "圆锥面与平面：圆锥曲线（拖动 k）", vec![s("x^2 + y^2 = z^2", 0), s("z = k*x + 1", 1)]),
        ("Two cylinders at right angles", "两个正交的圆柱面", vec![s("x^2 + y^2 = 1", 0), s("x^2 + z^2 = 1", 1)]),
        ("Plane tangent to a sphere", "平面与球面相切", vec![s("x^2 + y^2 + z^2 = 4", 0), s("z = 2", 1)]),
        ("Paraboloid and plane", "抛物面与平面", vec![s("z = x^2 + y^2", 0), s("z = x + 1", 1)]),
        ("Lines on a hyperboloid (ruled surface)", "单叶双曲面上的直线（直纹面）", vec![s("x^2 + y^2 - z^2 = 1", 0), l("(1, t, t)", 3), l("(1, t, -t)", 2)]),
        ("Lines on a saddle", "马鞍面上的直线", vec![s("z = x*y", 0), l("(t, 1, t)", 3), l("(1, t, t)", 2)]),
        ("Helix on a cylinder", "圆柱面上的螺线", vec![s("x^2 + y^2 = 1", 0), l("(cos(t), sin(t), t/4)", 3)]),
        ("Line through a sphere", "直线穿过球面", vec![s("x^2 + y^2 + z^2 = 4", 0), l("(x - 1)/2 = (y + 1)/3 = z/1", 3)]),
        ("Line, plane and a tangent line", "直线与平面、球面的切线", vec![s("x^2 + y^2 + z^2 = 4", 0), s("x + y + z = 1", 1), l("(t, 2, t)", 3)]),
        ("Two skew lines", "两条异面直线", vec![l("(x - 1)/1 = y/2 = z/0", 3), l("x = 0, y + z = 2", 2), s("z = 0", 1)]),
        ("Torus and a bitangent plane", "圆环面与双切平面", vec![s("(sqrt(x^2 + y^2) - 2)^2 + z^2 = 0.64", 0), s("z = 0.8*x/sqrt(3.36)", 1)]),
        ("Ball cut by a plane", "球体被平面所截", vec![solid(SolidKind::Ball, &|_| {}, 0), s("x + y + z = 1", 1)]),
        ("Cylinder and cone", "圆柱体与圆锥体", vec![solid(SolidKind::Cylinder, &|_| {}, 0), solid(SolidKind::Cone, &|c| { c.anchor = DVec3::new(0.0, 0.0, -1.5); c.radius = 1.5; c.height = 3.0 }, 1)]),
        ("Box and a slanted plane", "长方体与斜平面", vec![solid(SolidKind::Cuboid, &|_| {}, 2), s("x + 2y + 3z = 1", 1)]),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Quality {
    Coarse,
    Medium,
    Fine,
}

impl Quality {
    fn cells(self) -> usize {
        match self {
            Quality::Coarse => 48,
            Quality::Medium => 80,
            Quality::Fine => 128,
        }
    }
}

pub struct SpaceWorkspace {
    objects: Vec<Object>,
    next_uid: u32,
    params: Params,
    /// Box ranges.
    xr: (f64, f64),
    yr: (f64, f64),
    zr: (f64, f64),
    equal: bool,
    quality: Quality,
    /// Parameters moved recently: mesh coarsely until they rest.
    moving_since: Option<Instant>,
    show_axes: bool,
    show_grid: bool,
    show_legend: bool,
    layers_open: bool,
    view: View3d,
    scene: Scene,
    scene_key: String,
    scene_version: u64,
    frame3: Frame3,
    export: Export,
    toast: Option<(String, Instant)>,
    draw_ms: f64,
    mesh_ms: f64,
    inter: crate::space_inter::Inter,
}

impl Default for SpaceWorkspace {
    fn default() -> Self {
        Self::new()
    }
}

fn c32(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

fn vec_str(v: DVec3) -> String {
    format!("({}, {}, {})", fmt_short(v.x), fmt_short(v.y), fmt_short(v.z))
}

/// Direction with small integers when it is a multiple of one.
pub(crate) fn dir_str(v: DVec3) -> String {
    let m = v.abs().max_element();
    if m == 0.0 {
        return vec_str(v);
    }
    // Try scaling so the smallest non-zero component is ±1 … ±6.
    let min = [v.x, v.y, v.z].iter().filter(|c| c.abs() > 1e-9 * m).fold(f64::INFINITY, |a, c| a.min(c.abs()));
    for k in 1..=6 {
        let w = v * (k as f64 / min);
        if (w - w.round()).abs().max_element() < 1e-6 * w.abs().max_element() {
            return vec_str(w.round());
        }
    }
    vec_str(v / m)
}

const CIRCLED: [&str; 3] = ["①", "②", "③"];

impl SpaceWorkspace {
    pub fn new() -> Self {
        let mut s = Self {
            objects: Vec::new(),
            next_uid: 1,
            params: Params::default(),
            xr: (-3.0, 3.0),
            yr: (-3.0, 3.0),
            zr: (-3.0, 3.0),
            equal: true,
            quality: Quality::Medium,
            moving_since: None,
            show_axes: true,
            show_grid: false,
            show_legend: true,
            layers_open: false,
            view: View3d::default(),
            scene: Scene::default(),
            scene_key: String::new(),
            scene_version: 0,
            frame3: Frame3::new(DVec3::splat(-3.0), DVec3::splat(3.0), true),
            export: Export::default(),
            toast: None,
            draw_ms: 0.0,
            mesh_ms: 0.0,
            inter: Default::default(),
        };
        let first = scenes().swap_remove(0);
        s.set_objects(first.2);
        s
    }

    /// Replace all objects (examples, preview).
    pub fn set_objects(&mut self, objects: Vec<Object>) {
        self.objects = objects.into_iter().take(MAX_OBJECTS).collect();
        for o in &mut self.objects {
            o.uid = self.next_uid;
            self.next_uid += 1;
        }
        self.sync_params();
    }

    /// Load example scene `k` (used by the preview example).
    pub fn load_scene(&mut self, k: usize) {
        if let Some(sc) = scenes().into_iter().nth(k) {
            self.set_objects(sc.2);
        }
    }

    pub fn relocalize(&mut self) {
        for o in &mut self.objects {
            o.recompile();
        }
        self.sync_params();
    }

    fn box_lo_hi(&self) -> (DVec3, DVec3) {
        (DVec3::new(self.xr.0, self.yr.0, self.zr.0), DVec3::new(self.xr.1, self.yr.1, self.zr.1))
    }

    /// Match the parameter sliders to the letters used by the objects and
    /// hand every object the current values.
    fn sync_params(&mut self) {
        let mut names: Vec<String> = Vec::new();
        for o in &self.objects {
            for p in o.params() {
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
        for o in &mut self.objects {
            o.set_params(&named);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let t = tokens(ui.ctx());
        self.status_bar(ui, &t);
        egui::Panel::left("space_controls")
            .resizable(true)
            .default_size(380.0)
            .min_size(320.0)
            .max_size(580.0)
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin { left: 12, right: 6, top: 12, bottom: 4 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    ui.set_width(ui.available_width() - 8.0);
                    self.controls(ui);
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin::same(10)))
            .show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                let t0 = Instant::now();
                self.canvas(ui, frame, rect, &t);
                self.draw_ms = 0.8 * self.draw_ms + 0.2 * t0.elapsed().as_secs_f64() * 1e3;
                if let Some(msg) = self.export.step(ui, rect, "space") {
                    self.toast = Some((msg, Instant::now()));
                }
                if self.export.overlays_visible() {
                    self.overlays(ui, rect, &t);
                }
            });
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let adapter = self.view.adapter();
        egui::Panel::bottom("space_status")
            .frame(egui::Frame::new().fill(t.sidebar).inner_margin(egui::Margin::symmetric(14, 5)).stroke(Stroke::new(1.0, t.card_border)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let small = |s: &str, c: Color32| RichText::new(s).size(12.0).color(c);
                    ui.label(small(tr("Drag to rotate · Right-drag to pan · Scroll to zoom · Double-click to reset", "拖动旋转 · 右键拖动平移 · 滚轮缩放 · 双击重置"), t.faint));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &self.toast {
                        Some((msg, at)) if at.elapsed().as_secs_f32() < 5.0 => {
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                            ui.label(small(msg, if export::is_ok(msg) { t.ok } else { t.error }));
                        }
                        _ => {
                            if !adapter.is_empty() {
                                ui.label(small(&adapter, t.faint));
                                ui.label(small("·", t.faint));
                            }
                            let msg = if i18n::zh() {
                                format!("网格 {:.0} ms · 绘制 {:.1} ms", self.mesh_ms, self.draw_ms)
                            } else {
                                format!("meshes {:.0} ms · drawn in {:.1} ms", self.mesh_ms, self.draw_ms)
                            };
                            ui.label(small(&msg, t.faint));
                        }
                    });
                });
            });
    }

    // ---- Sidebar ---------------------------------------------------------

    fn controls(&mut self, ui: &mut Ui) {
        let t = tokens(ui.ctx());
        // Scene: examples and adding objects.
        let mut load: Option<Vec<Object>> = None;
        let mut add: Option<Kind> = None;
        let count = self.objects.len();
        card(
            ui,
            "s_scene",
            icon::CUBE,
            tr("Scene", "场景"),
            true,
            |ui| {
                ui.menu_button(with_icon(icon::BOOK, tr("Examples", "示例")), |ui| {
                    ui.set_min_width(300.0);
                    for (en, zh, objs) in scenes() {
                        if ui.button(tr(en, zh)).clicked() {
                            load = Some(objs);
                            ui.close();
                        }
                    }
                });
            },
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    let full = count >= MAX_OBJECTS;
                    for (kind, en, zh) in [(Kind::Surface, "Surface", "曲面"), (Kind::Curve, "Line / curve", "直线 / 曲线"), (Kind::Solid, "Solid", "立体")] {
                        let b = ui.add_enabled(!full, egui::Button::new(format!("+ {}", tr(en, zh))));
                        if b.clicked() {
                            add = Some(kind);
                        }
                    }
                });
                caption(
                    ui,
                    &if i18n::zh() { format!("同时最多 {MAX_OBJECTS} 个对象（现有 {count} 个）。") } else { format!("Up to {MAX_OBJECTS} objects at a time ({count} now).") },
                );
            },
        );
        if let Some(objs) = load {
            self.set_objects(objs);
        }
        if let Some(kind) = add {
            let used: Vec<usize> = self.objects.iter().map(|o| o.color).collect();
            let color = (0..COLORS.len()).find(|c| !used.contains(c)).unwrap_or(0);
            let mut o = match kind {
                Kind::Surface => Object::new(Kind::Surface, "x + y + z = 1", color),
                Kind::Curve => Object::new(Kind::Curve, DEFAULT_CURVE, color),
                Kind::Solid => Object::solid(Solid::preset(SolidKind::Ball), color),
            };
            o.uid = self.next_uid;
            self.next_uid += 1;
            self.objects.push(o);
            self.sync_params();
        }

        let mut remove = None;
        let mut resync = false;
        for k in 0..self.objects.len() {
            let (gone, changed) = object_card(ui, &t, k, &mut self.objects[k]);
            if gone {
                remove = Some(k);
            }
            resync |= changed;
        }
        if let Some(k) = remove {
            self.objects.remove(k);
            resync = true;
        }
        if resync {
            self.sync_params();
        }

        let ch = self.params.ui(ui, "space_params");
        if ch.changed {
            self.push_params();
            self.moving_since = Some(Instant::now());
        }
        let (lo, hi) = self.box_lo_hi();
        self.inter.card(ui, &t, &self.objects);
        self.inter.section_card(ui, &t, &self.objects, lo, hi);

        // ---- Box ----
        let (mut xr, mut yr, mut zr) = (self.xr, self.yr, self.zr);
        let mut equal = self.equal;
        let mut quality = self.quality;
        let mut reset = false;
        card(
            ui,
            "s_box",
            icon::RULER,
            tr("View box", "视图范围"),
            false,
            |ui| {
                if ui.small_button(with_icon(icon::RESTORE, "")).on_hover_text(tr("Back to −3 … 3", "恢复为 −3 … 3")).clicked() {
                    reset = true;
                }
            },
            |ui| {
                egui::Grid::new("s_box_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    for (name, r) in [("x", &mut xr), ("y", &mut yr), ("z", &mut zr)] {
                        ui.label(RichText::new(name).monospace().color(t.text));
                        ui.horizontal(|ui| range_edit(ui, r));
                        ui.end_row();
                    }
                });
                ui.checkbox(&mut equal, tr("Equal scale on all axes", "三轴等比例"));
                ui.horizontal(|ui| {
                    ui.label(tr("Surface mesh", "曲面网格"));
                    segmented(
                        ui,
                        &mut quality,
                        &[
                            (Quality::Coarse, tr("Coarse", "粗"), tr("48 cells along the longest side", "最长边 48 格")),
                            (Quality::Medium, tr("Medium", "中"), tr("80 cells along the longest side", "最长边 80 格")),
                            (Quality::Fine, tr("Fine", "细"), tr("128 cells along the longest side (slower)", "最长边 128 格（较慢）")),
                        ],
                    );
                });
                caption(ui, tr("Surfaces are drawn inside this box only.", "曲面只画在这个范围内。"));
            },
        );
        if reset {
            (xr, yr, zr) = ((-3.0, 3.0), (-3.0, 3.0), (-3.0, 3.0));
        }
        (self.xr, self.yr, self.zr, self.equal, self.quality) = (xr, yr, zr, equal, quality);
    }

    // ---- Canvas ----------------------------------------------------------

    fn mesh_cells(&mut self, ctx: &egui::Context) -> usize {
        match self.moving_since {
            Some(at) if at.elapsed().as_secs_f32() < 0.35 || self.params.any_playing() => {
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
                Quality::Coarse.cells().min(self.quality.cells())
            }
            _ => {
                self.moving_since = None;
                self.quality.cells()
            }
        }
    }

    fn canvas(&mut self, ui: &mut Ui, frame: &mut eframe::Frame, rect: Rect, t: &Tokens) {
        let (lo, hi) = self.box_lo_hi();
        let n = self.mesh_cells(ui.ctx());
        let theme = if t.dark { Theme::Dark } else { Theme::Light };
        let pvals: Vec<String> = self.params.items.iter().map(|p| format!("{}={}", p.name, p.value)).collect();
        // The wall grid is drawn with the camera-dependent decorations, but
        // the canvas only re-renders when this key (the version) changes.
        let mut key = format!("{lo}|{hi}|{n}|{}|{}|{}|{:?}|{pvals:?}", self.equal, self.show_axes, self.show_grid, theme);
        for o in &self.objects {
            key += &format!("#{:?}|{}|{:?}|{}|{}|{}|{}|{:?}", o.kind, o.src, o.solid, o.color, o.opacity, o.visible, o.lines, o.t_range);
        }
        key += &self.inter.key();
        if key != self.scene_key {
            let t0 = Instant::now();
            let frame3 = Frame3::new(lo, hi, self.equal);
            let step = space_view::line_step(lo, hi);
            let mut scene = Scene::default();
            // Meshes (and the classification) for every object, hidden ones
            // too: the sidebar and the intersections use them.
            for o in &mut self.objects {
                if o.kind != Kind::Curve {
                    o.mesh(lo, hi, n, "");
                }
            }
            self.inter.compute(&self.objects, lo, hi, (hi - lo).max_element() / n as f64);
            let originals = self.inter.originals;
            let faint = originals == Originals::Faint;
            for (k, o) in self.objects.iter().enumerate() {
                let (color, opacity, visible, lines, kind) = (COLORS[o.color], o.opacity, o.visible, o.lines, o.kind);
                if !visible || originals == Originals::Hide {
                    continue;
                }
                // Faint: barely there, no grid lines, light edges.
                let (opacity, lines) = if faint { (opacity.min(0.12), false) } else { (opacity, lines) };
                let alpha = if faint { 0.3 } else { 1.0 };
                match kind {
                    Kind::Surface | Kind::Solid => {
                        let plane = o.plane().is_some();
                        let m = self.inter.mesh_for(k, o);
                        let style = if kind == Kind::Solid || !lines {
                            Lines::None
                        } else if plane {
                            Lines::Grid
                        } else {
                            Lines::Levels
                        };
                        scene.meshes.push(space_view::to_mesh(m, &frame3, color, opacity, style, step));
                        let mut edge = space_view::darker(color, if kind == Kind::Solid { 0.55 } else { 0.7 });
                        edge[3] = alpha;
                        space_view::edges(m, &frame3, edge, if kind == Kind::Solid { 1.6 } else { 1.2 }, &mut scene.lines);
                    }
                    Kind::Curve => {
                        let pts = o.curve_points(lo, hi);
                        space_view::curve(&pts, &frame3, hex(color, alpha), if faint { 2.0 } else { 3.0 }, &mut scene.lines);
                    }
                }
            }
            self.inter.add_to_scene(&mut scene, &self.objects, &frame3, lo, hi, t.dark);
            if self.show_axes {
                let pal = theme.palette();
                let c = pal.frame;
                space_view::axes(lo, hi, &frame3, [c[0], c[1], c[2], 0.7], &mut scene.lines);
            }
            self.scene = scene;
            self.frame3 = frame3;
            self.scene_key = key;
            self.scene_version += 1;
            self.mesh_ms = t0.elapsed().as_secs_f64() * 1e3;
        }
        self.view.allocate(ui, rect);
        let proj = self.view.projector(rect);
        let settings = calc_view::Settings { xr: self.xr, yr: self.yr, zr: self.zr, show_grid: self.show_grid, show_ticks: true, theme, ..Default::default() };
        let (deco, labels) = calc_view::decorations(&settings, &self.frame3, &self.view.camera, &proj, ["x", "y", "z"]);
        let pal = theme.palette();
        if !self.view.render(ui, frame, rect, &self.scene, self.scene_version, deco, self.frame3.half, pal.background, pal.wire) {
            return;
        }
        canvas3d::paint_labels(&ui.painter_at(rect), rect, &proj, labels);
    }

    fn overlays(&mut self, ui: &mut Ui, rect: Rect, t: &Tokens) {
        let inner = rect.shrink(12.0);
        let mut top_right = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Min)));
        let toolbar = overlay_frame(t).inner_margin(egui::Margin::same(5)).show(&mut top_right, |ui| {
            ui.horizontal(|ui| {
                if ui.button(with_icon(icon::CAMERA, "")).on_hover_text(tr("Save the view as PNG in ~/Pictures/Math-crystal", "把视图保存为 PNG（~/Pictures/Math-crystal）")).clicked() {
                    self.export.start();
                }
                self.view.projection_controls(ui);
                ui.add_space(4.0);
                self.view.preset_controls(ui, ["x", "y", "z"]);
            });
        });
        // The legend gets the width left of the toolbar.
        let legend_w = (toolbar.response.rect.left() - inner.left() - 12.0).max(120.0);
        if self.show_legend && !self.objects.is_empty() {
            let mut tl = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
            overlay_frame(t).show(&mut tl, |ui| {
                ui.set_max_width(legend_w - 20.0);
                ui.spacing_mut().item_spacing.y = 3.0;
                for (k, o) in self.objects.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        let c = c32(COLORS[o.color]);
                        ui.painter().circle_filled(r.center(), 5.0, if o.visible { c } else { c.gamma_multiply(0.3) });
                        let text = format!("{} {}", CIRCLED[k.min(2)], short_name(o));
                        ui.label(RichText::new(text).size(13.0).color(if o.visible { t.text } else { t.faint }));
                        let formula = match o.kind {
                            Kind::Solid => o.solid.inequalities(),
                            _ => pretty(&o.src),
                        };
                        ui.add(egui::Label::new(RichText::new(formula).monospace().size(12.0).color(t.muted)).truncate());
                    });
                }
            });
        }
        // Names of intersection and tangent points.
        let proj = self.view.projector(rect);
        let painter = ui.painter_at(rect);
        for (p, name, tangent) in self.inter.marks() {
            if let Some(s) = proj.project(self.frame3.map(p)) {
                let at = rect.min + vec2(s.x + 7.0, s.y - 7.0);
                let col = if tangent { Color32::from_rgb(0xe0, 0x31, 0x31) } else { t.text };
                painter.text(at, egui::Align2::LEFT_BOTTOM, name, FontId::proportional(13.0), col);
            }
        }
        let mut only_curves = self.inter.originals == Originals::Hide;
        let before = only_curves;
        let chips: Vec<(&mut bool, &str, &str, Option<Color32>, &str)> = vec![
            (&mut only_curves, tr("Curves only", "只看交线"), tr("Curves", "交线"), None, tr("Hide the objects and show only their intersections", "隐藏原图形，只显示交线与交点")),
            (&mut self.show_axes, tr("Axes", "坐标轴"), tr("Axes", "坐标轴"), None, tr("Coordinate axes through the origin", "过原点的坐标轴")),
            (&mut self.show_grid, tr("Wall grid", "背景网格"), tr("Grid", "网格"), None, tr("Grid on the back walls of the box", "盒子背面的网格")),
            (&mut self.show_legend, tr("Legend", "图例"), tr("Legend", "图例"), None, tr("List of the objects", "对象列表")),
        ];
        layer_bar(ui, inner, &mut self.layers_open, chips);
        if only_curves != before {
            self.inter.originals = if only_curves { Originals::Hide } else { Originals::Show };
            if only_curves {
                self.inter.show = true;
            }
        }
    }
}

/// Formula text with superscripts and dots, for display only.
fn pretty(src: &str) -> String {
    if crate::expr::latex::looks_like_latex(src) {
        return src.to_string();
    }
    src.replace("^2", "²").replace("^3", "³").replace("**2", "²").replace('*', "·").replace("sqrt", "√")
}

/// Short name: the kind of surface, line, curve or solid.
pub(crate) fn short_name(o: &Object) -> String {
    match o.kind {
        Kind::Solid => o.solid.name().to_string(),
        Kind::Curve => match &o.curve {
            Some(c) if c.error.is_some() => tr("line / curve", "直线 / 曲线").into(),
            Some(c) if c.line.is_some() => tr("line", "直线").into(),
            Some(_) => tr("curve", "曲线").into(),
            None => tr("line / curve", "直线 / 曲线").into(),
        },
        Kind::Surface => match o.surface.as_ref().map(|s| &s.kind) {
            Some(SurfaceKind::Plane { .. }) => tr("plane", "平面").into(),
            Some(SurfaceKind::Quadric(q)) => q.name().into(),
            Some(SurfaceKind::Constant) => tr("no surface (F is constant)", "不是曲面（F 为常数）").into(),
            _ => tr("surface", "曲面").into(),
        },
    }
}

/// One object's card. Returns (removed, parameters may have changed).
fn object_card(ui: &mut Ui, t: &Tokens, k: usize, o: &mut Object) -> (bool, bool) {
    let mut visible = o.visible;
    let mut remove = false;
    let mut changed = false;
    let title = format!("{} {}", CIRCLED[k.min(2)], short_name(o));
    let ic = match o.kind {
        Kind::Surface => icon::LAYERS,
        Kind::Curve => icon::CURVE,
        Kind::Solid => icon::CUBE,
    };
    card(
        ui,
        &format!("s_obj_{}", o.uid),
        ic,
        &title,
        true,
        |ui| {
            if ui.add(egui::Button::new(RichText::new("✕").size(13.0).color(t.muted)).frame(false)).on_hover_text(tr("Remove this object", "删除这个对象")).clicked() {
                remove = true;
            }
            eye(ui, &mut visible, tr("Show in the view", "在视图中显示"));
        },
        |ui| {
            ui.horizontal(|ui| {
                let mut kind = o.kind;
                if segmented(
                    ui,
                    &mut kind,
                    &[
                        (Kind::Surface, tr("Surface", "曲面"), tr("An equation F(x, y, z) = 0", "方程 F(x, y, z) = 0")),
                        (Kind::Curve, tr("Line / curve", "直线 / 曲线"), tr("A line or a parametric curve", "直线或参数曲线")),
                        (Kind::Solid, tr("Solid", "立体"), tr("Ball, cylinder, cone or box", "球体、圆柱体、圆锥体或长方体")),
                    ],
                ) {
                    o.set_kind(kind);
                    changed = true;
                }
            });
            match o.kind {
                Kind::Surface => changed |= surface_body(ui, t, o),
                Kind::Curve => changed |= curve_body(ui, t, o),
                Kind::Solid => solid_body(ui, t, o),
            }
            appearance(ui, t, o);
        },
    );
    o.visible = visible;
    (remove, changed)
}

fn formula_input(ui: &mut Ui, o: &mut Object, hint: &str, examples: &[(&str, &str, &str)]) -> bool {
    let mut pick: Option<&str> = None;
    let mut changed = false;
    // Right to left: the button first, the input takes what is left (so the
    // row never asks for more width than the panel has).
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        ui.menu_button(with_icon(icon::BOOK, ""), |ui| {
            ui.set_min_width(300.0);
            for (en, zh, f) in examples {
                if ui.button(tr(en, zh)).on_hover_text(RichText::new(*f).monospace()).clicked() {
                    pick = Some(f);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(tr("Examples", "示例"));
        let edit = ui.add(
            egui::TextEdit::multiline(&mut o.src)
                .font(FontId::monospace(14.5))
                .desired_rows(1)
                .desired_width(ui.available_width())
                .margin(egui::Margin::symmetric(8, 6))
                .hint_text(hint),
        );
        if edit.changed() {
            o.recompile();
            changed = true;
        }
    });
    if let Some(f) = pick {
        o.src = f.to_string();
        o.recompile();
        changed = true;
    }
    changed
}

fn error_line(ui: &mut Ui, t: &Tokens, e: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(with_icon(icon::ALERT, e)).size(12.5).color(t.error));
    });
}

fn surface_body(ui: &mut Ui, t: &Tokens, o: &mut Object) -> bool {
    let changed = formula_input(ui, o, tr("e.g. x^2 + y^2 = z^2", "如 x^2 + y^2 = z^2"), SURFACES);
    caption(ui, tr("F(x, y, z) = 0, or left side = right side. LaTeX works too.", "写成 F(x, y, z) = 0，或「左边 = 右边」；也支持 LaTeX。"));
    if let Some(e) = &o.error {
        error_line(ui, t, e);
        caption(ui, tr("Showing the last valid surface.", "下面显示的是上一个有效的曲面。"));
    }
    let Some(s) = &o.surface else { return changed };
    match &s.kind {
        SurfaceKind::Plane { n, d } => {
            ui.horizontal_wrapped(|ui| {
                badge(ui, tr("plane", "平面"), t.ok);
            });
            kv(ui, t, tr("Normal vector", "法向量"), "n", &dir_str(*n));
            let dist = d.abs() / n.length();
            kv(ui, t, tr("Distance to the origin", "到原点的距离"), "", &fmt_short(dist));
        }
        SurfaceKind::Quadric(q) => {
            ui.horizontal_wrapped(|ui| {
                let col = if q.class == Class::Empty { t.warn } else { t.ok };
                badge(ui, q.name(), col);
                badge(ui, tr("quadric", "二次曲面"), t.accent_text);
            });
            if q.class != Class::Empty {
                ui.label(RichText::new(tr("Standard form", "标准方程")).size(12.0).color(t.muted));
                egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(q.standard_form()).monospace().size(14.5).color(t.text));
                });
                let centre_name = match q.rhs {
                    Rhs::Linear(_) => tr("Vertex", "顶点"),
                    _ if matches!(q.class, Class::EllipticCylinder | Class::HyperbolicCylinder | Class::IntersectingPlanes | Class::ParallelPlanes | Class::Line) => tr("A point on the axis", "轴上一点"),
                    _ => tr("Centre", "中心"),
                };
                if q.class == Class::Ellipsoid && q.sphere {
                    kv(ui, t, tr("Centre", "球心"), "", &vec_str(q.origin));
                    kv(ui, t, tr("Radius", "半径"), "R", &fmt_short(q.semi_axes()[0]));
                } else {
                    let cylinder = matches!(q.class, Class::EllipticCylinder | Class::HyperbolicCylinder | Class::ParabolicCylinder);
                    kv(ui, t, centre_name, "", &vec_str(q.origin));
                    if q.class == Class::EllipticCylinder && q.revolution.is_some() {
                        kv(ui, t, tr("Radius", "半径"), "R", &fmt_short(q.semi_axes()[0]));
                    } else if matches!(q.rhs, Rhs::One) {
                        let a: Vec<String> = q.semi_axes().iter().map(|v| fmt_short(*v)).collect();
                        kv(ui, t, tr("Semi-axes", "半轴长"), "", &a.join(", "));
                    }
                    if cylinder {
                        // Rulings run along the direction missing from the terms.
                        let mut g = q.terms.iter().fold(DVec3::ONE, |acc, tm| acc - tm.axis * tm.axis.dot(acc));
                        if let Rhs::Linear(w) = q.rhs {
                            g -= w * w.dot(g);
                        }
                        if let Some(g) = g.try_normalize() {
                            kv(ui, t, tr("Rulings parallel to", "母线方向"), "", &dir_str(g));
                        }
                    } else if let Some(axis) = q.revolution {
                        kv(ui, t, tr("Axis of revolution", "旋转轴方向"), "", &dir_str(axis));
                    }
                }
                if !q.axis_aligned() {
                    let primes = ["x′", "y′", "z′"];
                    let mut dirs: Vec<_> = q.terms.iter().map(|tm| tm.axis).collect();
                    if let Rhs::Linear(w) = q.rhs {
                        dirs.push(w);
                    }
                    for (p, d) in primes.iter().zip(dirs) {
                        kv(ui, t, &format!("{} {p}", tr("Axis", "轴")), "", &dir_str(d));
                    }
                    caption(ui, tr("The principal axes are rotated: x′, y′, z′ point along the directions above.", "主轴是转过的：x′、y′、z′ 分别沿上面的方向。"));
                }
            }
        }
        SurfaceKind::General => {
            ui.horizontal_wrapped(|ui| {
                badge(ui, tr("surface (not a quadric)", "曲面（非二次曲面）"), t.accent_text);
            });
        }
        SurfaceKind::Constant => error_line(ui, t, tr("F does not depend on x, y, z: no surface", "F 与 x、y、z 无关：不是曲面")),
    }
    changed
}

fn curve_body(ui: &mut Ui, t: &Tokens, o: &mut Object) -> bool {
    let changed = formula_input(ui, o, tr("e.g. (x-1)/2 = (y+1)/3 = z", "如 (x-1)/2 = (y+1)/3 = z"), CURVES);
    caption(
        ui,
        tr(
            "Parametric (1+2t, −1+3t, t), symmetric (x−1)/2 = (y+1)/3 = z, or two plane equations separated by a comma.",
            "参数式 (1+2t, −1+3t, t)；对称式 (x−1)/2 = (y+1)/3 = z；一般式：两个平面方程，用逗号隔开。",
        ),
    );
    if let Some(e) = &o.error {
        error_line(ui, t, e);
    }
    let Some(c) = &o.curve else { return changed };
    if let Some(e) = &c.error {
        error_line(ui, t, e);
        return changed;
    }
    if let Some((p, d)) = c.line {
        ui.horizontal_wrapped(|ui| {
            badge(ui, tr("line", "直线"), t.ok);
        });
        kv(ui, t, tr("Through the point", "过点"), "M₀", &vec_str(p));
        kv(ui, t, tr("Direction vector", "方向向量"), "s", &dir_str(d));
    } else {
        ui.horizontal_wrapped(|ui| {
            badge(ui, tr("curve", "曲线"), t.accent_text);
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("t").monospace().color(t.text));
            range_edit(ui, &mut o.t_range);
        });
    }
    changed
}

fn solid_body(ui: &mut Ui, t: &Tokens, o: &mut Object) {
    let s = &mut o.solid;
    let mut kind = s.kind;
    if segmented(
        ui,
        &mut kind,
        &[
            (SolidKind::Ball, tr("Ball", "球体"), ""),
            (SolidKind::Cylinder, tr("Cylinder", "圆柱体"), ""),
            (SolidKind::Cone, tr("Cone", "圆锥体"), ""),
            (SolidKind::Cuboid, tr("Box", "长方体"), ""),
        ],
    ) {
        let keep = s.anchor;
        *s = Solid::preset(kind);
        if kind != SolidKind::Cuboid {
            s.anchor = keep;
        }
    }
    let drag = |ui: &mut Ui, v: &mut f64, min: f64| {
        ui.add(egui::DragValue::new(v).speed(0.02).max_decimals(3).range(min..=1e6));
    };
    egui::Grid::new(("solid_grid", o.uid)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        let anchor_name = match s.kind {
            SolidKind::Ball => tr("Centre", "球心"),
            SolidKind::Cylinder | SolidKind::Cone => tr("Base centre", "底面中心"),
            SolidKind::Cuboid => tr("Corner", "起点角"),
        };
        ui.label(anchor_name);
        ui.horizontal(|ui| {
            for c in [&mut s.anchor.x, &mut s.anchor.y, &mut s.anchor.z] {
                drag(ui, c, -1e6);
            }
        });
        ui.end_row();
        match s.kind {
            SolidKind::Ball => {
                ui.label(tr("Radius", "半径"));
                drag(ui, &mut s.radius, 0.01);
                ui.end_row();
            }
            SolidKind::Cylinder | SolidKind::Cone => {
                ui.label(tr("Axis", "轴"));
                let mut axis = s.axis;
                if segmented(ui, &mut axis, &[(0, "x", ""), (1, "y", ""), (2, "z", "")]) {
                    s.axis = axis;
                }
                ui.end_row();
                ui.label(tr("Radius", "半径"));
                drag(ui, &mut s.radius, 0.01);
                ui.end_row();
                ui.label(tr("Height", "高"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut s.height).speed(0.02).max_decimals(3)).on_hover_text(tr("Negative points the other way", "取负值则朝反方向"));
                    if s.kind == SolidKind::Cone {
                        ui.label(RichText::new(tr("(apex at the top)", "（顶点在高的一端）")).size(11.5).color(t.faint));
                    }
                });
                ui.end_row();
                if s.height.abs() < 1e-3 {
                    s.height = 1e-3;
                }
            }
            SolidKind::Cuboid => {
                ui.label(tr("Edges", "棱长"));
                ui.horizontal(|ui| {
                    for c in [&mut s.size.x, &mut s.size.y, &mut s.size.z] {
                        drag(ui, c, 0.01);
                    }
                });
                ui.end_row();
            }
        }
    });
    egui::Frame::new().fill(t.input).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(s.inequalities()).monospace().size(13.5).color(t.text));
    });
    kv(ui, t, tr("Volume", "体积"), "V", &fmt_short(s.volume()));
    kv(ui, t, tr("Surface area", "表面积"), "S", &fmt_short(s.area()));
}

fn appearance(ui: &mut Ui, t: &Tokens, o: &mut Object) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (k, rgb) in COLORS.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
            ui.painter().circle_filled(r.center(), 6.5, c32(*rgb));
            if o.color == k {
                ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, t.text));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                o.color = k;
            }
        }
        if o.kind != Kind::Curve {
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if o.kind == Kind::Surface {
                    let tip = if o.plane().is_some() { tr("Grid lines x, y, z = const on the plane", "平面上的坐标网格线") } else { tr("Level lines z = const", "等高线 z = 常数") };
                    ui.toggle_value(&mut o.lines, tr("Lines", "网格线")).on_hover_text(tip);
                }
                let mut op = o.opacity;
                ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(40.0);
                if ui.add(egui::Slider::new(&mut op, 0.15..=1.0).show_value(false)).on_hover_text(tr("Opacity", "不透明度")).changed() {
                    o.opacity = op;
                }
            });
        }
    });
}
