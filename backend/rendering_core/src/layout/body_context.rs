// Ports of services/rendering/layout/payload/body_common.py and body_context.py
// — the body-target / density-resolution leaves plus the body-context geometry,
// adjacent-body smoothing and density capping helpers the body pipeline (C3-N3)
// consumes. Operates on the raw JSON block-payload dicts.

use serde_json::{json, Value};

use crate::item::{formula_map, Item};
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::layout::typography_capacity as typography;
use crate::leading_fit::{
    normalize_leading_em_for_font_size, BODY_LEADING_FLOOR_MIN, BODY_LEADING_MAX, BODY_LEADING_MIN,
    BODY_LEADING_SIZE_ADJUST,
};
use crate::payload::capacity::{estimated_render_height_pt, estimated_required_lines, text_demand_units};
use crate::payload::text_common::{source_word_count, translated_zh_char_count};
use crate::semantics::{block_kind, is_bodylike_block, is_caption_like_block, is_footnote_like_block};
use crate::util::{median_f64, py_round};

pub const BODY_DENSITY_TARGET_MAX: f64 = 0.92;
pub const ADJACENT_BODY_SMOOTH_MAX_GAP_PT: f64 = 42.0;
pub const ADJACENT_BODY_SMOOTH_MIN_WIDTH_RATIO: f64 = 0.72;
pub const ADJACENT_BODY_SMOOTH_MIN_WIDTH_OVERLAP_RATIO: f64 = 0.64;
pub const ADJACENT_BODY_SMOOTH_MAX_LEFT_DELTA_PT: f64 = 18.0;
pub const ADJACENT_BODY_SMOOTH_MAX_CENTER_DELTA_PT: f64 = 22.0;
pub const ADJACENT_BODY_SMOOTH_MIN_BOX_HEIGHT_PT: f64 = 36.0;
pub const ADJACENT_BODY_SMOOTH_MIN_WIDTH_PT: f64 = 64.0;
pub const ADJACENT_BODY_SMOOTH_MIN_PAGE_WIDTH_RATIO: f64 = 0.38;
pub const ADJACENT_BODY_SMOOTH_MIN_SOURCE_WORDS: usize = 10;
pub const ADJACENT_BODY_SMOOTH_MIN_TRANSLATED_ZH_CHARS: usize = 18;
pub const ADJACENT_BODY_SMOOTH_MAX_FONT_DELTA_PT: f64 = 0.24;
pub const ADJACENT_BODY_SMOOTH_RELAXED_FONT_DELTA_PT: f64 = 0.34;
pub const ADJACENT_BODY_SMOOTH_MAX_LEADING_DELTA_EM: f64 = 0.06;
pub const ADJACENT_BODY_SMOOTH_RELAXED_LEADING_DELTA_EM: f64 = 0.09;
pub const ADJACENT_BODY_SMOOTH_GROW_DENSITY_MAX: f64 = 0.95;
pub const ADJACENT_BODY_SMOOTH_RELAXED_GROW_DENSITY_MAX: f64 = 0.99;

pub const BODY_DENSITY_TARGET_MIN: f64 = 0.82;
pub const SHORT_BODY_INHERIT_MAX_HEIGHT_PT: f64 = 16.0;
pub const SHORT_BODY_INHERIT_LEFT_TOLERANCE_PT: f64 = 22.0;
pub const SHORT_BODY_INHERIT_CENTER_TOLERANCE_RATIO: f64 = 0.18;
pub const BODY_CONTEXT_MIN_ANCHORS: usize = 2;

/// `page_box_area_ratio`: fraction of the page area covered by `bbox`, or 0.0
/// when `bbox` is not a 4-float list or either page dimension is missing or
/// non-positive.
pub fn page_box_area_ratio(bbox: &[f64], page_width: Option<f64>, page_height: Option<f64>) -> f64 {
    let page_width = match page_width {
        Some(v) if v > 0.0 => v,
        _ => return 0.0,
    };
    let page_height = match page_height {
        Some(v) if v > 0.0 => v,
        _ => return 0.0,
    };
    if bbox.len() != 4 {
        return 0.0;
    }
    let width = (bbox[2] - bbox[0]).max(0.0);
    let height = (bbox[3] - bbox[1]).max(0.0);
    if width <= 0.0 || height <= 0.0 {
        return 0.0;
    }
    (width * height) / (page_width * page_height)
}

/// `payload_inner_width`: inner bbox width, floored at 8pt.
pub fn payload_inner_width(payload: &Value) -> f64 {
    let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
    (bbox[2] - bbox[0]).max(8.0)
}

/// `payload_inner_height`: inner bbox height, floored at 8pt.
pub fn payload_inner_height(payload: &Value) -> f64 {
    let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
    (bbox[3] - bbox[1]).max(8.0)
}

pub fn payload_inner_top(payload: &Value) -> f64 {
    inner_bbox4(payload).unwrap_or([0.0; 4])[1]
}

pub fn payload_inner_bottom(payload: &Value) -> f64 {
    inner_bbox4(payload).unwrap_or([0.0; 4])[3]
}

pub fn payload_center_x(payload: &Value) -> f64 {
    let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
    (bbox[0] + bbox[2]) / 2.0
}

/// `payload_estimated_density`: estimated render height over effective inner
/// height (`density_effective_height_pt` wins), honoring explicit font/leading.
pub fn payload_estimated_density(
    payload: &Value,
    font_size_pt: Option<f64>,
    leading_em: Option<f64>,
) -> f64 {
    let effective = payload_f64(payload, "density_effective_height_pt", 0.0);
    let inner_height = if effective > 0.0 {
        effective.max(8.0)
    } else {
        payload_inner_height(payload)
    };
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

fn payload_has_enough_text_for_smoothing(payload: &Value) -> bool {
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    let source_words = source_word_count(&item);
    let translated_zh_chars = translated_zh_char_count(&payload_string(payload, "translated_text", ""));
    source_words >= ADJACENT_BODY_SMOOTH_MIN_SOURCE_WORDS
        || translated_zh_chars >= ADJACENT_BODY_SMOOTH_MIN_TRANSLATED_ZH_CHARS
}

fn is_adjacent_body_smoothing_candidate(payload: &Value, page_text_width_med: f64) -> bool {
    if !payload_bool(payload, "is_body") || payload_string(payload, "render_kind", "") != "markdown" {
        return false;
    }
    if payload_bool(payload, "heavy_dense_small_box") {
        return false;
    }
    if payload_inner_height(payload) < ADJACENT_BODY_SMOOTH_MIN_BOX_HEIGHT_PT {
        return false;
    }
    let width = payload_inner_width(payload);
    let mut min_width = ADJACENT_BODY_SMOOTH_MIN_WIDTH_PT;
    if page_text_width_med > 0.0 {
        min_width = min_width.max(page_text_width_med * ADJACENT_BODY_SMOOTH_MIN_PAGE_WIDTH_RATIO);
    }
    if width < min_width {
        return false;
    }
    payload_has_enough_text_for_smoothing(payload)
}

/// `is_same_column_adjacent_body_pair`: two consecutive body payloads stacked in
/// the same column and visually close enough to smooth font/leading rhythm.
pub fn is_same_column_adjacent_body_pair(current: &Value, nxt: &Value, page_text_width_med: f64) -> bool {
    if !is_adjacent_body_smoothing_candidate(current, page_text_width_med)
        || !is_adjacent_body_smoothing_candidate(nxt, page_text_width_med)
    {
        return false;
    }
    if payload_inner_top(nxt) < payload_inner_top(current) {
        return false;
    }

    let current_width = payload_inner_width(current);
    let next_width = payload_inner_width(nxt);
    let width_ratio = current_width.min(next_width) / current_width.max(next_width);
    if width_ratio < ADJACENT_BODY_SMOOTH_MIN_WIDTH_RATIO {
        return false;
    }

    let cb = inner_bbox4(current).unwrap_or([0.0; 4]);
    let nb = inner_bbox4(nxt).unwrap_or([0.0; 4]);
    let overlap_width = (cb[2].min(nb[2]) - cb[0].max(nb[0])).max(0.0);
    if overlap_width / current_width.min(next_width) < ADJACENT_BODY_SMOOTH_MIN_WIDTH_OVERLAP_RATIO {
        return false;
    }

    let gap = payload_inner_top(nxt) - payload_inner_bottom(current);
    let max_gap = ADJACENT_BODY_SMOOTH_MAX_GAP_PT
        .max(payload_inner_height(current).min(payload_inner_height(nxt)) * 0.45);
    if gap < -4.0 || gap > max_gap {
        return false;
    }

    let left_delta = (cb[0] - nb[0]).abs();
    let center_delta = (payload_center_x(current) - payload_center_x(nxt)).abs();
    let left_limit = ADJACENT_BODY_SMOOTH_MAX_LEFT_DELTA_PT.max(current_width.max(next_width) * 0.06);
    let center_limit = ADJACENT_BODY_SMOOTH_MAX_CENTER_DELTA_PT.max(current_width.max(next_width) * 0.08);
    if left_delta > left_limit && center_delta > center_limit {
        return false;
    }
    true
}

/// `cap_font_growth_by_density`: the largest font (8 bisection iterations) whose
/// estimated density stays within `density_limit`.
pub fn cap_font_growth_by_density(payload: &Value, target_font_size_pt: f64, density_limit: f64) -> f64 {
    let current_font_size = payload_f64(payload, "font_size_pt", 0.0);
    if target_font_size_pt <= current_font_size {
        return py_round(target_font_size_pt, 2);
    }
    let mut low = current_font_size;
    let mut high = target_font_size_pt;
    let mut best = current_font_size;
    for _ in 0..8 {
        let mid = (low + high) / 2.0;
        if payload_estimated_density(payload, Some(mid), None) <= density_limit {
            best = mid;
            low = mid;
        } else {
            high = mid;
        }
    }
    py_round(best, 2)
}

/// `cap_leading_growth_by_density`: the largest leading (8 bisection iterations)
/// whose estimated density stays within `density_limit`.
pub fn cap_leading_growth_by_density(payload: &Value, target_leading_em: f64, density_limit: f64) -> f64 {
    let current_leading = payload_f64(payload, "leading_em", 0.0);
    if target_leading_em <= current_leading {
        return py_round(target_leading_em, 2);
    }
    let mut low = current_leading;
    let mut high = target_leading_em;
    let mut best = current_leading;
    for _ in 0..8 {
        let mid = (low + high) / 2.0;
        if payload_estimated_density(payload, None, Some(mid)) <= density_limit {
            best = mid;
            low = mid;
        } else {
            high = mid;
        }
    }
    py_round(best, 2)
}

/// `normalize_body_payload_leading`: clamp leading to the body floor/ceiling,
/// using the page body font reference when available.
pub fn normalize_body_payload_leading(payload: &mut Value) {
    let page_font = payload_f64(payload, "page_body_font_size_pt", 0.0);
    let reference_font_size_pt = if page_font > 0.0 {
        page_font
    } else {
        payload_f64(payload, "font_size_pt", 0.0)
    };
    let cap = payload_f64(payload, "_body_dynamic_leading_cap_em", BODY_LEADING_MAX);
    let max_leading_em = BODY_LEADING_MAX.max(cap);
    let normalized = normalize_leading_em_for_font_size(
        payload_f64(payload, "font_size_pt", 0.0),
        payload_f64(payload, "leading_em", 0.0),
        reference_font_size_pt,
        BODY_LEADING_MIN,
        max_leading_em,
        BODY_LEADING_SIZE_ADJUST,
        Some(BODY_LEADING_FLOOR_MIN),
    );
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("leading_em".to_string(), Value::from(normalized));
    }
}

/// `_source_line_pitch_pressure`: source line pitch relative to font size (0 when
/// fewer than two pitched lines).
fn source_line_pitch_pressure(payload: &Value) -> f64 {
    let lines = payload
        .get("item")
        .and_then(|v| v.get("lines"))
        .and_then(|v| v.as_array());
    let Some(lines) = lines else { return 0.0 };
    if lines.len() < 2 {
        return 0.0;
    }
    let mut pitches: Vec<f64> = Vec::new();
    for pair in lines.windows(2) {
        let (Some(prev), Some(cur)) = (pair[0].as_object(), pair[1].as_object()) else {
            continue;
        };
        let pb = prev.get("bbox").and_then(|v| v.as_array());
        let cb = cur.get("bbox").and_then(|v| v.as_array());
        let (Some(pb), Some(cb)) = (pb, cb) else { continue };
        if pb.len() != 4 || cb.len() != 4 {
            continue;
        }
        let pitch = cb[1].as_f64().unwrap_or(0.0) - pb[1].as_f64().unwrap_or(0.0);
        if pitch > 0.0 {
            pitches.push(pitch);
        }
    }
    if pitches.is_empty() {
        return 0.0;
    }
    pitches.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pitch = pitches[pitches.len() / 2];
    let font_size = payload_f64(payload, "font_size_pt", 0.0).max(0.1);
    (pitch / font_size - 1.0).max(0.0)
}

/// `_source_line_count_pressure`: capped source line count pressure.
fn source_line_count_pressure(payload: &Value) -> f64 {
    let source_lines = payload
        .get("item")
        .and_then(|v| v.get("lines"))
        .and_then(|v| v.as_array())
        .map(|l| l.len())
        .unwrap_or(0);
    if source_lines == 0 {
        return 0.0;
    }
    (source_lines as f64 / 4.0).min(4.0)
}

fn source_vertical_pressure(payload: &Value) -> f64 {
    source_line_pitch_pressure(payload).max(source_line_count_pressure(payload))
}

fn set_f64(payload: &mut Value, key: &str, value: f64) {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(key.to_string(), Value::from(value));
    }
}

/// Grow the smaller font toward the larger until the excess is absorbed (within
/// `density_limit`), then pull the larger font down by what remains.
fn smooth_font_pair(current: &mut Value, nxt: &mut Value, max_font_delta: f64, density_limit: f64) {
    let cur_font = payload_f64(current, "font_size_pt", 0.0);
    let nxt_font = payload_f64(nxt, "font_size_pt", 0.0);
    let smaller_is_current = cur_font <= nxt_font;
    let font_delta = if smaller_is_current {
        nxt_font - cur_font
    } else {
        cur_font - nxt_font
    };
    if font_delta <= max_font_delta {
        return;
    }
    let excess = font_delta - max_font_delta;
    let (small, large) = if smaller_is_current {
        (&mut *current, &mut *nxt)
    } else {
        (&mut *nxt, &mut *current)
    };
    let grow_allowed = !payload_bool(small, "prefer_typst_fit")
        && !payload_bool(small, "heavy_dense_small_box")
        && !payload_bool(small, "dense_small_box")
        && payload_estimated_density(small, None, None) <= density_limit;
    let mut grown = 0.0;
    if grow_allowed {
        let desired = payload_f64(small, "font_size_pt", 0.0) + excess * 0.6;
        let bounded = cap_font_growth_by_density(small, desired, density_limit);
        grown = (bounded - payload_f64(small, "font_size_pt", 0.0)).max(0.0);
        set_f64(small, "font_size_pt", bounded);
    }
    let new_larger = (payload_f64(large, "font_size_pt", 0.0) - (excess - grown).max(0.0)).max(6.4);
    set_f64(large, "font_size_pt", py_round(new_larger, 2));
}

/// Grow the smaller leading toward the larger (within the density cap) unless
/// the source pressure gap makes the delta meaningful, then pull the larger down.
fn smooth_leading_pair(current: &mut Value, nxt: &mut Value, max_leading_delta: f64, density_limit: f64) {
    let cur_leading = payload_f64(current, "leading_em", 0.0);
    let nxt_leading = payload_f64(nxt, "leading_em", 0.0);
    let smaller_is_current = cur_leading <= nxt_leading;
    let leading_delta = if smaller_is_current {
        nxt_leading - cur_leading
    } else {
        cur_leading - nxt_leading
    };
    let source_pressure_delta =
        (source_vertical_pressure(current) - source_vertical_pressure(nxt)).abs();
    if leading_delta <= max_leading_delta || source_pressure_delta > 0.9 {
        return;
    }
    let excess = leading_delta - max_leading_delta;
    let density_cap = density_limit.max(BODY_DENSITY_TARGET_MAX) - 0.02;
    let (small, large) = if smaller_is_current {
        (&mut *current, &mut *nxt)
    } else {
        (&mut *nxt, &mut *current)
    };
    let grow_allowed = !payload_bool(small, "prefer_typst_fit")
        && !payload_bool(small, "heavy_dense_small_box")
        && !payload_bool(small, "dense_small_box")
        && payload_estimated_density(small, None, None) <= density_cap;
    let mut grown = 0.0;
    if grow_allowed {
        let desired = payload_f64(small, "leading_em", 0.0) + excess * 0.35;
        let bounded = cap_leading_growth_by_density(small, desired, density_cap);
        grown = (bounded - payload_f64(small, "leading_em", 0.0)).max(0.0);
        set_f64(small, "leading_em", bounded);
    }
    let new_larger = (payload_f64(large, "leading_em", 0.0) - (excess - grown).max(0.0)).max(0.18);
    set_f64(large, "leading_em", py_round(new_larger, 2));
}

/// `smooth_adjacent_body_pair`: converge the font/leading of two same-column body
/// payloads. Relaxed limits apply when either is already dense or prefers fit.
pub fn smooth_adjacent_body_pair(current: &mut Value, nxt: &mut Value) {
    let current_density = payload_estimated_density(current, None, None);
    let next_density = payload_estimated_density(nxt, None, None);
    let relaxed = current_density.max(next_density) > 0.92
        || payload_bool(current, "dense_small_box")
        || payload_bool(nxt, "dense_small_box")
        || payload_bool(current, "prefer_typst_fit")
        || payload_bool(nxt, "prefer_typst_fit");
    let max_font_delta = if relaxed {
        ADJACENT_BODY_SMOOTH_RELAXED_FONT_DELTA_PT
    } else {
        ADJACENT_BODY_SMOOTH_MAX_FONT_DELTA_PT
    };
    let max_leading_delta = if relaxed {
        ADJACENT_BODY_SMOOTH_RELAXED_LEADING_DELTA_EM
    } else {
        ADJACENT_BODY_SMOOTH_MAX_LEADING_DELTA_EM
    };
    let density_limit = if relaxed {
        ADJACENT_BODY_SMOOTH_RELAXED_GROW_DENSITY_MAX
    } else {
        ADJACENT_BODY_SMOOTH_GROW_DENSITY_MAX
    };

    smooth_font_pair(current, nxt, max_font_delta, density_limit);
    smooth_leading_pair(current, nxt, max_leading_delta, density_limit);
    normalize_body_payload_leading(current);
    normalize_body_payload_leading(nxt);
}

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
    let budget = typography::CapacityBudget::body_density();
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
        if line_count < budget.tall_bbox_min_lines
            || bbox_h < budget.tall_bbox_min_height_pt
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
        if cand.ratio < budget.tall_bbox_height_ratio_trigger
            || cand.ratio < page_ratio_ref * budget.tall_bbox_page_ratio_multiplier
        {
            continue;
        }
        let bbox_h = payload_height(&body_payloads[cand.idx]);
        let effective = (cand.natural_h * budget.tall_bbox_effective_natural_multiplier)
            .max(bbox_h * budget.tall_bbox_effective_min_original_ratio)
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
mod tests_context {
    use super::*;

    #[test]
    fn ratio_of_full_box() {
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 100.0, 100.0], Some(200.0), Some(200.0)), 0.25);
    }

    #[test]
    fn missing_dimension_returns_zero() {
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 10.0, 10.0], None, Some(100.0)), 0.0);
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 10.0, 10.0], Some(100.0), None), 0.0);
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 10.0, 10.0], Some(0.0), Some(100.0)), 0.0);
    }

    #[test]
    fn degenerate_box_returns_zero() {
        assert_eq!(page_box_area_ratio(&[5.0, 5.0, 5.0, 10.0], Some(100.0), Some(100.0)), 0.0);
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 1.0], Some(100.0), Some(100.0)), 0.0);
    }
}

#[cfg(test)]
mod tests_common {
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
