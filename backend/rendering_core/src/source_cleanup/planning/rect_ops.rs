//! Ports of `planning/rects.py::merge_rects` and `planning/geometry.py::rect_tuple`.

use crate::rect::{rect_key, round_to_digits, Rect};

/// `rect_tuple(rect)` — rounded to 3 decimals (Python `round(x, 3)`).
pub fn rect_tuple(rect: &Rect) -> [f64; 4] {
    [
        round_to_digits(rect.x0, 3),
        round_to_digits(rect.y0, 3),
        round_to_digits(rect.x1, 3),
        round_to_digits(rect.y1, 3),
    ]
}

/// `merge_rects(rects)` from `planning/rects.py` — drop empty / area<=0.5,
/// dedup by `rect_key`, sort by `(round(y0,2), round(x0,2), round(y1,2))`.
pub fn merge_rects(rects: impl IntoIterator<Item = Rect>) -> Vec<Rect> {
    let mut deduped: std::collections::BTreeMap<(i64, i64, i64, i64), Rect> =
        std::collections::BTreeMap::new();
    for rect in rects {
        if rect.is_empty() || rect.area() <= 0.5 {
            continue;
        }
        deduped.entry(rect_key(&rect)).or_insert(rect);
    }
    let mut values: Vec<Rect> = deduped.into_values().collect();
    values.sort_by(|a, b| {
        let ka = (
            round_to_digits(a.y0, 2),
            round_to_digits(a.x0, 2),
            round_to_digits(a.y1, 2),
        );
        let kb = (
            round_to_digits(b.y0, 2),
            round_to_digits(b.x0, 2),
            round_to_digits(b.y1, 2),
        );
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    });
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_rects_dedups_by_key() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(0.001, 0.0, 10.0, 10.0); // same rounded key
        let merged = merge_rects(vec![a, b]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0], a);
    }

    #[test]
    fn merge_rects_drops_small() {
        let tiny = Rect::new(0.0, 0.0, 0.5, 0.5); // area 0.25 <= 0.5
        let merged = merge_rects(vec![tiny]);
        assert!(merged.is_empty());
    }

    #[test]
    fn rect_tuple_rounds_to_3() {
        let r = Rect::new(1.23456, 2.34567, 3.45678, 4.56789);
        assert_eq!(rect_tuple(&r), [1.235, 2.346, 3.457, 4.568]);
    }
}
