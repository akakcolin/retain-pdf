// Port of services/rendering/layout/payload/body_font_unify_policy.py — the body
// font-unify policy that pulls similar body fonts toward one page/book target
// (C3-N3, gated by FONT_UNIFY_MODE). Operates on raw JSON block-payload dicts.

use serde_json::Value;

use crate::layout::body_common::{
    body_context_anchors, is_body_context_text_payload, payload_density, payload_height, payload_width, required_lines,
    same_body_column,
};
use crate::layout::payload_dict::{payload_bool, payload_f64, payload_string};
use crate::layout::typography_policy as typography;
use crate::util::{median_f64, py_round};

/// `unify_similar_body_fonts`: apply the book/page body font target to every
/// same-column body candidate.
pub fn unify_similar_body_fonts(
    body_payloads: &[Value],
    all_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
    book_body_font_target: Option<f64>,
) {
    let anchors = stable_body_font_anchors(body_payloads, page_text_width_med);
    if anchors.len() < typography::BODY_FONT_UNIFY_ANCHOR_COUNT {
        return;
    }
    let eligible_indices: Vec<usize> = all_payloads
        .iter()
        .enumerate()
        .filter(|(_, p)| is_unify_candidate(p, &anchors, page_text_width_med))
        .map(|(idx, _)| idx)
        .collect();
    if eligible_indices.len() < 2 {
        return;
    }
    let target_font = match book_body_font_target {
        Some(target) if target > 0.0 => target,
        _ => {
            let eligible_values: Vec<Value> = eligible_indices.iter().map(|&idx| all_payloads[idx].clone()).collect();
            low_page_font_target(&eligible_values)
        }
    };
    if target_font <= 0.0 {
        return;
    }
    for idx in eligible_indices {
        apply_page_font_target(&mut all_payloads[idx], target_font);
    }
}

/// `resolve_book_body_font_target`: the lowest stable body font across pages, or
/// None when the book lacks enough stable anchors.
pub fn resolve_book_body_font_target(page_payloads: &[(Vec<Value>, f64)]) -> Option<f64> {
    let mut eligible: Vec<Value> = Vec::new();
    for (block_payloads, page_text_width_med) in page_payloads {
        let body_payloads: Vec<Value> = block_payloads
            .iter()
            .filter(|p| payload_bool(p, "is_body"))
            .cloned()
            .collect();
        if stable_body_font_anchors(&body_payloads, *page_text_width_med).len() < typography::BODY_FONT_UNIFY_ANCHOR_COUNT {
            continue;
        }
        for payload in block_payloads {
            if is_book_target_candidate(payload, *page_text_width_med) {
                eligible.push(payload.clone());
            }
        }
    }
    if eligible.len() < typography::BODY_FONT_UNIFY_ANCHOR_COUNT {
        return None;
    }
    let target = low_page_font_target(&eligible);
    if target > 0.0 {
        Some(target)
    } else {
        None
    }
}

fn stable_body_font_anchors(body_payloads: &[Value], page_text_width_med: f64) -> Vec<Value> {
    let mut candidates: Vec<Value> = body_payloads
        .iter()
        .filter(|p| {
            is_body_context_text_payload(p)
                && !payload_bool(p, "dense_small_box")
                && !payload_bool(p, "heavy_dense_small_box")
                && payload_f64(p, "font_size_pt", 0.0) > 0.0
                && payload_width(p) >= (page_text_width_med * typography::BODY_FONT_UNIFY_ANCHOR_MIN_WIDTH_RATIO).max(1.0)
                && payload_height(p) >= typography::BODY_FONT_UNIFY_ANCHOR_MIN_HEIGHT_PT
                && payload_density(p, None, None) <= typography::BODY_FONT_UNIFY_ANCHOR_MAX_DENSITY
        })
        .cloned()
        .collect();
    if candidates.len() >= typography::BODY_FONT_UNIFY_ANCHOR_COUNT {
        candidates.sort_by(|a, b| anchor_score(b).partial_cmp(&anchor_score(a)).unwrap());
        candidates.truncate(typography::BODY_FONT_UNIFY_ANCHOR_COUNT);
        return candidates;
    }
    body_context_anchors(body_payloads, page_text_width_med)
        .into_iter()
        .filter(|p| is_body_context_text_payload(p) && payload_f64(p, "font_size_pt", 0.0) > 0.0)
        .collect()
}

fn anchor_score(payload: &Value) -> f64 {
    // Python: `translated_text or source_text or ""`, then strip.
    let raw = payload_string(payload, "translated_text", "");
    let raw = if raw.is_empty() { payload_string(payload, "source_text", "") } else { raw };
    let text_len = raw.trim().chars().count();
    payload_height(payload) * 2.0 + payload_width(payload) * 0.25 + (180.0_f64).min(text_len as f64)
}

fn is_unify_candidate(payload: &Value, anchors: &[Value], page_text_width_med: f64) -> bool {
    if !is_body_context_text_payload(payload) {
        return false;
    }
    if payload_f64(payload, "font_size_pt", 0.0) <= 0.0 {
        return false;
    }
    if payload_width(payload) < (page_text_width_med * typography::BODY_FONT_UNIFY_CANDIDATE_MIN_WIDTH_RATIO).max(1.0) {
        return false;
    }
    anchors.iter().any(|anchor| same_body_column(payload, anchor, page_text_width_med))
}

fn is_book_target_candidate(payload: &Value, page_text_width_med: f64) -> bool {
    is_body_context_text_payload(payload)
        && !payload_bool(payload, "dense_small_box")
        && !payload_bool(payload, "heavy_dense_small_box")
        && payload_f64(payload, "font_size_pt", 0.0) > 0.0
        && payload_width(payload) >= (page_text_width_med * typography::BODY_FONT_UNIFY_ANCHOR_MIN_WIDTH_RATIO).max(1.0)
        && payload_height(payload) >= typography::BODY_FONT_UNIFY_ANCHOR_MIN_HEIGHT_PT
        && payload_density(payload, None, None) <= typography::BODY_FONT_UNIFY_ANCHOR_MAX_DENSITY
}

fn low_page_font_target(payloads: &[Value]) -> f64 {
    let mut fonts: Vec<f64> = payloads
        .iter()
        .map(|p| payload_f64(p, "font_size_pt", 0.0))
        .filter(|f| *f > 0.0)
        .collect();
    fonts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if fonts.is_empty() {
        return 0.0;
    }
    let fonts = without_extreme_small_fonts(fonts);
    let index = ((fonts.len() - 1) as f64 * typography::BODY_FONT_UNIFY_TARGET_QUANTILE) as usize;
    py_round(fonts[index], 2)
}

fn without_extreme_small_fonts(fonts: Vec<f64>) -> Vec<f64> {
    if fonts.len() < typography::BODY_FONT_UNIFY_MIN_FILTERED_COUNT + 1 {
        return fonts;
    }
    let median_font = median_f64(&fonts);
    let floor = (median_font * typography::BODY_FONT_UNIFY_EXTREME_SMALL_RATIO)
        .max(median_font - typography::BODY_FONT_UNIFY_EXTREME_SMALL_DELTA_PT);
    let filtered: Vec<f64> = fonts.iter().cloned().filter(|f| *f >= floor).collect();
    if filtered.len() < typography::BODY_FONT_UNIFY_MIN_FILTERED_COUNT {
        return fonts;
    }
    filtered
}

fn apply_page_font_target(payload: &mut Value, target_font: f64) {
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    let new_font = py_round(target_font, 2);
    let can_direct = can_render_unified_body_directly(payload);
    let density_ok = payload_density(payload, Some(target_font), None) <= typography::BODY_FONT_UNIFY_GROW_DENSITY_LIMIT;
    let applies = (current_font - target_font).abs() <= typography::BODY_FONT_UNIFY_APPLY_TOLERANCE_PT
        || current_font > target_font
        || can_direct
        || density_ok;
    let floor = payload_f64(payload, "_short_body_inherited_font_floor_pt", 0.0);
    let obj = payload.as_object_mut().expect("payload is an object");
    obj.insert("page_body_font_size_pt".to_string(), Value::from(new_font));
    if !applies {
        return;
    }
    obj.insert("font_size_pt".to_string(), Value::from(new_font));
    obj.insert("_body_font_unified".to_string(), Value::Bool(true));
    if can_direct {
        obj.insert("prefer_typst_fit".to_string(), Value::Bool(false));
    }
    if floor > 0.0 {
        obj.insert("_short_body_inherited_font_floor_pt".to_string(), Value::from(py_round(floor.min(new_font), 2)));
    }
}

fn can_render_unified_body_directly(payload: &Value) -> bool {
    if !payload_bool(payload, "dense_small_box") && !payload_bool(payload, "heavy_dense_small_box") {
        return true;
    }
    required_lines(payload) <= typography::BODY_FONT_UNIFY_DIRECT_DENSE_MAX_LINES
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(x0: f64, y0: f64, x1: f64, y1: f64, font: f64) -> Value {
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
            "item": {"source_text": "x"},
        })
    }

    #[test]
    fn unifies_same_column_candidates() {
        // Both same-column candidates converge to the low page target (9.0).
        let body_payloads = vec![payload(50.0, 0.0, 250.0, 40.0, 12.0), payload(50.0, 50.0, 250.0, 90.0, 11.0)];
        let mut all_payloads = vec![payload(50.0, 100.0, 250.0, 130.0, 9.0), payload(50.0, 140.0, 250.0, 170.0, 10.0)];
        unify_similar_body_fonts(&body_payloads, &mut all_payloads, 200.0, None);
        let font0: f64 = all_payloads[0]["font_size_pt"].as_f64().unwrap();
        let font1: f64 = all_payloads[1]["font_size_pt"].as_f64().unwrap();
        assert_eq!(font0, 9.0);
        assert_eq!(font1, 9.0);
        assert_eq!(all_payloads[0]["_body_font_unified"], json!(true));
        assert_eq!(all_payloads[1]["_body_font_unified"], json!(true));
    }

    #[test]
    fn book_target_from_pages() {
        let page_a = vec![payload(50.0, 0.0, 250.0, 40.0, 12.0), payload(50.0, 50.0, 250.0, 90.0, 11.0)];
        let page_b = vec![payload(50.0, 0.0, 250.0, 40.0, 12.5), payload(50.0, 50.0, 250.0, 90.0, 11.5)];
        let pages = vec![(page_a, 200.0), (page_b, 200.0)];
        let target = resolve_book_body_font_target(&pages);
        assert!(target.is_some());
        assert!(target.unwrap() > 0.0);
    }
}
