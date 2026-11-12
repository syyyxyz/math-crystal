//! Render the whole UI offscreen to PNG files for visual checks:
//! `cargo run --example ui_preview -- <out_dir>`
use egui_kittest::Harness;
use math_crystal::app::MathCrystal;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or("preview".into()));
    std::fs::create_dir_all(&dir).unwrap();
    let dark = std::env::args().any(|a| a == "--dark");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 880.0))
        .with_pixels_per_point(1.0)
        .with_accessibility_check(false)
        .wgpu()
        .build_eframe(|cc| MathCrystal::new(cc));
    h.ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    let zh = std::env::args().any(|a| a == "--zh");
    math_crystal::i18n::set_zh(zh);
    let classic = std::env::args().any(|a| a == "--classic");
    if classic {
        let c = h.state_mut().calc_mut();
        c.set_function("if(x^2 + y^2 == 0, 0, x*y*(x^2 - y^2)/(x^2 + y^2))");
        let s = c.settings_mut();
        s.show_plane = true;
        s.show_tangent_x = true;
        s.show_tangent_y = true;
        s.show_sections = true;
        s.show_grid = true;
        s.show_model = false;
    }
    let complex = std::env::args().any(|a| a == "--complex");
    if complex {
        h.state_mut().show_complex();
    }
    let spectrum = std::env::args().any(|a| a == "--spectrum");
    if spectrum {
        h.state_mut().show_spectrum();
    }
    h.run_steps(4);
    let img = h.render().expect("render");
    let path = dir.join(format!("ui{}{}{}.png", if dark { "_dark" } else { "_light" }, if classic { "_classic" } else { "" }, if zh { "_zh" } else { "" }).replace("ui", if complex { "complex" } else if spectrum { "spectrum" } else { "ui" }));
    img.save(&path).unwrap();
    eprintln!("wrote {}", path.display());
}
