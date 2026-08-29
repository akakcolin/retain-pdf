// Port of services/rendering/layout/payload/body_common.py — the body-target /
// density-resolution leaves the body pipeline (C3-N3) reads. Operates on the raw
// JSON block-payload dicts.

use serde_json::{json, Value};

use crate::item::{formula_map, Item};
use crate::layout::body_context::{payload_center_x, BODY_DENSITY_TARGET_MAX};
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::layout::typography_policy as typography;
use crate::payload::capacity::{estimated_render_height_pt, estimated_required_lines, text_demand_units};
use crate::semantics::{block_kind, is_bodylike_block, is_caption_like_block, is_footnote_like_block};
use crate::util::{median_f64, py_round};

pub const BODY_DENSITY_TARGET_MIN: f64 = 0.82;
pub const SHORT_BODY_INHERIT_MAX_HEIGHT_PT: f64 = 16.0;
pub const SHORT_BODY_INHERIT_LEFT_TOLERANCE_PT: f64 = 22.0;
pub const SHORT_BODY_INHERIT_CENTER_TOLERANCE_RATIO: f64 = 0.18;
pub const BODY_CONTEXT_MIN_ANCHORS: usize = 2;

/// `payload_is_continuation_member`: the item carries a non-empty continuation
/// group (either key), so it continues an earlier body block. Group ids are
/// strings in this pipeline.
pub fn payload_is_continuation_member(payload: &Value) -> bool {
    let item = payload.get("item").unwrap_or(&Value::Null);
    let text = item
        .get("continuation_group")
        .or_else(|| item.get("continuation_group_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    !text.trim().is_empty()
}

/// `payload_density`: estimated render height over inner height, using explicit
/// font/leading when given (the density-capping callers pass only one).
pub fn payload_density(
    payload: &Value,
    font_size_pt: Option<f64>,
    leading_em: Option<f64>,
) -> f64 {
    let inner_height = payload_density_height(payload);
    let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
    let estimated_height = estimated_render_height_pt(
        &bbox,
        &payload_string(payload, "translated_text", ""),
        &formula_map(payload.get("formula_map")),
        font_size_pt.unwrap_or_else(|| payload_f64(payload, "font_size_pt", 0.0)),
        leading_em.unwrap_or_else(|| payload_f64(payload, "leading_em", 0.0)),
    );
    estimated_height / inner_height
}

/// `payload_density_height`: `density_effective_height_pt` wins (the tall-bbox
/// annotation), else the raw inner bbox height, floored at 8pt.
pub fn payload_density_height(payload: &Value) -> f64 {
    let effective = payload_f64(payload, "density_effective_height_pt", 0.0);
    if effective > 0.0 {
        return effective.max(8.0);
    }
    let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
    (bbox[3] - bbox[1]).max(8.0)
}

/// `payload_width`: inner bbox width, floored at 0.
pub fn payload_width(payload: &Value) -> f64 {
    let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
    (bbox[2] - bbox[0]).max(0.0)
}

/// `payload_height`: inner bbox height, floored at 0.
pub fn payload_height(payload: &Value) -> f64 {
    let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
    (bbox[3] - bbox[1]).max(0.0)
}

/// `required_lines`: `estimated_required_lines`, or 1 when the bbox or font is
/// missing/invalid.
pub fn required_lines(payload: &Value) -> i64 {
    let bbox = inner_bbox4(payload);
    let font_size = payload_f64(payload, "font_size_pt", 0.0);
    match bbox {
        Some(b) if font_size > 0.0 => estimated_required_lines(
            &b,
            &payload_string(payload, "translated_text", ""),
            &formula_map(payload.get("formula_map")),
            font_size,
        ),
        _ => 1,
    }
}

/// `resolve_body_targets`: annotate tall-bbox density heights, compute the
/// stable body font median (annotating `page_body_font_size_pt` on each), and
/// return `(body_font_median, body_density_target, body_pressure_median)`.
pub fn resolve_body_targets(body_payloads: &mut Vec<Value>) -> (f64, f64, f64) {
    annotate_tall_body_density_heights(body_payloads);

    let stable_fonts: Vec<f64> = body_payloads
        .iter()
        .filter(|p| !payload_bool(p, "dense_small_box") && !payload_bool(p, "heavy_dense_small_box"))
        .map(|p| payload_f64(p, "font_size_pt", 0.0))
        .collect();
    let body_font_median = if stable_fonts.is_empty() {
        median_f64(
            &body_payloads
                .iter()
                .map(|p| payload_f64(p, "font_size_pt", 0.0))
                .collect::<Vec<_>>(),
        )
    } else {
        median_f64(&stable_fonts)
    };
    for p in body_payloads.iter_mut() {
        if let Some(obj) = p.as_object_mut() {
            obj.insert("page_body_font_size_pt".to_string(), Value::from(py_round(body_font_median, 2)));
        }
    }

    let mut body_density_values: Vec<f64> = Vec::new();
    let mut body_pressure_values: Vec<f64> = Vec::new();
    for p in body_payloads.iter() {
        let inner_height = payload_density_height(p);
        let bbox = inner_bbox4(p).unwrap_or([0.0; 4]);
        let inner_width = (bbox[2] - bbox[0]).max(8.0);
        let translated = payload_string(p, "translated_text", "");
        let formula = formula_map(p.get("formula_map"));
        let demand = text_demand_units(&translated, &formula);
        let estimated_height =
            estimated_render_height_pt(&bbox, &translated, &formula, payload_f64(p, "font_size_pt", 0.0), payload_f64(p, "leading_em", 0.0));
        body_density_values.push(estimated_height / inner_height);
        body_pressure_values.push(demand / (inner_width * inner_height).max(1.0));
    }

    let body_density_target = if body_density_values.is_empty() {
        0.72
    } else {
        median_f64(&body_density_values).clamp(BODY_DENSITY_TARGET_MIN, BODY_DENSITY_TARGET_MAX)
    };
    let body_pressure_median = if body_pressure_values.is_empty() {
        0.0
    } else {
        median_f64(&body_pressure_values)
    };
    (body_font_median, body_density_target, body_pressure_median)
}

/// `annotate_tall_body_density_heights`: for body text whose OCR bbox is far
/// taller than its natural text height, record an effective density height so
/// density decisions ignore source paragraph slack.
pub fn annotate_tall_body_density_heights(body_payloads: &mut Vec<Value>) {
    struct Candidate {
        idx: usize,
        ratio: f64,
        natural_h: f64,
        line_count: i64,
    }

    let mut ratios: Vec<f64> = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for (idx, payload) in body_payloads.iter_mut().enumerate() {
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("density_effective_height_pt");
            obj.remove("density_height_ratio");
            obj.remove("density_height_policy");
        }
        if !is_body_context_text_payload(payload) {
            continue;
        }
        if payload_bool(payload, "dense_small_box") || payload_bool(payload, "heavy_dense_small_box") {
            continue;
        }
        let line_count = required_lines(payload);
        let bbox_h = payload_height(payload);
        if line_count < typography::BODY_TALL_BBOX_MIN_LINES
            || bbox_h < typography::BODY_TALL_BBOX_MIN_HEIGHT_PT
        {
            continue;
        }
        let natural_h = natural_body_text_height(payload, line_count);
        if natural_h <= 0.0 {
            continue;
        }
        let ratio = bbox_h / natural_h;
        ratios.push(ratio);
        candidates.push(Candidate {
            idx,
            ratio,
            natural_h,
            line_count,
        });
    }
    if ratios.is_empty() {
        return;
    }
    let page_ratio_ref = median_f64(&ratios).max(1.0);
    for cand in &candidates {
        if cand.ratio < typography::BODY_TALL_BBOX_HEIGHT_RATIO_TRIGGER
            || cand.ratio < page_ratio_ref * typography::BODY_TALL_BBOX_PAGE_RATIO_MULTIPLIER
        {
            continue;
        }
        let bbox_h = payload_height(&body_payloads[cand.idx]);
        let effective = (cand.natural_h * typography::BODY_TALL_BBOX_EFFECTIVE_NATURAL_MULTIPLIER)
            .max(bbox_h * typography::BODY_TALL_BBOX_EFFECTIVE_MIN_ORIGINAL_RATIO)
            .min(bbox_h);
        if effective >= bbox_h - 0.5 {
            continue;
        }
        if let Some(obj) = body_payloads[cand.idx].as_object_mut() {
            obj.insert("density_effective_height_pt".to_string(), Value::from(py_round(effective, 2)));
            obj.insert("density_height_ratio".to_string(), Value::from(py_round(cand.ratio, 3)));
            obj.insert(
                "density_height_policy".to_string(),
                json!({
                    "kind": "tall_body_bbox_effective_height",
                    "bbox_height_pt": py_round(bbox_h, 2),
                    "effective_height_pt": py_round(effective, 2),
                    "natural_height_pt": py_round(cand.natural_h, 2),
                    "height_ratio": py_round(cand.ratio, 3),
                    "page_ratio_ref": py_round(page_ratio_ref, 3),
                    "line_count": cand.line_count,
                }),
            );
        }
    }
}

/// `_natural_body_text_height`: the estimated text block height for a font /
/// leading / line count.
fn natural_body_text_height(payload: &Value, line_count: i64) -> f64 {
    let font_size = payload_f64(payload, "font_size_pt", 0.0);
    let leading = payload_f64(payload, "leading_em", 0.0);
    if font_size <= 0.0 || line_count <= 0 {
        return 0.0;
    }
    font_size * (line_count as f64).max(1.0) * (1.0 + leading.max(0.0))
}

/// `same_body_column`: the payload shares a column with the anchor by left edge
/// or center (tolerance scaled by the wider box).
pub fn same_body_column(payload: &Value, anchor: &Value, page_text_width_med: f64) -> bool {
    let pb = inner_bbox4(payload).unwrap_or([0.0; 4]);
    let ab = inner_bbox4(anchor).unwrap_or([0.0; 4]);
    let left_delta = (pb[0] - ab[0]).abs();
    let center_delta = (payload_center_x(payload) - payload_center_x(anchor)).abs();
    let width_ref = page_text_width_med.max(payload_width(anchor)).max(1.0);
    left_delta <= SHORT_BODY_INHERIT_LEFT_TOLERANCE_PT
        || center_delta <= width_ref * SHORT_BODY_INHERIT_CENTER_TOLERANCE_RATIO
}

/// `is_body_context_text_payload`: a body-context text block eligible for the
/// tall-bbox density annotation.
pub fn is_body_context_text_payload(payload: &Value) -> bool {
    if payload_is_continuation_member(payload) {
        return false;
    }
    if payload_string(payload, "render_kind", "") != "markdown" {
        return false;
    }
    if payload.get("title_fit").map(|v| !v.is_null()).unwrap_or(false) {
        return false;
    }
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    if is_caption_like_block(&item) || is_footnote_like_block(&item) {
        return false;
    }
    payload_bool(payload, "is_body") || block_kind(&item) == "text" || is_bodylike_block(&item)
}

/// `body_context_anchors`: wide + tall body payloads usable as column anchors.
pub fn body_context_anchors(body_payloads: &[Value], page_text_width_med: f64) -> Vec<Value> {
    body_payloads
        .iter()
        .filter(|p| payload_width(p) >= (page_text_width_med * 0.72).max(1.0) && payload_height(p) >= 18.0)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body_payload(font_size_pt: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
        json!({
            "inner_bbox": [x0, y0, x1, y1],
            "translated_text": "abc",
            "formula_map": [],
            "font_size_pt": font_size_pt,
            "leading_em": 0.5,
            "render_kind": "markdown",
            "is_body": true,
            "item": {},
        })
    }

    #[test]
    fn continuation_member_reads_group() {
        assert!(payload_is_continuation_member(&json!({"item": {"continuation_group": "g1"}})));
        assert!(payload_is_continuation_member(&json!({"item": {"continuation_group_id": "g2"}})));
        assert!(!payload_is_continuation_member(&json!({"item": {}})));
        assert!(!payload_is_continuation_member(&json!({})));
    }

    #[test]
    fn density_height_uses_effective() {
        let mut p = body_payload(12.0, 0.0, 0.0, 100.0, 50.0);
        assert_eq!(payload_density_height(&p), 50.0);
        p.as_object_mut().unwrap().insert("density_effective_height_pt".to_string(), json!(30.0));
        assert_eq!(payload_density_height(&p), 30.0);
    }

    #[test]
    fn resolve_body_targets_annotates_median_font() {
        let mut payloads = vec![
            body_payload(11.0, 0.0, 0.0, 200.0, 60.0),
            body_payload(12.0, 0.0, 0.0, 200.0, 60.0),
            body_payload(13.0, 0.0, 0.0, 200.0, 60.0),
        ];
        let (median, density, pressure) = resolve_body_targets(&mut payloads);
        assert_eq!(median, 12.0);
        assert_eq!(payloads[0]["page_body_font_size_pt"], json!(12.0));
        assert!((0.82..=0.92).contains(&density));
        assert!(pressure >= 0.0);
    }

    #[test]
    fn same_body_column_center_ok() {
        let a = body_payload(12.0, 100.0, 100.0, 200.0, 160.0);
        let b = body_payload(12.0, 102.0, 160.0, 198.0, 220.0);
        assert!(same_body_column(&a, &b, 400.0));
    }

    #[test]
    fn body_context_text_excludes_captions() {
        let caption = json!({
            "render_kind": "markdown",
            "is_body": false,
            "item": {"layout_role": "caption"},
            "inner_bbox": [0.0, 0.0, 100.0, 20.0],
            "translated_text": "cap",
            "formula_map": [],
        });
        assert!(!is_body_context_text_payload(&caption));
        assert!(is_body_context_text_payload(&body_payload(12.0, 0.0, 0.0, 100.0, 50.0)));
    }
}
