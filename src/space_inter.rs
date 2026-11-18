//! Intersections in the Space workspace: drawing curves, points and
//! tangency, projections onto a coordinate plane, a movable section plane,
//! and the sidebar cards describing how the objects meet.

use egui::{Color32, RichText, Sense, Stroke, Ui, pos2, vec2};
use glam::DVec3;

use crate::app::fmt_short;
use crate::i18n::{self, tr};
use crate::render::scene::{Frame3, MeshData, MeshVertex, Scene, hex, line, sphere};
use crate::space::intersect::{self, Conic, ConicKind, Field, march, refine};
use crate::space::mesh::plane_mesh;
use crate::space::object::{COLORS, Kind, Object};
use crate::space::relate::{self, Analysis, LinePlaneKind, LinesKind, PlanesKind, Rel};
use crate::space::surface::SurfaceKind;
use crate::space_view::{self, Lines};
use crate::ui::theme::{Tokens, icon};
use crate::ui::widgets::{caption, card, eye, segmented};

/// Coordinate plane for projections.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proj {
    Off,
    XOy,
    YOz,
    ZOx,
}

impl Proj {
    /// The coordinate that is dropped.
    fn axis(self) -> Option<usize> {
        match self {
            Proj::Off => None,
            Proj::XOy => Some(2),
            Proj::YOz => Some(0),
            Proj::ZOx => Some(1),
        }
    }
}

/// How the objects themselves are drawn next to their intersections.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Originals {
    Show,
    Faint,
    Hide,
}

pub struct Section {
    pub on: bool,
    pub axis: usize,
    pub value: f64,
}

pub struct SectionCurves {
    pub obj: usize,
    pub curves: Vec<Vec<DVec3>>,
    pub conic: Option<Conic>,
}

pub struct Inter {
    pub show: bool,
    pub originals: Originals,
    pub projection: Proj,
    pub section: Section,
    pub analysis: Analysis,
    /// Per pair: names of the hit points and of the tangent points.
    names: Vec<(Vec<String>, Vec<String>)>,
    pub sections: Vec<SectionCurves>,
}

impl Default for Inter {
    fn default() -> Self {
        Self { show: true, originals: Originals::Show, projection: Proj::Off, section: Section { on: false, axis: 2, value: 0.0 }, analysis: Analysis::default(), names: vec![], sections: vec![] }
    }
}

fn sub(n: usize) -> String {
    n.to_string().chars().map(|c| char::from_u32(0x2080 + c.to_digit(10).unwrap()).unwrap()).collect()
}

fn vec_str(v: DVec3) -> String {
    format!("({}, {}, {})", fmt_short(v.x), fmt_short(v.y), fmt_short(v.z))
}

fn deg(a: f64) -> String {
    format!("{}°", fmt_short(a))
}

const CIRCLED: [&str; 3] = ["①", "②", "③"];
const SECTION_RGB: u32 = 0x868e96;

/// Colour of intersection curves and points.
pub fn ink(dark: bool) -> u32 {
    if dark { 0xf1f3f5 } else { 0x1b1f2a }
}
const TANGENT_RGB: u32 = 0xe03131;

fn c32(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Inter {
    /// The section plane as a field.
    pub fn section_field(&self) -> Option<Field<'static>> {
        let mut n = DVec3::ZERO;
        n[self.section.axis] = 1.0;
        self.section.on.then_some(Field::Plane(n, self.section.value))
    }

    pub fn key(&self) -> String {
        format!("{}|{:?}|{:?}|{}|{}|{}", self.show, self.originals, self.projection, self.section.on, self.section.axis, self.section.value)
    }

    /// Recompute after the meshes changed.
    pub fn compute(&mut self, objs: &[Object], lo: DVec3, hi: DVec3, h: f64) {
        let extra: Vec<Field> = self.section_field().into_iter().collect();
        self.analysis = relate::analyse(objs, lo, hi, h, &extra);
        let (mut np, mut nt) = (0, 0);
        self.names = self
            .analysis
            .pairs
            .iter()
            .map(|p| {
                let hits = p.hits.iter().map(|_| { np += 1; format!("P{}", sub(np)) }).collect();
                let touches = p.touches.iter().map(|_| { nt += 1; format!("T{}", sub(nt)) }).collect();
                (hits, touches)
            })
            .collect();
        self.sections.clear();
        if let Some(plane) = self.section_field() {
            let Field::Plane(n, d) = plane else { unreachable!() };
            let scale = (hi - lo).max_element();
            for (k, o) in objs.iter().enumerate() {
                let Some(f) = o.field() else { continue };
                let m = o.cached_mesh();
                let g = plane.vals(&m.pos);
                let mut curves = march(m, &g, 1e-9 * scale);
                for c in &mut curves {
                    refine(c, &f, &plane, h);
                }
                let conic = match (&o.surface, o.kind) {
                    (Some(s), Kind::Surface) => match &s.kind {
                        SurfaceKind::Quadric(q) => Some(intersect::conic(q, n, d, scale)),
                        _ => None,
                    },
                    _ => None,
                };
                self.sections.push(SectionCurves { obj: k, curves, conic });
            }
        }
    }

    /// Mesh to draw for object `k` (cut along the intersections).
    pub fn mesh_for<'a>(&'a self, k: usize, o: &'a Object) -> &'a crate::space::mesh::DMesh {
        self.analysis.meshes.get(k).and_then(Option::as_ref).unwrap_or(o.cached_mesh())
    }

    /// Named points: (position, name, tangent).
    pub fn marks(&self) -> Vec<(DVec3, String, bool)> {
        let mut out = Vec::new();
        if !self.show {
            return out;
        }
        for (p, (hn, tn)) in self.analysis.pairs.iter().zip(&self.names) {
            for (h, n) in p.hits.iter().zip(hn) {
                out.push((h.p, n.clone(), h.tangent));
            }
            for (t, n) in p.touches.iter().zip(tn) {
                out.push((*t, n.clone(), true));
            }
        }
        out
    }

    pub fn add_to_scene(&self, scene: &mut Scene, objs: &[Object], frame: &Frame3, lo: DVec3, hi: DVec3, dark: bool) {
        let ink_c = hex(ink(dark), 1.0);
        let red = hex(TANGENT_RGB, 1.0);
        let muted = hex(if dark { 0xadb5bd } else { 0x5f677a }, 0.9);
        if self.show {
            for p in &self.analysis.pairs {
                for c in &p.curves {
                    space_view::curve(&c.iter().map(|q| Some(*q)).collect::<Vec<_>>(), frame, ink_c, 3.2, &mut scene.lines);
                }
                for h in &p.hits {
                    scene.meshes.push(sphere(frame.map(h.p), if h.tangent { 0.036 } else { 0.03 }, if h.tangent { red } else { ink_c }));
                }
                for t in &p.touches {
                    scene.meshes.push(sphere(frame.map(*t), 0.036, red));
                }
                // Common perpendicular of skew lines, dashed.
                if let Rel::LineLine { feet: Some((a, b)), .. } = p.rel {
                    let (wa, wb) = (frame.map(a), frame.map(b));
                    for k in (0..20).step_by(2) {
                        let s = |i: usize| wa + (wb - wa) * (i as f32 / 20.0);
                        scene.lines.push(line(s(k), s(k + 1), muted, 2.0, true));
                    }
                    scene.meshes.push(sphere(wa, 0.024, muted));
                    scene.meshes.push(sphere(wb, 0.024, muted));
                }
            }
            if let Some(axis) = self.projection.axis() {
                let c = if lo[axis] <= 0.0 && hi[axis] >= 0.0 { 0.0 } else { lo[axis] };
                let flat = |p: DVec3| {
                    let mut q = p;
                    q[axis] = c;
                    q
                };
                let mut strip = MeshData { vertices: vec![], indices: vec![], translucent: true };
                let sc = hex(SECTION_RGB, 0.2);
                for p in &self.analysis.pairs {
                    for cv in &p.curves {
                        let proj: Vec<Option<DVec3>> = cv.iter().map(|q| Some(flat(*q))).collect();
                        space_view::curve(&proj, frame, [ink_c[0], ink_c[1], ink_c[2], 0.85], 2.2, &mut scene.lines);
                        // The projecting cylinder through the curve.
                        for w in cv.windows(2) {
                            let quad = [w[0], w[1], flat(w[1]), flat(w[0])].map(|q| frame.map(q));
                            let n = (quad[1] - quad[0]).cross(quad[3] - quad[0]).normalize_or_zero();
                            let base = strip.vertices.len() as u32;
                            for v in quad {
                                strip.vertices.push(MeshVertex { pos: v.to_array(), normal: n.to_array(), color: sc, grid: [0.0; 3] });
                            }
                            strip.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                        }
                    }
                }
                scene.meshes.push(strip);
            }
        }
        if let Some(Field::Plane(n, d)) = self.section_field() {
            let m = plane_mesh(n, d, lo, hi, (hi - lo).max_element() / 24.0);
            scene.meshes.push(space_view::to_mesh(&m, frame, SECTION_RGB, 0.22, Lines::None, 1.0));
            space_view::edges(&m, frame, hex(SECTION_RGB, 1.0), 1.4, &mut scene.lines);
            for s in &self.sections {
                let c = space_view::darker(COLORS[objs[s.obj].color], 0.75);
                for cv in &s.curves {
                    space_view::curve(&cv.iter().map(|q| Some(*q)).collect::<Vec<_>>(), frame, c, 3.0, &mut scene.lines);
                }
            }
        }
    }

    /// Sidebar card listing how the objects meet. Returns true when a
    /// setting changed.
    pub fn card(&mut self, ui: &mut Ui, t: &Tokens, objs: &[Object]) -> bool {
        let mut show = self.show;
        let mut originals = self.originals;
        let mut proj = self.projection;
        let pairs = &self.analysis.pairs;
        let names = &self.names;
        card(
            ui,
            "s_inter",
            icon::INTEGRAL,
            tr("Intersections", "交点与交线"),
            false,
            |ui| {
                eye(ui, &mut show, tr("Show intersection curves and points", "显示交线与交点"));
            },
            |ui| {
                if objs.len() < 2 {
                    caption(ui, tr("Add a second object to see how they meet.", "再添加一个对象，就能看到它们怎样相交。"));
                }
                for (p, (hn, tn)) in pairs.iter().zip(names) {
                    if p.i >= objs.len() || p.j >= objs.len() {
                        continue;
                    }
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        for (k, idx) in [p.i, p.j].iter().enumerate() {
                            if k == 1 {
                                ui.label(RichText::new("×").color(t.faint));
                            }
                            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.5, c32(COLORS[objs[*idx].color]));
                            ui.label(RichText::new(format!("{} {}", CIRCLED[(*idx).min(2)], crate::space_app::short_name(&objs[*idx]))).strong().color(t.text));
                        }
                    });
                    for (text, col) in describe(p, objs, hn, tn, t) {
                        ui.horizontal_wrapped(|ui| {
                            ui.add_space(14.0);
                            ui.label(RichText::new(text).size(13.0).color(col));
                        });
                    }
                }
                if !pairs.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(tr("Project curves onto", "交线投影到"));
                        segmented(
                            ui,
                            &mut proj,
                            &[
                                (Proj::Off, tr("Off", "无"), ""),
                                (Proj::XOy, "xOy", tr("The plane z = 0 (with the projecting cylinder)", "z = 0 平面（并画出投影柱面）")),
                                (Proj::YOz, "yOz", tr("The plane x = 0 (with the projecting cylinder)", "x = 0 平面（并画出投影柱面）")),
                                (Proj::ZOx, "zOx", tr("The plane y = 0 (with the projecting cylinder)", "y = 0 平面（并画出投影柱面）")),
                            ],
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr("Objects", "原图形"));
                        segmented(
                            ui,
                            &mut originals,
                            &[
                                (Originals::Show, tr("Show", "显示"), tr("Draw the objects as set in their cards", "按各自卡片的设置显示")),
                                (Originals::Faint, tr("Faint", "淡化"), tr("Draw the objects very faintly, so the curves stand out", "把原图形画得很淡，突出交线")),
                                (Originals::Hide, tr("Hide", "隐藏"), tr("Only the intersection curves and points", "只显示交线和交点")),
                            ],
                        );
                    });
                    caption(ui, tr("Computed inside the view box. Red points: tangency.", "只在视图范围内计算。红色的点是切点。"));
                }
            },
        );
        // Hiding the objects only makes sense with the curves shown.
        if originals != self.originals && originals != Originals::Show {
            show = true;
        }
        let changed = show != self.show || proj != self.projection || originals != self.originals;
        self.originals = originals;
        self.show = show;
        self.projection = proj;
        changed
    }

    /// Section plane card with the true shape of the section. Returns true
    /// when a setting changed.
    pub fn section_card(&mut self, ui: &mut Ui, t: &Tokens, objs: &[Object], lo: DVec3, hi: DVec3) -> bool {
        let mut on = self.section.on;
        let mut axis = self.section.axis;
        let mut value = self.section.value;
        let mut moved = false;
        let sections = &self.sections;
        card(
            ui,
            "s_section",
            icon::LAYERS,
            tr("Cross-section", "截面"),
            false,
            |ui| {
                eye(ui, &mut on, tr("Cut with a plane x, y or z = c", "用平面 x、y 或 z = c 去截"));
            },
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(tr("Plane", "截平面"));
                    segmented(ui, &mut axis, &[(0, "x = c", ""), (1, "y = c", ""), (2, "z = c", "")]);
                });
                ui.horizontal(|ui| {
                    ui.label("c");
                    ui.spacing_mut().slider_width = (ui.available_width() - 70.0).max(60.0);
                    let mut v = value;
                    if ui.add(egui::Slider::new(&mut v, lo[axis]..=hi[axis]).max_decimals(3)).changed() {
                        value = v;
                        moved = true;
                    }
                });
                if !self.section.on {
                    caption(ui, tr("Turn on the eye to cut the objects.", "点亮右上角的眼睛开始截取。"));
                    return;
                }
                section_plot(ui, t, objs, sections, self.section.axis, lo, hi);
                for s in sections {
                    let o = &objs[s.obj];
                    let what = match &s.conic {
                        Some(c) => conic_text(c),
                        None if s.curves.is_empty() => tr("no section in the box", "在视图范围内不相交").to_string(),
                        None if o.plane().is_some() => tr("a line", "一条直线").to_string(),
                        None => tr("a curve", "一条曲线").to_string(),
                    };
                    ui.horizontal_wrapped(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.5, c32(COLORS[o.color]));
                        ui.label(RichText::new(format!("{} {}", CIRCLED[s.obj.min(2)], what)).size(13.0).color(t.text));
                    });
                }
            },
        );
        // Moving the slider turns the section on.
        on |= moved;
        if axis != self.section.axis {
            value = value.clamp(lo[axis], hi[axis]);
        }
        let changed = on != self.section.on || axis != self.section.axis || value != self.section.value;
        self.section = Section { on, axis, value };
        changed
    }
}

/// The section curves drawn flat in the plane's two coordinates.
fn section_plot(ui: &mut Ui, t: &Tokens, objs: &[Object], sections: &[SectionCurves], axis: usize, lo: DVec3, hi: DVec3) {
    // x = c → (y, z); y = c → (x, z); z = c → (x, y).
    let (a, b) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let names = ["x", "y", "z"];
    let w = ui.available_width();
    let (ra, rb) = (hi[a] - lo[a], hi[b] - lo[b]);
    let hgt = (w * (rb / ra) as f32).min(260.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, hgt), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, t.input);
    // Equal scale.
    let s = ((rect.width() - 16.0) as f64 / ra).min((rect.height() - 16.0) as f64 / rb);
    let c = rect.center();
    let mid = ((lo[a] + hi[a]) * 0.5, (lo[b] + hi[b]) * 0.5);
    let to = |u: f64, v: f64| pos2(c.x + ((u - mid.0) * s) as f32, c.y - ((v - mid.1) * s) as f32);
    let axis_c = t.faint;
    if lo[a] <= 0.0 && hi[a] >= 0.0 {
        p.line_segment([to(0.0, lo[b]), to(0.0, hi[b])], Stroke::new(1.0, axis_c));
    }
    if lo[b] <= 0.0 && hi[b] >= 0.0 {
        p.line_segment([to(lo[a], 0.0), to(hi[a], 0.0)], Stroke::new(1.0, axis_c));
    }
    p.text(to(hi[a], 0.0f64.clamp(lo[b], hi[b])) + vec2(-4.0, -4.0), egui::Align2::RIGHT_BOTTOM, names[a], egui::FontId::proportional(12.0), t.muted);
    p.text(to(0.0f64.clamp(lo[a], hi[a]), hi[b]) + vec2(5.0, 2.0), egui::Align2::LEFT_TOP, names[b], egui::FontId::proportional(12.0), t.muted);
    for sc in sections {
        let col = c32(COLORS[objs[sc.obj].color]);
        for cv in &sc.curves {
            let pts: Vec<egui::Pos2> = cv.iter().map(|q| to(q[a], q[b])).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
        }
    }
}

fn conic_text(c: &Conic) -> String {
    let e = c.e.map(|e| format!("e = {}", fmt_short(e))).unwrap_or_default();
    match c.kind {
        ConicKind::Circle => {
            let r = c.axes.map_or(String::new(), |a| fmt_short(a.0));
            if i18n::zh() { format!("圆，r = {r}") } else { format!("circle, r = {r}") }
        }
        ConicKind::Ellipse => {
            let (a, b) = c.axes.unwrap_or((0.0, 0.0));
            if i18n::zh() { format!("椭圆，{e}，a = {}，b = {}", fmt_short(a), fmt_short(b)) } else { format!("ellipse, {e}, a = {}, b = {}", fmt_short(a), fmt_short(b)) }
        }
        ConicKind::Parabola => if i18n::zh() { format!("抛物线，{e}") } else { format!("parabola, {e}") },
        ConicKind::Hyperbola => {
            let (a, b) = c.axes.unwrap_or((0.0, 0.0));
            if i18n::zh() { format!("双曲线，{e}，a = {}，b = {}", fmt_short(a), fmt_short(b)) } else { format!("hyperbola, {e}, a = {}, b = {}", fmt_short(a), fmt_short(b)) }
        }
        ConicKind::TwoLines => tr("two crossing lines", "两条相交直线").into(),
        ConicKind::ParallelLines => tr("two parallel lines", "两条平行直线").into(),
        ConicKind::DoubleLine => tr("one line (touching along it)", "一条直线（沿它相切）").into(),
        ConicKind::Point => tr("a single point", "只有一个点").into(),
        ConicKind::Empty => tr("no real points: they do not meet", "没有实点：不相交").into(),
    }
}

/// Lines of text for one pair.
fn describe(p: &relate::Pair, objs: &[Object], hn: &[String], tn: &[String], t: &Tokens) -> Vec<(String, Color32)> {
    let zh = i18n::zh();
    let mut out: Vec<(String, Color32)> = Vec::new();
    let txt = t.text;
    let point_name = |x: DVec3| p.hits.iter().zip(hn).find(|(h, _)| (h.p - x).length() < 1e-9).map(|(_, n)| n.clone());
    match &p.rel {
        Rel::PlanePlane { angle, kind, line, dist } => match kind {
            PlanesKind::Crossing => {
                out.push((if zh { format!("相交，二面角 {}", deg(*angle)) } else { format!("crossing at a dihedral angle of {}", deg(*angle)) }, txt));
                if let Some((q, d)) = line {
                    out.push((if zh { format!("交线过 {}，方向 {}", vec_str(*q), dir_str(*d)) } else { format!("line through {}, direction {}", vec_str(*q), dir_str(*d)) }, t.muted));
                }
            }
            PlanesKind::Parallel => out.push((if zh { format!("平行，距离 {}", fmt_short(*dist)) } else { format!("parallel, {} apart", fmt_short(*dist)) }, txt)),
            PlanesKind::Same => out.push((tr("the same plane", "重合").into(), txt)),
        },
        Rel::LinePlane { angle, kind, point, dist } => match kind {
            LinePlaneKind::Crossing => {
                let q = point.unwrap();
                let name = point_name(q).unwrap_or_default();
                out.push((if zh { format!("相交于 {name} {}，线面角 {}", vec_str(q), deg(*angle)) } else { format!("meet at {name} {}, angle {}", vec_str(q), deg(*angle)) }, txt));
            }
            LinePlaneKind::Parallel => out.push((if zh { format!("直线与平面平行，距离 {}", fmt_short(*dist)) } else { format!("parallel, {} apart", fmt_short(*dist)) }, txt)),
            LinePlaneKind::InPlane => out.push((tr("the line lies in the plane", "直线在平面内").into(), txt)),
        },
        Rel::LineLine { angle, kind, point, dist, .. } => match kind {
            LinesKind::Crossing => {
                let q = point.unwrap();
                let name = point_name(q).unwrap_or_default();
                out.push((if zh { format!("相交于 {name} {}，夹角 {}", vec_str(q), deg(*angle)) } else { format!("meet at {name} {}, angle {}", vec_str(q), deg(*angle)) }, txt));
            }
            LinesKind::Parallel => out.push((if zh { format!("平行，距离 {}", fmt_short(*dist)) } else { format!("parallel, {} apart", fmt_short(*dist)) }, txt)),
            LinesKind::Same => out.push((tr("the same line", "重合").into(), txt)),
            LinesKind::Skew => out.push((if zh { format!("异面，距离 {}（虚线为公垂线），夹角 {}", fmt_short(*dist), deg(*angle)) } else { format!("skew, {} apart (dashed: common perpendicular), angle {}", fmt_short(*dist), deg(*angle)) }, txt)),
        },
        Rel::General => {
            let curve_pair = objs[p.i].kind == Kind::Curve || objs[p.j].kind == Kind::Curve;
            if curve_pair {
                if objs[p.i].kind == Kind::Curve && objs[p.j].kind == Kind::Curve {
                    out.push((tr("(only lines are compared with each other)", "（只比较两条直线的位置关系）").into(), t.faint));
                } else if p.lies_on {
                    let line = objs[p.i].line().is_some() || objs[p.j].line().is_some();
                    out.push((if line { tr("the line lies on the surface", "直线在曲面上") } else { tr("the curve lies on the surface", "曲线在曲面上") }.into(), t.accent_text));
                } else if p.hits.is_empty() {
                    out.push((tr("no common points in the box", "在视图范围内没有交点").into(), txt));
                } else {
                    for (h, n) in p.hits.iter().zip(hn).take(6) {
                        let tag = if h.tangent { tr("  tangent", "（相切）") } else { "" };
                        out.push((format!("{n} {}{tag}", vec_str(h.p)), if h.tangent { c32(TANGENT_RGB) } else { txt }));
                    }
                    if p.hits.len() > 6 {
                        out.push((if zh { format!("…共 {} 个交点", p.hits.len()) } else { format!("… {} points in all", p.hits.len()) }, t.muted));
                    }
                }
            } else {
                let tangent = !p.touches.is_empty();
                let head = match (p.curves.is_empty(), tangent) {
                    (true, false) => tr("they do not meet in the box", "在视图范围内不相交"),
                    (true, true) => tr("they touch without crossing", "相切（不穿过）"),
                    (false, false) => tr("they meet in a curve", "相交于曲线"),
                    (false, true) => tr("they meet, and are tangent at", "相交，且在下列点相切"),
                };
                out.push((head.into(), txt));
                if p.touches.len() > 6 {
                    out.push((if zh { format!("沿一条曲线相切（找到 {} 个切点）", p.touches.len()) } else { format!("tangent along a curve ({} points found)", p.touches.len()) }, c32(TANGENT_RGB)));
                } else {
                    for (q, n) in p.touches.iter().zip(tn) {
                        out.push((format!("{n} {}", vec_str(*q)), c32(TANGENT_RGB)));
                    }
                }
                if let Some(c) = &p.conic {
                    out.push((format!("{}{}", tr("Section: ", "截线："), conic_text(c)), t.accent_text));
                }
            }
        }
    }
    out
}

fn dir_str(v: DVec3) -> String {
    crate::space_app::dir_str(v)
}
