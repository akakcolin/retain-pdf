//! Port of `planning/item_classifier.py` — text-strip / vector-overlap role
//! allowlists and the first-matching cleanup item class.

use serde_json::Value;

pub const VECTOR_OVERLAP_ROLE_ALLOWLIST: [&str; 5] =
    ["heading", "title", "toc", "table_of_contents", "page_number"];
pub const TEXT_STRIP_ROLE_ALLOWLIST: [&str; 9] = [
    "caption",
    "figure_caption",
    "image_caption",
    "table_caption",
    "footnote",
    "table_footnote",
    "image_footnote",
    "vision_footnote",
    "metadata",
];
pub const ITEM_COVER_FALLBACK_ROLE_ALLOWLIST: [&str; 9] = TEXT_STRIP_ROLE_ALLOWLIST;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupItemClass {
    ForceStripText,
    SafeDecoratedText,
    OrdinaryText,
}

/// `page_all_strip_items_allow_vector_overlap` — every text item allows vector
/// overlap.
pub fn page_all_strip_items_allow_vector_overlap(items: &[Value]) -> bool {
    let strip_text_items: Vec<&Value> = items.iter().filter(|item| item_is_text(item)).collect();
    !strip_text_items.is_empty()
        && strip_text_items.iter().all(|item| item_allows_vector_overlap(item))
}

pub fn item_allows_vector_overlap(item: &Value) -> bool {
    matches!(
        first_cleanup_item_class(item),
        Some(CleanupItemClass::ForceStripText | CleanupItemClass::SafeDecoratedText)
    )
}

pub fn item_allows_forced_text_strip(item: &Value) -> bool {
    item_is_text(item) && item_matches_role_allowlist(item, &TEXT_STRIP_ROLE_ALLOWLIST)
}

pub fn item_allows_item_cover_fallback(item: &Value) -> bool {
    item_is_text(item) && item_matches_role_allowlist(item, &ITEM_COVER_FALLBACK_ROLE_ALLOWLIST)
}

pub fn item_is_text(item: &Value) -> bool {
    crate::source_cleanup::planning::item_block_kind(item) == "text"
}

/// `item_role_values` — lowercased role values across the four role keys.
pub fn item_role_values(item: &Value) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    for key in [
        "layout_role",
        "semantic_role",
        "structure_role",
        "normalized_sub_type",
    ] {
        let value = crate::source_cleanup::planning::item_str(item, key).to_lowercase();
        if !value.is_empty() {
            values.push(value);
        }
    }
    values
}

pub fn item_matches_role_allowlist(item: &Value, allowlist: &[&str]) -> bool {
    item_role_values(item).iter().any(|value| allowlist.contains(&value.as_str()))
}

/// First matching cleanup item class (mirrors `first_cleanup_item_class`).
pub fn first_cleanup_item_class(item: &Value) -> Option<CleanupItemClass> {
    if item_allows_forced_text_strip(item) {
        return Some(CleanupItemClass::ForceStripText);
    }
    if item_is_text(item) && item_matches_role_allowlist(item, &VECTOR_OVERLAP_ROLE_ALLOWLIST) {
        return Some(CleanupItemClass::SafeDecoratedText);
    }
    if item_is_text(item) {
        return Some(CleanupItemClass::OrdinaryText);
    }
    None
}
