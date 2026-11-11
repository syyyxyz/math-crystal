#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};

fn main() -> eframe::Result {
    env_logger::init();
    // A few threads are plenty for grid sampling and keep the laptop cool.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    let _ = rayon::ThreadPoolBuilder::new().num_threads(threads).build_global();

    let mut setup = WgpuSetupCreateNew::without_display_handle();
    // Integrated GPU by default (WGPU_POWER_PREF=high overrides).
    setup.power_preference = wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::LowPower);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Math-crystal")
            .with_app_id("math-crystal")
            .with_inner_size([1400.0, 880.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(icon()),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: WgpuConfiguration {
            wgpu_setup: WgpuSetup::CreateNew(setup),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native("Math-crystal", options, Box::new(|cc| Ok(Box::new(math_crystal::app::MathCrystal::new(cc)))))
}

fn icon() -> egui::IconData {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!("../assets/icon.png")));
    let mut reader = decoder.read_info().expect("icon header");
    let mut buf = vec![0; reader.output_buffer_size().expect("icon size")];
    let info = reader.next_frame(&mut buf).expect("icon data");
    buf.truncate(info.buffer_size());
    egui::IconData { rgba: buf, width: info.width, height: info.height }
}
