//! Redaction item DTO (the stable serde surface translated items arrive as)
//! and the valid-item filter, port of `source/items.py::iter_valid_translated_items`
//! restricted to the non-unit-translation path. Unknown keys are ignored; only
//! fields the ported logic reads are decoded.

use serde::Deserialize;

use rendering_core::source_cleanup::hit_test::RectTuple;

/// A translated item plus its resolved page-space bbox and protected text.
#[derive(Debug, Clone)]
pub struct ValidRedactionItem {
    pub rect: RectTuple,
    pub item: RedactionItem,
    pub translated_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RedactionItem {
    /// Any-length array in production (`iter_valid_translated_items` checks
    /// `len(bbox) != 4`); `None` when absent.
    pub bbox: Option<Vec<f64>>,
    #[serde(default)]
    pub translated_text: String,
    /// Translated-item id; the `redaction_items_from_layout_blocks` by-id key
    /// (production `source_items_by_id = {str(item.get("item_id") or ""): item}`).
    #[serde(default)]
    pub item_id: String,
    /// Source-item id carried onto page-spec-replaced items; `None` when the
    /// source association is absent (production JSON `null`).
    #[serde(default)]
    pub source_item_id: Option<String>,
    /// Third key of the visual-profile fill match (`runtime.py`
    /// `background_fill_for_item`); production replaced items only carry
    /// `_render_block_id`, so this stays empty for them.
    #[serde(default)]
    pub block_id: String,
    #[serde(default)]
    pub protected_translated_text: String,
    #[serde(default)]
    pub render_protected_text: String,
    #[serde(default)]
    pub translation_unit_protected_translated_text: String,
    #[serde(default)]
    pub translation_unit_protected_source_text: String,
    #[serde(default)]
    pub translation_unit_translated_text: String,
    #[serde(default)]
    pub source_text: String,
    #[serde(default)]
    pub protected_source_text: String,
    #[serde(default)]
    pub render_source_text: String,
    #[serde(default)]
    pub block_kind: String,
    #[serde(default)]
    pub block_type: String,
    #[serde(default)]
    pub raw_block_type: String,
    #[serde(default)]
    pub normalized_sub_type: String,
    #[serde(default)]
    pub continuation_group: Option<String>,
    #[serde(default)]
    pub continuation_group_id: Option<String>,
    #[serde(default, rename = "_force_visual_cover_only")]
    pub force_visual_cover_only: bool,
    #[serde(default, rename = "_formula_guard_fragment")]
    pub formula_guard_fragment: bool,
    #[serde(default, rename = "_formula_guard_fragment_index")]
    pub formula_guard_fragment_index: Option<i32>,
    /// Per-item visual-profile fill, injected by the Python shim via
    /// `visual_profile.background_fill_for_item(item)` (production's
    /// `visual_cover_execution.py` resolves the fill on the valid item set).
    /// `None` when the profile is absent/not loaded or the item ids miss.
    #[serde(default, rename = "_visual_profile_fill")]
    pub visual_profile_fill: Option<[f64; 3]>,
    #[serde(default, rename = "_render_cleanup_mode")]
    pub render_cleanup_mode: String,
    #[serde(default, rename = "_render_overlay_fill")]
    pub render_overlay_fill: String,
    #[serde(default, rename = "_render_use_cover_fill")]
    pub render_use_cover_fill: bool,
    #[serde(default)]
    pub render_policy: Option<RenderPolicyPayload>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RenderPolicyPayload {
    #[serde(default)]
    pub cleanup_mode: String,
    #[serde(default)]
    pub overlay_fill: String,
}

/// `policy/cleanup_plan.py::RenderCleanupItemPlan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCleanupItemPlan {
    pub visual_cover_only: bool,
    pub white_overlay_fill: bool,
    pub bbox_text_strip_allowed: bool,
}

/// `policy/compat.py::item_cleanup_mode`.
pub fn item_cleanup_mode(item: &RedactionItem) -> String {
    let policy_mode = item
        .render_policy
        .as_ref()
        .map(|p| p.cleanup_mode.as_str())
        .unwrap_or("");
    let mode = if !policy_mode.is_empty() {
        policy_mode
    } else {
        item.render_cleanup_mode.as_str()
    };
    mode.trim().to_lowercase()
}

/// `policy/compat.py::item_overlay_fill`.
pub fn item_overlay_fill(item: &RedactionItem) -> String {
    let policy_fill = item
        .render_policy
        .as_ref()
        .map(|p| p.overlay_fill.as_str())
        .unwrap_or("");
    let fill = if !policy_fill.is_empty() {
        policy_fill
    } else {
        item.render_overlay_fill.as_str()
    };
    fill.trim().to_lowercase()
}

/// `policy/compat.py::item_requires_visual_cover_only`.
pub fn item_requires_visual_cover_only(item: &RedactionItem) -> bool {
    item_cleanup_mode(item) == "visual_cover" || item.force_visual_cover_only
}

/// `policy/compat.py::item_uses_white_overlay_fill`.
pub fn item_uses_white_overlay_fill(item: &RedactionItem) -> bool {
    matches!(item_overlay_fill(item).as_str(), "white" | "sampled") || item.render_use_cover_fill
}

/// `policy/cleanup_plan.py::build_cleanup_item_plan`.
pub fn build_cleanup_item_plan(item: &RedactionItem) -> RenderCleanupItemPlan {
    let visual_cover_only = item_requires_visual_cover_only(item);
    RenderCleanupItemPlan {
        visual_cover_only,
        white_overlay_fill: item_uses_white_overlay_fill(item),
        bbox_text_strip_allowed: !visual_cover_only,
    }
}

fn first_non_empty(fields: &[&str]) -> String {
    fields
        .iter()
        .copied()
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

/// `get_render_protected_text` non-unit-translation path. Group continuation
/// items resolve through the member text first; plain items fall through the
/// translated chain. The source-block fallback is a documented divergence:
/// corpus items always carry non-empty translated text, so the source path is
/// unreachable for valid items.
pub fn item_translated_text(item: &RedactionItem) -> String {
    let raw = if item.continuation_group.is_some() || item.continuation_group_id.is_some() {
        let member = first_non_empty(&[&item.protected_translated_text, &item.translated_text]);
        if !member.is_empty() {
            member
        } else {
            first_non_empty(&[
                &item.protected_translated_text,
                &item.translated_text,
                &item.translation_unit_protected_translated_text,
                &item.translation_unit_translated_text,
            ])
        }
    } else {
        first_non_empty(&[
            &item.protected_translated_text,
            &item.translated_text,
            &item.translation_unit_protected_translated_text,
            &item.translation_unit_translated_text,
        ])
    };
    // `get_render_protected_text` strips every path.
    raw.trim().to_string()
}

/// `iter_valid_redaction_items` — keep items with a 4-float bbox and non-empty
/// protected text; skip empty rects.
pub fn iter_valid_redaction_items(translated_items: &[RedactionItem]) -> Vec<ValidRedactionItem> {
    let mut out = Vec::new();
    for item in translated_items {
        let Some(b) = item.bbox.as_ref() else {
            continue;
        };
        if b.len() != 4 {
            continue;
        }
        let translated_text = item_translated_text(item);
        if translated_text.is_empty() {
            continue;
        }
        let rect = [b[0], b[1], b[2], b[3]];
        if rect[2] <= rect[0] || rect[3] <= rect[1] {
            continue;
        }
        out.push(ValidRedactionItem {
            rect,
            item: item.clone(),
            translated_text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: &str) -> RedactionItem {
        serde_json::from_str(json).expect("parse")
    }

    #[test]
    fn valid_items_filter_bbox_and_text() {
        let items = serde_json::from_str::<Vec<RedactionItem>>(
            r#"[{"bbox":[0.0,0.0,10.0,10.0],"translated_text":"hi"},
                {"bbox":[1.0,1.0,2.0,2.0],"translated_text":"  "},
                {"bbox":[],"translated_text":"x"},
                {"bbox":[0.0,0.0,0.0,0.0],"translated_text":"y"}]"#,
        )
        .unwrap();
        let valid = iter_valid_redaction_items(&items);
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].translated_text, "hi");
    }

    #[test]
    fn item_translated_text_chain() {
        assert_eq!(item_translated_text(&item(r#"{"translated_text":"A"}"#)), "A");
        assert_eq!(
            item_translated_text(&item(
                r#"{"protected_translated_text":"B","translated_text":"A"}"#
            )),
            "B"
        );
        assert_eq!(item_translated_text(&item(r#"{}"#)), "");
    }

    #[test]
    fn unknown_keys_ignored() {
        let it = item(r#"{"bbox":[0,0,1,1],"translated_text":"x","bogus_key":123}"#);
        assert_eq!(it.translated_text, "x");
    }

    #[test]
    fn visual_profile_fill_roundtrips() {
        let it = item(r#"{"translated_text":"x","_visual_profile_fill":[0.12,0.34,0.56]}"#);
        assert_eq!(it.visual_profile_fill, Some([0.12, 0.34, 0.56]));
        assert_eq!(item(r#"{"translated_text":"x"}"#).visual_profile_fill, None);
    }

    #[test]
    fn source_association_fields_roundtrip() {
        let it = item(
            r#"{"translated_text":"x","item_id":"line-0","source_item_id":"src","block_id":"blk"}"#,
        );
        assert_eq!(it.item_id, "line-0");
        assert_eq!(it.source_item_id, Some("src".to_string()));
        assert_eq!(it.block_id, "blk");
        assert_eq!(item(r#"{"translated_text":"x","source_item_id":null}"#).source_item_id, None);
        assert_eq!(item(r#"{"translated_text":"x"}"#).source_item_id, None);
    }
}
