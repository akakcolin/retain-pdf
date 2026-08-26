// Port of Python's `round(x, ndigits)` — banker's rounding (round half to even).
// Rust's `f64::round` rounds half away from zero, so it cannot be used directly.
//
// CPython rounds the *exact binary value* of x, not the scaled double. Naive
// `round_half_even(x * 10^n) / 10^n` double-rounds: e.g. `2.675 * 100.0 == 267.5`
// exactly, yet `round(2.675, 2)` is `2.67` because the true value is
// 2.6749999... < 2.675. So we round via exact rational arithmetic:
//   x = m * 2^e  (m: 53-bit integer mantissa)
//   x * 10^n     = m * 2^(e+n) * 5^n          (n >= 0)
//   x / 10^|n|   = m * 2^(e-n) / 5^|n|        (n < 0)
// and round-half-even using integer comparisons against the exact half point.
pub fn py_round(value: f64, ndigits: i32) -> f64 {
    if !value.is_finite() || value == 0.0 {
        return value;
    }
    let (sign, m, e) = decompose(value);
    if ndigits >= 0 {
        let n = ndigits;
        let five_n = match pow5(n) {
            Some(v) => v,
            None => return round_naive(value, n),
        };
        let num = match (m as i128).checked_mul(five_n) {
            Some(v) => v,
            None => return round_naive(value, n),
        };
        // value * 10^n = sign * num * 2^(e+n)
        let k = e + n;
        match round_half_even_shift(num, k) {
            Some(rounded) => sign * rounded as f64 / 10f64.powi(n),
            None => round_naive(value, n),
        }
    } else {
        let a = -ndigits;
        let five_a = match pow5(a) {
            Some(v) => v,
            None => return round_naive(value, ndigits),
        };
        // value / 10^a = sign * m * 2^(e-a) / 5^a
        let k = e - a;
        match round_half_even_denom(m as i128, k, five_a) {
            Some(rounded) => sign * rounded as f64 * 10f64.powi(a),
            None => round_naive(value, ndigits),
        }
    }
}

/// Decompose a positive finite double into `sign * m * 2^e` with m an integer
/// mantissa (u64, up to 2^53).
fn decompose(value: f64) -> (f64, u64, i32) {
    let bits = value.to_bits();
    let sign = if bits >> 63 == 0 { 1.0 } else { -1.0 };
    let exp_field = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    let (m, e) = if exp_field == 0 {
        // Denormal: value = frac * 2^-1074.
        (frac, -1074)
    } else {
        // Normal: implicit leading 1 bit; value = (1.frac) * 2^(exp_field-1023).
        (frac | (1u64 << 52), exp_field - 1075)
    };
    (sign, m, e)
}

/// 5^n as i128, or None on overflow.
fn pow5(n: i32) -> Option<i128> {
    let mut result: i128 = 1;
    for _ in 0..n {
        result = result.checked_mul(5)?;
    }
    Some(result)
}

/// Round `num * 2^k` to the nearest integer, ties-to-even. num > 0.
/// Returns None when the exact integer does not fit in i128 (caller falls back).
fn round_half_even_shift(num: i128, k: i32) -> Option<i128> {
    if k >= 0 {
        if k >= 127 {
            return None;
        }
        return num.checked_shl(k as u32);
    }
    let shift = (-(k as i64)) as u32;
    if shift >= 127 {
        return None;
    }
    let q = num >> shift;
    let r = num & ((1i128 << shift) - 1);
    let half = 1i128 << (shift - 1);
    if r > half || (r == half && (q & 1) == 1) {
        Some(q + 1)
    } else {
        Some(q)
    }
}

/// Round `num * 2^k / den` to the nearest integer, ties-to-even.
/// num, den > 0. Returns None on i128 overflow.
fn round_half_even_denom(num: i128, k: i32, den: i128) -> Option<i128> {
    if k >= 0 {
        if k >= 127 {
            return None;
        }
        let numerator = num.checked_shl(k as u32)?;
        let q = numerator / den;
        let r = numerator % den;
        if 2 * r > den || (2 * r == den && (q & 1) == 1) {
            Some(q + 1)
        } else {
            Some(q)
        }
    } else {
        let shift = (-(k as i64)) as u32;
        if shift >= 127 {
            return None;
        }
        let den2 = den.checked_shl(shift as u32)?;
        let q = num / den2;
        let r = num % den2;
        if 2 * r > den2 || (2 * r == den2 && (q & 1) == 1) {
            Some(q + 1)
        } else {
            Some(q)
        }
    }
}

/// Fallback for exotic inputs (denormals, huge ndigits, i128 overflow):
/// scale in double, round half-to-even the scaled value, unscale.
fn round_naive(value: f64, ndigits: i32) -> f64 {
    let factor = 10f64.powi(ndigits.abs());
    let scaled = if ndigits >= 0 {
        value * factor
    } else {
        value / factor
    };
    let rounded: f64 = if scaled.is_finite() {
        let (sign, m, e) = decompose(scaled.abs());
        let r = round_half_even_shift(m as i128, e).unwrap_or(m as i128);
        if sign < 0.0 { -(r as f64) } else { r as f64 }
    } else {
        scaled
    };
    if ndigits >= 0 {
        rounded / factor
    } else {
        rounded * factor
    }
}

/// Python `statistics.median` — sorts, and for even-length lists averages the
/// two middle values. Assumes `values` is already sorted ascending.
pub fn median_sorted_f64(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// `statistics.median` over unsorted f64 values (filters nothing).
pub fn median_f64(values: &[f64]) -> f64 {
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    median_sorted_f64(&sorted)
}

/// `statistics.median` over unsorted integer values.
pub fn median_usize(values: &[usize]) -> f64 {
    let mut sorted: Vec<usize> = values.to_vec();
    sorted.sort_unstable();
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2] as f64
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) as f64 / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values captured from CPython.
    #[test]
    fn matches_cpython_reference() {
        let cases: &[(f64, i32, f64)] = &[
            (2.675, 2, 2.67),
            (2.675, 3, 2.675),
            (1.005, 2, 1.0),
            (3.14159, 3, 3.142),
            (3.14159, 2, 3.14),
            (1.2345, 2, 1.23),
            (1.235, 2, 1.24),
            (1.245, 2, 1.25),
            (1.25, 2, 1.25),
            (1.255, 2, 1.25),
            (1.35, 1, 1.4),
            (2.5, 0, 2.0),
            (3.5, 0, 4.0),
            (-2.5, 0, -2.0),
            (1.5, 0, 2.0),
            (0.5, 0, 0.0),
            (267.5, 0, 268.0),
            (12345.0, -2, 12300.0),
            (12350.0, -2, 12400.0),
            (12450.0, -2, 12400.0),
            (12500.0, -2, 12500.0),
            (0.5, 1, 0.5),
            (0.25, 1, 0.2),
            (0.35, 1, 0.3),
            (0.45, 1, 0.5),
            (0.05, 1, 0.1),
            (0.15, 1, 0.1),
            (11.4, 2, 11.4),
            (8.5, 1, 8.5),
            (123.456, 3, 123.456),
            (12.35, 1, 12.3),
            (12.45, 1, 12.4),
            (12.55, 1, 12.6),
            (0.125, 2, 0.12),
            (0.135, 2, 0.14),
            (0.145, 2, 0.14),
        ];
        for &(v, n, expected) in cases {
            let got = py_round(v, n);
            assert_eq!(got, expected, "round({v}, {n})");
        }
    }

    #[test]
    fn trivial() {
        assert_eq!(py_round(0.0, 2), 0.0);
        assert_eq!(py_round(-0.0, 2), -0.0);
        assert_eq!(py_round(1.0, 0), 1.0);
        assert_eq!(py_round(3.7, 0), 4.0);
        assert!(py_round(f64::INFINITY, 2).is_infinite());
        assert!(py_round(f64::NAN, 2).is_nan());
    }
}
