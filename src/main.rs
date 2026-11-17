#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};

#[cfg(windows)]
mod startup_win;

fn main() -> eframe::Result {
    #[cfg(windows)]
    {
        startup_win::apply_saved_backend();
        startup_win::install_panic_hook();
    }
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
    let result = eframe::run_native("Math-crystal", options, Box::new(|cc| Ok(Box::new(math_crystal::app::MathCrystal::new(cc)))));
    #[cfg(windows)]
    if let Err(e) = &result {
        startup_win::report(&e.to_string());
    }
    result
}

fn icon() -> egui::IconData {
    let (width, height, rgba) = math_crystal::ui::icon_rgba();
    egui::IconData { rgba, width, height }
}
