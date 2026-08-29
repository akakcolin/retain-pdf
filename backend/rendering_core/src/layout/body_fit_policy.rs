// Port of services/rendering/layout/payload/body_fit_policy.py — relaxes the
// fit-height budget for short body-context blocks that sit in a column with
// enough tall anchors (C3-N3). Operates on the raw JSON block-payload dicts,
// mutating in place.

use serde_json::Value;

use crate::layout::body_common::{
    body_context_anchors, is_body_context_text_payload, payload_height, same_body_column,
    BODY_CONTEXT_MIN_ANCHORS, SHORT_BODY_INHERIT_MAX_HEIGHT_PT,
};
use crate::util::{median_f64, py_round};

pub const BODY_SHORT_HEIGHT_RELAX_RATIO: f64 = 1.55;
pub const BODY_SHORT_HEIGHT_RELAX_MAX_EXTRA_PT: f64 = 10.0;

/// `relax_short_body_context_heights`: raise the fit budget of short body-context
/// blocks so they inherit the (larger) column anchor height.
pub fn relax_short_body_context_heights(
    body_payloads: &[Value],
    all_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    let anchors = body_context_anchors(body_payloads, page_text_width_med);
    if anchors.len() < BODY_CONTEXT_MIN_ANCHORS {
        return;
    }
    let anchor_heights: Vec<f64> = anchors
        .iter()
        .filter(|anchor| payload_height(anchor) > SHORT_BODY_INHERIT_MAX_HEIGHT_PT)
        .map(payload_height)
        .collect();
    if anchor_heights.is_empty() {
        return;
    }
    let target_height = median_f64(&anchor_heights);
    for payload in all_payloads {
        if !is_body_context_text_payload(payload) {
            continue;
        }
        let height = payload_height(payload);
        if height <= 0.0 || height > SHORT_BODY_INHERIT_MAX_HEIGHT_PT {
            continue;
        }
        let local_anchor_count = anchors
            .iter()
            .filter(|anchor| same_body_column(payload, anchor, page_text_width_med))
            .count();
        if local_anchor_count < BODY_CONTEXT_MIN_ANCHORS {
            continue;
        }
        let relaxed_height = target_height
            .min(height * BODY_SHORT_HEIGHT_RELAX_RATIO)
            .min(height + BODY_SHORT_HEIGHT_RELAX_MAX_EXTRA_PT);
        if relaxed_height <= height + 0.5 {
            continue;
        }
        let obj = payload.as_object_mut().expect("payload is an object");
        obj.insert("_relaxed_fit_height_pt".to_string(), Value::from(py_round(relaxed_height, 2)));
        obj.insert("prefer_typst_fit".to_string(), Value::Bool(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body_payload(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
        json!({
            "inner_bbox": [x0, y0, x1, y1],
            "translated_text": "abc",
            "formula_map": [],
            "font_size_pt": 12.0,
            "leading_em": 0.5,
            "render_kind": "markdown",
            "is_body": true,
            "item": {},
        })
    }

    #[test]
    fn relaxes_short_block_near_two_anchors() {
        // Two wide/tall anchors (width 200 >= 72, height >= 18) with a short
        // block (height 10) sharing the same column.
        let body_payloads = vec![body_payload(50.0, 0.0, 250.0, 30.0), body_payload(50.0, 40.0, 250.0, 80.0)];
        let mut all_payloads = vec![body_payload(50.0, 85.0, 250.0, 95.0)];
        relax_short_body_context_heights(&body_payloads, &mut all_payloads, 100.0);
        assert_eq!(all_payloads[0]["_relaxed_fit_height_pt"], json!(15.5));
        assert_eq!(all_payloads[0]["prefer_typst_fit"], json!(true));
    }

    #[test]
    fn skips_when_fewer_than_two_anchors() {
        let body_payloads = vec![body_payload(50.0, 0.0, 250.0, 30.0)];
        let mut all_payloads = vec![body_payload(50.0, 35.0, 250.0, 45.0)];
        relax_short_body_context_heights(&body_payloads, &mut all_payloads, 100.0);
        assert!(all_payloads[0].get("_relaxed_fit_height_pt").is_none());
    }

    #[test]
    fn skips_short_block_outside_anchor_column() {
        // Short block far to the right: no local anchors.
        let body_payloads = vec![body_payload(50.0, 0.0, 250.0, 30.0), body_payload(50.0, 40.0, 250.0, 80.0)];
        let mut all_payloads = vec![body_payload(300.0, 85.0, 500.0, 95.0)];
        relax_short_body_context_heights(&body_payloads, &mut all_payloads, 100.0);
        assert!(all_payloads[0].get("_relaxed_fit_height_pt").is_none());
    }
}
