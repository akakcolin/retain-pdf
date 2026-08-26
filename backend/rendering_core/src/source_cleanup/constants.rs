//! Port of backend/scripts/services/rendering/source_cleanup/pdf/constants.py.

/// Max decoded content-stream size (bytes) to still attempt bbox text strip.
pub const BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD: usize = 1_000_000;

pub const STRIP_SEGMENT_PAD_X_PT: f64 = 1.0;
pub const STRIP_SEGMENT_PAD_Y_PT: f64 = 1.0;
pub const FORMULA_SPLIT_SEGMENT_PAD_X_PT: f64 = 1.0;
pub const FORMULA_SPLIT_SEGMENT_PAD_Y_PT: f64 = 0.0;
pub const BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT: f64 = 1.0;
pub const BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT: f64 = 1.0;
