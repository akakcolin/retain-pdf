// Port of services/rendering/layout/payload/collision_context.py — the
// adjacent-collision budget helpers the body pipeline (C3-N3) reads. Operates on
// the raw JSON block-payload dicts.

use serde_json::Value;

use crate::item::Item;
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::payload::fit_common::VERTICAL_COLLISION_GAP_PT;

pub const VERTICAL_COLLISION_MIN_WIDTH_OVERLAP_RATIO: f64 = 0.6;
pub const VERTICAL_COLLISION_SOURCE_GAP_TRIGGER_PT: f64 = 3.0;
pub const VERTICAL_COLLISION_SAFETY_PAD_PT: f64 = 2.2;
pub const VERTICAL_COLLISION_TIGHT_SOURCE_GAP_PT: f64 = 0.8;
pub const VERTICAL_COLLISION_FORMULA_SAFETY_PAD_PT: f64 = 6.8;

/// Python `@dataclass(frozen=True) AdjacentCollisionContext`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdjacentCollisionContext {
    pub max_height_pt: f64,
    pub source_gap_pt: f64,
}

/// `collision_safety_pad_pt`: extra reserved height when the source gap is tight
/// or the payload carries formulas / a literal `$` in its translated text.
pub fn collision_safety_pad_pt(payload: &Value, source_gap: f64) -> f64 {
    let mut safety_pad = VERTICAL_COLLISION_SAFETY_PAD_PT;
    if source_gap <= VERTICAL_COLLISION_TIGHT_SOURCE_GAP_PT {
        safety_pad = safety_pad.max(3.4);
    }
    let has_formula = payload
        .get("formula_map")
        .and_then(|v| v.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false);
    let has_dollar = payload_string(payload, "translated_text", "").contains('$');
    if has_formula || has_dollar {
        safety_pad = safety_pad.max(VERTICAL_COLLISION_FORMULA_SAFETY_PAD_PT);
    }
    safety_pad
}

/// `adjacent_collision_context`: the vertical height budget the current block may
/// fill before colliding with the next, or None when they do not overlap
/// horizontally, the source gap is roomy, or no budget remains.
pub fn adjacent_collision_context(current: &Value, nxt: &Value) -> Option<AdjacentCollisionContext> {
    let cb = inner_bbox4(current)?;
    let nb = inner_bbox4(nxt)?;
    let (current_left, current_top, current_right, current_bottom) = (cb[0], cb[1], cb[2], cb[3]);
    let (next_left, next_top, next_right, _next_bottom) = (nb[0], nb[1], nb[2], nb[3]);

    let overlap_width = (current_right.min(next_right) - current_left.max(next_left)).max(0.0);
    let min_width = (current_right - current_left).min(next_right - next_left).max(1.0);
    if overlap_width / min_width < VERTICAL_COLLISION_MIN_WIDTH_OVERLAP_RATIO {
        return None;
    }

    let source_gap = next_top - current_bottom;
    if source_gap > VERTICAL_COLLISION_SOURCE_GAP_TRIGGER_PT {
        return None;
    }

    let max_height_pt =
        next_top - current_top - VERTICAL_COLLISION_GAP_PT - collision_safety_pad_pt(current, source_gap);
    if max_height_pt <= 0.0 {
        return None;
    }
    Some(AdjacentCollisionContext {
        max_height_pt,
        source_gap_pt: source_gap,
    })
}

/// `collision_fit_item`: the fit Item for the collision solver — the payload's
/// item augmented with the fit-item flags the seed factory normally sets.
pub fn collision_fit_item(payload: &Value) -> Item {
    let mut item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    item.render_inner_bbox = inner_bbox4(payload);
    item.is_body_text_candidate = payload_bool(payload, "is_body");
    item.dense_small_box = payload_bool(payload, "dense_small_box");
    item.heavy_dense_small_box = payload_bool(payload, "heavy_dense_small_box");
    item.short_body_inherited_font_floor_pt = payload_f64(payload, "_short_body_inherited_font_floor_pt", 0.0);
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(top: f64, bottom: f64, translated: &str) -> Value {
        json!({
            "inner_bbox": [50.0, top, 250.0, bottom],
            "translated_text": translated,
            "formula_map": [],
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {},
        })
    }

    #[test]
    fn context_from_stacked_blocks() {
        let current = payload(0.0, 40.0, "some translated text");
        let nxt = payload(41.5, 90.0, "next block text");
        let ctx = adjacent_collision_context(&current, &nxt);
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.source_gap_pt, 1.5);
        assert!(ctx.max_height_pt > 0.0);
    }

    #[test]
    fn wide_gap_returns_none() {
        let current = payload(0.0, 40.0, "text");
        let nxt = payload(60.0, 100.0, "text");
        assert!(adjacent_collision_context(&current, &nxt).is_none());
    }

    #[test]
    fn safety_pad_grows_for_formulas() {
        let plain = json!({"formula_map": [], "translated_text": "no math"});
        let formula = json!({"formula_map": [{"placeholder": "@f@", "latex": "x"}], "translated_text": "a @f@"});
        let dollar = json!({"formula_map": [], "translated_text": "cost is $5"});
        assert_eq!(collision_safety_pad_pt(&plain, 2.0), VERTICAL_COLLISION_SAFETY_PAD_PT);
        assert_eq!(collision_safety_pad_pt(&formula, 2.0), VERTICAL_COLLISION_FORMULA_SAFETY_PAD_PT);
        assert_eq!(collision_safety_pad_pt(&dollar, 2.0), VERTICAL_COLLISION_FORMULA_SAFETY_PAD_PT);
    }

    #[test]
    fn fit_item_override_render_keys() {
        let p = json!({
            "inner_bbox": [10.0, 20.0, 110.0, 120.0],
            "is_body": true,
            "dense_small_box": true,
            "heavy_dense_small_box": false,
            "_short_body_inherited_font_floor_pt": 10.5,
            "item": {"block_type": "paragraph", "layout_role": "body"},
        });
        let item = collision_fit_item(&p);
        assert_eq!(item.render_inner_bbox, Some([10.0, 20.0, 110.0, 120.0]));
        assert!(item.is_body_text_candidate);
        assert!(item.dense_small_box);
        assert_eq!(item.short_body_inherited_font_floor_pt, 10.5);
    }
}
