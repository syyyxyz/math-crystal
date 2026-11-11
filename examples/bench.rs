use math_crystal::{calc::Surface, calc_view};
use std::time::Instant;
fn main() {
    let _ = rayon::ThreadPoolBuilder::new().num_threads(8).build_global();
    let src = "3(1-x)^2 exp(-x^2-(y+1)^2) - 10(x/5 - x^3 - y^5) exp(-x^2-y^2) - exp(-(x+1)^2-y^2)/3";
    let t = Instant::now(); let s = Surface::new(src).unwrap(); println!("parse + 6 symbolic derivatives: {:?}", t.elapsed());
    for n in [129, 401] {
        let t = Instant::now(); let reps = 20;
        for _ in 0..reps { std::hint::black_box(s.sample((-3.0,3.0),(-3.0,3.0), n)); }
        println!("sample {n}x{n}: {:?}", t.elapsed() / reps);
    }
    let t = Instant::now(); for _ in 0..100 { std::hint::black_box(s.derivs(0.3, -0.4)); } println!("point derivatives (cross-checked): {:?}", t.elapsed() / 100);
    let st = calc_view::Settings { show_plane: true, show_sections: true, ..Default::default() };
    let t = Instant::now(); for _ in 0..20 { std::hint::black_box(calc_view::build(Some(&s), &st)); } println!("full scene rebuild (129 grid): {:?}", t.elapsed() / 20);
}
