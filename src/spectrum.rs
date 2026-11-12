//! Fourier transform of an absolutely integrable signal f(t):
//! `F(ω) = ∫ f(t) e^{−iωt} dt` (or `F(ν) = ∫ f(t) e^{−2πiνt} dt`).
//!
//! f is sampled on a window that holds (almost) all of it, with the
//! trapezoid rule; one zero-padded FFT then gives F on a fine frequency
//! grid. Single values F(ω₀) come from the direct sum. The same samples
//! give band-limited reconstructions, Parseval's identity and the
//! time–frequency uncertainty product.

use num_complex::Complex64 as C64;
use rustfft::FftPlanner;

use crate::expr::{Id, Pool, Program, Vars, parse};

pub const VARS: [&str; 1] = ["t"];
const TAU: f64 = std::f64::consts::TAU;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Convention {
    /// ω in rad per unit time: F(ω) = ∫ f e^{−iωt} dt, f = (1/2π) ∫ F e^{iωt} dω.
    Angular,
    /// ν in cycles per unit time: F(ν) = ∫ f e^{−2πiνt} dt, f = ∫ F e^{2πiνt} dν.
    Hertz,
}

impl Convention {
    /// Angular frequency of a frequency-axis value.
    pub fn to_omega(self, x: f64) -> f64 {
        match self {
            Convention::Angular => x,
            Convention::Hertz => x * TAU,
        }
    }
    pub fn from_omega(self, w: f64) -> f64 {
        match self {
            Convention::Angular => w,
            Convention::Hertz => w / TAU,
        }
    }
}

pub struct Signal {
    pub source: String,
    pool: Pool,
    f: Id,
    prog: Program,
}

/// Where f lives and whether it is absolutely integrable.
#[derive(Clone, Copy, Debug)]
pub struct Support {
    pub t0: f64,
    pub t1: f64,
    /// ∫|f| dt estimated on a wide logarithmic grid.
    pub l1: f64,
    /// |f| does not decay fast enough for ∫|f| to converge.
    pub integrable: bool,
    /// Bound on the part of ∫|f| left outside the window.
    pub tail: f64,
}

impl Signal {
    pub fn new(source: &str) -> Result<Self, crate::expr::ParseError> {
        let mut pool = Pool::new();
        let f = parse(source, &mut pool, &Vars(VARS.to_vec()), true)?;
        let prog = Program::compile(&pool, &[f]);
        Ok(Self { source: source.to_string(), pool, f, prog })
    }

    pub fn expr(&self) -> (&Pool, Id) {
        (&self.pool, self.f)
    }

    pub fn latex(&self) -> String {
        self.pool.to_latex(self.f, &VARS)
    }

    pub fn eval(&self, t: f64) -> C64 {
        self.prog.eval_complex(&[C64::new(t, 0.0)])
    }

    pub fn eval_many(&self, ts: &[f64]) -> Vec<C64> {
        let zs: Vec<C64> = ts.iter().map(|&t| C64::new(t, 0.0)).collect();
        self.prog.eval_complex_batch(&[&zs], zs.len())
    }

    /// Find the window holding f and check absolute integrability.
    pub fn support(&self) -> Support {
        const FAR: f64 = 1e4;
        // Dense near 0, logarithmic further out.
        let mut ts: Vec<f64> = (0..=4000).map(|k| -50.0 + 100.0 * k as f64 / 4000.0).collect();
        let logs: Vec<f64> = (0..=600).map(|k| 10f64.powf(-3.0 + 7.0 * k as f64 / 600.0)).filter(|&t| t <= FAR).collect();
        ts.extend(logs.iter().copied());
        ts.extend(logs.iter().map(|t| -t));
        ts.sort_by(f64::total_cmp);
        ts.dedup();
        let mags: Vec<f64> = self.eval_many(&ts).iter().map(|w| if w.is_finite() { w.norm() } else { 0.0 }).collect();
        let max = mags.iter().copied().fold(0.0, f64::max);
        if max == 0.0 {
            return Support { t0: -1.0, t1: 1.0, l1: 0.0, integrable: true, tail: 0.0 };
        }
        let l1: f64 = ts.windows(2).zip(mags.windows(2)).map(|(t, m)| (t[1] - t[0]) * (m[0] + m[1]) / 2.0).sum();
        // ~1/t^p tails: what lies beyond |t| is about |f(t)|·|t|. If that is
        // not small near the far end, ∫|f| does not converge.
        let tail_at = |t: f64, m: f64| m * t.abs();
        let far_tail = ts.iter().zip(&mags).filter(|(t, _)| t.abs() > FAR / 10.0).map(|(&t, &m)| tail_at(t, m)).fold(0.0, f64::max);
        let integrable = far_tail < 0.05 * l1;
        if !integrable {
            // Show the truncated signal on a fixed window.
            return Support { t0: -50.0, t1: 50.0, l1, integrable, tail: far_tail };
        }
        // Outermost points that still matter: |f| above 1e-13·max and a
        // tail estimate above 1e-8·∫|f|; at most ±1000.
        let matters = |t: f64, m: f64| m > 1e-13 * max && tail_at(t, m) > 1e-8 * l1;
        let lo = ts.iter().zip(&mags).find(|(t, m)| matters(**t, **m)).map_or(-1.0, |(t, _)| *t);
        let hi = ts.iter().zip(&mags).rev().find(|(t, m)| matters(**t, **m)).map_or(1.0, |(t, _)| *t);
        let pad = ((hi - lo) * 0.08).max(0.05);
        let (t0, t1) = ((lo - pad).max(-1000.0), (hi + pad).min(1000.0));
        let tail = ts.iter().zip(&mags).filter(|(t, _)| **t < t0 || **t > t1).map(|(&t, &m)| tail_at(t, m)).fold(0.0, f64::max);
        Support { t0, t1, l1, integrable, tail }
    }

    /// Transform on the window; `w_max` is the displayed |ω| range in rad
    /// (None = automatic).
    pub fn spectrum(&self, t0: f64, t1: f64, w_max: Option<f64>) -> Spectrum {
        let width = (t1 - t0).max(1e-9);
        let w_max = w_max.unwrap_or_else(|| {
            let probe = Spectrum::compute(self, t0, t1, 2f64.powf((width / 8192.0).log2().floor()), 1 << 15);
            nice_up(3.0 * probe.energy_radius(0.95)).max(1e-6)
        });
        // About ten samples per period at the display edge (keeps aliasing
        // away from the shown band) and at least 4096 over the window.
        let mut h = (width / 4096.0).min(std::f64::consts::PI / (10.0 * w_max));
        h = h.max(width / (1 << 20) as f64);
        // A power-of-two step on a grid through 0: jumps at 0, ±1/2, integers
        // (rect, step, ...) fall on samples, where the trapezoid rule with the
        // midpoint value step(0) = 1/2 stays exact.
        h = 2f64.powf(h.log2().floor());
        let nt = (width / h).ceil() as usize + 1;
        // Frequency step at most w_max / 1500.
        let need = (TAU / (h * w_max / 1500.0)).ceil() as usize;
        let n = need.max(nt).next_power_of_two().clamp(1 << 12, 1 << 22);
        let mut s = Spectrum::compute(self, t0, t1, h, n);
        s.w_max = w_max;
        s
    }
}

/// Round up to a "nice" number: 1, 1.5, 2, 2.5, 3, 4, 5, 6 or 8 times a
/// power of ten.
pub fn nice_up(x: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() {
        return 1.0;
    }
    let p = 10f64.powf(x.log10().floor());
    let r = x / p;
    p * [1.0, 1.5, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0].into_iter().find(|&m| r <= m * (1.0 + 1e-9)).unwrap_or(10.0)
}

pub struct Spectrum {
    pub t0: f64,
    pub h: f64,
    /// Sample times and values on the window.
    pub t: Vec<f64>,
    pub f: Vec<C64>,
    /// FFT of the weighted samples (natural order, length n).
    g: Vec<C64>,
    pub n: usize,
    /// Displayed |ω| range (rad).
    pub w_max: f64,
}

impl Spectrum {
    fn compute(sig: &Signal, t0: f64, t1: f64, h: f64, n: usize) -> Spectrum {
        let (t0, t1) = ((t0 / h).floor() * h, (t1 / h).ceil() * h);
        let nt = (((t1 - t0) / h).round() as usize + 1).min(n);
        let t: Vec<f64> = (0..nt).map(|k| t0 + h * k as f64).collect();
        let f: Vec<C64> = sig.eval_many(&t).into_iter().map(|w| if w.is_finite() { w } else { C64::new(0.0, 0.0) }).collect();
        let mut g = vec![C64::new(0.0, 0.0); n];
        for (k, w) in f.iter().enumerate() {
            let weight = if k == 0 || k + 1 == nt { 0.5 } else { 1.0 };
            g[k] = w * weight;
        }
        FftPlanner::new().plan_fft_forward(n).process(&mut g);
        Spectrum { t0, h, t, f, g, n, w_max: 0.0 }
    }

    /// Angular frequency of FFT bin m.
    fn omega(&self, m: usize) -> f64 {
        let mm = if m < self.n / 2 { m as f64 } else { m as f64 - self.n as f64 };
        TAU * mm / (self.n as f64 * self.h)
    }

    fn value(&self, m: usize) -> C64 {
        let w = self.omega(m);
        self.g[m] * C64::from_polar(self.h, -w * self.t0)
    }

    /// Spacing of the frequency grid (rad).
    pub fn d_omega(&self) -> f64 {
        TAU / (self.n as f64 * self.h)
    }

    /// Highest frequency the sampling resolves (rad).
    pub fn nyquist(&self) -> f64 {
        std::f64::consts::PI / self.h
    }

    /// (ω, F(ω)) for |ω| ≤ limit, ascending.
    pub fn band(&self, limit: f64) -> Vec<(f64, C64)> {
        let mut v: Vec<(f64, C64)> = (0..self.n).filter(|&m| self.omega(m).abs() <= limit).map(|m| (self.omega(m), self.value(m))).collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v
    }

    /// F(ω) by the direct trapezoid sum (any ω).
    pub fn at(&self, w: f64) -> C64 {
        let nt = self.t.len();
        let mut s = C64::new(0.0, 0.0);
        for (k, (&t, f)) in self.t.iter().zip(&self.f).enumerate() {
            let weight = if k == 0 || k + 1 == nt { 0.5 } else { 1.0 };
            s += f * C64::from_polar(weight, -w * t);
        }
        s * self.h
    }

    /// Smallest Ω with (1/2π)∫_{|ω|≤Ω} |F|² dω ≥ frac · total.
    pub fn energy_radius(&self, frac: f64) -> f64 {
        let mut bins: Vec<(f64, f64)> = (0..self.n).map(|m| (self.omega(m).abs(), self.value(m).norm_sqr())).collect();
        bins.sort_by(|a, b| a.0.total_cmp(&b.0));
        let total: f64 = bins.iter().map(|b| b.1).sum();
        if total <= 0.0 {
            return 1.0;
        }
        let mut acc = 0.0;
        for (w, e) in &bins {
            acc += e;
            if acc >= frac * total {
                return w.max(self.d_omega());
            }
        }
        bins.last().map_or(1.0, |b| b.0)
    }

    /// Fraction of the energy with |ω| ≤ Ω.
    pub fn energy_within(&self, limit: f64) -> f64 {
        let (mut inside, mut total) = (0.0, 0.0);
        for m in 0..self.n {
            let e = self.value(m).norm_sqr();
            total += e;
            if self.omega(m).abs() <= limit {
                inside += e;
            }
        }
        if total > 0.0 { inside / total } else { 1.0 }
    }

    /// f restricted to |ω| ≤ Ω and transformed back, at the sample times.
    pub fn band_limited(&self, limit: f64) -> Vec<C64> {
        let mut g: Vec<C64> = (0..self.n).map(|m| if self.omega(m).abs() <= limit { self.g[m] } else { C64::new(0.0, 0.0) }).collect();
        FftPlanner::new().plan_fft_inverse(self.n).process(&mut g);
        let nt = self.t.len();
        let scale = 1.0 / self.n as f64;
        g.truncate(nt);
        // Undo the half weights at the window ends.
        for (k, v) in g.iter_mut().enumerate() {
            *v *= scale * if k == 0 || k + 1 == nt { 2.0 } else { 1.0 };
        }
        g
    }

    /// ∫|f| dt over the window (trapezoid).
    pub fn l1(&self) -> f64 {
        let nt = self.f.len();
        self.f.iter().enumerate().map(|(k, w)| w.norm() * if k == 0 || k + 1 == nt { 0.5 } else { 1.0 }).sum::<f64>() * self.h
    }

    /// Energy ∫|f|² dt (equal to (1/2π)∫|F|² dω).
    pub fn energy_time(&self) -> f64 {
        let nt = self.f.len();
        self.f.iter().enumerate().map(|(k, w)| w.norm_sqr() * if k == 0 || k + 1 == nt { 0.5 } else { 1.0 }).sum::<f64>() * self.h
    }

    pub fn energy_freq(&self) -> f64 {
        (0..self.n).map(|m| self.value(m).norm_sqr()).sum::<f64>() * self.d_omega() / TAU
    }

    /// RMS duration Δt and RMS bandwidth Δω (rad) of |f|² and |F|². Δω is
    /// None when it does not converge (|F|² decays like 1/ω² or slower).
    pub fn spreads(&self) -> (f64, Option<f64>) {
        let e: Vec<f64> = self.f.iter().map(|w| w.norm_sqr()).collect();
        let total: f64 = e.iter().sum();
        let mean = self.t.iter().zip(&e).map(|(t, e)| t * e).sum::<f64>() / total;
        let dt = (self.t.iter().zip(&e).map(|(t, e)| (t - mean).powi(2) * e).sum::<f64>() / total).sqrt();
        let spread = |limit: f64| {
            let (mut s0, mut s1, mut s2) = (0.0, 0.0, 0.0);
            for m in 0..self.n {
                let w = self.omega(m);
                if w.abs() <= limit {
                    let e = self.value(m).norm_sqr();
                    s0 += e;
                    s1 += w * e;
                    s2 += w * w * e;
                }
            }
            let mean = s1 / s0;
            (s2 / s0 - mean * mean).max(0.0).sqrt()
        };
        let full = spread(self.nyquist());
        let half = spread(self.nyquist() / 2.0);
        let dw = ((full - half).abs() <= 0.02 * full.max(1e-300)).then_some(full);
        (dt, dw)
    }
}

/// Symmetries of f on [−T, T].
#[derive(Clone, Copy, Debug, Default)]
pub struct Symmetry {
    pub real: bool,
    pub even: bool,
    pub odd: bool,
}

pub fn symmetry(sig: &Signal, t_max: f64) -> Symmetry {
    let ts: Vec<f64> = (1..=400).map(|k| t_max * k as f64 / 400.0 * 0.999 + 1e-3 * (k as f64).sqrt()).collect();
    let neg: Vec<f64> = ts.iter().map(|t| -t).collect();
    let a = sig.eval_many(&ts);
    let b = sig.eval_many(&neg);
    let scale = a.iter().chain(&b).filter(|w| w.is_finite()).map(|w| w.norm()).fold(0.0, f64::max).max(1e-300);
    let tol = 1e-9 * scale;
    let pairs = || a.iter().zip(&b).filter(|(x, y)| x.is_finite() && y.is_finite());
    Symmetry {
        real: a.iter().chain(&b).filter(|w| w.is_finite()).all(|w| w.im.abs() <= tol),
        even: pairs().all(|(x, y)| (x - y).norm() <= tol),
        odd: pairs().all(|(x, y)| (x + y).norm() <= tol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(src: &str, exact: impl Fn(f64) -> C64, tol: f64) {
        let s = Signal::new(src).unwrap();
        let sup = s.support();
        assert!(sup.integrable, "{src}");
        let sp = s.spectrum(sup.t0, sup.t1, None);
        for w in [0.0, 0.3, 1.0, 2.5, -1.7] {
            let got = sp.at(w);
            assert!((got - exact(w)).norm() < tol, "{src} at ω = {w}: {got} vs {}", exact(w));
        }
        // The FFT grid agrees with the direct sum.
        for (w, v) in sp.band(sp.w_max).iter().step_by(97) {
            assert!((v - sp.at(*w)).norm() < 1e-9 * (1.0 + v.norm()), "{src}: FFT vs sum at {w}");
        }
    }

    #[test]
    fn known_transforms() {
        let r = |v: f64| C64::new(v, 0.0);
        check("rect(t)", |w| r(if w == 0.0 { 1.0 } else { 2.0 * (w / 2.0).sin() / w }), 1e-6);
        check("tri(t)", |w| r(if w == 0.0 { 1.0 } else { ((w / 2.0).sin() / (w / 2.0)).powi(2) }), 1e-4);
        check("exp(-|t|)", |w| r(2.0 / (1.0 + w * w)), 1e-4);
        check("exp(-t^2/2)", |w| r(TAU.sqrt() * (-w * w / 2.0).exp()), 1e-9);
        check("step(t) exp(-t)", |w| C64::new(1.0, w).inv(), 1e-5);
        check("1/(1 + t^2)", |w| r(std::f64::consts::PI * (-w.abs()).exp()), 5e-3);
        check("t exp(-t^2)", |w| C64::new(0.0, -w * std::f64::consts::PI.sqrt() / 2.0 * (-w * w / 4.0).exp()), 1e-9);
        // Shift theorem: rect(t - 2) → e^{−2iω} F(ω).
        check("rect(t - 2)", |w| C64::from_polar(1.0, -2.0 * w) * r(if w == 0.0 { 1.0 } else { 2.0 * (w / 2.0).sin() / w }), 1e-6);
    }

    #[test]
    fn integrability() {
        for src in ["sin(t)/t", "1", "cos(t)", "1/(1 + |t|)"] {
            assert!(!Signal::new(src).unwrap().support().integrable, "{src}");
        }
        for src in ["exp(-t^2)", "1/(1+t^2)", "rect(t)", "exp(-|t|) cos(40 t)"] {
            assert!(Signal::new(src).unwrap().support().integrable, "{src}");
        }
    }

    #[test]
    fn parseval_uncertainty_reconstruction() {
        let s = Signal::new("exp(-t^2/2)").unwrap();
        let sup = s.support();
        let sp = s.spectrum(sup.t0, sup.t1, None);
        assert!((sp.energy_time() - std::f64::consts::PI.sqrt()).abs() < 1e-9);
        assert!((sp.energy_freq() - sp.energy_time()).abs() < 1e-9);
        // The Gaussian attains Δt·Δω = 1/2.
        let (dt, dw) = sp.spreads();
        assert!((dt * dw.unwrap() - 0.5).abs() < 1e-6, "{dt} {dw:?}");
        // rect: Δω diverges.
        let r = Signal::new("rect(t)").unwrap();
        let sup = r.support();
        assert!(r.spectrum(sup.t0, sup.t1, None).spreads().1.is_none());
        // Band-limiting the Gaussian well past its width changes nothing.
        let back = sp.band_limited(20.0);
        let err = back.iter().zip(&sp.f).map(|(a, b)| (a - b).norm()).fold(0.0, f64::max);
        assert!(err < 1e-6, "{err}");
        // Gibbs: the band-limited rect overshoots by about 9% of the jump.
        let rs = r.spectrum(sup.t0, sup.t1, Some(200.0));
        let over = rs.band_limited(60.0).iter().map(|w| w.re).fold(f64::MIN, f64::max) - 1.0;
        assert!((over - 0.0895).abs() < 0.01, "{over}");
    }

    #[test]
    fn symmetries() {
        let sy = symmetry(&Signal::new("exp(-t^2)").unwrap(), 5.0);
        assert!(sy.real && sy.even && !sy.odd);
        let sy = symmetry(&Signal::new("t exp(-t^2)").unwrap(), 5.0);
        assert!(sy.real && !sy.even && sy.odd);
        let sy = symmetry(&Signal::new("exp(i t - t^2)").unwrap(), 5.0);
        assert!(!sy.real);
    }
}
