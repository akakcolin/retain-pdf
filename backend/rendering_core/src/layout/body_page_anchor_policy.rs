// Port of services/rendering/layout/payload/body_page_anchor_policy.py — the
// page body-font anchor policy (C3-N3). Operates on raw JSON block-payload dicts.

use serde_json::Value;

use crate::item::Item;
use crate::layout::body_common::{
    is_body_context_text_payload, payload_density, payload_height, payload_width, same_body_column,
};
use crate::layout::payload_dict::{payload_bool, payload_f64, payload_string};
use crate::layout::typography_decision::{set_page_body_anchor_decision, PageBodyAnchorDecision};
use crate::layout::typography_policy as typography;
use crate::util::py_round;

/// `apply_page_body_font_anchor`: anchor the page body font to the lowest
/// stable body font, applying it to same-column candidates.
pub fn apply_page_body_font_anchor(
    body_payloads: &[Value],
    all_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    let anchors = page_body_font_anchors(body_payloads, page_text_width_med);
    if anchors.len() < typography::PAGE_BODY_FONT_ANCHOR_COUNT {
        return;
    }
    let target_font = low_anchor_font_target(&anchors);
    if target_font <= 0.0 {
        return;
    }
    for payload in all_payloads {
        if !is_page_anchor_font_candidate(payload, page_text_width_med) {
            continue;
        }
        if !anchors.iter().any(|anchor| same_body_column(payload, anchor, page_text_width_med)) {
            continue;
        }
        let current_font = payload_f64(payload, "font_size_pt", 0.0);
        if current_font <= target_font + 0.04 {
            payload
                .as_object_mut()
                .expect("payload is an object")
                .insert("page_body_font_size_pt".to_string(), Value::from(target_font));
            set_page_body_anchor_decision(payload, &PageBodyAnchorDecision { target_font_pt: target_font, applied: true });
            continue;
        }
        let floor = payload_f64(payload, "_short_body_inherited_font_floor_pt", 0.0);
        let new_floor = floor.max(target_font);
        let obj = payload.as_object_mut().expect("payload is an object");
        obj.insert("font_size_pt".to_string(), Value::from(target_font));
        obj.insert("page_body_font_size_pt".to_string(), Value::from(target_font));
        if floor > 0.0 {
            obj.insert("_short_body_inherited_font_floor_pt".to_string(), Value::from(py_round(new_floor, 2)));
        }
        set_page_body_anchor_decision(payload, &PageBodyAnchorDecision { target_font_pt: target_font, applied: true });
    }
}

fn page_body_font_anchors(body_payloads: &[Value], page_text_width_med: f64) -> Vec<Value> {
    let mut candidates: Vec<Value> = body_payloads
        .iter()
        .filter(|p| {
            is_body_context_text_payload(p)
                && !payload_bool(p, "dense_small_box")
                && !payload_bool(p, "heavy_dense_small_box")
                && payload_f64(p, "font_size_pt", 0.0) > 0.0
                && payload_width(p) >= (page_text_width_med * typography::PAGE_BODY_FONT_ANCHOR_MIN_WIDTH_RATIO).max(1.0)
                && payload_height(p) >= typography::PAGE_BODY_FONT_ANCHOR_MIN_HEIGHT_PT
                && source_line_count(p) >= typography::PAGE_BODY_FONT_ANCHOR_MIN_LINES
        })
        .cloned()
        .collect();
    candidates.sort_by(|a, b| anchor_score(b).partial_cmp(&anchor_score(a)).unwrap());
    candidates.truncate(typography::PAGE_BODY_FONT_ANCHOR_COUNT);
    candidates
}

fn anchor_score(payload: &Value) -> f64 {
    // Python: `translated_text or source_text or ""`, then strip.
    let raw = payload_string(payload, "translated_text", "");
    let raw = if raw.is_empty() { payload_string(payload, "source_text", "") } else { raw };
    let text_len = raw.trim().chars().count();
    payload_height(payload) * 2.0 + payload_width(payload) * 0.25 + (160.0_f64).min(text_len as f64)
}

fn low_anchor_font_target(payloads: &[Value]) -> f64 {
    let mut fonts: Vec<f64> = payloads
        .iter()
        .map(|p| payload_f64(p, "font_size_pt", 0.0))
        .filter(|f| *f > 0.0)
        .collect();
    fonts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    fonts.first().copied().map(|f| py_round(f, 2)).unwrap_or(0.0)
}

fn source_line_count(payload: &Value) -> i64 {
    Item::from_json_value(payload.get("item").unwrap_or(&Value::Null))
        .lines
        .len() as i64
}

fn is_page_anchor_font_candidate(payload: &Value, page_text_width_med: f64) -> bool {
    if !is_body_context_text_payload(payload) {
        return false;
    }
    if payload_bool(payload, "dense_small_box") || payload_bool(payload, "heavy_dense_small_box") {
        return false;
    }
    if payload_bool(payload, "prefer_typst_fit") && payload_density(payload, None, None) > typography::PAGE_BODY_FONT_ANCHOR_APPLY_DENSITY_LIMIT {
        return false;
    }
    payload_width(payload) >= (page_text_width_med * 0.32).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(x0: f64, y0: f64, x1: f64, y1: f64, font: f64, lines: usize) -> Value {
        let item_lines: Vec<Value> = (0..lines)
            .map(|i| {
                json!({
                    "bbox": [x0, y0 + i as f64 * 20.0, x1, y0 + (i + 1) as f64 * 20.0],
                    "spans": [{"type": "text", "text": "行"}],
                })
            })
            .collect();
        json!({
            "inner_bbox": [x0, y0, x1, y1],
            "translated_text": "a",
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": 0.5,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "prefer_typst_fit": false,
            "item": {"source_text": "x", "lines": item_lines},
        })
    }

    #[test]
    fn anchors_page_font_and_applies() {
        let body_payloads = vec![payload(50.0, 0.0, 250.0, 80.0, 12.0, 3), payload(50.0, 90.0, 250.0, 170.0, 11.0, 3)];
        let mut all_payloads = vec![payload(50.0, 180.0, 250.0, 200.0, 13.0, 1)];
        apply_page_body_font_anchor(&body_payloads, &mut all_payloads, 200.0);
        assert_eq!(all_payloads[0]["page_body_font_size_pt"], json!(11.0));
        assert_eq!(all_payloads[0]["font_size_pt"], json!(11.0));
        assert_eq!(all_payloads[0]["_page_body_anchor_font_applied"], json!(true));
    }

    #[test]
    fn candidate_at_or_below_target_just_annotates() {
        let body_payloads = vec![payload(50.0, 0.0, 250.0, 80.0, 12.0, 3), payload(50.0, 90.0, 250.0, 170.0, 11.0, 3)];
        let mut all_payloads = vec![payload(50.0, 180.0, 250.0, 200.0, 10.0, 1)];
        apply_page_body_font_anchor(&body_payloads, &mut all_payloads, 200.0);
        assert_eq!(all_payloads[0]["font_size_pt"], json!(10.0));
        assert_eq!(all_payloads[0]["page_body_font_size_pt"], json!(11.0));
    }
}
