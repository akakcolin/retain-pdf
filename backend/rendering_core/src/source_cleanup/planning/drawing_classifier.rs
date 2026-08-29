//! Port of `planning/drawing_classifier.py` — which bboxlog path kinds block
//! text stripping.

use crate::rect::Rect;
use crate::source_cleanup::path_removal::rect_is_text_like_fill_path;

pub const MAX_TEXT_LIKE_FILL_PATH_HEIGHT_PT: f64 = 32.0;
pub const MAX_TEXT_LIKE_FILL_PATH_AREA_PT2: f64 = 3500.0;

/// `bboxlog_path_blocks_text_strip` — stroke paths always block; fills block
/// when they look like text (small glyph-scale rects).
pub fn bboxlog_path_blocks_text_strip(kind: &str, rect: &Rect) -> bool {
    if kind.starts_with("stroke-") && kind.contains("path") {
        return true;
    }
    is_text_like_fill_path(kind, rect)
}

/// `is_text_like_fill_path` — normalized kind gating before the rect test.
pub fn is_text_like_fill_path(kind: &str, rect: &Rect) -> bool {
    let normalized_kind = kind.trim().to_lowercase();
    if normalized_kind == "f" || normalized_kind == "fs" {
        return rect_is_text_like_fill_path(rect);
    }
    if !normalized_kind.contains("path") || !normalized_kind.starts_with("fill-") {
        return false;
    }
    rect_is_text_like_fill_path(rect)
}
