//! Port of `planning/page_gate.py` — the formula-page early skip.

use serde_json::Value;

use crate::source_cleanup::planning::policy::page_should_skip_bbox_text_strip;
use crate::source_cleanup::planning::SkipReason;

/// `bbox_text_strip_items_skip_reason` — formula-page skip when enabled.
pub fn bbox_text_strip_items_skip_reason(
    items: &[Value],
    skip_formula_pages: bool,
) -> SkipReason {
    if skip_formula_pages && page_should_skip_bbox_text_strip(items) {
        SkipReason::Complex
    } else {
        SkipReason::None
    }
}
