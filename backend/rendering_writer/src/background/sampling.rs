//! Port of `source/background/sampling.py`: the ordered-data quantile that
//! every brightness/channel sampler builds on. Indexing uses Python `round`
//! (round-half-even), so a synthetic pixel run produced by fitz and by the Rust
//! port selects identical sample positions.

/// Python `round(x)` with no digit argument — round-half-even on the exact
/// binary value. (This is *not* `round(x, 2)`, which rounds the exact binary
/// value at a fixed decimal place.)
fn py_round(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// `sampling.py::quantile` — value at the `numerator/denominator` rank over
/// `sorted_values`. Empty input yields 255 (safe white); the rank index is
/// clamped into `[0, len-1]`.
pub fn quantile(sorted_values: &[u8], numerator: i64, denominator: i64) -> u8 {
    if sorted_values.is_empty() {
        return 255;
    }
    let index = py_round((sorted_values.len() - 1) as f64 * numerator as f64 / denominator as f64);
    let index = index.clamp(0, sorted_values.len() as i64 - 1) as usize;
    sorted_values[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantile_matches_python() {
        assert_eq!(quantile(&[], 1, 2), 255);
        // median of 4 → round((4-1)*1/2)=round(1.5)=2 (ties-to-even) → 30
        assert_eq!(quantile(&[10, 20, 30, 40], 1, 2), 30);
        assert_eq!(quantile(&[10, 20, 30], 1, 2), 20);
        let range: Vec<u8> = (0..100).collect();
        assert_eq!(quantile(&range, 9, 10), 89); // round(89.1)=89
        assert_eq!(quantile(&range, 1, 10), 10); // round(9.9)=10
        assert_eq!(quantile(&[7], 9, 10), 7);
    }
}
