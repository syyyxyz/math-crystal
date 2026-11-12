use super::*;
use num_complex::Complex64 as C64;

fn xy() -> Vars {
    Vars(vec!["x", "y"])
}

fn ev(src: &str, x: f64, y: f64) -> f64 {
    let mut p = Pool::new();
    let id = parse(src, &mut p, &xy(), false).unwrap();
    Program::compile(&p, &[id]).eval(&[x, y])[0]
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs()))
}

#[test]
fn precedence_and_implicit_multiplication() {
    assert!(close(ev("-x^2", 3.0, 0.0), -9.0));
    assert!(close(ev("2^-x", 1.0, 0.0), 0.5));
    assert!(close(ev("2x^2", 3.0, 0.0), 18.0));
    assert!(close(ev("3(x+1)", 1.0, 0.0), 6.0));
    assert!(close(ev("x sin(y)", 2.0, 1.0), 2.0 * 1f64.sin()));
    assert!(close(ev("2pi", 0.0, 0.0), std::f64::consts::TAU));
    assert!(close(ev("x**2 + y**2", 3.0, 4.0), 25.0));
    assert!(close(ev("2^3^2", 0.0, 0.0), 512.0));
    assert!(close(ev("|x - 5|", 2.0, 0.0), 3.0));
    assert!(close(ev("1e-3 * x", 2.0, 0.0), 2e-3));
    assert!(close(ev("2e", 0.0, 0.0), 2.0 * std::f64::consts::E));
    assert!(close(ev("x/2/2", 8.0, 0.0), 2.0));
    assert!(close(ev("x - y - 1", 5.0, 1.0), 3.0));
}

#[test]
fn piecewise_and_comparisons() {
    assert_eq!(ev("if(x > 0, 1, -1)", 2.0, 0.0), 1.0);
    assert_eq!(ev("if(x > 0, 1, -1)", -2.0, 0.0), -1.0);
    assert_eq!(ev("0 < x < 1", 0.5, 0.0), 1.0);
    assert_eq!(ev("0 < x < 1", 1.5, 0.0), 0.0);
    assert_eq!(ev("x > 0 and y > 0", 1.0, -1.0), 0.0);
    assert_eq!(ev("x > 0 or y > 0", 1.0, -1.0), 1.0);
    assert_eq!(ev("rect(x)", 0.2, 0.0), 1.0);
    assert_eq!(ev("rect(x)", 0.7, 0.0), 0.0);
    assert_eq!(ev("tri(x)", 0.25, 0.0), 0.75);
    assert_eq!(ev("sign(0)", 0.0, 0.0), 0.0);
}

#[test]
fn errors_are_helpful() {
    let mut p = Pool::new();
    let e = parse("xy + 1", &mut p, &xy(), false).unwrap_err();
    assert!(e.msg.contains("x*y"), "{}", e.msg);
    let e = parse("sin(x", &mut p, &xy(), false).unwrap_err();
    assert!(e.msg.contains("')'"), "{}", e.msg);
    let e = parse("foo(x)", &mut p, &xy(), false).unwrap_err();
    assert!(e.msg.contains("foo"), "{}", e.msg);
    let e = parse("atan2(x)", &mut p, &xy(), false).unwrap_err();
    assert!(e.msg.contains("argument"), "{}", e.msg);
    assert!(parse("", &mut p, &xy(), false).is_err());
    assert!(parse("x +", &mut p, &xy(), false).is_err());
    assert!(parse("x $ y", &mut p, &xy(), false).is_err());
    let deep = "(".repeat(500) + "x" + &")".repeat(500);
    assert!(parse(&deep, &mut p, &xy(), false).is_err());
}

#[test]
fn simplification_and_sharing() {
    let mut p = Pool::new();
    let v = xy();
    let a = parse("x*y", &mut p, &v, false).unwrap();
    let b = parse("y*x", &mut p, &v, false).unwrap();
    assert_eq!(a, b);
    let z = parse("x - x + 0*y", &mut p, &v, false).unwrap();
    assert_eq!(p.as_const(z), Some(0.0));
    let f = parse("x^3", &mut p, &v, false).unwrap();
    let d = p.diff(f, 0);
    assert_eq!(p.to_string(d, &v.0), "3*x^2");
    let dd = p.diff(d, 0);
    assert_eq!(p.to_string(dd, &v.0), "6*x");
}

#[test]
fn products_are_flattened() {
    let v = xy();
    let show = |src: &str| {
        let mut p = Pool::new();
        let id = parse(src, &mut p, &v, false).unwrap();
        p.to_string(id, &v.0)
    };
    assert_eq!(show("y*(y*sin(x*y))"), show("y^2*sin(x*y)"));
    assert_eq!(show("x*(y*x)"), "x^2*y");
    assert_eq!(show("2*(3*x)*(-y)"), "-6*x*y");
    assert_eq!(show("x^0.5*x^0.5"), "x^0.5*x^0.5");
    assert_eq!(show("x^0.5*x"), "x^1.5");
    // Mixed partials of a smooth function now share one canonical form.
    let mut p = Pool::new();
    let f = parse("0.15*x^2 - 0.08*y^2 + 0.12*sin(x*y)", &mut p, &v, false).unwrap();
    let fx = p.diff(f, 0);
    let fy = p.diff(f, 1);
    let fxy = p.diff(fx, 1);
    let fyx = p.diff(fy, 0);
    assert_eq!(fxy, fyx, "{} vs {}", p.to_string(fxy, &v.0), p.to_string(fyx, &v.0));
    let fxx = p.diff(fx, 0);
    assert_eq!(p.to_string(fxx, &v.0), "0.3 - 0.12*y^2*sin(x*y)");
}

/// Compare symbolic derivatives against central differences on many points.
#[test]
fn derivatives_match_finite_differences() {
    let funcs = [
        "sin(x)*cos(y)",
        "x^2*y^3 - 2x*y + 1",
        "exp(-(x^2 + y^2))",
        "ln(1 + x^2 + y^2)",
        "sqrt(1 + x^2*y^2)",
        "atan2(y, x)",
        "x^y",
        "tan(x*y/4)",
        "asin(x/3) + acos(y/3)",
        "atan(x) * tanh(y)",
        "sinh(x)/cosh(y)",
        "asinh(x) + acosh(2 + y^2) + atanh(x/4)",
        "cbrt(1 + x^2) * |y - 0.1|",
        "sinc(x) * sinc(y)",
        "max(x, y) + min(x^2, y)",
        "x/(1 + y^2)",
        "log(2, 3 + x) + log10(4 + y) + log2(5 + x*y)",
        "cot(1 + x^2) + sec(y/2) + csc(2 + x^2)",
        "hypot(x, y + 3)",
        "(1 + x^2)^(1 + y^2)",
        "2^(x*y)",
        "mod(x + 7, 3) * y",
    ];
    let pts = [(0.3, 0.7), (-0.6, 0.4), (1.1, -0.9), (0.45, 1.3), (-1.2, -0.35)];
    for src in funcs {
        let mut p = Pool::new();
        let f = parse(src, &mut p, &xy(), false).unwrap();
        let fx = p.diff(f, 0);
        let fy = p.diff(f, 1);
        let fxx = p.diff(fx, 0);
        let fxy = p.diff(fx, 1);
        let prog = Program::compile(&p, &[f, fx, fy, fxx, fxy]);
        for &(x, y) in &pts {
            let h = 1e-5;
            let r = prog.eval(&[x, y]);
            if !r.iter().all(|v| v.is_finite()) {
                continue;
            }
            let at = |dx: f64, dy: f64| prog.eval(&[x + dx, y + dy]);
            let nfx = (at(h, 0.0)[0] - at(-h, 0.0)[0]) / (2.0 * h);
            let nfy = (at(0.0, h)[0] - at(0.0, -h)[0]) / (2.0 * h);
            let nfxx = (at(h, 0.0)[1] - at(-h, 0.0)[1]) / (2.0 * h);
            let nfxy = (at(0.0, h)[1] - at(0.0, -h)[1]) / (2.0 * h);
            for (name, s, n) in [("fx", r[1], nfx), ("fy", r[2], nfy), ("fxx", r[3], nfxx), ("fxy", r[4], nfxy)] {
                assert!(
                    (s - n).abs() <= 1e-5 * (1.0 + s.abs()),
                    "{src}: {name} at ({x},{y}) symbolic {s} numeric {n}"
                );
            }
        }
    }
}

#[test]
fn printing_round_trips() {
    let srcs = [
        "x - (y - 1)",
        "-(x + y)^2",
        "x/(y*2)",
        "(-x)^2",
        "2^(x - 1)",
        "if(x > 0 and y < 1, x, -y)",
        "|x + y| + sin(x)^2",
        "x*-y",
    ];
    let v = xy();
    for src in srcs {
        let mut p = Pool::new();
        let a = parse(src, &mut p, &v, false).unwrap();
        let text = p.to_string(a, &v.0);
        let b = parse(&text, &mut p, &v, false).unwrap();
        let prog = Program::compile(&p, &[a, b]);
        for (x, y) in [(0.3, 0.7), (-1.5, 2.0), (2.0, -0.4)] {
            let r = prog.eval(&[x, y]);
            assert!(close(r[0], r[1]), "{src} -> {text}: {} vs {}", r[0], r[1]);
        }
    }
}

#[test]
fn batch_matches_scalar() {
    let mut p = Pool::new();
    let f = parse("sin(3x)*exp(-y^2) + if(x > y, x, y)", &mut p, &xy(), false).unwrap();
    let fx = p.diff(f, 0);
    let prog = Program::compile(&p, &[f, fx]);
    let n = 10_000;
    let xs: Vec<f64> = (0..n).map(|k| (k as f64 * 0.37).sin() * 2.0).collect();
    let ys: Vec<f64> = (0..n).map(|k| (k as f64 * 0.11).cos() * 2.0).collect();
    let out = prog.eval_batch(&[&xs, &ys], n);
    for k in (0..n).step_by(97) {
        let r = prog.eval(&[xs[k], ys[k]]);
        assert_eq!(out[0][k].to_bits(), r[0].to_bits());
        assert_eq!(out[1][k].to_bits(), r[1].to_bits());
    }
}

#[test]
fn complex_evaluation() {
    let vars = Vars(vec!["z"]);
    let run = |src: &str, z: C64| {
        let mut p = Pool::new();
        let id = parse(src, &mut p, &vars, true).unwrap();
        Program::compile(&p, &[id]).eval_complex(&[z])
    };
    let near = |a: C64, b: C64| (a - b).norm() < 1e-12;
    assert!(near(run("i^2", C64::new(0.0, 0.0)), C64::new(-1.0, 0.0)));
    assert!(near(run("sqrt(-1)", C64::new(0.0, 0.0)), C64::new(0.0, 1.0)));
    assert!(near(run("exp(i*pi)", C64::new(0.0, 0.0)), C64::new(-1.0, 0.0)));
    assert!(near(run("z^2 + 1", C64::new(0.0, 1.0)), C64::new(0.0, 0.0)));
    assert!(near(run("conj(z)*z", C64::new(3.0, 4.0)), C64::new(25.0, 0.0)));
    assert!(near(run("|z|", C64::new(3.0, 4.0)), C64::new(5.0, 0.0)));
    assert!(near(run("re(z) + im(z)", C64::new(3.0, 4.0)), C64::new(7.0, 0.0)));
    assert!(near(run("2i z", C64::new(1.0, 0.0)), C64::new(0.0, 2.0)));
    // sqrt(-1) is not folded away, so the real evaluator reports NaN.
    let mut p = Pool::new();
    let id = parse("sqrt(-1)", &mut p, &Vars(vec!["x"]), false).unwrap();
    assert!(Program::compile(&p, &[id]).eval(&[0.0])[0].is_nan());
}

#[test]
fn parameters() {
    use super::parse_with_params;
    let mut p = Pool::new();
    let mut names = Vec::new();
    let f = parse_with_params("a*sin(k x) + y^b + a", &mut p, &xy(), false, &mut names).unwrap();
    assert_eq!(names, ["a", "k", "b"]);
    let mut prog = Program::compile(&p, &[f]);
    prog.set_params(&[2.0, 3.0, 2.0]);
    let want = |x: f64, y: f64| 2.0 * (3.0 * x).sin() + y * y + 2.0;
    assert!((prog.eval(&[0.4, 1.5])[0] - want(0.4, 1.5)).abs() < 1e-12);
    let (xs, ys): (Vec<f64>, Vec<f64>) = (0..200).map(|k| (k as f64 * 0.01, 1.0 + k as f64 * 0.02)).unzip();
    let out = prog.eval_batch(&[&xs, &ys], 200);
    assert!((out[0][150] - want(xs[150], ys[150])).abs() < 1e-12);
    // Derivatives treat parameters as constants and keep their names.
    let fx = p.diff(f, 0);
    assert_eq!(p.to_string(fx, &["x", "y", "a", "k", "b"]), "a*k*cos(k*x)");
    // Without parameters the same input is still an error, and typos in
    // variable-like names are never taken as parameters.
    assert!(parse("a*x", &mut p, &xy(), false).is_err());
    for bad in ["x*t", "ab*x", "x + z"] {
        assert!(parse_with_params(bad, &mut p, &xy(), false, &mut Vec::new()).is_err(), "{bad}");
    }
    // LaTeX: adjacent letters multiply, Greek commands work.
    let mut names = Vec::new();
    parse_with_params("\\frac{ax}{b} + \\omega y", &mut p, &xy(), false, &mut names).unwrap();
    assert_eq!(names, ["a", "b", "ω"]);
}

#[test]
fn powers_keep_their_domain() {
    let mut p = Pool::new();
    let f = parse("(x^0.5)^2", &mut p, &xy(), false).unwrap();
    assert!(Program::compile(&p, &[f]).eval(&[-1.0, 0.0])[0].is_nan());
    let g = parse("(x^2)^3", &mut p, &xy(), false).unwrap();
    assert_eq!(p.to_string(g, &["x", "y"]), "x^6");
    // exp(u)^v must not print as a double superscript.
    let h = parse("exp(x)^y", &mut p, &xy(), false).unwrap();
    let tex = p.to_latex(h, &["x", "y"]);
    assert_eq!(tex, "\\left(e^{x}\\right)^{y}");
    let back = parse(&tex, &mut p, &xy(), false).unwrap();
    let v = |id| Program::compile(&p, &[id]).eval(&[0.7, 1.3])[0];
    assert!((v(h) - v(back)).abs() < 1e-12);
}
