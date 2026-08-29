//! Port of `planning/evidence.py` — the intent-classifier evidence bundle.

use serde_json::Value;

use crate::source_cleanup::planning::item_classifier::item_allows_forced_text_strip;
use crate::source_cleanup::planning::item_block_kind;
use crate::source_cleanup::planning::item_str;
use crate::source_cleanup::planning::mixed_content::item_has_unresolved_embedded_formula;
use crate::source_cleanup::planning::policy::{
    item_has_formula_region, item_is_marked_non_translated, item_render_output_text,
    item_render_source_text,
};

#[derive(Debug, Clone)]
pub struct SourceCleanupEvidence {
    pub item: Value,
    pub item_id: String,
    pub block_kind: String,
    pub has_formula_region: bool,
    pub source_text: String,
    pub output_text: String,
    pub is_marked_non_translated: bool,
    pub has_unresolved_embedded_formula: bool,
    pub is_force_strip_text: bool,
}

/// `build_source_cleanup_evidence`.
pub fn build_source_cleanup_evidence(item: &Value) -> SourceCleanupEvidence {
    SourceCleanupEvidence {
        item: item.clone(),
        item_id: item_str(item, "item_id"),
        block_kind: item_block_kind(item),
        has_formula_region: item_has_formula_region(item),
        source_text: item_render_source_text(item),
        output_text: item_render_output_text(item),
        is_marked_non_translated: item_is_marked_non_translated(item),
        has_unresolved_embedded_formula: item_has_unresolved_embedded_formula(item),
        is_force_strip_text: item_allows_forced_text_strip(item),
    }
}

/// `evidence_has_text_overlay`.
pub fn evidence_has_text_overlay(evidence: &SourceCleanupEvidence) -> bool {
    !evidence.output_text.is_empty() && !evidence.is_marked_non_translated
}
