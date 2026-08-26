//! Port of backend/scripts/services/rendering/source_cleanup/pdf/hit_test.py.
//!
//! `RectTuple` is a `[x0, y0, x1, y1]` slice (PyMuPDF rect order, same as
//! `rendering_core::rect::Rect`). The index sorts rects by y0 and bisects on it
//! to avoid scanning rects that start below the probe; `limit(y)` is
//! `bisect_right(y0_sorted, y)`.

/// `RectTuple = tuple[float, float, float, float]` — (x0, y0, x1, y1).
pub type RectTuple = [f64; 4];

pub const MIN_PROTECTED_OVERLAP_AREA_PT2: f64 = 1.0;
pub const MIN_PROTECTED_OVERLAP_TEXT_RATIO: f64 = 0.15;
pub const MIN_PROTECTED_OVERLAP_HEIGHT_RATIO: f64 = 0.2;

#[derive(Debug, Clone)]
pub struct RectIndex {
    /// Normalized, non-degenerate rects sorted by y0.
    pub rects: Vec<RectTuple>,
    /// y0 of each rect in `rects` (ascending).
    y0_sorted: Vec<f64>,
    /// Union bounds of all rects, or None when empty.
    bounds: Option<RectTuple>,
}

impl RectIndex {
    /// `RectIndex.build(rects)` — drop degenerate rects, sort by y0.
    pub fn build(rects: impl IntoIterator<Item = RectTuple>) -> Self {
        let mut normalized: Vec<RectTuple> = rects
            .into_iter()
            .filter(|r| r[0] < r[2] && r[1] < r[3])
            .collect();
        normalized.sort_by(|a, b| a[1].partial_cmp(&b[1]).unwrap());
        if normalized.is_empty() {
            return RectIndex {
                rects: Vec::new(),
                y0_sorted: Vec::new(),
                bounds: None,
            };
        }
        let y0_sorted: Vec<f64> = normalized.iter().map(|r| r[1]).collect();
        let bounds = [
            normalized.iter().map(|r| r[0]).fold(f64::INFINITY, f64::min),
            normalized.iter().map(|r| r[1]).fold(f64::INFINITY, f64::min),
            normalized.iter().map(|r| r[2]).fold(f64::NEG_INFINITY, f64::max),
            normalized.iter().map(|r| r[3]).fold(f64::NEG_INFINITY, f64::max),
        ];
        RectIndex {
            rects: normalized,
            y0_sorted,
            bounds: Some(bounds),
        }
    }

    /// Number of rects with y0 <= `y` (`bisect_right` on y0_sorted).
    fn limit(&self, y: f64) -> usize {
        self.y0_sorted.partition_point(|&v| v <= y)
    }

    /// `contains_point(x, y)` — any stored rect contains the point.
    pub fn contains_point(&self, x: f64, y: f64) -> bool {
        let Some(bounds) = self.bounds else {
            return false;
        };
        if !point_in_rect(x, y, &bounds) {
            return false;
        }
        let limit = self.limit(y);
        for i in 0..limit {
            let rect = &self.rects[i];
            if rect[3] < y {
                continue;
            }
            if point_in_rect(x, y, rect) {
                return true;
            }
        }
        false
    }

    /// `intersects(rect)` — any stored rect strictly intersects the given one.
    pub fn intersects(&self, rect: &RectTuple) -> bool {
        let Some(bounds) = self.bounds else {
            return false;
        };
        if !rect_intersects(rect, &bounds) {
            return false;
        }
        let limit = self.limit(rect[3]);
        for i in 0..limit {
            let candidate = &self.rects[i];
            if candidate[3] < rect[1] {
                continue;
            }
            if rect_intersects(rect, candidate) {
                return true;
            }
        }
        false
    }

    /// `contains_point_or_intersects(x, y, rect)`.
    pub fn contains_point_or_intersects(&self, x: f64, y: f64, rect: &RectTuple) -> bool {
        let Some(bounds) = self.bounds else {
            return false;
        };
        let point_may_match = point_in_rect(x, y, &bounds);
        let rect_may_match = rect_intersects(rect, &bounds);
        if !point_may_match && !rect_may_match {
            return false;
        }
        let limit = self.limit(y.max(rect[3]));
        for i in 0..limit {
            let candidate = &self.rects[i];
            if point_may_match && candidate[3] >= y && point_in_rect(x, y, candidate) {
                return true;
            }
            if rect_may_match && candidate[3] >= rect[1] && rect_intersects(rect, candidate) {
                return true;
            }
        }
        false
    }

    /// `matches_text_for_removal(x, y, rect)` — point hit or substantial rect
    /// overlap with a stored strip rect.
    pub fn matches_text_for_removal(&self, x: f64, y: f64, rect: &RectTuple) -> bool {
        let Some(bounds) = self.bounds else {
            return false;
        };
        let point_may_match = point_in_rect(x, y, &bounds);
        let rect_may_match = rect_intersects(rect, &bounds);
        if !point_may_match && !rect_may_match {
            return false;
        }
        let limit = self.limit(y.max(rect[3]));
        for i in 0..limit {
            let candidate = &self.rects[i];
            if candidate[3] < y.min(rect[1]) {
                continue;
            }
            if point_may_match && point_in_rect(x, y, candidate) {
                return true;
            }
            if rect_may_match && rect_substantially_overlaps_text(rect, candidate) {
                return true;
            }
        }
        false
    }

    /// `protects_text_rect(x, y, rect)` — point hit or substantial overlap.
    pub fn protects_text_rect(&self, x: f64, y: f64, rect: &RectTuple) -> bool {
        let Some(bounds) = self.bounds else {
            return false;
        };
        let point_may_match = point_in_rect(x, y, &bounds);
        let rect_may_match = rect_intersects(rect, &bounds);
        if !point_may_match && !rect_may_match {
            return false;
        }
        let limit = self.limit(y.max(rect[3]));
        for i in 0..limit {
            let candidate = &self.rects[i];
            if candidate[3] < y.min(rect[1]) {
                continue;
            }
            if point_in_rect(x, y, candidate) || rect_substantially_overlaps_text(rect, candidate) {
                return true;
            }
        }
        false
    }
}

/// `inside_any_rect(x, y, rects)`.
pub fn inside_any_rect(x: f64, y: f64, rects: &[RectTuple]) -> bool {
    RectIndex::build(rects.iter().copied()).contains_point(x, y)
}

/// `intersects_any_rect(rect, rects)`.
pub fn intersects_any_rect(rect: &RectTuple, rects: &[RectTuple]) -> bool {
    RectIndex::build(rects.iter().copied()).intersects(rect)
}

/// `is_protected_text_op(user_point, text_rect, protected_rects, protected_index)`.
pub fn is_protected_text_op(
    user_point: (f64, f64),
    text_rect: &RectTuple,
    protected_rects: Option<&[RectTuple]>,
    protected_index: Option<&RectIndex>,
) -> bool {
    let index = match protected_index {
        Some(index) => index.clone(),
        None => RectIndex::build(protected_rects.unwrap_or(&[]).iter().copied()),
    };
    if index.rects.is_empty() {
        return false;
    }
    index.protects_text_rect(user_point.0, user_point.1, text_rect)
}

fn point_in_rect(x: f64, y: f64, rect: &RectTuple) -> bool {
    rect[0] <= x && x <= rect[2] && rect[1] <= y && y <= rect[3]
}

fn rect_intersects(left: &RectTuple, right: &RectTuple) -> bool {
    left[0] < right[2] && left[2] > right[0] && left[1] < right[3] && left[3] > right[1]
}

fn rect_intersection(left: &RectTuple, right: &RectTuple) -> RectTuple {
    [
        left[0].max(right[0]),
        left[1].max(right[1]),
        left[2].min(right[2]),
        left[3].min(right[3]),
    ]
}

fn rect_area(rect: &RectTuple) -> f64 {
    (rect[2] - rect[0]).max(0.0) * (rect[3] - rect[1]).max(0.0)
}

fn rect_substantially_overlaps_text(text_rect: &RectTuple, protected_rect: &RectTuple) -> bool {
    let overlap = rect_intersection(text_rect, protected_rect);
    let overlap_area = rect_area(&overlap);
    if overlap_area < MIN_PROTECTED_OVERLAP_AREA_PT2 {
        return false;
    }
    let text_area = rect_area(text_rect).max(0.001);
    let overlap_height = (overlap[3] - overlap[1]).max(0.0);
    let text_height = (text_rect[3] - text_rect[1]).max(0.001);
    overlap_area / text_area >= MIN_PROTECTED_OVERLAP_TEXT_RATIO
        && overlap_height / text_height >= MIN_PROTECTED_OVERLAP_HEIGHT_RATIO
}
