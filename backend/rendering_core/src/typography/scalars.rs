// Port of services/rendering/layout/typography/scalars.py.

pub fn clamp(value: f64, min_value: f64, max_value: f64) -> f64 {
    value.max(min_value).min(max_value)
}

pub fn percentile_value(values: &[f64], q: f64) -> f64 {
    let mut filtered: Vec<f64> = values.iter().cloned().filter(|v| *v > 0.0).collect();
    filtered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if filtered.is_empty() {
        return 0.0;
    }
    if filtered.len() == 1 {
        return filtered[0];
    }
    let q = clamp(q, 0.0, 1.0);
    let pos = (filtered.len() - 1) as f64 * q;
    let low = pos as usize;
    let high = (filtered.len() - 1).min(low + 1);
    let frac = pos - low as f64;
    filtered[low] * (1.0 - frac) + filtered[high] * frac
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_matches_python() {
        assert_eq!(percentile_value(&[], 0.42), 0.0);
        assert_eq!(percentile_value(&[3.0], 0.9), 3.0);
        assert_eq!(percentile_value(&[0.0, -1.0], 0.5), 0.0); // all filtered
        let vals = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        assert_eq!(percentile_value(&vals, 0.42), 3.94);
        assert_eq!(percentile_value(&vals, 0.0), 1.0);
        assert_eq!(percentile_value(&vals, 1.0), 8.0);
    }

    #[test]
    fn clamp_basic() {
        assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(-1.0, 0.0, 1.0), 0.0);
        assert_eq!(clamp(0.5, 0.0, 1.0), 0.5);
    }
}
