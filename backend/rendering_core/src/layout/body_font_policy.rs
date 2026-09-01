// Ports of services/rendering/layout/payload/body_font_dense_policy.py,
// body_font_harmonize_policy.py, body_font_underfill_policy.py, and
// body_fit_policy.py — the body font tightening / long-block harmonizing /
// underfilled font-growth / short-context height-relax policy family (C3-N3).
// Operate on the raw JSON block-payload dicts, mutating in place.

use serde_json::Value;

use crate::item::Item;
use crate::item::formula_map;
use crate::layout::body_context::{
    body_context_anchors, is_body_context_text_payload, payload_density, payload_height,
    payload_is_continuation_member, required_lines, same_body_column, BODY_CONTEXT_MIN_ANCHORS,
    SHORT_BODY_INHERIT_MAX_HEIGHT_PT,
};
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::layout::typography_capacity::{
    font_growth_grew_pt, font_growth_seed_font_pt, set_font_growth_decision, FontGrowthDecision,
};
use crate::layout::typography_capacity as typography;
use crate::payload::capacity::text_demand_units;
use crate::typography::line_count::source_visual_line_count;
use crate::util::{median_f64, py_round};

pub const BODY_PRESSURE_TIGHTEN_TRIGGER: f64 = 1.38;
pub const BODY_PRESSURE_TIGHTEN_TRIGGER_HIGH: f64 = 1.30;
pub const BODY_FINAL_FORCE_FIT_DENSITY: f64 = 1.12;
pub const BODY_FONT_SMOOTH_BAND_PT: f64 = 0.16;
pub const BODY_DENSE_FONT_MAX_PT: f64 = 10.35;
pub const BODY_HEAVY_DENSE_FONT_MAX_PT: f64 = 10.2;

/// `tighten_body_payloads`: clamp each body font toward the page median and
/// shrink high-pressure dense blocks that still overflow the density target.
pub fn tighten_body_payloads(
    body_payloads: &mut Vec<Value>,
    body_font_median: f64,
    body_density_target: f64,
    body_pressure_median: f64,
) {
    for payload in body_payloads {
        let mut smooth_floor = payload_f64(payload, "font_size_pt", 0.0);
        let mut smooth_cap = body_font_median + BODY_FONT_SMOOTH_BAND_PT;
        if payload_bool(payload, "heavy_dense_small_box") {
            smooth_cap = smooth_cap.min(BODY_HEAVY_DENSE_FONT_MAX_PT);
        } else if payload_bool(payload, "dense_small_box") {
            smooth_cap = smooth_cap.min(BODY_DENSE_FONT_MAX_PT);
        }
        if smooth_cap < smooth_floor {
            smooth_floor = smooth_cap;
        }
        let smooth = py_round(smooth_floor.max(payload_f64(payload, "font_size_pt", 0.0)).min(smooth_cap), 2);
        let obj = payload.as_object_mut().expect("payload is an object");
        obj.insert("font_size_pt".to_string(), Value::from(smooth));

        let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
        let inner_height = (bbox[3] - bbox[1]).max(8.0);
        let inner_width = (bbox[2] - bbox[0]).max(8.0);
        let demand = text_demand_units(&payload_string(payload, "translated_text", ""), &formula_map(payload.get("formula_map")));
        let pressure = demand / (inner_width * inner_height).max(1.0);
        let pressure_ratio = if body_pressure_median > 0.0 {
            pressure / body_pressure_median.max(1e-6)
        } else {
            1.0
        };
        let mut density = payload_density(payload, None, None);
        let pressure_trigger = if density > body_density_target + 0.03 {
            BODY_PRESSURE_TIGHTEN_TRIGGER_HIGH
        } else {
            BODY_PRESSURE_TIGHTEN_TRIGGER
        };

        if pressure_ratio > pressure_trigger
            && payload_bool(payload, "dense_small_box")
            && density > body_density_target + 0.04
        {
            let steps = ((pressure_ratio - pressure_trigger) / 0.26).ceil().clamp(1.0, 3.0);
            let font = payload_f64(payload, "font_size_pt", 0.0);
            let leading = payload_f64(payload, "leading_em", 0.0);
            let shrink_floor = font.min(body_font_median - 0.34);
            let obj = payload.as_object_mut().expect("payload is an object");
            obj.insert("font_size_pt".to_string(), Value::from(py_round(shrink_floor.max(font - steps * 0.08), 2)));
            obj.insert("leading_em".to_string(), Value::from(py_round(0.52_f64.max(leading - 0.01 * steps.min(2.0)), 2)));
            obj.insert("prefer_typst_fit".to_string(), Value::Bool(true));
            density = payload_density(payload, None, None);
        }

        if density > body_density_target + 0.06
            && payload_bool(payload, "dense_small_box")
            && pressure_ratio > BODY_PRESSURE_TIGHTEN_TRIGGER_HIGH
        {
            payload
                .as_object_mut()
                .expect("payload is an object")
                .insert("prefer_typst_fit".to_string(), Value::Bool(true));
        } else if !payload_bool(payload, "dense_small_box")
            && !payload_bool(payload, "heavy_dense_small_box")
            && density < body_density_target - 0.12
            && pressure_ratio < 0.94
        {
            let steps = ((body_density_target - density) / 0.12).ceil().clamp(1.0, 2.0);
            let font = payload_f64(payload, "font_size_pt", 0.0);
            let grown = (body_font_median + 0.08).min(font + steps * 0.04);
            let obj = payload.as_object_mut().expect("payload is an object");
            obj.insert("font_size_pt".to_string(), Value::from(py_round(grown, 2)));
        }
    }
}

/// `mark_force_fit_dense_outliers`: force-fit heavy dense blocks whose density
/// still exceeds the final force-fit threshold.
pub fn mark_force_fit_dense_outliers(body_payloads: &mut Vec<Value>) {
    for payload in body_payloads {
        if payload_bool(payload, "heavy_dense_small_box") && payload_density(payload, None, None) > BODY_FINAL_FORCE_FIT_DENSITY {
            payload
                .as_object_mut()
                .expect("payload is an object")
                .insert("prefer_typst_fit".to_string(), Value::Bool(true));
        }
    }
}

const LONG_BODY_MIN_HEIGHT_PT: f64 = 90.0;
const LONG_BODY_MAX_DENSITY: f64 = 0.98;
const LONG_BODY_FONT_SMOOTH_PT: f64 = 0.14;
const LONG_BODY_LEADING_SMOOTH_EM: f64 = 0.05;

/// `harmonize_long_body_payloads`: clamp long body fonts and leading toward
/// their shared medians when at least two qualify.
pub fn harmonize_long_body_payloads(body_payloads: &mut Vec<Value>, page_text_width_med: f64) {
    let long_indices: Vec<usize> = body_payloads
        .iter()
        .enumerate()
        .filter(|(_, payload)| {
            let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
            let inner_height = (bbox[3] - bbox[1]).max(8.0);
            let inner_width = (bbox[2] - bbox[0]).max(8.0);
            inner_height >= LONG_BODY_MIN_HEIGHT_PT
                && inner_width >= page_text_width_med * 0.72
                && payload_density(payload, None, None) <= LONG_BODY_MAX_DENSITY
        })
        .map(|(idx, _)| idx)
        .collect();

    if long_indices.len() < 2 {
        return;
    }

    let fonts: Vec<f64> = long_indices
        .iter()
        .map(|&idx| payload_f64(&body_payloads[idx], "font_size_pt", 0.0))
        .collect();
    let leadings: Vec<f64> = long_indices
        .iter()
        .map(|&idx| payload_f64(&body_payloads[idx], "leading_em", 0.0))
        .collect();
    let font_median = median_f64(&fonts);
    let leading_median = median_f64(&leadings);

    for idx in long_indices {
        let obj = body_payloads[idx].as_object_mut().expect("payload is an object");
        let font = obj.get("font_size_pt").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let leading = obj.get("leading_em").and_then(|v| v.as_f64()).unwrap_or(0.0);
        obj.insert("font_size_pt".to_string(), Value::from(py_round(font.max(font_median - LONG_BODY_FONT_SMOOTH_PT).min(font_median + LONG_BODY_FONT_SMOOTH_PT), 2)));
        obj.insert("leading_em".to_string(), Value::from(py_round(leading.max(leading_median - LONG_BODY_LEADING_SMOOTH_EM).min(leading_median + LONG_BODY_LEADING_SMOOTH_EM), 2)));
    }
}

/// `grow_underfilled_body_payloads`: grow the font of underfilled body blocks
/// toward the page target, recording a font-growth decision.
pub fn grow_underfilled_body_payloads(
    body_payloads: &mut Vec<Value>,
    body_font_median: f64,
    page_text_width_med: f64,
) {
    let budget = typography::CapacityBudget::body_underfilled();
    let page_font_target = page_font_target(body_payloads, body_font_median, page_text_width_med);
    let page_underfill_ratio = page_underfill_ratio(body_payloads);
    for payload in body_payloads {
        if !is_underfilled_growth_candidate(payload) {
            continue;
        }
        let density = payload_density(payload, None, None);
        if density >= budget.font_grow_density_trigger {
            continue;
        }
        let previous_font = payload_f64(payload, "font_size_pt", 0.0);
        if previous_font < page_font_target - budget.font_grow_low_font_skip_delta_pt {
            continue;
        }
        let target_font = target_font_for_payload(
            payload,
            page_font_target,
            density,
            page_underfill_ratio,
        );
        if target_font <= previous_font + 0.03 {
            continue;
        }
        let best = largest_font_within_density(payload, previous_font, target_font, None);
        if best <= previous_font + 0.04 {
            continue;
        }
        let obj = payload.as_object_mut().expect("payload is an object");
        obj.insert("font_size_pt".to_string(), Value::from(py_round(best, 2)));
        set_font_growth_decision(
            payload,
            &FontGrowthDecision::new(
                previous_font,
                payload_f64(payload, "font_size_pt", 0.0),
                density_slack_ratio(density),
            ),
        );
    }
}

/// `harmonize_underfilled_body_fonts`: pull underfilled body fonts in the same
/// column toward the lowest common font.
pub fn harmonize_underfilled_body_fonts(
    body_payloads: &[Value],
    all_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    let budget = typography::CapacityBudget::body_underfilled();
    let anchors = body_context_anchors(body_payloads, page_text_width_med);
    if anchors.len() < 2 {
        return;
    }
    let eligible_indices: Vec<usize> = all_payloads
        .iter()
        .enumerate()
        .filter(|(_, payload)| {
            is_underfilled_growth_candidate(payload)
                && is_body_context_text_payload(payload)
                && payload_f64(payload, "font_size_pt", 0.0) > 0.0
                && anchors
                    .iter()
                    .filter(|anchor| same_body_column(payload, anchor, page_text_width_med))
                    .count()
                    >= 2
        })
        .map(|(idx, _)| idx)
        .collect();
    if eligible_indices.len() < 2 || !eligible_indices.iter().any(|&idx| font_growth_grew_pt(&all_payloads[idx]) > 0.0) {
        return;
    }
    let fonts: Vec<f64> = eligible_indices
        .iter()
        .map(|&idx| payload_f64(&all_payloads[idx], "font_size_pt", 0.0))
        .collect();
    let min_font = fonts.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_font = fonts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if min_font <= 0.0 || max_font / min_font > budget.font_harmonize_max_ratio {
        return;
    }
    let target_font = low_harmonize_font(&eligible_indices, all_payloads);
    for idx in eligible_indices {
        let previous_font = payload_f64(&all_payloads[idx], "font_size_pt", 0.0);
        if previous_font <= target_font + 0.04 {
            continue;
        }
        let new_font = py_round(target_font, 2);
        let seed_font = font_growth_seed_font_pt(&all_payloads[idx], previous_font);
        all_payloads[idx]
            .as_object_mut()
            .expect("payload is an object")
            .insert("font_size_pt".to_string(), Value::from(new_font));
        if new_font > seed_font {
            set_font_growth_decision(
                &mut all_payloads[idx],
                &FontGrowthDecision {
                    seed_font_pt: seed_font,
                    target_font_pt: new_font,
                    slack_ratio: 0.0,
                    reason: "underfilled_body_harmonized".to_string(),
                },
            );
        }
    }
}

/// `recover_underfilled_body_density`: grow font/leading in small steps until the
/// underfilled body block reaches its density-recovery target.
pub fn recover_underfilled_body_density(body_payloads: &mut Vec<Value>) {
    let budget = typography::CapacityBudget::body_underfilled();
    for payload in body_payloads {
        if !is_underfilled_growth_candidate(payload) {
            continue;
        }
        if payload_density(payload, None, None) >= budget.density_floor_trigger {
            continue;
        }
        recover_payload_density(payload);
    }
}

/// `largest_font_within_density`: binary search the largest font whose density
/// stays under the limit (default per-payload limit).
pub fn largest_font_within_density(payload: &Value, low: f64, high: f64, density_limit: Option<f64>) -> f64 {
    let mut low = low;
    let mut high = high;
    let density_limit = density_limit.unwrap_or_else(|| density_limit_for_payload(payload));
    let mut best = low;
    for _ in 0..9 {
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

/// `short_body_density_limit`: the per-payload density limit for short body blocks.
pub fn short_body_density_limit(payload: &Value) -> f64 {
    let budget = typography::CapacityBudget::body_underfilled();
    (budget.font_grow_density_limit + 0.08).max(density_limit_for_payload(payload))
}

fn low_harmonize_font(eligible_indices: &[usize], payloads: &[Value]) -> f64 {
    let mut fonts: Vec<f64> = eligible_indices
        .iter()
        .map(|&idx| payload_f64(&payloads[idx], "font_size_pt", 0.0))
        .filter(|f| *f > 0.0)
        .collect();
    fonts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    fonts.first().copied().unwrap_or(0.0)
}

fn page_font_target(body_payloads: &[Value], body_font_median: f64, page_text_width_med: f64) -> f64 {
    let anchors: Vec<f64> = body_context_anchors(body_payloads, page_text_width_med)
        .iter()
        .filter(|p| !payload_bool(p, "dense_small_box") && !payload_bool(p, "heavy_dense_small_box"))
        .map(|p| payload_f64(p, "font_size_pt", 0.0))
        .collect();
    if anchors.is_empty() {
        return body_font_median;
    }
    body_font_median.max(median_f64(&anchors))
}

fn is_underfilled_growth_candidate(payload: &Value) -> bool {
    let budget = typography::CapacityBudget::body_underfilled();
    if payload_is_continuation_member(payload) {
        return false;
    }
    if payload_bool(payload, "dense_small_box") || payload_bool(payload, "heavy_dense_small_box") {
        return false;
    }
    if payload_string(payload, "render_kind", "") != "markdown" || payload_bool(payload, "prefer_typst_fit") {
        return false;
    }
    required_lines(payload) <= budget.font_grow_max_lines as i64
}

fn target_font_for_payload(payload: &Value, page_font_target: f64, density: f64, page_underfill_ratio: f64) -> f64 {
    let budget = typography::CapacityBudget::body_underfilled();
    let line_count = required_lines(payload);
    let slack_ratio = density_slack_ratio(density);
    let source_line_weight = source_line_rich_weight(payload, line_count);
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    if line_count < budget.font_grow_min_lines as i64
        || payload_height_of(payload) < budget.font_grow_min_height_pt
    {
        let context_cap = page_font_target.min(current_font + budget.font_grow_short_max_pt);
        return context_cap.min(current_font + budget.font_grow_short_max_pt);
    }
    let recovery_font = font_for_recovery_density(payload, density);
    let mut growth_budget = budget.font_grow_max_pt;
    growth_budget += budget.font_grow_short_line_bonus * short_line_weight(line_count);
    growth_budget += budget.font_grow_tall_slack_bonus * height_slack_weight(payload, line_count);
    growth_budget += budget.font_grow_source_line_bonus_pt * source_line_weight;
    let eased_growth = growth_budget * (1.0 - (-budget.font_grow_exp_rate * slack_ratio).exp());
    let mut context_cap = page_font_target.max(recovery_font);
    context_cap += budget.font_grow_page_bonus_pt * page_underfill_ratio;
    context_cap += budget.font_grow_context_bonus_pt * slack_ratio;
    context_cap += budget.font_grow_source_line_cap_bonus_pt * source_line_weight;
    context_cap.min(current_font + eased_growth)
}

fn font_for_recovery_density(payload: &Value, density: f64) -> f64 {
    let budget = typography::CapacityBudget::body_underfilled();
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    if current_font <= 0.0 || density <= 0.0 {
        return current_font;
    }
    let scale = (budget.density_recovery_target / density.max(0.01)).sqrt();
    current_font * scale
}

fn recover_payload_density(payload: &mut Value) {
    let budget = typography::CapacityBudget::body_underfilled();
    let target_density = density_recovery_target(payload);
    for _ in 0..budget.recovery_max_iterations {
        if payload_density(payload, None, None) >= target_density {
            return;
        }
        // Python tracks `changed = font_step or changed`; the loop keeps running
        // as long as either the font or the leading step made progress.
        let mut changed = recover_payload_font_step(payload);
        if payload_density(payload, None, None) >= target_density {
            return;
        }
        changed = recover_payload_leading_step(payload) || changed;
        if !changed {
            return;
        }
    }
}

fn recover_payload_font_step(payload: &mut Value) -> bool {
    let budget = typography::CapacityBudget::body_underfilled();
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    if current_font <= 0.0 {
        return false;
    }
    let mut font_step = budget.recovery_font_step_pt;
    if payload_bool(payload, "_body_font_unified") {
        font_step = budget.unified_font_max_step_pt;
    }
    if font_step <= 0.0 {
        return false;
    }
    let target_font = font_for_recovery_density(payload, payload_density(payload, None, None)).min(current_font + font_step);
    let best = largest_font_within_density(payload, current_font, target_font, Some(budget.density_safe_max));
    if best <= current_font + 0.02 {
        return false;
    }
    payload
        .as_object_mut()
        .expect("payload is an object")
        .insert("font_size_pt".to_string(), Value::from(py_round(best, 2)));
    true
}

fn recover_payload_leading_step(payload: &mut Value) -> bool {
    let budget = typography::CapacityBudget::body_underfilled();
    let current_leading = payload_f64(payload, "leading_em", 0.0);
    if current_leading <= 0.0 {
        return false;
    }
    let target_leading = leading_cap_for_recovery(payload)
        .min(current_leading + budget.recovery_leading_step_em);
    let best = largest_leading_within_density(payload, current_leading, target_leading, budget.density_safe_max);
    if best <= current_leading + 0.01 {
        return false;
    }
    payload
        .as_object_mut()
        .expect("payload is an object")
        .insert("leading_em".to_string(), Value::from(py_round(best, 2)));
    true
}

fn largest_leading_within_density(payload: &Value, low: f64, high: f64, density_limit: f64) -> f64 {
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

fn leading_cap_for_recovery(payload: &Value) -> f64 {
    let comfort_budget = typography::CapacityBudget::body_comfort_leading();
    let line_count = required_lines(payload);
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    let source_lines = item.lines.len();
    if line_count <= 2 || source_lines == 0 {
        return comfort_budget.low_source_line_leading_max;
    }
    if source_lines as i64 <= comfort_budget.low_source_line_count_max {
        return comfort_budget.low_source_line_leading_max;
    }
    let source_line_weight = source_line_rich_weight(payload, line_count);
    0.82 + 0.20 * source_line_weight
}

fn density_recovery_target(payload: &Value) -> f64 {
    let budget = typography::CapacityBudget::body_underfilled();
    let line_count = required_lines(payload);
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    let has_source_lines = !item.lines.is_empty();
    if line_count <= 2 {
        return budget.density_recovery_target_short;
    }
    if !has_source_lines {
        return budget.density_recovery_target_no_source;
    }
    budget.density_recovery_target
}

fn density_limit_for_payload(payload: &Value) -> f64 {
    let budget = typography::CapacityBudget::body_underfilled();
    let line_count = required_lines(payload);
    let source_line_bonus =
        budget.font_grow_source_line_density_bonus * source_line_rich_weight(payload, line_count);
    if line_count <= 4 {
        return budget.font_grow_density_limit + source_line_bonus;
    }
    (budget.font_grow_density_limit + source_line_bonus)
        .min(1.00 + 0.01 * (8 - line_count).max(0) as f64 + source_line_bonus)
}

fn page_underfill_ratio(body_payloads: &[Value]) -> f64 {
    let densities: Vec<f64> = body_payloads
        .iter()
        .filter(|p| !payload_bool(p, "dense_small_box") && !payload_bool(p, "heavy_dense_small_box"))
        .map(|p| payload_density(p, None, None))
        .collect();
    if densities.is_empty() {
        return 0.0;
    }
    density_slack_ratio(median_f64(&densities))
}

fn density_slack_ratio(density: f64) -> f64 {
    let budget = typography::CapacityBudget::body_underfilled();
    let slack = (budget.font_grow_density_trigger - density)
        / budget.font_grow_density_trigger.max(0.01);
    slack.max(0.0).min(1.0)
}

fn short_line_weight(line_count: i64) -> f64 {
    (5.0 - line_count as f64) / 4.0
}

fn height_slack_weight(payload: &Value, line_count: i64) -> f64 {
    let font_size = payload_f64(payload, "font_size_pt", 0.0);
    let Some(inner) = inner_bbox4(payload) else {
        return 0.0;
    };
    if font_size <= 0.0 || line_count <= 0 {
        return 0.0;
    }
    let height = (inner[3] - inner[1]).max(8.0);
    let natural_height = font_size * line_count.max(1) as f64 * 1.1;
    ((height - natural_height) / height.max(1.0)).max(0.0).min(1.0)
}

fn payload_height_of(payload: &Value) -> f64 {
    inner_bbox4(payload)
        .map(|b| (b[3] - b[1]).max(0.0))
        .unwrap_or(0.0)
}

fn source_line_rich_weight(payload: &Value, line_count: i64) -> f64 {
    let budget = typography::CapacityBudget::body_underfilled();
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    let source_lines = source_visual_line_count(&item);
    if source_lines <= 0 || line_count <= 0 {
        return 0.0;
    }
    let ratio = source_lines as f64 / line_count.max(1) as f64;
    ((ratio - budget.font_grow_source_line_ratio_offset)
        / budget.font_grow_source_line_ratio_range)
        .max(0.0)
        .min(1.0)
}

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
mod tests_dense {
    use super::*;
    use serde_json::json;

    fn dense_payload(font: f64, dense: bool, heavy: bool) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 250.0, 120.0],
            "translated_text": "a",
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": 0.5,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": dense,
            "heavy_dense_small_box": heavy,
            "prefer_typst_fit": false,
            "item": {},
        })
    }

    #[test]
    fn clamps_font_toward_median_band() {
        // Font above the median band gets pulled down to the band cap (12.0 + 0.16).
        let mut payloads = vec![dense_payload(13.0, false, false)];
        tighten_body_payloads(&mut payloads, 12.0, 0.9, 1.0);
        let font: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        assert!((font - 12.0).abs() <= BODY_FONT_SMOOTH_BAND_PT);
    }

    #[test]
    fn heavy_dense_caps_band() {
        let mut payloads = vec![dense_payload(12.0, true, false)];
        tighten_body_payloads(&mut payloads, 13.0, 0.9, 1.0);
        let font: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        assert!(font <= BODY_DENSE_FONT_MAX_PT);
    }

    #[test]
    fn force_fit_marks_heavy_dense_outlier() {
        // Heavy dense block with long text in a short box overflows force-fit density.
        let mut p = dense_payload(12.0, false, true);
        p.as_object_mut().unwrap().insert("translated_text".to_string(), json!(DENSE_TEXT));
        p.as_object_mut().unwrap().insert("inner_bbox".to_string(), json!([50.0, 0.0, 250.0, 40.0]));
        let mut payloads = vec![p];
        mark_force_fit_dense_outliers(&mut payloads);
        assert!(payloads[0].get("prefer_typst_fit").map(|v| v.as_bool().unwrap_or(false)).unwrap_or(false));
    }

    const DENSE_TEXT: &str = "这是一段很长的中文文字内容，它被用作正文的密集排版测试用例。我们需要足够多的字符来填充这个版面，让文本能够稳定地在给定的宽度内折行并占据足够的垂直空间，从而模拟真实的正文块在页面上的表现。";
}

#[cfg(test)]
mod tests_harmonize {
    use super::*;
    use serde_json::json;

    fn harmonize_payload(font: f64, leading: f64, height: f64) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 350.0, height],
            "translated_text": "这是一段足够长的中文段落文字内容，它需要能够在给定的宽度之内被排版成多行文字才能满足段落的自然阅读习惯。这里再补充一部分内容让文本继续变长。",
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": leading,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {},
        })
    }

    #[test]
    fn harmonizes_two_long_blocks() {
        let mut payloads = vec![harmonize_payload(10.0, 0.4, 120.0), harmonize_payload(12.0, 0.6, 130.0)];
        harmonize_long_body_payloads(&mut payloads, 200.0);
        let font0: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        let font1: f64 = payloads[1]["font_size_pt"].as_f64().unwrap();
        assert!(font0 >= 11.0 - 0.15 && font0 <= 11.0 + 0.15);
        assert!(font1 >= 11.0 - 0.15 && font1 <= 11.0 + 0.15);
    }

    #[test]
    fn single_long_block_left_alone() {
        let mut payloads = vec![harmonize_payload(10.0, 0.4, 120.0)];
        harmonize_long_body_payloads(&mut payloads, 200.0);
        assert_eq!(payloads[0]["font_size_pt"], json!(10.0));
    }
}

#[cfg(test)]
mod tests_underfill {
    use super::*;
    use serde_json::json;

    const MED_TEXT: &str = "这是一段中等长度的中文文本内容，用于测试正文密度的恢复增长逻辑，它的行数应当能够超过两行以便走常规增长路径。";

    fn underfill_payload(font: f64, leading: f64, translated: &str) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 350.0, 120.0],
            "translated_text": translated,
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": leading,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "prefer_typst_fit": false,
            "item": {
                "source_text": "source text",
                "lines": [
                    {"bbox": [50.0, 0.0, 350.0, 20.0], "spans": [{"type": "text", "text": "第一行"}]},
                    {"bbox": [50.0, 24.0, 350.0, 44.0], "spans": [{"type": "text", "text": "第二行"}]},
                ],
            },
        })
    }

    #[test]
    fn grows_underfilled_short_text() {
        // Multi-line text near the page font target gets grown toward it.
        let mut payloads = vec![underfill_payload(11.8, 0.44, MED_TEXT)];
        grow_underfilled_body_payloads(&mut payloads, 12.0, 200.0);
        let font: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        assert!(font > 11.8);
        assert!(payloads[0].get("_body_font_growth_decision").is_some());
    }

    #[test]
    fn skips_dense_candidates() {
        let mut p = underfill_payload(11.8, 0.44, MED_TEXT);
        p.as_object_mut().unwrap().insert("dense_small_box".to_string(), json!(true));
        let mut payloads = vec![p];
        grow_underfilled_body_payloads(&mut payloads, 12.0, 200.0);
        assert_eq!(payloads[0]["font_size_pt"], json!(11.8));
        assert!(payloads[0].get("_body_font_growth_decision").is_none());
    }

    #[test]
    fn recovery_steps_leading() {
        let mut payloads = vec![underfill_payload(9.0, 0.4, "短文本")];
        recover_underfilled_body_density(&mut payloads);
        let leading: f64 = payloads[0]["leading_em"].as_f64().unwrap();
        assert!(leading >= 0.4);
    }

    #[test]
    fn largest_font_respects_density_limit() {
        let p = underfill_payload(10.0, 0.5, "短文本");
        let best = largest_font_within_density(&p, 10.0, 14.0, Some(1.0));
        assert!(best >= 10.0 && best <= 14.0);
    }
}

#[cfg(test)]
mod tests_fit {
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
