// Port of services/rendering/layout/payload/collision.py — the adjacent-body
// collision-risk marking step the body pipeline (C3-N3) runs before emit.
// Operates on the ordered JSON block-payload dicts, mutating in place.

use serde_json::Value;

use crate::item::formula_map;
use crate::layout::collision_context::{adjacent_collision_context, collision_fit_item};
use crate::layout::fit_vertical::fit_block_to_vertical_limit;
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::layout::typography_policy as typography;
use crate::payload::capacity::estimated_render_height_pt;
use crate::util::py_round;

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
