//! Port of `planning/rect_filter.py` — unsafe-vector overlap probe.

use crate::rect::Rect;
use crate::source_cleanup::planning::spatial_index::RectOverlapIndex;

pub const MIN_UNSAFE_VECTOR_OVERLAP_AREA_PT2: f64 = 0.5;

/// `rect_overlaps_any_unsafe_vector` — min overlap area > 0.5pt2 over the
/// unsafe-vector index.
pub fn rect_overlaps_any_unsafe_vector(rect: &Rect, unsafe_rects: &RectOverlapIndex) -> bool {
    unsafe_rects.overlaps_any(rect, MIN_UNSAFE_VECTOR_OVERLAP_AREA_PT2)
}
