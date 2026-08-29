// Port of services/rendering/layout/payload/annotation_font_policy.py — the
// caption/footnote font-unify and underfilled-density recovery policy (C3-N3).
// Operates on the raw JSON block-payload dicts, mutating in place.

use serde_json::Value;

use crate::item::Item;
use crate::layout::body_common::{is_body_context_text_payload, payload_density};
use crate::layout::payload_dict::{payload_f64, payload_string};
use crate::layout::typography_policy as typography;
use crate::semantics::{is_caption_like_block, is_footnote_like_block};
use crate::util::{median_f64, py_round};

/// `unify_annotation_fonts`: pull caption/footnote fonts toward the role target,
/// capped by the body font reference.
pub fn unify_annotation_fonts(ordered_payloads: &mut Vec<Value>) {
    let body_font_cap = body_font_reference(ordered_payloads);
    for role in ["caption", "footnote"] {
        let role_payloads: Vec<usize> = ordered_payloads
            .iter()
            .enumerate()
            .filter(|(_, payload)| annotation_role(payload) == role && payload_string(payload, "render_kind", "") == "markdown" && payload_f64(payload, "font_size_pt", 0.0) > 0.0)
            .map(|(idx, _)| idx)
            .collect();
        if role_payloads.len() < 2 {
            continue;
        }
        let role_values: Vec<Value> = role_payloads.iter().map(|&idx| ordered_payloads[idx].clone()).collect();
        let mut target_font = low_role_font_target(&role_values);
        if role == "caption" {
            target_font = annotation_target_font(&role_values, target_font, typography::CAPTION_FONT_UNIFY_TARGET_BONUS_PT);
            target_font = cap_annotation_font(target_font, role, body_font_cap);
        } else {
            target_font = annotation_target_font(&role_values, target_font, typography::FOOTNOTE_FONT_UNIFY_TARGET_BONUS_PT);
            target_font = cap_annotation_font(target_font, role, body_font_cap);
        }
        for idx in role_payloads {
            let current_font = payload_f64(&ordered_payloads[idx], "font_size_pt", 0.0);
            if (current_font - target_font).abs() <= typography::ANNOTATION_FONT_UNIFY_APPLY_TOLERANCE_PT {
                continue;
            }
            let new_font = if current_font > target_font {
                target_font.max(current_font - typography::ANNOTATION_FONT_UNIFY_MAX_SHRINK_PT)
            } else if role == "caption" {
                target_font.min(current_font + typography::CAPTION_FONT_UNIFY_MAX_GROW_PT)
            } else {
                target_font.min(current_font + typography::FOOTNOTE_FONT_UNIFY_MAX_GROW_PT)
            };
            let obj = ordered_payloads[idx].as_object_mut().expect("payload is an object");
            obj.insert("font_size_pt".to_string(), Value::from(py_round(new_font, 2)));
        }
    }
}

/// `recover_underfilled_annotation_density`: grow caption/footnote font/leading
/// toward the recovery target, then clamp to the body-font cap.
pub fn recover_underfilled_annotation_density(ordered_payloads: &mut Vec<Value>) {
    let body_font_cap = body_font_reference(ordered_payloads);
    for payload in ordered_payloads {
        let role = annotation_role(payload);
        if role.is_empty() {
            continue;
        }
        if payload_string(payload, "render_kind", "") != "markdown" {
            continue;
        }
        if payload_f64(payload, "font_size_pt", 0.0) <= 0.0 || payload_f64(payload, "leading_em", 0.0) <= 0.0 {
            continue;
        }
        if payload_density(payload, None, None) >= typography::ANNOTATION_UNDERFILLED_DENSITY_FLOOR_TRIGGER {
            clamp_annotation_payload_font(payload, role, body_font_cap);
            continue;
        }
        recover_annotation_payload_density(payload, role, body_font_cap);
        clamp_annotation_payload_font(payload, role, body_font_cap);
    }
}

fn annotation_role(payload: &Value) -> &'static str {
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    if is_footnote_like_block(&item) {
        return "footnote";
    }
    if is_caption_like_block(&item) {
        return "caption";
    }
    ""
}

fn low_role_font_target(payloads: &[Value]) -> f64 {
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
    let index = ((fonts.len() - 1) as f64 * typography::ANNOTATION_FONT_UNIFY_TARGET_QUANTILE) as usize;
    py_round(fonts[index], 2)
}

fn annotation_target_font(payloads: &[Value], low_target: f64, target_bonus_pt: f64) -> f64 {
    let mut fonts: Vec<f64> = payloads
        .iter()
        .map(|p| payload_f64(p, "font_size_pt", 0.0))
        .filter(|f| *f > 0.0)
        .collect();
    fonts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if fonts.is_empty() {
        return low_target;
    }
    let fonts = without_extreme_small_fonts(fonts);
    let mid = fonts[fonts.len() / 2];
    py_round(mid.min(low_target + target_bonus_pt), 2)
}

fn without_extreme_small_fonts(fonts: Vec<f64>) -> Vec<f64> {
    if fonts.len() < typography::ANNOTATION_FONT_UNIFY_MIN_FILTERED_COUNT + 1 {
        return fonts;
    }
    let median_font = median_f64(&fonts);
    let floor = (median_font * typography::ANNOTATION_FONT_UNIFY_EXTREME_SMALL_RATIO)
        .max(median_font - typography::ANNOTATION_FONT_UNIFY_EXTREME_SMALL_DELTA_PT);
    let filtered: Vec<f64> = fonts.iter().cloned().filter(|f| *f >= floor).collect();
    if filtered.len() < typography::ANNOTATION_FONT_UNIFY_MIN_FILTERED_COUNT {
        return fonts;
    }
    filtered
}

fn body_font_reference(payloads: &[Value]) -> Option<f64> {
    let mut fonts: Vec<f64> = payloads
        .iter()
        .filter(|p| is_body_context_text_payload(p) && payload_f64(p, "font_size_pt", 0.0) > 0.0)
        .map(|p| payload_f64(p, "font_size_pt", 0.0))
        .collect();
    fonts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if fonts.is_empty() {
        return None;
    }
    Some(fonts[(fonts.len() - 1) / 2])
}

fn cap_annotation_font(font_size_pt: f64, role: &str, body_font_cap: Option<f64>) -> f64 {
    match body_font_cap {
        Some(cap) if cap > 0.0 => {
            let ratio = if role == "caption" {
                typography::CAPTION_BODY_FONT_CAP_RATIO
            } else {
                typography::FOOTNOTE_BODY_FONT_CAP_RATIO
            };
            font_size_pt.min(cap * ratio)
        }
        _ => font_size_pt,
    }
}

fn clamp_annotation_payload_font(payload: &mut Value, role: &str, body_font_cap: Option<f64>) {
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    let capped = cap_annotation_font(current_font, role, body_font_cap);
    if capped < current_font {
        payload
            .as_object_mut()
            .expect("payload is an object")
            .insert("font_size_pt".to_string(), Value::from(py_round(capped, 2)));
    }
}

fn recover_annotation_payload_density(payload: &mut Value, role: &str, body_font_cap: Option<f64>) {
    for _ in 0..typography::ANNOTATION_UNDERFILLED_RECOVERY_MAX_ITERATIONS {
        if payload_density(payload, None, None) >= typography::ANNOTATION_UNDERFILLED_DENSITY_RECOVERY_TARGET {
            return;
        }
        recover_annotation_font_step(payload, role, body_font_cap);
        if payload_density(payload, None, None) >= typography::ANNOTATION_UNDERFILLED_DENSITY_RECOVERY_TARGET {
            return;
        }
        let changed = recover_annotation_leading_step(payload, role);
        if !changed {
            return;
        }
    }
}

fn recover_annotation_font_step(payload: &mut Value, role: &str, body_font_cap: Option<f64>) -> bool {
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    if current_font <= 0.0 {
        return false;
    }
    let step = if role == "caption" {
        typography::CAPTION_UNDERFILLED_RECOVERY_FONT_STEP_PT
    } else {
        typography::FOOTNOTE_UNDERFILLED_RECOVERY_FONT_STEP_PT
    };
    let target_font = cap_annotation_font(font_for_annotation_recovery_density(payload), role, body_font_cap)
        .min(current_font + step);
    let best = largest_annotation_font_within_density(payload, current_font, target_font, typography::ANNOTATION_UNDERFILLED_DENSITY_SAFE_MAX);
    if best <= current_font + 0.02 {
        return false;
    }
    payload
        .as_object_mut()
        .expect("payload is an object")
        .insert("font_size_pt".to_string(), Value::from(py_round(best, 2)));
    true
}

fn recover_annotation_leading_step(payload: &mut Value, role: &str) -> bool {
    let current_leading = payload_f64(payload, "leading_em", 0.0);
    if current_leading <= 0.0 {
        return false;
    }
    let (cap, step) = if role == "caption" {
        (typography::CAPTION_UNDERFILLED_RECOVERY_LEADING_CAP_EM, typography::CAPTION_UNDERFILLED_RECOVERY_LEADING_STEP_EM)
    } else {
        (typography::FOOTNOTE_UNDERFILLED_RECOVERY_LEADING_CAP_EM, typography::FOOTNOTE_UNDERFILLED_RECOVERY_LEADING_STEP_EM)
    };
    let target_leading = cap.min(current_leading + step);
    let best = largest_annotation_leading_within_density(payload, current_leading, target_leading, typography::ANNOTATION_UNDERFILLED_DENSITY_SAFE_MAX);
    if best <= current_leading + 0.01 {
        return false;
    }
    payload
        .as_object_mut()
        .expect("payload is an object")
        .insert("leading_em".to_string(), Value::from(py_round(best, 2)));
    true
}

fn font_for_annotation_recovery_density(payload: &Value) -> f64 {
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    let density = payload_density(payload, None, None);
    if current_font <= 0.0 || density <= 0.0 {
        return current_font;
    }
    let scale = (typography::ANNOTATION_UNDERFILLED_DENSITY_RECOVERY_TARGET / density.max(0.01)).sqrt();
    current_font * scale
}

fn largest_annotation_font_within_density(payload: &Value, low: f64, high: f64, density_limit: f64) -> f64 {
    let mut low = low;
    let mut high = high;
    let mut best = low;
    for _ in 0..8 {
        let mid = (low + high) / 2.0;
        if payload_density(payload, Some(mid), None) <= density_limit {
            best = mid;
            low = mid;
        } else {
            high = mid;
        }
    }
    best
}

fn largest_annotation_leading_within_density(payload: &Value, low: f64, high: f64, density_limit: f64) -> f64 {
    let mut low = low;
    let mut high = high;
    let mut best = low;
    for _ in 0..8 {
        let mid = (low + high) / 2.0;
        if payload_density(payload, None, Some(mid)) <= density_limit {
            best = mid;
            low = mid;
        } else {
            high = mid;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn caption_payload(font: f64, text: &str) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 250.0, 40.0],
            "translated_text": text,
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": 0.5,
            "render_kind": "markdown",
            "is_body": false,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {"block_type": "figure", "layout_role": "caption", "source_text": "x"},
        })
    }

    fn body_payload(font: f64) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 250.0, 40.0],
            "translated_text": "a",
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
    fn unifies_caption_fonts() {
        let mut payloads = vec![caption_payload(9.0, "图一说明"), caption_payload(11.0, "图二说明")];
        unify_annotation_fonts(&mut payloads);
        let font0: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        let font1: f64 = payloads[1]["font_size_pt"].as_f64().unwrap();
        assert!((font0 - font1).abs() < 1.5);
    }

    #[test]
    fn single_caption_untouched() {
        let mut payloads = vec![caption_payload(9.0, "图一说明")];
        unify_annotation_fonts(&mut payloads);
        assert_eq!(payloads[0]["font_size_pt"], json!(9.0));
    }

    #[test]
    fn body_font_reference_uses_median() {
        let payloads = vec![body_payload(10.0), body_payload(12.0), body_payload(14.0)];
        assert_eq!(body_font_reference(&payloads), Some(12.0));
    }
}
