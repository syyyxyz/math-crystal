//! Recognising simple closed forms of computed numbers (π/2, 1/3, √2/2)
//! and labels for ticks in multiples of π.

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

fn near_int(v: f64, tol: f64) -> Option<i64> {
    let r = v.round();
    ((v - r).abs() <= tol && r.abs() < 1e6).then_some(r as i64)
}

/// `k·name/q` in lowest terms: `π/2`, `-3π/4`, `2√3`, `√2/2`.
fn multiple(k: i64, q: i64, name: &str) -> String {
    let g = gcd(k, q).max(1);
    let (k, q) = (k / g, q / g);
    let (sign, k) = (if k < 0 { "−" } else { "" }, k.abs());
    let num = if k == 1 { name.to_string() } else { format!("{k}{name}") };
    if q == 1 { format!("{sign}{num}") } else { format!("{sign}{num}/{q}") }
}

/// The simple number near `v` (within `tol`) and how to write it: k·π/q,
/// p/q or k·√n/q with small integers. Integers give None.
pub fn find(v: f64, tol: f64) -> Option<(f64, String)> {
    if !v.is_finite() || near_int(v, tol).is_some() {
        return None;
    }
    let pi = std::f64::consts::PI;
    for q in [1i64, 2, 3, 4, 6, 8, 12] {
        if let Some(k) = near_int(v * q as f64 / pi, tol * q as f64 / pi)
            && k != 0
            && k.abs() <= 60
        {
            return Some((k as f64 * pi / q as f64, multiple(k, q, "π")));
        }
    }
    for q in 2i64..=12 {
        if let Some(p) = near_int(v * q as f64, tol * q as f64)
            && p.abs() <= 1200
        {
            let g = gcd(p, q);
            let (p, q) = (p / g, q / g);
            if q == 1 {
                return None;
            }
            return Some((p as f64 / q as f64, format!("{}{}/{q}", if p < 0 { "−" } else { "" }, p.abs())));
        }
    }
    for n in [2i64, 3, 5, 6, 7, 10] {
        let s = (n as f64).sqrt();
        for q in [1i64, 2, 3, 4, 6] {
            if let Some(k) = near_int(v * q as f64 / s, tol * q as f64 / s)
                && k != 0
                && k.abs() <= 24
            {
                return Some((k as f64 * s / q as f64, multiple(k, q, &format!("√{n}"))));
            }
        }
    }
    None
}

/// A short exact form of `v` when it is within `tol` of a simple number
/// (see [`find`]).
pub fn closed_form_tol(v: f64, tol: f64) -> Option<String> {
    find(v, tol).map(|f| f.1)
}

/// `v` moved onto a nearby integer or simple number (within `tol`).
pub fn snap(v: f64, tol: f64) -> f64 {
    if let Some(k) = near_int(v, tol) {
        return k as f64;
    }
    find(v, tol).map_or(v, |f| f.0)
}

/// [`closed_form_tol`] for numbers computed to near machine precision.
pub fn closed_form(v: f64) -> Option<String> {
    closed_form_tol(v, 1e-9 * v.abs().max(1.0))
}

/// A value with up to six decimals, and its exact form when recognised:
/// `1.570796 (π/2)`.
pub fn show(v: f64) -> String {
    let base = crate::app::fmt_val(v);
    match closed_form(v) {
        Some(c) => format!("{base} ({c})"),
        None => base,
    }
}

/// Compact: the exact form when there is one, else four decimals.
pub fn short(v: f64) -> String {
    if let Some(c) = closed_form(v) {
        return c;
    }
    if v != 0.0 && (v.abs() >= 1e5 || v.abs() < 1e-3) {
        return format!("{v:.3e}");
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.replace('-', "−") }
}

/// Ticks at multiples of π / q for a range: (values, label for each).
pub fn pi_ticks(lo: f64, hi: f64, target: usize) -> Vec<(f64, String)> {
    let pi = std::f64::consts::PI;
    if !(hi > lo) || !lo.is_finite() || !hi.is_finite() {
        return vec![];
    }
    let raw = (hi - lo) / target.max(1) as f64 / pi;
    // Steps in units of π: 1/12 … 1/2, then 1, 2, 5, 10, …
    let mut steps: Vec<(i64, i64)> = vec![(1, 12), (1, 6), (1, 4), (1, 3), (1, 2), (1, 1)];
    let mut m = 1i64;
    while m < 1_000_000 {
        steps.extend([(2 * m, 1), (5 * m, 1), (10 * m, 1)]);
        m *= 10;
    }
    let (n, q) = steps.into_iter().find(|(n, q)| *n as f64 / *q as f64 >= raw).unwrap_or((1_000_000, 1));
    let step = n as f64 / q as f64 * pi;
    let first = (lo / step).ceil() as i64;
    let last = (hi / step).floor() as i64;
    if last - first > 400 {
        return vec![];
    }
    (first..=last)
        .map(|k| {
            let v = k as f64 * step;
            let label = if k == 0 { "0".to_string() } else { multiple(k * n, q, "π") };
            (v, label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_forms() {
        let pi = std::f64::consts::PI;
        assert_eq!(closed_form(pi / 2.0).as_deref(), Some("π/2"));
        assert_eq!(closed_form(-3.0 * pi / 4.0).as_deref(), Some("−3π/4"));
        assert_eq!(closed_form(2.0 * pi).as_deref(), Some("2π"));
        assert_eq!(closed_form(1.0 / 3.0).as_deref(), Some("1/3"));
        assert_eq!(closed_form(-2.5).as_deref(), Some("−5/2"));
        assert_eq!(closed_form(2f64.sqrt() / 2.0).as_deref(), Some("√2/2"));
        assert_eq!(closed_form(2.0 * 3f64.sqrt()).as_deref(), Some("2√3"));
        assert_eq!(closed_form(3.0), None);
        assert_eq!(closed_form(0.123456789), None);
        assert_eq!(short(1.23456789), "1.2346");
    }

    #[test]
    fn pi_tick_labels() {
        let pi = std::f64::consts::PI;
        let t = pi_ticks(-pi - 0.01, pi + 0.01, 5);
        let labels: Vec<&str> = t.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(labels, vec!["−π", "−π/2", "0", "π/2", "π"]);
    }
}
