//! Small numeric helpers shared by the Typst emitter modules.

/// Python `round(x, 2)`.
///
/// CPython's `float_round` (default `_PY_SHORT_FLOAT_REPR` path) rounds the
/// *exact binary value* to `ndigits` decimal places with ties-to-even, via a
/// correctly-rounded decimal conversion. It is **not** `(x * 100).round() /
/// 100`: scaling by a double first can round a sub-tie value onto the tie
/// (e.g. `7.745 * 100.0` is exactly `774.5` while the true product is slightly
/// above, so CPython rounds up to `7.75`). We reproduce it with exact integer
/// arithmetic on the mantissa/exponent decomposition.
pub fn round_2dp(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    // Values with no fractional part round to themselves.
    if x.abs() >= 4_503_599_627_370_496.0 {
        return x;
    }
    let negative = x.is_sign_negative();
    let a = x.abs();
    let bits = a.to_bits();
    let exp_bits = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    let (mantissa, exponent): (u64, i32) = if exp_bits == 0 {
        (frac, -1074)
    } else {
        (frac | 0x0010_0000_0000_0000, exp_bits - 1075)
    };
    // a * 100 = mantissa * 100 * 2^exponent. Find the nearest integer with
    // ties-to-even.
    let scaled_num: u128 = (mantissa as u128) * 100;
    let integer: u128 = if exponent >= 0 {
        scaled_num << exponent
    } else {
        let shift = (-exponent) as u32;
        let den: u128 = 1u128 << shift;
        let q = scaled_num >> shift;
        let r = scaled_num & (den - 1);
        if r == 0 {
            q
        } else {
            let two_r = r << 1;
            if two_r < den {
                q
            } else if two_r > den {
                q + 1
            } else {
                // Tie: round to even.
                q + (q & 1)
            }
        }
    };
    let mut out = (integer as f64) / 100.0;
    if negative {
        out = -out;
    }
    out
}

/// Format a float like Python `repr` for the common emitter range: shortest
/// round-trip decimal, keeping a trailing `.0` on integral values (Python
/// `repr(8.0)` is `"8.0"`, Rust's `{}` would print `"8"`).
pub fn fmt_f(x: f64) -> String {
    let s = format!("{x}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Python-`re` `\s` predicate (ASCII whitespace).
pub fn is_py_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c' | '\x0b')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_2dp_matches_cpython() {
        // Empirically captured from CPython 3.14 `round(x, 2)` (correctly
        // rounded decimal of the exact binary value, ties-to-even).
        let cases: &[(f64, f64)] = &[
            (7.745, 7.75),
            (0.305, 0.3),
            (0.015, 0.01),
            (0.005, 0.01),
            (0.125, 0.12),
            (2.675, 2.67),
            (2.5, 2.5),
            (-1.5, -1.5),
            (-0.005, -0.01),
            (-7.745, -7.75),
            (-2.5, -2.5),
            (12.0, 12.0),
            (0.075, 0.07),
            (0.025, 0.03),
            (0.095, 0.1),
            (1.005, 1.0),
            (4.995, 5.0),
            (3.14159, 3.14),
            (0.045, 0.04),
            (0.065, 0.07),
            (0.105, 0.1),
            (0.255, 0.26),
            (0.355, 0.35),
            (0.25, 0.25),
            (0.335, 0.34),
            (0.365, 0.36),
            (0.0, 0.0),
        ];
        for &(input, expected) in cases {
            assert_eq!(
                round_2dp(input),
                expected,
                "round_2dp({input}) != {expected}"
            );
        }
    }

    #[test]
    fn fmt_f_keeps_trailing_dot_zero() {
        assert_eq!(fmt_f(8.0), "8.0");
        assert_eq!(fmt_f(8.5), "8.5");
        assert_eq!(fmt_f(0.0), "0.0");
        assert_eq!(fmt_f(-3.25), "-3.25");
    }
}
