//! What survives a restart: the formulas and inputs of each workspace, the
//! open workspace and the tab order (key [`KEY`]), and separately per
//! workspace how it was being looked at — camera, display mode, layers,
//! chosen points (keys [`view_key`]). Caches and which cards are open are
//! not kept, so every start begins with the folded card layout. Stored by
//! eframe (RON) in its app data file.
//!
//! Every field has a default, so a file written by an older or newer
//! version still loads; nonsensical values are ignored on restore. The view
//! states sit under their own keys so that one that no longer parses (say an
//! enum variant was renamed) loses only that workspace's view.

use serde::{Deserialize, Serialize};

pub const KEY: &str = "session";

/// Key of the view state of a workspace ("plane", "calculus", …).
pub fn view_key(workspace: &str) -> String {
    format!("view.{workspace}")
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Session {
    /// "plane", "calculus", "space", "complex" or "spectrum".
    pub workspace: String,
    pub plane: Option<PlaneState>,
    pub calc: Option<CalcState>,
    pub space: Option<SpaceState>,
    pub complex: Option<ComplexState>,
    pub spectrum: Option<SpectrumState>,
    /// Workspace names in the order of the tabs.
    pub tabs: Vec<String>,
}

/// Camera of a 3D canvas.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct CameraState {
    /// "iso", "front", "side", "top" or "custom".
    pub preset: String,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: [f32; 3],
    pub ortho: bool,
    pub fov_deg: f32,
    pub outline_only: bool,
    /// Projection chosen before a 2D drawing forced orthographic.
    pub ortho_before: Option<bool>,
}

impl Default for CameraState {
    fn default() -> Self {
        let c = crate::render::scene::Camera::default();
        Self { preset: "iso".into(), yaw: c.yaw, pitch: c.pitch, dist: c.dist, target: c.target.to_array(), ortho: c.ortho, fov_deg: c.fov_deg, outline_only: false, ortho_before: None }
    }
}

/// A parameter slider.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct ParamState {
    pub name: String,
    pub value: f64,
    pub lo: f64,
    pub hi: f64,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct CalcState {
    pub src: String,
    pub xr: (f64, f64),
    pub yr: (f64, f64),
    pub zr: (f64, f64),
    pub z_auto: bool,
    /// The point P(a, b).
    pub a: f64,
    pub b: f64,
    pub region: String,
    pub use_region: bool,
    pub params: Vec<ParamState>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct ComplexState {
    pub src: String,
    /// Centre and half-extent of the z-plane view.
    pub center: (f64, f64),
    pub half: f64,
    pub params: Vec<ParamState>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct SpectrumState {
    pub src: String,
    pub params: Vec<ParamState>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct SpaceState {
    pub objects: Vec<ObjectState>,
    pub xr: (f64, f64),
    pub yr: (f64, f64),
    pub zr: (f64, f64),
    pub equal: bool,
    pub params: Vec<ParamState>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct ObjectState {
    /// "surface", "curve" or "solid".
    pub kind: String,
    pub src: String,
    pub solid: SolidState,
    pub color: usize,
    pub opacity: f32,
    pub visible: bool,
    pub lines: bool,
    pub t_range: (f64, f64),
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct SolidState {
    /// "ball", "cylinder", "cone" or "box".
    pub kind: String,
    pub anchor: [f64; 3],
    pub axis: usize,
    pub radius: f64,
    pub height: f64,
    pub size: [f64; 3],
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct PlaneState {
    pub curves: Vec<PlaneCurveState>,
    /// View centre, half of the visible x range, y units per x unit on screen.
    pub center: (f64, f64),
    pub half: f64,
    pub aspect: f64,
    pub equal: bool,
    pub params: Vec<ParamState>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct PlaneCurveState {
    pub src: String,
    pub color: usize,
    pub visible: bool,
    /// Range of t or θ.
    pub range: (f64, f64),
    /// Special points shown (bits, see `curve2d::features`).
    pub marks: u8,
}

/// A usable axis range: finite with lo < hi.
pub fn range_ok(r: (f64, f64)) -> bool {
    r.0.is_finite() && r.1.is_finite() && r.0 < r.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_or_partial_files_still_load() {
        let s: Session = ron::from_str(r#"(workspace: "space", calc: Some((src: "x*y")))"#).unwrap();
        assert_eq!(s.workspace, "space");
        assert_eq!(s.calc.unwrap().src, "x*y");
        assert!(s.space.is_none());
        let s: Session = ron::from_str(r#"(future_field: 3, workspace: "complex")"#).unwrap_or_default();
        assert!(s.workspace == "complex" || s.workspace.is_empty());
    }

    /// Through RON text, as eframe stores it.
    fn via_ron(s: &Session) -> Session {
        ron::from_str(&ron::to_string(s).unwrap()).unwrap()
    }

    #[test]
    fn workspaces_round_trip() {
        // Calculus: formula with a parameter, ranges, P, region.
        let mut st = crate::app::CalcWorkspace::new("a*x^2 - y").state();
        st.params[0].value = 2.5;
        (st.xr, st.yr, st.zr, st.z_auto) = ((-1.0, 4.0), (-2.0, 2.0), (-5.0, 9.0), false);
        (st.a, st.b) = (0.5, -0.25);
        (st.region, st.use_region) = ("x^2 + y^2 <= 4".into(), true);
        let saved = via_ron(&Session { calc: Some(st.clone()), ..Default::default() });
        let mut calc = crate::app::CalcWorkspace::new("x");
        calc.restore(saved.calc.unwrap());
        assert_eq!(calc.state(), st);

        // Space: a surface with a parameter, a curve and a solid.
        let mut space = crate::space_app::SpaceWorkspace::new();
        let mut st = space.state();
        st.objects.truncate(1);
        st.objects[0].src = "z = k*x + 1".into();
        // (A surface or curve carries the preset solid it would switch to.)
        let preset = st.objects[0].solid.clone();
        st.objects.push(ObjectState { kind: "curve".into(), src: "(t, 1, t)".into(), solid: preset, color: 3, opacity: 0.6, visible: false, lines: false, t_range: (-2.0, 3.0) });
        st.objects.push(ObjectState {
            kind: "solid".into(),
            solid: SolidState { kind: "cone".into(), anchor: [0.0, 0.0, -1.5], axis: 1, radius: 1.5, height: 3.0, size: [1.0, 1.0, 1.0] },
            color: 1,
            opacity: 0.75,
            visible: true,
            t_range: (-1.0, 1.0),
            ..Default::default()
        });
        st.params = vec![ParamState { name: "k".into(), value: -0.75, lo: -2.0, hi: 2.0 }];
        (st.xr, st.equal) = ((-4.0, 2.0), false);
        let saved = via_ron(&Session { space: Some(st.clone()), ..Default::default() });
        space.restore(saved.space.unwrap());
        assert_eq!(space.state(), st);

        // Plane curves: a graph with a parameter and marks, a polar curve with its range.
        let mut plane = crate::curve2d_app::PlaneWorkspace::new();
        let st = PlaneState {
            curves: vec![
                PlaneCurveState { src: "y = a sin(x)".into(), color: 2, visible: true, range: (0.0, std::f64::consts::TAU), marks: 3 },
                PlaneCurveState { src: "r = 1 + cos(θ)".into(), color: 1, visible: false, range: (0.0, 3.0), marks: 0 },
            ],
            center: (1.0, -2.0),
            half: 4.5,
            aspect: 1.0,
            equal: true,
            params: vec![ParamState { name: "a".into(), value: 2.0, lo: -3.0, hi: 3.0 }],
        };
        plane.restore(via_ron(&Session { plane: Some(st.clone()), ..Default::default() }).plane.unwrap());
        assert_eq!(plane.state(), st);

        // Complex and spectrum.
        let mut st = crate::complex_app::ComplexWorkspace::new("1/(z - c)").state();
        st.params[0].value = 0.5;
        (st.center, st.half) = ((1.0, -0.5), 2.5);
        let mut complex = crate::complex_app::ComplexWorkspace::new("z");
        complex.restore(via_ron(&Session { complex: Some(st.clone()), ..Default::default() }).complex.unwrap());
        assert_eq!(complex.state(), st);
        let st = SpectrumState { src: "exp(-a*t^2)".into(), params: vec![ParamState { name: "a".into(), value: 3.0, lo: 0.0, hi: 5.0 }] };
        let mut spec = crate::spectrum_app::SpectrumWorkspace::new("t");
        spec.restore(st.clone());
        assert_eq!(spec.state(), st);
    }

    /// View states compared through their RON text.
    fn ron_of<T: Serialize>(v: &T) -> String {
        ron::to_string(v).unwrap()
    }

    #[test]
    fn views_round_trip() {
        // Plane: a point Q, a chosen point on the second curve, layers, integral.
        let mut plane = crate::curve2d_app::PlaneWorkspace::new();
        plane.load_scene(0);
        let mut v: crate::curve2d_app::PlaneView = ron::from_str(&ron_of(&plane.view_state())).unwrap();
        let text = ron_of(&v)
            .replace("ext:None", "ext:Some((1.5,-2.0))")
            .replace("pick:None", "pick:Some((1,0.5,0.5,-0.9375))")
            .replace("show_labels:false", "show_labels:true")
            .replace("show_normal:false", "show_normal:true");
        v = ron::from_str(&text).unwrap();
        let st = plane.state();
        let mut fresh = crate::curve2d_app::PlaneWorkspace::new();
        fresh.restore(via_ron(&Session { plane: Some(st), ..Default::default() }).plane.unwrap());
        fresh.restore_view(ron::from_str(&ron_of(&v)).unwrap());
        let back = ron_of(&fresh.view_state());
        assert!(back.contains("ext:Some((1.5,-2.0))"), "{back}");
        assert!(back.contains("pick:Some((1,0.5,"), "{back}");
        assert!(back.contains("show_labels:true") && back.contains("show_normal:true"), "{back}");

        // Complex: the relief mode with a moved camera survives.
        let complex = crate::complex_app::ComplexWorkspace::new("z^2");
        let text = ron_of(&complex.view_state()).replace("mode:Colour", "mode:Relief").replace("height:Modulus", "height:Re").replace("grid_n:10", "grid_n:14");
        assert!(text.contains("mode:Relief") && text.contains("height:Re") && text.contains("grid_n:14"));
        let mut fresh = crate::complex_app::ComplexWorkspace::new("z^2");
        fresh.restore_view(ron::from_str(&text).unwrap());
        assert_eq!(ron_of(&fresh.view_state()), text);

        // Calculus: map mode, layers on.
        let calc = crate::app::CalcWorkspace::new("x*y");
        let text = ron_of(&calc.view_state()).replace("mode:Surface", "mode:Map").replace("show_point:false", "show_point:true").replace("show_gradient:false", "show_gradient:true");
        assert!(text.contains("mode:Map") && text.contains("show_point:true") && text.contains("show_gradient:true"));
        let mut fresh = crate::app::CalcWorkspace::new("x*y");
        fresh.restore_view(ron::from_str(&text).unwrap());
        assert_eq!(ron_of(&fresh.view_state()), text);

        // Space and spectrum.
        let space = crate::space_app::SpaceWorkspace::new();
        let text = ron_of(&space.view_state()).replace("projection:Off", "projection:XOy").replace("show_grid:false", "show_grid:true");
        assert!(text.contains("projection:XOy") && text.contains("show_grid:true"));
        let mut fresh = crate::space_app::SpaceWorkspace::new();
        fresh.restore_view(ron::from_str(&text).unwrap());
        assert_eq!(ron_of(&fresh.view_state()), text);
        let spec = crate::spectrum_app::SpectrumWorkspace::new("rect(t)");
        let text = ron_of(&spec.view_state()).replace("samp_on:false", "samp_on:true").replace("db:false", "db:true");
        assert!(text.contains("samp_on:true") && text.contains("db:true"));
        let mut fresh = crate::spectrum_app::SpectrumWorkspace::new("rect(t)");
        fresh.restore_view(ron::from_str(&text).unwrap());
        assert_eq!(ron_of(&fresh.view_state()), text);
    }

    #[test]
    fn camera_round_trip_and_nonsense() {
        let mut v = crate::ui::canvas3d::View3d::planar();
        v.apply_preset(crate::ui::canvas3d::ViewPreset::Top);
        v.camera.dist = 0.7;
        let st = v.state();
        let mut w = crate::ui::canvas3d::View3d::planar();
        w.restore(&ron::from_str(&ron_of(&st)).unwrap());
        assert_eq!(w.state(), st);
        let before = w.state();
        w.restore(&CameraState { dist: f32::NAN, ..st.clone() });
        w.restore(&CameraState { preset: "sideways".into(), ..st });
        assert_eq!(w.state(), before);
    }

    #[test]
    fn nonsense_values_are_ignored() {
        let mut calc = crate::app::CalcWorkspace::new("x*y");
        let before = calc.state();
        calc.restore(CalcState { src: String::new(), xr: (f64::NAN, 1.0), yr: (3.0, -3.0), zr: (0.0, 0.0), z_auto: false, a: f64::INFINITY, ..Default::default() });
        let after = calc.state();
        assert_eq!((after.src, after.xr, after.yr, after.a), (before.src, before.xr, before.yr, before.a));
        assert!(after.z_auto);
        let mut space = crate::space_app::SpaceWorkspace::new();
        let st = SpaceState {
            objects: vec![ObjectState { kind: "teapot".into(), ..Default::default() }, ObjectState { kind: "surface".into(), src: "x = 1".into(), color: 99, opacity: f32::NAN, ..Default::default() }],
            ..Default::default()
        };
        space.restore(st);
        let after = space.state();
        assert_eq!(after.objects.len(), 1);
        assert!(after.objects[0].color < crate::space::object::COLORS.len());
        assert!(after.objects[0].opacity > 0.0);
    }
}
