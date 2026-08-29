// Port of services/rendering/policy/cleanup_policy.py —
// `apply_render_pages_policy_fields` / `apply_render_page_policy_fields`, the
// C3-N8 boundary. Builds per-page render-item policies (typst-fill / default
// text-overlay-cover-fill / no-op modes) and patches `_render_policy` onto the
// translated items. Operates on the raw JSON item dicts so unknown keys round
// trip byte-exact; the typed `Item` DTO does not carry `_render_policy`. The two
// config flags (`use_typst_fill_cleanup` / `use_default_text_overlay_cover_fill`)
// are runtime settings resolved by the Python shim, so they arrive as
// parameters.

use crate::layout::payload_dict::py_str;
use crate::rect::Rect;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

const NON_TRANSLATED_FINAL_STATUSES: [&str; 10] = [
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
const NON_TRANSLATED_DECISIONS: [&str; 5] = [
    "ignore",
    "keep_origin",
    "preserve_source",
    "skip",
    "skip_translation",
];
const SKIP_TRANSLATION_TAGS: [&str; 3] = ["skip_translation", "keep_origin", "preserve_source"];

/// Python `bool(x)` over scalar dict values (missing/null, empty string, zero,
/// false, empty container are falsy).
fn value_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64().map_or(true, |f| f == 0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// Python `str(item.get(k) or item.get(k2) or ... or "")` — the first truthy
/// value in the key chain, `str()`-converted, else `""`.
fn or_chain_str(item: &Value, keys: &[&str]) -> String {
    for key in keys {
        match item.get(*key) {
            Some(v) if !value_falsy(v) => return py_str(v),
            _ => {}
        }
    }
    String::new()
}

/// Python `float(x)` over JSON values: numbers pass through, numeric strings
/// parse (mirrors `float()`'s string acceptance).
fn py_float(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// `semantics.block_kind` over the raw item dict: explicit `block_kind` wins
/// (stripped + lowercased) when present and non-empty, else `block_type`
/// (defaulting to `"unknown"`).
fn value_block_kind(item: &Value) -> String {
    let explicit = or_chain_str(item, &["block_kind"]);
    if !explicit.is_empty() {
        let kind = explicit.trim().to_lowercase();
        if !kind.is_empty() {
            return kind;
        }
    }
    let block_type = or_chain_str(item, &["block_type"]);
    let block_type = if block_type.is_empty() {
        "unknown".to_string()
    } else {
        block_type
    };
    let normalized = block_type.trim().to_lowercase();
    if normalized.is_empty() {
        "unknown".to_string()
    } else {
        normalized
    }
}

/// `item_has_formula_region`: formula block kind, formula block type, or a
/// display-formula raw-block-type / normalized sub-type.
fn item_has_formula_region(item: &Value) -> bool {
    let normalized_sub_type = or_chain_str(item, &["normalized_sub_type"]).trim().to_lowercase();
    let raw_block_type = or_chain_str(item, &["raw_block_type"]).trim().to_lowercase();
    let block_type = or_chain_str(item, &["block_type"]).trim().to_lowercase();
    value_block_kind(item) == "formula"
        || block_type == "formula"
        || raw_block_type == "display_formula"
        || normalized_sub_type == "display_formula"
}

/// `item_rect`: `bbox` as a non-empty `Rect`, else `None` (mirrors
/// `policy.geometry.item_rect`).
fn item_rect(item: &Value) -> Option<Rect> {
    let arr = item.get("bbox")?.as_array()?;
    if arr.len() != 4 {
        return None;
    }
    let mut coords = [0.0f64; 4];
    for (i, value) in arr.iter().enumerate() {
        match py_float(value) {
            Some(f) => coords[i] = f,
            None => return None,
        }
    }
    let rect = Rect::new(coords[0], coords[1], coords[2], coords[3]);
    if rect.is_empty() {
        None
    } else {
        Some(rect)
    }
}

/// `page_has_formula_region`: any formula-region item with a non-empty bbox.
fn page_has_formula_region(items: &[Value]) -> bool {
    items
        .iter()
        .any(|item| item_has_formula_region(item) && item_rect(item).is_some())
}

/// `item_is_marked_non_translated`: a non-translated final status, a
/// skip-translation decision, or a skip-translation tag.
fn item_is_marked_non_translated(item: &Value) -> bool {
    let status = or_chain_str(item, &["final_status", "translation_status", "status"])
        .trim()
        .to_lowercase();
    if NON_TRANSLATED_FINAL_STATUSES.contains(&status.as_str()) {
        return true;
    }
    let decision = or_chain_str(item, &["decision", "translation_decision"])
        .trim()
        .to_lowercase();
    if NON_TRANSLATED_DECISIONS.contains(&decision.as_str()) {
        return true;
    }
    if let Some(Value::Array(tags)) = item.get("tags") {
        for tag in tags {
            let normalized = py_str(tag).trim().to_lowercase();
            if SKIP_TRANSLATION_TAGS.contains(&normalized.as_str()) {
                return true;
            }
        }
    }
    false
}

/// `item_render_output_text`: the render output text with the same key
/// precedence as the Python reference (render-protected first, then the
/// continuation-member branch, then the group/unit chain).
fn item_render_output_text(item: &Value) -> String {
    if item.get("render_protected_text").is_some() {
        return or_chain_str(item, &["render_protected_text"]).trim().to_string();
    }
    let has_continuation = item
        .get("continuation_group")
        .map_or(false, |v| !value_falsy(v))
        || item
            .get("continuation_group_id")
            .map_or(false, |v| !value_falsy(v));
    let has_member_text = item
        .get("protected_translated_text")
        .map_or(false, |v| !value_falsy(v))
        || item
            .get("translated_text")
            .map_or(false, |v| !value_falsy(v));
    if has_continuation && has_member_text {
        return or_chain_str(item, &["protected_translated_text", "translated_text"])
            .trim()
            .to_string();
    }
    or_chain_str(
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
    .trim()
    .to_string()
}

/// `item_will_render_translated_overlay`: not marked non-translated and has
/// render output text.
fn item_will_render_translated_overlay(item: &Value) -> bool {
    if item_is_marked_non_translated(item) {
        return false;
    }
    !item_render_output_text(item).is_empty()
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderItemPolicy {
    pub item_id: String,
    pub cleanup_mode: String,
    pub overlay_fill: String,
    pub formula_protection_role: String,
    pub reason: String,
}

impl RenderItemPolicy {
    /// `RenderItemPolicy.to_payload()` — the four-key `_render_policy` dict.
    fn to_payload(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("cleanup_mode".into(), Value::String(self.cleanup_mode.clone()));
        map.insert("overlay_fill".into(), Value::String(self.overlay_fill.clone()));
        map.insert(
            "formula_protection_role".into(),
            Value::String(self.formula_protection_role.clone()),
        );
        map.insert("reason".into(), Value::String(self.reason.clone()));
        map
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderPagePolicy {
    pub page_has_formula_region: bool,
    pub item_policies: BTreeMap<String, RenderItemPolicy>,
}

/// `_build_typst_fill_page_policy` / `_build_default_cover_fill_page_policy`:
/// one policy per text item that will render a translated overlay.
fn build_cover_fill_page_policy(items: &[Value], cleanup_mode: &str, reason: &str) -> RenderPagePolicy {
    let mut policies: BTreeMap<String, RenderItemPolicy> = BTreeMap::new();
    for item in items {
        let item_id = or_chain_str(item, &["item_id"]).trim().to_string();
        if item_id.is_empty() || value_block_kind(item) != "text" {
            continue;
        }
        if !item_will_render_translated_overlay(item) {
            continue;
        }
        policies.insert(
            item_id.clone(),
            RenderItemPolicy {
                item_id,
                cleanup_mode: cleanup_mode.to_string(),
                overlay_fill: "sampled".to_string(),
                formula_protection_role: "none".to_string(),
                reason: reason.to_string(),
            },
        );
    }
    RenderPagePolicy {
        page_has_formula_region: page_has_formula_region(items),
        item_policies: policies,
    }
}

/// `build_render_page_policy`: the typst-fill / default-cover-fill / no-op
/// mode decision. Config flags come from the Python shim.
pub fn build_render_page_policy(
    items: &[Value],
    use_typst_fill_cleanup: bool,
    use_default_text_overlay_cover_fill: bool,
) -> RenderPagePolicy {
    if use_typst_fill_cleanup {
        return build_cover_fill_page_policy(items, "visual_cover", "typst_fill_default");
    }
    if use_default_text_overlay_cover_fill {
        return build_cover_fill_page_policy(items, "delete_text", "default_text_overlay_cover_fill");
    }
    RenderPagePolicy {
        page_has_formula_region: page_has_formula_region(items),
        item_policies: BTreeMap::new(),
    }
}

/// `apply_render_page_policy_fields`: patch `_render_policy` onto the matching
/// items; the list passes through unchanged when no item policy applies.
pub fn apply_render_page_policy_fields(
    items: &[Value],
    use_typst_fill_cleanup: bool,
    use_default_text_overlay_cover_fill: bool,
) -> Vec<Value> {
    let policy = build_render_page_policy(items, use_typst_fill_cleanup, use_default_text_overlay_cover_fill);
    if policy.item_policies.is_empty() {
        return items.to_vec();
    }
    let mut patched: Vec<Value> = Vec::with_capacity(items.len());
    for item in items {
        let item_id = or_chain_str(item, &["item_id"]).trim().to_string();
        match policy.item_policies.get(&item_id) {
            Some(item_policy) => {
                let mut patched_item = item.clone();
                patched_item["_render_policy"] = Value::Object(item_policy.to_payload());
                patched.push(patched_item);
            }
            None => patched.push(item.clone()),
        }
    }
    patched
}

/// `apply_render_pages_policy_fields`: per-page policy-field patching.
pub fn apply_render_pages_policy_fields(
    translated_pages: &BTreeMap<i64, Vec<Value>>,
    use_typst_fill_cleanup: bool,
    use_default_text_overlay_cover_fill: bool,
) -> BTreeMap<i64, Vec<Value>> {
    translated_pages
        .iter()
        .map(|(&page_idx, items)| {
            (
                page_idx,
                apply_render_page_policy_fields(
                    items,
                    use_typst_fill_cleanup,
                    use_default_text_overlay_cover_fill,
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text_item(item_id: &str, translated_text: &str) -> Value {
        json!({
            "item_id": item_id,
            "block_type": "text",
            "block_kind": "text",
            "bbox": [40.0, 40.0, 260.0, 70.0],
            "translated_text": translated_text,
        })
    }

    #[test]
    fn default_cover_fill_adds_delete_text_only_for_translated() {
        let items = vec![
            text_item("p001-b001", "译文"),
            json!({
                "item_id": "p001-b002",
                "block_type": "text",
                "block_kind": "text",
                "bbox": [40.0, 80.0, 260.0, 110.0],
                "source_text": "kept source",
                "translation_status": "keep_origin",
            }),
        ];
        let patched = apply_render_page_policy_fields(&items, false, true);
        assert_eq!(patched[0]["_render_policy"]["cleanup_mode"], json!("delete_text"));
        assert_eq!(patched[0]["_render_policy"]["overlay_fill"], json!("sampled"));
        assert_eq!(patched[0]["_render_policy"]["reason"], json!("default_text_overlay_cover_fill"));
        assert!(patched[1].get("_render_policy").is_none());
        // Non-policy keys preserved byte-exact.
        assert_eq!(patched[0]["block_type"], json!("text"));
        assert_eq!(patched[0]["translated_text"], json!("译文"));
    }

    #[test]
    fn formula_page_region_flagged_and_text_neighbor_deletable() {
        let items = vec![
            text_item("p001-b001", "上文"),
            json!({
                "item_id": "p001-b002",
                "block_type": "formula",
                "block_kind": "formula",
                "normalized_sub_type": "display_formula",
                "bbox": [96.0, 76.0, 224.0, 104.0],
            }),
        ];
        let policy = build_render_page_policy(&items, false, true);
        assert!(policy.page_has_formula_region);
        assert_eq!(policy.item_policies["p001-b001"].cleanup_mode, "delete_text");
    }

    #[test]
    fn typst_fill_mode_uses_visual_cover() {
        let items = vec![text_item("a", "译文")];
        let patched = apply_render_page_policy_fields(&items, true, false);
        assert_eq!(patched[0]["_render_policy"]["cleanup_mode"], json!("visual_cover"));
        assert_eq!(patched[0]["_render_policy"]["reason"], json!("typst_fill_default"));
    }

    #[test]
    fn no_op_mode_returns_items_unchanged() {
        let items = vec![text_item("a", "译文")];
        let patched = apply_render_page_policy_fields(&items, false, false);
        assert_eq!(patched, items);
    }

    #[test]
    fn empty_item_id_skips_policy() {
        let items = vec![json!({
            "block_type": "text",
            "translated_text": "译文",
            "bbox": [40.0, 40.0, 260.0, 70.0],
        })];
        let patched = apply_render_page_policy_fields(&items, false, true);
        assert!(patched[0].get("_render_policy").is_none());
    }

    #[test]
    fn render_protected_text_preference() {
        let item = json!({
            "item_id": "a",
            "block_type": "text",
            "block_kind": "text",
            "render_protected_text": "受保护译文",
            "translated_text": "原始译文",
            "bbox": [40.0, 40.0, 260.0, 70.0],
        });
        let items = vec![item];
        let patched = apply_render_page_policy_fields(&items, false, true);
        assert_eq!(patched[0]["_render_policy"]["reason"], json!("default_text_overlay_cover_fill"));
    }

    #[test]
    fn skip_tags_exclude_item() {
        let items = vec![json!({
            "item_id": "a",
            "block_type": "text",
            "block_kind": "text",
            "translated_text": "译文",
            "tags": ["skip_translation"],
            "bbox": [40.0, 40.0, 260.0, 70.0],
        })];
        let patched = apply_render_page_policy_fields(&items, false, true);
        assert!(patched[0].get("_render_policy").is_none());
    }

    #[test]
    fn empty_bbox_item_not_a_formula_region() {
        let item = json!({
            "item_id": "a",
            "block_type": "formula",
            "normalized_sub_type": "display_formula",
            "bbox": [],
        });
        assert!(!page_has_formula_region(&[item]));
    }

    #[test]
    fn caller_input_untouched() {
        let items = vec![text_item("a", "译文")];
        let pristine = items.clone();
        apply_render_page_policy_fields(&items, false, true);
        assert_eq!(items, pristine);
    }

    #[test]
    fn pages_map_routes_each_page() {
        let mut pages = BTreeMap::new();
        pages.insert(0, vec![text_item("a", "译文")]);
        pages.insert(1, vec![text_item("b", "也译文")]);
        let result = apply_render_pages_policy_fields(&pages, false, true);
        assert_eq!(result[&0][0]["_render_policy"]["reason"], json!("default_text_overlay_cover_fill"));
        assert_eq!(result[&1][0]["_render_policy"]["reason"], json!("default_text_overlay_cover_fill"));
    }
}
