//! Port of `planning/spatial_index.py::RectOverlapIndex` — y0-sorted overlap
//! probe (bisect on `y0`, then y/x sweep).

use crate::rect::Rect;

#[derive(Debug, Clone)]
pub struct RectOverlapIndex {
    pub rects: Vec<Rect>,
    pub y0_sorted: Vec<f64>,
}

impl RectOverlapIndex {
    /// `RectOverlapIndex.build(rects)` — sort non-empty rects by `y0`.
    pub fn build(rects: impl IntoIterator<Item = Rect>) -> Self {
        let mut ordered: Vec<Rect> = rects.into_iter().filter(|r| !r.is_empty()).collect();
        ordered.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
        let y0_sorted = ordered.iter().map(|r| r.y0).collect();
        RectOverlapIndex { rects: ordered, y0_sorted }
    }

    /// `overlaps_any(target, min_overlap_area)` — bisect on y0, then x sweep.
    pub fn overlaps_any(&self, target_rect: &Rect, min_overlap_area: f64) -> bool {
        if target_rect.is_empty() || self.rects.is_empty() {
            return false;
        }
        let limit = bisect_right(&self.y0_sorted, target_rect.y1);
        for index in 0..limit {
            let rect = &self.rects[index];
            if rect.y1 < target_rect.y0 {
                continue;
            }
            if rect.x1 < target_rect.x0 || rect.x0 > target_rect.x1 {
                continue;
            }
            if rect.intersect(target_rect).area() > min_overlap_area {
                return true;
            }
        }
        false
    }
}

/// Python `bisect_right(sorted_values, x)` — count of elements `<= x`.
pub fn bisect_right(values: &[f64], x: f64) -> usize {
    values.partition_point(|value| *value <= x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bisect_right_counts_le() {
        let values = vec![1.0, 2.0, 2.0, 3.0];
        assert_eq!(bisect_right(&values, 2.0), 3);
        assert_eq!(bisect_right(&values, 2.5), 3);
        assert_eq!(bisect_right(&values, 0.0), 0);
    }

    #[test]
    fn overlaps_any_detects() {
        let index = RectOverlapIndex::build(vec![Rect::new(10.0, 10.0, 20.0, 20.0)]);
        assert!(index.overlaps_any(&Rect::new(15.0, 15.0, 25.0, 25.0), 0.0));
        assert!(!index.overlaps_any(&Rect::new(0.0, 0.0, 5.0, 5.0), 0.0));
    }
}
