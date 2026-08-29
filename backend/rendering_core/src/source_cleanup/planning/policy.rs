//! Port of the `services/rendering/policy/cleanup_policy.py` predicates the
//! source-cleanup planning path consumes.

use serde_json::Value;

use crate::source_cleanup::planning::{item_block_kind, item_rect, item_str};

pub const NON_TRANSLATED_FINAL_STATUSES: [&str; 10] = [
    "empty_translation",
    "failed",
    "ignored",
    "keep_origin",
    "kept_origin",
    "not_translated",
    "preserve_source",
    "source_preserved",
    "skipped",
    "skip_translation",
];
pub const NON_TRANSLATED_DECISIONS: [&str; 5] = ["ignore", "keep_origin", "preserve_source", "skip", "skip_translation"];
pub const SKIP_TRANSLATION_TAGS: [&str; 3] = ["skip_translation", "keep_origin", "preserve_source"];

/// `item_has_formula_region` — formula block kind/type or display-formula raw /
/// normalized subtype.
pub fn item_has_formula_region(item: &Value) -> bool {
    let normalized_sub_type = item_str(item, "normalized_sub_type").to_lowercase();
    let raw_block_type = item_str(item, "raw_block_type").to_lowercase();
    let block_type = item_str(item, "block_type").to_lowercase();
    item_block_kind(item) == "formula"
        || block_type == "formula"
        || raw_block_type == "display_formula"
        || normalized_sub_type == "display_formula"
}

/// `page_has_formula_region` — any formula item with a valid rect.
pub fn page_has_formula_region(translated_items: &[Value]) -> bool {
    translated_items
        .iter()
        .any(|item| item_has_formula_region(item) && item_rect(item).is_some())
}

/// `page_should_skip_bbox_text_strip` — formula region present.
pub fn page_should_skip_bbox_text_strip(translated_items: &[Value]) -> bool {
    page_has_formula_region(translated_items)
}

/// `item_is_marked_non_translated` — final status / decision / tags.
pub fn item_is_marked_non_translated(item: &Value) -> bool {
    let status = crate::source_cleanup::planning::item_first_str(
        item,
        &["final_status", "translation_status", "status"],
    )
    .to_lowercase();
    if NON_TRANSLATED_FINAL_STATUSES.contains(&status.as_str()) {
        return true;
    }
    let decision = crate::source_cleanup::planning::item_first_str(
        item,
        &["decision", "translation_decision"],
    )
    .to_lowercase();
    if NON_TRANSLATED_DECISIONS.contains(&decision.as_str()) {
        return true;
    }
    let tags = crate::source_cleanup::planning::item_tags(item);
    if tags.iter().any(|tag| SKIP_TRANSLATION_TAGS.contains(&tag.as_str())) {
        return true;
    }
    false
}

/// `item_render_output_text` — the translated-overlay text chain.
pub fn item_render_output_text(item: &Value) -> String {
    if item.get("render_protected_text").is_some() {
        return item_str(item, "render_protected_text");
    }
    let has_continuation = item.get("continuation_group").is_some()
        || item.get("continuation_group_id").is_some();
    let has_protected_translated = item.get("protected_translated_text").is_some()
        || item.get("translated_text").is_some();
    if has_continuation && has_protected_translated {
        return crate::source_cleanup::planning::item_first_str(
            item,
            &["protected_translated_text", "translated_text"],
        );
    }
    crate::source_cleanup::planning::item_first_str(
        item,
        &[
            "render_translation_overlay_text",
            "translation_overlay_text",
            "translation_unit_protected_translated_text",
            "group_protected_translated_text",
            "protected_translated_text",
            "translation_unit_translated_text",
            "group_translated_text",
            "translated_text",
        ],
    )
}

/// `item_render_source_text` — the source-text chain.
pub fn item_render_source_text(item: &Value) -> String {
    crate::source_cleanup::planning::item_first_str(
        item,
        &[
            "translation_unit_protected_source_text",
            "protected_source_text",
            "source_text",
        ],
    )
}
