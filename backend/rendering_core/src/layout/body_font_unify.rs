// Ports of services/rendering/layout/payload/body_font_unify_policy.py,
// body_font_inheritance_policy.py, and body_page_anchor_policy.py — the body
// font-unify / short-low-height inheritance / page-body-anchor policy family
// (C3-N3, unify gated by FONT_UNIFY_MODE). Operate on raw JSON block-payload
// dicts, mutating in place.

use serde_json::Value;

use crate::item::Item;
use crate::layout::body_context::{
    body_context_anchors, is_body_context_text_payload, payload_density, payload_height,
    payload_is_continuation_member, payload_width, required_lines, same_body_column,
    SHORT_BODY_INHERIT_MAX_HEIGHT_PT,
};
use crate::layout::payload_dict::{payload_bool, payload_f64, payload_string};
use crate::layout::typography_capacity::{set_page_body_anchor_decision, PageBodyAnchorDecision};
use crate::layout::typography_capacity as typography;
use crate::semantics::{block_kind, is_bodylike_block, is_caption_like_block, is_footnote_like_block};
use crate::util::{median_f64, py_round};

/// `unify_similar_body_fonts`: apply the book/page body font target to every
/// same-column body candidate.
pub fn unify_similar_body_fonts(
    body_payloads: &[Value],
    all_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
    book_body_font_target: Option<f64>,
) {
    let budget = typography::CapacityBudget::font_unify();
    let anchors = stable_body_font_anchors(body_payloads, page_text_width_med);
    if anchors.len() < budget.unify_anchor_count {
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
    let budget = typography::CapacityBudget::font_unify();
    let mut eligible: Vec<Value> = Vec::new();
    for (block_payloads, page_text_width_med) in page_payloads {
        let body_payloads: Vec<Value> = block_payloads
            .iter()
            .filter(|p| payload_bool(p, "is_body"))
            .cloned()
            .collect();
        if stable_body_font_anchors(&body_payloads, *page_text_width_med).len() < budget.unify_anchor_count {
            continue;
        }
        for payload in block_payloads {
            if is_book_target_candidate(payload, *page_text_width_med) {
                eligible.push(payload.clone());
            }
        }
    }
    if eligible.len() < budget.unify_anchor_count {
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
    let budget = typography::CapacityBudget::font_unify();
    let mut candidates: Vec<Value> = body_payloads
        .iter()
        .filter(|p| {
            is_body_context_text_payload(p)
                && !payload_bool(p, "dense_small_box")
                && !payload_bool(p, "heavy_dense_small_box")
                && payload_f64(p, "font_size_pt", 0.0) > 0.0
                && payload_width(p) >= (page_text_width_med * budget.unify_anchor_min_width_ratio).max(1.0)
                && payload_height(p) >= budget.unify_anchor_min_height_pt
                && payload_density(p, None, None) <= budget.unify_anchor_max_density
        })
        .cloned()
        .collect();
    if candidates.len() >= budget.unify_anchor_count {
        candidates.sort_by(|a, b| anchor_score(b).partial_cmp(&anchor_score(a)).unwrap());
        candidates.truncate(budget.unify_anchor_count);
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
    let budget = typography::CapacityBudget::font_unify();
    if !is_body_context_text_payload(payload) {
        return false;
    }
    if payload_f64(payload, "font_size_pt", 0.0) <= 0.0 {
        return false;
    }
    if payload_width(payload) < (page_text_width_med * budget.unify_candidate_min_width_ratio).max(1.0) {
        return false;
    }
    anchors.iter().any(|anchor| same_body_column(payload, anchor, page_text_width_med))
}

fn is_book_target_candidate(payload: &Value, page_text_width_med: f64) -> bool {
    let budget = typography::CapacityBudget::font_unify();
    is_body_context_text_payload(payload)
        && !payload_bool(payload, "dense_small_box")
        && !payload_bool(payload, "heavy_dense_small_box")
        && payload_f64(payload, "font_size_pt", 0.0) > 0.0
        && payload_width(payload) >= (page_text_width_med * budget.unify_anchor_min_width_ratio).max(1.0)
        && payload_height(payload) >= budget.unify_anchor_min_height_pt
        && payload_density(payload, None, None) <= budget.unify_anchor_max_density
}

fn low_page_font_target(payloads: &[Value]) -> f64 {
    let budget = typography::CapacityBudget::font_unify();
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
    let index = ((fonts.len() - 1) as f64 * budget.unify_target_quantile) as usize;
    py_round(fonts[index], 2)
}

fn without_extreme_small_fonts(fonts: Vec<f64>) -> Vec<f64> {
    let budget = typography::CapacityBudget::font_unify();
    if fonts.len() < budget.unify_min_filtered_count + 1 {
        return fonts;
    }
    let median_font = median_f64(&fonts);
    let floor = (median_font * budget.unify_extreme_small_ratio)
        .max(median_font - budget.unify_extreme_small_delta_pt);
    let filtered: Vec<f64> = fonts.iter().cloned().filter(|f| *f >= floor).collect();
    if filtered.len() < budget.unify_min_filtered_count {
        return fonts;
    }
    filtered
}

fn apply_page_font_target(payload: &mut Value, target_font: f64) {
    let budget = typography::CapacityBudget::font_unify();
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    let new_font = py_round(target_font, 2);
    let can_direct = can_render_unified_body_directly(payload);
    let density_ok = payload_density(payload, Some(target_font), None) <= budget.unify_grow_density_limit;
    let applies = (current_font - target_font).abs() <= budget.unify_apply_tolerance_pt
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
    let budget = typography::CapacityBudget::font_unify();
    if !payload_bool(payload, "dense_small_box") && !payload_bool(payload, "heavy_dense_small_box") {
        return true;
    }
    required_lines(payload) <= budget.unify_direct_dense_max_lines
}

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

/// `apply_page_body_font_anchor`: anchor the page body font to the lowest
/// stable body font, applying it to same-column candidates.
pub fn apply_page_body_font_anchor(
    body_payloads: &[Value],
    all_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    let budget = typography::CapacityBudget::page_anchor();
    let anchors = page_body_font_anchors(body_payloads, page_text_width_med);
    if anchors.len() < budget.page_anchor_count {
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
    let budget = typography::CapacityBudget::page_anchor();
    let mut candidates: Vec<Value> = body_payloads
        .iter()
        .filter(|p| {
            is_body_context_text_payload(p)
                && !payload_bool(p, "dense_small_box")
                && !payload_bool(p, "heavy_dense_small_box")
                && payload_f64(p, "font_size_pt", 0.0) > 0.0
                && payload_width(p) >= (page_text_width_med * budget.page_anchor_min_width_ratio).max(1.0)
                && payload_height(p) >= budget.page_anchor_min_height_pt
                && source_line_count(p) >= budget.page_anchor_min_lines
        })
        .cloned()
        .collect();
    candidates.sort_by(|a, b| page_anchor_score(b).partial_cmp(&page_anchor_score(a)).unwrap());
    candidates.truncate(budget.page_anchor_count);
    candidates
}

fn page_anchor_score(payload: &Value) -> f64 {
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
    let budget = typography::CapacityBudget::page_anchor();
    if !is_body_context_text_payload(payload) {
        return false;
    }
    if payload_bool(payload, "dense_small_box") || payload_bool(payload, "heavy_dense_small_box") {
        return false;
    }
    if payload_bool(payload, "prefer_typst_fit") && payload_density(payload, None, None) > budget.page_anchor_apply_density_limit {
        return false;
    }
    payload_width(payload) >= (page_text_width_med * 0.32).max(1.0)
}

#[cfg(test)]
mod tests_unify {
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

#[cfg(test)]
mod tests_inheritance {
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

#[cfg(test)]
mod tests_page_anchor {
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
