//! Render the calculus scene offscreen to PNG (no window needed):
//! `cargo run --example headless -- out.png "expr" [yaw_deg pitch_deg]`
use glam::Vec2;
use math_crystal::{calc::Surface, calc_view, render::{gpu::{Gpu3d, View, write_png}, scene::{Camera, Projector}}};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or("headless.png".into());
    let src = args.get(2).cloned().unwrap_or("0.15*x^2 - 0.08*y^2 + 0.12*sin(x*y)".into());
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    })).expect("no adapter");
    eprintln!("adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).expect("device");
    let surf = Surface::new(&src).expect("parse");
    let mut s = if std::env::var("ICON").is_ok() {
        calc_view::Settings { show_model: false, show_point: false, resolution: 97, ..Default::default() }
    } else {
        calc_view::Settings { a: 0.6, b: -0.4, show_plane: true, show_tangent_x: true, show_tangent_y: true, show_grid: true, show_sections: false, show_gradient: true, ..Default::default() }
    };
    let d = surf.derivs(s.a, s.b);
    s.model_fxx = d.fxx.v; s.model_fyy = d.fyy.v; s.model_fxy = d.fxy.v;
    let z = surf.sample(s.xr, s.yr, 65);
    if let Some(zr) = calc_view::auto_z(&z) { s.zr = zr; }
    let built = calc_view::build(Some(&surf), &s);
    let mut cam = Camera::default();
    if let (Some(y), Some(p)) = (args.get(3), args.get(4)) { cam.yaw = y.parse::<f32>().unwrap().to_radians(); cam.pitch = p.parse::<f32>().unwrap().to_radians(); }
    let icon = std::env::var("ICON").is_ok();
    let size = if icon { [256u32, 256u32] } else { [1200u32, 800u32] };
    let proj = Projector { vp: cam.view_proj(size[0] as f32 / size[1] as f32), size: Vec2::new(size[0] as f32, size[1] as f32) };
    let (deco, labels) = calc_view::decorations(&s, &built.frame, &cam, &proj);
    let mut scene = built.scene;
    if !icon { scene.lines.extend(deco); }
    if icon { cam.dist = 3.9; cam.target = glam::Vec3::new(0.0, 0.0, -0.1); }
    let mut gpu = Gpu3d::new(&device);
    gpu.set_scene(&device, &scene);
    let pal = s.theme.palette();
    gpu.render(&device, &queue, size, &View { camera: cam, clip_half: built.frame.half, background: if icon { [0.0; 4] } else { pal.background }, wire: pal.wire });
    let (w, h, px) = gpu.read_rgba(&device, &queue).expect("readback");
    write_png(std::path::Path::new(&out), w, h, &px).unwrap();
    eprintln!("wrote {out} ({} meshes, {} lines, {} labels)", scene.meshes.len(), scene.lines.len(), labels.len());
}
