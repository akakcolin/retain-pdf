//! Redaction-engine thresholds, port of `source/cleanup/config.py` plus the
//! rect-merge constants from `source/rects.py`.

/// Safe-direct redaction: max relative width/height error between the item
/// rect and a matched span.
pub const SAFE_DIRECT_REDACTION_SIZE_TOLERANCE: f64 = 0.05;
/// Safe-direct redaction: min IoU between the item rect and a matched span.
pub const SAFE_DIRECT_REDACTION_IOU_THRESHOLD: f64 = 0.8;

/// `expand_word_rect` padding (points).
pub const WORD_REDACTION_PAD_X: f64 = 1.0;
pub const WORD_REDACTION_PAD_Y: f64 = 0.6;

/// `rects_should_merge`: max horizontal gap between same-row rects.
pub const RECT_MERGE_GAP_X_PT: f64 = 3.0;
/// `rects_should_merge`: max vertical misalignment for same-row merging.
pub const RECT_MERGE_MAX_VERTICAL_MISALIGN_PT: f64 = 6.0;
/// `rects_should_merge`: union/combined area growth above which merging is refused.
pub const RECT_MERGE_MAX_AREA_GROWTH_RATIO: f64 = 2.4;
/// `rects_should_merge`: overlap/area ratio at/above which overlapping rects merge.
pub const RECT_MERGE_MIN_OVERLAP_RATIO: f64 = 0.8;
