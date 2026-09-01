// Port of services/rendering/layout/payload/collision.py + collision_context.py —
// the adjacent-body collision budget helpers and risk-marking step the body
// pipeline (C3-N3) runs before emit. Operates on the ordered JSON block-payload
// dicts, mutating in place.

use serde_json::Value;

use crate::item::Item;
use crate::item::formula_map;
use crate::layout::fit::fit_block_to_vertical_limit;
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::layout::typography_capacity as typography;
use crate::payload::capacity::estimated_render_height_pt;
use crate::payload::fit_common::VERTICAL_COLLISION_GAP_PT;
use crate::util::py_round;

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

pub const VERTICAL_COLLISION_TRIGGER_RATIO: f64 = 0.9;

/// `mark_adjacent_collision_risk`: for each body block that would collide with
/// the block below it, either clamp leading only (unified page font) or re-fit
/// font/leading to the available height.
pub fn mark_adjacent_collision_risk(ordered_payloads: &mut Vec<Value>) {
    let mut idx = 0;
    while idx + 1 < ordered_payloads.len() {
        let (left, right) = ordered_payloads.split_at_mut(idx + 1);
        let current = &mut left[idx];
        let nxt = &right[0];
        let Some(context) = adjacent_collision_context(current, nxt) else {
            idx += 1;
            continue;
        };

        let estimated_height = estimated_render_height_pt(
            &inner_bbox4(current).unwrap_or([0.0; 4]),
            &payload_string(current, "translated_text", ""),
            &formula_map(current.get("formula_map")),
            payload_f64(current, "font_size_pt", 0.0),
            payload_f64(current, "leading_em", 0.0),
        );
        if estimated_height <= context.max_height_pt * VERTICAL_COLLISION_TRIGGER_RATIO {
            idx += 1;
            continue;
        }

        if has_unified_body_font(current) {
            let current_leading = payload_f64(current, "leading_em", 0.0);
            let clamped = py_round(
                typography::BODY_COLLISION_UNIFIED_MIN_LEADING_EM.max(current_leading.min(0.56)),
                2,
            );
            let obj = current.as_object_mut().expect("payload is an object");
            obj.insert("leading_em".to_string(), Value::from(clamped));
            obj.insert("_body_collision_leading_only".to_string(), Value::Bool(true));
            idx += 1;
            continue;
        }

        let item = collision_fit_item(current);
        let translated = payload_string(current, "translated_text", "");
        let formulas = formula_map(current.get("formula_map"));
        let (fitted_font, fitted_leading) = fit_block_to_vertical_limit(
            &item,
            &translated,
            &formulas,
            payload_f64(current, "font_size_pt", 0.0),
            payload_f64(current, "leading_em", 0.0),
            context.max_height_pt,
            Some(payload_f64(current, "page_body_font_size_pt", 0.0)),
        );
        let obj = current.as_object_mut().expect("payload is an object");
        obj.insert("font_size_pt".to_string(), Value::from(fitted_font));
        obj.insert("leading_em".to_string(), Value::from(fitted_leading));
        obj.insert("prefer_typst_fit".to_string(), Value::Bool(true));
        obj.insert("adjacent_collision_risk".to_string(), Value::Bool(true));
        remember_adjacent_height_limit(current, context.max_height_pt);

        idx += 1;
    }
}

/// `_has_unified_body_font`: body text at (or within tolerance of) the page body
/// font, so collision handling only compresses leading.
fn has_unified_body_font(payload: &Value) -> bool {
    if !payload_bool(payload, "is_body") {
        return false;
    }
    let page_font = payload_f64(payload, "page_body_font_size_pt", 0.0);
    let font = payload_f64(payload, "font_size_pt", 0.0);
    page_font > 0.0 && (font - page_font).abs() <= typography::BODY_COLLISION_UNIFIED_FONT_TOLERANCE_PT
}

/// `_remember_adjacent_height_limit`: track the tightest available height seen
/// so later stages can respect it.
fn remember_adjacent_height_limit(payload: &mut Value, max_height_pt: f64) {
    let previous = payload.get("adjacent_available_height_pt").and_then(|v| v.as_f64());
    if previous.is_some_and(|prev| max_height_pt >= prev) {
        return;
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "adjacent_available_height_pt".to_string(),
            Value::from(max_height_pt.max(6.0)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const LONG_TEXT: &str =
        "段落文字内容足够长的时候会占满整个高度并且还有更多的文本需要换行排列这样才能触发碰撞拟合逻辑的正常路径执行。这段内容再次重复一遍以确保它足够长，能够稳定地超出相邻块可用的垂直高度预算从而真正进入碰撞处理的分支而不是提前返回。再加上更多的文字让段落变得足够高，最终拟合时字体尺寸一定会被压缩到更小的数值以满足垂直空间的限制要求。";

    fn payload(is_body: bool, top: f64, bottom: f64, font: f64, leading: f64, page_font: f64) -> Value {
        json!({
            "inner_bbox": [50.0, top, 250.0, bottom],
            "translated_text": LONG_TEXT,
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": leading,
            "page_body_font_size_pt": page_font,
            "render_kind": "markdown",
            "is_body": is_body,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "prefer_typst_fit": false,
            "item": {},
        })
    }

    #[test]
    fn leaves_roomy_pairs_untouched() {
        let mut payloads = vec![
            payload(true, 0.0, 40.0, 12.0, 0.5, 12.0),
            payload(true, 90.0, 140.0, 12.0, 0.5, 12.0),
        ];
        mark_adjacent_collision_risk(&mut payloads);
        assert_eq!(payloads[0]["font_size_pt"], json!(12.0));
        assert!(payloads[0].get("adjacent_collision_risk").is_none());
    }

    #[test]
    fn fits_collision_block() {
        // Tightly stacked and off the unified page font: block 0 re-fits.
        let mut payloads = vec![
            payload(true, 0.0, 100.0, 11.0, 0.5, 12.0),
            payload(true, 101.5, 200.0, 12.0, 0.5, 12.0),
        ];
        mark_adjacent_collision_risk(&mut payloads);
        let font: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        assert!(font < 11.0);
        assert_eq!(payloads[0]["prefer_typst_fit"], json!(true));
        assert_eq!(payloads[0]["adjacent_collision_risk"], json!(true));
        assert!(payloads[0]["adjacent_available_height_pt"].as_f64().unwrap() >= 6.0);
    }

    #[test]
    fn unified_font_compresses_leading_only() {
        let mut payloads = vec![
            payload(true, 0.0, 100.0, 12.0, 0.58, 12.0),
            payload(true, 101.5, 200.0, 12.0, 0.5, 12.0),
        ];
        mark_adjacent_collision_risk(&mut payloads);
        assert_eq!(payloads[0]["font_size_pt"], json!(12.0));
        assert_eq!(payloads[0]["_body_collision_leading_only"], json!(true));
        let leading: f64 = payloads[0]["leading_em"].as_f64().unwrap();
        assert!(leading <= 0.56);
    }
}

#[cfg(test)]
mod tests_context {
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
