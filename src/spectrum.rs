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

use crate::expr::{Id, Pool, Program, Vars, parse_with_params};

pub const VARS: [&str; 1] = ["t"];
const TAU: f64 = std::f64::consts::TAU;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
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
    /// Parameter names (letters other than t and i), values start at 1.
    pub params: Vec<String>,
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
        let mut params = Vec::new();
        let f = parse_with_params(source, &mut pool, &Vars(VARS.to_vec()), true, &mut params)?;
        let mut prog = Program::compile(&pool, &[f]);
        prog.set_params(&vec![1.0; params.len()]);
        Ok(Self { source: source.to_string(), pool, f, prog, params })
    }

    /// Give the parameters their values (same order as [`Self::params`]).
    pub fn set_params(&mut self, values: &[f64]) {
        self.prog.set_params(values);
    }

    /// Variable names followed by the parameter names.
    pub fn names(&self) -> Vec<&str> {
        VARS.iter().copied().chain(self.params.iter().map(String::as_str)).collect()
    }

    pub fn expr(&self) -> (&Pool, Id) {
        (&self.pool, self.f)
    }

    pub fn latex(&self) -> String {
        self.pool.to_latex(self.f, &self.names())
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
        const NEAR: f64 = 1000.0;
        const FAR: f64 = 1e4;
        // Uniform through 0 and the integers out to ±NEAR (narrow pulses far
        // from the origin are still hit), logarithmic towards 0 and out to FAR.
        let mut ts: Vec<f64> = (-64_000i64..=64_000).map(|k| k as f64 / 64.0).collect();
        for k in 0..=240 {
            let t = 10f64.powf(-6.0 + 4.2 * k as f64 / 240.0);
            ts.extend([t, -t]);
        }
        for k in 1..=400 {
            let t = NEAR * 10f64.powf(k as f64 / 400.0);
            ts.extend([t, -t]);
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup();
        let vals = self.eval_many(&ts);
        let mags: Vec<f64> = vals.iter().map(|w| if w.is_finite() { w.norm() } else { 0.0 }).collect();
        let max = mags.iter().copied().fold(0.0, f64::max);
        if max == 0.0 {
            return Support { t0: -1.0, t1: 1.0, l1: 0.0, integrable: true, tail: 0.0 };
        }
        // ∫|f| over lo < |t| < hi (trapezoid).
        let mass = |lo: f64, hi: f64| -> f64 {
            ts.windows(2)
                .zip(mags.windows(2))
                .filter(|(t, _)| {
                    let a = t[0].abs().min(t[1].abs());
                    let b = t[0].abs().max(t[1].abs());
                    a >= lo && b <= hi
                })
                .map(|(t, m)| (t[1] - t[0]) * (m[0] + m[1]) / 2.0)
                .sum()
        };
        let l1 = mass(0.0, f64::INFINITY);
        // Decay: what the last decade holds must be negligible or clearly
        // less than the decade before it. 1/t tails give equal amounts,
        // 1/t^p gives a ratio 10^(1-p), exponentials far less.
        let (m1, m2) = (mass(NEAR / 10.0, NEAR), mass(NEAR, FAR));
        let decays = m2 <= 1e-9 * l1 || m2 < 0.6 * m1;
        // Local blow-ups: where a sample is undefined, |f(t)|·|t − t_s| must
        // go to 0 (as for 1/sqrt|t|), not stay level (1/t) or grow.
        let mut blows_up = false;
        // (Only next to defined samples: a whole undefined stretch, such as
        // 0·∞ in step(t)·exp(−t) far left, is not a blow-up.)
        let defined = |k: usize| vals.get(k).is_some_and(|w| w.is_finite());
        for (k, _) in vals.iter().enumerate().filter(|(k, w)| !w.is_finite() && (defined(k + 1) || (*k > 0 && defined(k - 1)))).take(16) {
            let ts0 = ts[k];
            let g = |d: f64| {
                let d = d * (1.0 + ts0.abs());
                let m = self.eval(ts0 + d).norm().max(self.eval(ts0 - d).norm());
                m * d
            };
            let (near, nearer) = (g(1e-3), g(1e-7));
            if near.is_finite() && nearer.is_finite() && nearer >= 0.5 * near && nearer > 1e-12 * l1 {
                blows_up = true;
            }
        }
        let tail_at = |t: f64, m: f64| m * t.abs();
        if !decays || blows_up {
            // Show the truncated signal on a fixed window.
            let far_tail = ts.iter().zip(&mags).filter(|(t, _)| t.abs() > NEAR).map(|(&t, &m)| tail_at(t, m)).fold(0.0, f64::max);
            return Support { t0: -50.0, t1: 50.0, l1, integrable: false, tail: far_tail };
        }
        // Outermost points that still matter: |f| above 1e-13·max and a
        // tail estimate above 1e-8·∫|f|. Beyond ±NEAR only when a real
        // share of ∫|f| lives there (slow exponentials).
        let cap = if m2 > 0.01 * l1 { FAR } else { NEAR };
        let matters = |t: f64, m: f64| m > 1e-13 * max && tail_at(t, m).max(m * 1e-3) > 1e-8 * l1;
        let lo = ts.iter().zip(&mags).find(|(t, m)| matters(**t, **m)).map_or(-1.0, |(t, _)| *t);
        let hi = ts.iter().zip(&mags).rev().find(|(t, m)| matters(**t, **m)).map_or(1.0, |(t, _)| *t);
        let pad = ((hi - lo) * 0.08).max(0.05);
        let (t0, t1) = ((lo - pad).max(-cap), (hi + pad).min(cap));
        let tail = ts.iter().zip(&mags).filter(|(t, _)| **t < t0 || **t > t1).map(|(&t, &m)| tail_at(t, m)).fold(0.0, f64::max);
        Support { t0, t1, l1, integrable: true, tail }
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
        let nt = (width / h).ceil() as usize + 2;
        // The whole-band FFT (energy, spreads, reconstruction) only needs a
        // period several windows long; the displayed band is computed on its
        // own fine grid by a chirp-z transform instead of by zero-padding
        // out to the Nyquist frequency.
        let pad = if nt <= 1 << 14 {
            16
        } else if nt <= 1 << 16 {
            8
        } else {
            4
        };
        let n = (pad * nt).next_power_of_two().clamp(1 << 14, 1 << 22);
        let mut s = Spectrum::compute(self, t0, t1, h, n);
        s.w_max = w_max;
        let m = 2 * FINE_HALF + 1;
        let dw = w_max / FINE_HALF as f64;
        let weighted: Vec<C64> = s.f.iter().enumerate().map(|(k, w)| w * if k == 0 || k + 1 == s.f.len() { 0.5 * s.h } else { s.h }).collect();
        let vals = czt(&weighted, s.t[0], s.h, -w_max, dw, m);
        s.fine = vals.into_iter().enumerate().map(|(k, v)| ((k as f64 - FINE_HALF as f64) * dw, v)).collect();
        s
    }
}

/// f sampled with period `ts`: `x[k] = f((n0 + k)·ts)`, `k = 0..n`.
///
/// Sampling replaces the spectrum by its periodic sum: the discrete-time
/// transform `ts·Σ x[k] e^{−iω t_k}` equals `Σ_m F(ω − m·ω_s)` with
/// `ω_s = 2π/ts` (aliasing when F reaches past `ω_s/2`). An N-point DFT of
/// the samples gives that periodic spectrum at the N frequencies `j·ω_s/N`.
pub struct Sampled {
    pub ts: f64,
    pub t: Vec<f64>,
    pub x: Vec<C64>,
    /// `ts·Σ x[k] e^{−iω t_k}` on a fine grid over |ω| ≤ w_max.
    pub dtft: Vec<(f64, C64)>,
    /// The DFT bins `j·ω_s/N` inside |ω| ≤ w_max (repeating with period
    /// ω_s), scaled and phased like `dtft`; empty when there are too many
    /// to draw.
    pub bins: Vec<(f64, C64)>,
}

impl Sampled {
    /// Sampling frequency ω_s (rad).
    pub fn omega_s(&self) -> f64 {
        TAU / self.ts
    }
}

impl Signal {
    pub fn sampled(&self, ts: f64, n0: i64, n: usize, w_max: f64) -> Sampled {
        let t: Vec<f64> = (0..n).map(|k| (n0 + k as i64) as f64 * ts).collect();
        let x: Vec<C64> = self.eval_many(&t).into_iter().map(|w| if w.is_finite() { w } else { C64::new(0.0, 0.0) }).collect();
        let scaled: Vec<C64> = x.iter().map(|v| v * ts).collect();
        let t0 = n0 as f64 * ts;
        let dw = w_max / FINE_HALF as f64;
        let dtft = czt(&scaled, t0, ts, -w_max, dw, 2 * FINE_HALF + 1).into_iter().enumerate().map(|(k, v)| ((k as f64 - FINE_HALF as f64) * dw, v)).collect();
        let bin = TAU / (ts * n.max(1) as f64);
        let half = (w_max / bin).floor() as usize;
        let bins = if n == 0 || half > 1500 {
            Vec::new()
        } else {
            czt(&scaled, t0, ts, -(half as f64) * bin, bin, 2 * half + 1).into_iter().enumerate().map(|(k, v)| ((k as f64 - half as f64) * bin, v)).collect()
        };
        Sampled { ts, t, x, dtft, bins }
    }
}

/// Points on each side of 0 in the displayed band.
const FINE_HALF: usize = 1500;

/// Chirp-z (Bluestein) transform: `X[k] = Σ_j x[j] e^{−i ω_k (t0 + j h)}`
/// at `ω_k = wa + k·dw`, `k = 0..m`, in O((n + m) log(n + m)).
pub fn czt(x: &[C64], t0: f64, h: f64, wa: f64, dw: f64, m: usize) -> Vec<C64> {
    let nx = x.len();
    if nx == 0 || m == 0 {
        return vec![C64::new(0.0, 0.0); m];
    }
    let alpha = dw * h;
    // e^{−i α n²/2}, with the phase reduced before the trigonometry.
    let chirp = |n: usize| {
        let nn = n as f64;
        C64::from_polar(1.0, -((0.5 * alpha * nn * nn) % TAU))
    };
    let len = (nx + m - 1).next_power_of_two();
    let zero = C64::new(0.0, 0.0);
    let mut a = vec![zero; len];
    for (j, v) in x.iter().enumerate() {
        a[j] = v * C64::from_polar(1.0, -((wa * h * j as f64) % TAU)) * chirp(j);
    }
    let mut b = vec![zero; len];
    for n in 0..m.max(nx) {
        let c = chirp(n).conj();
        if n < m {
            b[n] = c;
        }
        if n > 0 && n < nx {
            b[len - n] = c;
        }
    }
    let mut planner = FftPlanner::new();
    let fwd = planner.plan_fft_forward(len);
    fwd.process(&mut a);
    fwd.process(&mut b);
    for (p, q) in a.iter_mut().zip(&b) {
        *p *= q;
    }
    planner.plan_fft_inverse(len).process(&mut a);
    let scale = 1.0 / len as f64;
    (0..m).map(|k| a[k] * chirp(k) * C64::from_polar(scale, -(((wa + k as f64 * dw) * t0) % TAU))).collect()
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
    /// (ω, F(ω)) on a fine grid over the displayed band.
    fine: Vec<(f64, C64)>,
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
        Spectrum { t0, h, t, f, g, n, w_max: 0.0, fine: vec![] }
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

    /// (ω, F(ω)) for |ω| ≤ limit, ascending: the fine display grid inside
    /// the displayed band, the FFT bins beyond it.
    pub fn band(&self, limit: f64) -> Vec<(f64, C64)> {
        if !self.fine.is_empty() && limit <= self.w_max * (1.0 + 1e-12) {
            return self.fine.iter().filter(|(w, _)| w.abs() <= limit * (1.0 + 1e-12)).copied().collect();
        }
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
        // Inside the displayed band: integrate |F|² on the fine grid and
        // compare with ∫|f|² (Parseval).
        let total_t = self.energy_time();
        if !self.fine.is_empty() && limit <= self.w_max && total_t > 0.0 {
            let mut inside = 0.0;
            for w in self.fine.windows(2) {
                let (a, b) = (w[0].0.max(-limit), w[1].0.min(limit));
                if b > a {
                    inside += (b - a) * (w[0].1.norm_sqr() + w[1].1.norm_sqr()) / 2.0;
                }
            }
            return (inside / TAU / total_t).clamp(0.0, 1.0);
        }
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
        // A local 1/t blow-up is not integrable either; 1/sqrt|t| is.
        assert!(!Signal::new("exp(-t^2)/t").unwrap().support().integrable);
        assert!(Signal::new("exp(-t^2)/sqrt(|t|)").unwrap().support().integrable);
        // Slow exponential: integrable, and the window grows to hold it.
        let slow = Signal::new("exp(-|t|/400)").unwrap().support();
        assert!(slow.integrable && slow.t1 > 3000.0, "{slow:?}");
        // A narrow pulse far from the origin is found.
        let far = Signal::new("rect((t-200)/0.05)").unwrap().support();
        assert!(far.t0 < 199.98 && far.t1 > 200.02 && far.t1 < 201.0 && (far.l1 - 0.05).abs() < 0.02, "{far:?}");
        for src in ["exp(-t^2)", "1/(1+t^2)", "rect(t)", "exp(-|t|) cos(40 t)", "sinc(t)^2", "1/(1+|t|^1.5)"] {
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
    fn sampling_aliases_and_dft_bins() {
        // e^{−|t|} sampled at ts = 1: ts·Σ e^{−|n|} e^{−iωn} has the closed
        // form (1 − q²)/(1 − 2q cos ω + q²) with q = 1/e, which is the
        // periodic sum of F(ω) = 2/(1 + ω²) up to the trapezoid weights.
        let s = Signal::new("exp(-|t|)").unwrap();
        let sm = s.sampled(1.0, -40, 81, 10.0);
        let q = (-1f64).exp();
        for (w, v) in sm.dtft.iter().step_by(173) {
            let exact = (1.0 - q * q) / (1.0 - 2.0 * q * w.cos() + q * q);
            assert!((v - C64::new(exact, 0.0)).norm() < 1e-10, "ω = {w}: {v} vs {exact}");
        }
        // Period ω_s = 2π, and clearly more than F alone at ω = π (aliasing).
        let at = |w: f64| sm.dtft.iter().min_by(|a, b| (a.0 - w).abs().total_cmp(&(b.0 - w).abs())).unwrap().1.re;
        assert!((at(1.0) - at(1.0 + TAU)).abs() < 2e-2);
        assert!(at(std::f64::consts::PI) > 1.5 * 2.0 / (1.0 + std::f64::consts::PI.powi(2)));
        // The bins are the DFT of the samples (with the start-time phase).
        let n = sm.x.len();
        let mut dft = sm.x.clone();
        FftPlanner::new().plan_fft_forward(n).process(&mut dft);
        assert!(!sm.bins.is_empty());
        for (w, v) in &sm.bins {
            let j = (w / (TAU / n as f64)).round() as i64;
            let want = dft[j.rem_euclid(n as i64) as usize] * C64::from_polar(sm.ts, -w * sm.t[0]);
            assert!((v - want).norm() < 1e-10, "bin {j}: {v} vs {want}");
        }
        // Fast enough sampling of a Gaussian reproduces F itself.
        let g = Signal::new("exp(-t^2/2)").unwrap();
        let sm = g.sampled(0.25, -40, 81, 6.0);
        for (w, v) in sm.dtft.iter().step_by(97) {
            assert!((v.re - TAU.sqrt() * (-w * w / 2.0).exp()).abs() < 1e-9 && v.im.abs() < 1e-9);
        }
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
