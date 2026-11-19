//! Adaptive Gauss–Kronrod (7–15 point) integration of a batched integrand.

const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_5,
    0.0,
];
const WGK: [f64; 8] = [
    0.022_935_322_010_529_22,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_18,
    0.140_653_259_715_525_92,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_83,
];
/// Gauss weights of XGK[1], XGK[3], XGK[5], XGK[7].
const WG: [f64; 4] = [0.129_484_966_168_869_7, 0.279_705_391_489_276_7, 0.381_830_050_505_118_9, 0.417_959_183_673_469_4];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Integral {
    pub value: f64,
    /// Estimated absolute error.
    pub error: f64,
    /// The integrand was finite everywhere it was sampled.
    pub finite: bool,
    /// The error estimate met the tolerance.
    pub converged: bool,
}

/// Kronrod and Gauss estimates on [a, b] for each interval, evaluated in
/// one batch.
fn rules(h: &dyn Fn(&[f64]) -> Vec<f64>, ivs: &[(f64, f64)]) -> Vec<(f64, f64, bool)> {
    let mut us = Vec::with_capacity(ivs.len() * 15);
    for &(a, b) in ivs {
        let (c, r) = ((a + b) * 0.5, (b - a) * 0.5);
        for k in 0..7 {
            us.push(c - r * XGK[k]);
            us.push(c + r * XGK[k]);
        }
        us.push(c);
    }
    let v = h(&us);
    ivs.iter()
        .enumerate()
        .map(|(n, &(a, b))| {
            let f = &v[n * 15..n * 15 + 15];
            let r = (b - a) * 0.5;
            let mut k = WGK[7] * f[14];
            let mut g = WG[3] * f[14];
            for j in 0..7 {
                let pair = f[2 * j] + f[2 * j + 1];
                k += WGK[j] * pair;
                if j % 2 == 1 {
                    g += WG[j / 2] * pair;
                }
            }
            (k * r, (k - g).abs() * r, f.iter().all(|x| x.is_finite()))
        })
        .collect()
}

/// ∫ₐᵇ h(u) du to a relative accuracy of about `rel`.
pub fn integrate(h: &dyn Fn(&[f64]) -> Vec<f64>, a: f64, b: f64, rel: f64) -> Integral {
    if !(a.is_finite() && b.is_finite()) {
        return Integral { value: f64::NAN, error: f64::NAN, finite: false, converged: false };
    }
    if a == b {
        return Integral { value: 0.0, error: 0.0, finite: true, converged: true };
    }
    // Start from 16 pieces so narrow features are not missed.
    let n0 = 16;
    let mut ivs: Vec<(f64, f64, f64, f64)> = Vec::new();
    let start: Vec<(f64, f64)> = (0..n0).map(|i| (a + (b - a) * i as f64 / n0 as f64, a + (b - a) * (i + 1) as f64 / n0 as f64)).collect();
    let mut finite = true;
    for (iv, (k, e, ok)) in start.iter().zip(rules(h, &start)) {
        finite &= ok;
        ivs.push((iv.0, iv.1, k, e));
    }
    for _ in 0..60 {
        let total: f64 = ivs.iter().map(|v| v.2).sum();
        let err: f64 = ivs.iter().map(|v| v.3).sum();
        let tol = rel * total.abs().max(1e-12 * (b - a).abs());
        if err <= tol || ivs.len() > 4000 || !finite {
            break;
        }
        // Split every interval carrying more than its share of the error.
        let share = tol / ivs.len() as f64;
        let mut split = Vec::new();
        let mut keep = Vec::new();
        for v in ivs {
            if v.3 > share && (v.1 - v.0).abs() > 1e-14 * (b - a).abs() {
                let m = 0.5 * (v.0 + v.1);
                split.push((v.0, m));
                split.push((m, v.1));
            } else {
                keep.push(v);
            }
        }
        if split.is_empty() {
            ivs = keep;
            break;
        }
        for (iv, (k, e, ok)) in split.iter().zip(rules(h, &split)) {
            finite &= ok;
            keep.push((iv.0, iv.1, k, e));
        }
        ivs = keep;
    }
    let value: f64 = ivs.iter().map(|v| v.2).sum();
    let error: f64 = ivs.iter().map(|v| v.3).sum();
    let converged = finite && error <= 1e3 * rel * value.abs().max(1e-12 * (b - a).abs());
    Integral { value, error, finite, converged }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_integrals() {
        let sq = |u: &[f64]| u.iter().map(|x| x * x).collect();
        let r = integrate(&sq, 0.0, 1.0, 1e-12);
        assert!((r.value - 1.0 / 3.0).abs() < 1e-14 && r.converged);
        // √x has an infinite slope at 0.
        let rt = |u: &[f64]| u.iter().map(|x| x.sqrt()).collect();
        let r = integrate(&rt, 0.0, 1.0, 1e-10);
        assert!((r.value - 2.0 / 3.0).abs() < 1e-9, "{}", r.value);
        // |x − 1/3| with a kink.
        let ab = |u: &[f64]| u.iter().map(|x| (x - 1.0 / 3.0).abs()).collect();
        let r = integrate(&ab, 0.0, 1.0, 1e-10);
        assert!((r.value - 5.0 / 18.0).abs() < 1e-9);
        // 1/x through 0 does not converge.
        let inv = |u: &[f64]| u.iter().map(|x| 1.0 / x).collect();
        let r = integrate(&inv, -1.0, 2.0, 1e-10);
        assert!(!r.converged);
    }
}
