// Port of services/rendering/layout/payload/body_font_inheritance_policy.py — the
// short/low-height body font inheritance policy (C3-N3). Operates on the raw JSON
// block-payload dicts, mutating in place.

use serde_json::Value;

use crate::item::Item;
use crate::layout::body_common::{
    body_context_anchors, payload_density, payload_height, payload_is_continuation_member, payload_width,
    required_lines, same_body_column, SHORT_BODY_INHERIT_MAX_HEIGHT_PT,
};
use crate::layout::payload_dict::{payload_bool, payload_f64, payload_string};
use crate::semantics::{block_kind, is_bodylike_block, is_caption_like_block, is_footnote_like_block};
use crate::util::{median_f64, py_round};

pub const SHORT_BODY_INHERIT_MIN_ANCHORS: usize = 2;
pub const SHORT_BODY_INHERIT_MAX_WIDTH_RATIO: f64 = 1.18;
pub const SHORT_BODY_INHERIT_MAX_FONT_GROW_PT: f64 = 1.8;
pub const LOW_HEIGHT_BODY_INHERIT_MAX_HEIGHT_RATIO: f64 = 0.72;
pub const LOW_HEIGHT_BODY_INHERIT_MAX_LINES: i64 = 8;
pub const LOW_HEIGHT_BODY_INHERIT_DENSITY_LIMIT: f64 = 1.08;

/// `inherit_short_body_fonts`: bump short body blocks up to the font of the
/// same-column anchors, capping growth and recording the inherited floor.
pub fn inherit_short_body_fonts(
    body_payloads: &[Value],
    all_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    let anchors = body_context_anchors(body_payloads, page_text_width_med);
    if anchors.len() < SHORT_BODY_INHERIT_MIN_ANCHORS {
        return;
    }
    let page_anchor_font: Vec<f64> = anchors.iter().map(|a| payload_f64(a, "font_size_pt", 0.0)).collect();
    let page_anchor_font = median_f64(&page_anchor_font);

    for payload in all_payloads {
        if !is_short_body_inherit_candidate(payload, page_text_width_med) {
            continue;
        }
        let local_anchors: Vec<f64> = anchors
            .iter()
            .filter(|anchor| same_body_column(payload, anchor, page_text_width_med))
            .map(|anchor| payload_f64(anchor, "font_size_pt", 0.0))
            .collect();
        if local_anchors.len() < 2 {
            continue;
        }
        let mut target_font = py_round(median_f64(&local_anchors), 2);
        target_font = target_font.min(page_anchor_font + 0.18);
        let inherited_font = short_body_target_font(payload, target_font);
        if inherited_font <= 0.0 {
            continue;
        }
        let obj = payload.as_object_mut().expect("payload is an object");
        obj.insert("font_size_pt".to_string(), Value::from(inherited_font));
        obj.insert("_short_body_inherited_font_floor_pt".to_string(), Value::from(inherited_font));
        obj.insert("page_body_font_size_pt".to_string(), Value::from(py_round(page_anchor_font, 2)));
        obj.insert("_short_body_font_inherited".to_string(), Value::Bool(true));
        obj.insert("_allow_short_text_bbox_overflow".to_string(), Value::Bool(true));
        obj.insert("prefer_typst_fit".to_string(), Value::Bool(false));
    }
}

/// `inherit_low_height_body_fonts`: annotate the page body font on short blocks
/// that sit under tall same-column anchors.
pub fn inherit_low_height_body_fonts(
    body_payloads: &[Value],
    all_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    let anchors = body_context_anchors(body_payloads, page_text_width_med);
    if anchors.len() < SHORT_BODY_INHERIT_MIN_ANCHORS {
        return;
    }
    let tall_anchors: Vec<&Value> = anchors
        .iter()
        .filter(|anchor| {
            !payload_bool(anchor, "dense_small_box")
                && !payload_bool(anchor, "heavy_dense_small_box")
                && payload_height(anchor) > SHORT_BODY_INHERIT_MAX_HEIGHT_PT
        })
        .collect();
    if tall_anchors.len() < SHORT_BODY_INHERIT_MIN_ANCHORS {
        return;
    }
    let tall_heights: Vec<f64> = tall_anchors.iter().map(|a| payload_height(a)).collect();
    let page_tall_height = median_f64(&tall_heights);

    for payload in all_payloads {
        if !is_low_height_body_inherit_candidate(payload) {
            continue;
        }
        let local_anchors: Vec<&&Value> = tall_anchors
            .iter()
            .filter(|anchor| same_body_column(payload, anchor, page_text_width_med))
            .collect();
        if local_anchors.len() < SHORT_BODY_INHERIT_MIN_ANCHORS {
            continue;
        }
        let local_heights: Vec<f64> = local_anchors.iter().map(|a| payload_height(a)).collect();
        let local_height = median_f64(&local_heights);
        let height_ref = page_tall_height.max(local_height).max(1.0);
        if payload_height(payload) > height_ref * LOW_HEIGHT_BODY_INHERIT_MAX_HEIGHT_RATIO {
            continue;
        }
        let tall_fonts: Vec<f64> = tall_anchors.iter().map(|a| payload_f64(a, "font_size_pt", 0.0)).collect();
        let target = py_round(median_f64(&tall_fonts), 2);
        let obj = payload.as_object_mut().expect("payload is an object");
        obj.insert("page_body_font_size_pt".to_string(), Value::from(target));
    }
}

fn is_short_body_inherit_candidate(payload: &Value, page_text_width_med: f64) -> bool {
    if payload_is_continuation_member(payload) {
        return false;
    }
    if payload_string(payload, "render_kind", "") != "markdown" {
        return false;
    }
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    if is_caption_like_block(&item) || is_footnote_like_block(&item) {
        return false;
    }
    if !payload_bool(payload, "is_body") && block_kind(&item) != "text" && !is_bodylike_block(&item) {
        return false;
    }
    if payload.get("title_fit").map(|v| !v.is_null()).unwrap_or(false) {
        return false;
    }
    if required_lines(payload) > 2 {
        return false;
    }
    let height = payload_height(payload);
    let width = payload_width(payload);
    if height <= 0.0 || height > SHORT_BODY_INHERIT_MAX_HEIGHT_PT {
        return false;
    }
    if page_text_width_med <= 0.0 || width >= page_text_width_med * SHORT_BODY_INHERIT_MAX_WIDTH_RATIO {
        return false;
    }
    has_source_text(&item)
}

fn short_body_target_font(payload: &Value, target_font: f64) -> f64 {
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    if current_font <= 0.0 || target_font <= 0.0 {
        return 0.0;
    }
    if target_font <= current_font {
        return py_round(target_font, 2);
    }
    py_round(target_font.min(current_font + SHORT_BODY_INHERIT_MAX_FONT_GROW_PT), 2)
}

fn is_low_height_body_inherit_candidate(payload: &Value) -> bool {
    if payload_is_continuation_member(payload) {
        return false;
    }
    if payload_string(payload, "render_kind", "") != "markdown" {
        return false;
    }
    if payload_bool(payload, "heavy_dense_small_box") && payload_density(payload, None, None) > LOW_HEIGHT_BODY_INHERIT_DENSITY_LIMIT {
        return false;
    }
    if payload.get("title_fit").map(|v| !v.is_null()).unwrap_or(false) {
        return false;
    }
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    if is_caption_like_block(&item) || is_footnote_like_block(&item) {
        return false;
    }
    if !payload_bool(payload, "is_body") && block_kind(&item) != "text" && !is_bodylike_block(&item) {
        return false;
    }
    if required_lines(payload) > LOW_HEIGHT_BODY_INHERIT_MAX_LINES {
        return false;
    }
    has_source_text(&item)
}

fn has_source_text(item: &Item) -> bool {
    !item.source_text.trim().is_empty() || !item.protected_source_text.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn anchor(x0: f64, y0: f64, x1: f64, y1: f64, font: f64) -> Value {
        json!({
            "inner_bbox": [x0, y0, x1, y1],
            "translated_text": "anchor text",
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": 0.5,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {"source_text": "anchor source text"},
        })
    }

    fn short_payload(x0: f64, y0: f64, x1: f64, y1: f64, font: f64) -> Value {
        json!({
            "inner_bbox": [x0, y0, x1, y1],
            "translated_text": "短",
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": 0.5,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {"source_text": "x"},
        })
    }

    #[test]
    fn short_block_inherits_anchor_font() {
        let body_payloads = vec![anchor(50.0, 0.0, 250.0, 30.0, 12.0), anchor(50.0, 40.0, 250.0, 80.0, 11.0)];
        let mut all_payloads = vec![short_payload(50.0, 85.0, 250.0, 95.0, 9.0)];
        inherit_short_body_fonts(&body_payloads, &mut all_payloads, 200.0);
        let font: f64 = all_payloads[0]["font_size_pt"].as_f64().unwrap();
        assert!(font > 9.0);
        assert_eq!(all_payloads[0]["_short_body_font_inherited"], json!(true));
        assert!(all_payloads[0]["_short_body_inherited_font_floor_pt"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn short_block_without_local_anchors_untouched() {
        let body_payloads = vec![anchor(50.0, 0.0, 250.0, 30.0, 12.0), anchor(50.0, 40.0, 250.0, 80.0, 11.0)];
        let mut all_payloads = vec![short_payload(400.0, 85.0, 500.0, 95.0, 9.0)];
        inherit_short_body_fonts(&body_payloads, &mut all_payloads, 200.0);
        assert_eq!(all_payloads[0]["font_size_pt"], json!(9.0));
        assert!(all_payloads[0].get("_short_body_font_inherited").is_none());
    }

    #[test]
    fn low_height_annotates_page_font() {
        let body_payloads = vec![anchor(50.0, 0.0, 250.0, 60.0, 12.0), anchor(50.0, 70.0, 250.0, 130.0, 11.0)];
        let mut all_payloads = vec![short_payload(50.0, 135.0, 250.0, 145.0, 9.0)];
        inherit_low_height_body_fonts(&body_payloads, &mut all_payloads, 200.0);
        let page_font: f64 = all_payloads[0]["page_body_font_size_pt"].as_f64().unwrap();
        assert!(page_font > 0.0);
    }
}
