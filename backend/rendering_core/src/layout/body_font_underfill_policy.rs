// Port of services/rendering/layout/payload/body_font_underfill_policy.py — the
// body font-growth / density-recovery policy for underfilled blocks (C3-N3).
// Operates on the raw JSON block-payload dicts, mutating in place.

use serde_json::Value;

use crate::item::Item;
use crate::layout::body_common::{
    body_context_anchors, is_body_context_text_payload, payload_density, payload_is_continuation_member,
    required_lines, same_body_column,
};
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::layout::typography_decision::{font_growth_grew_pt, font_growth_seed_font_pt, set_font_growth_decision, FontGrowthDecision};
use crate::layout::typography_policy as typography;
use crate::typography::line_count::source_visual_line_count;
use crate::util::{median_f64, py_round};

/// `grow_underfilled_body_payloads`: grow the font of underfilled body blocks
/// toward the page target, recording a font-growth decision.
pub fn grow_underfilled_body_payloads(
    body_payloads: &mut Vec<Value>,
    body_font_median: f64,
    page_text_width_med: f64,
) {
    let page_font_target = page_font_target(body_payloads, body_font_median, page_text_width_med);
    let page_underfill_ratio = page_underfill_ratio(body_payloads);
    for payload in body_payloads {
        if !is_underfilled_growth_candidate(payload) {
            continue;
        }
        let density = payload_density(payload, None, None);
        if density >= typography::BODY_UNDERFILLED_FONT_GROW_DENSITY_TRIGGER {
            continue;
        }
        let previous_font = payload_f64(payload, "font_size_pt", 0.0);
        if previous_font < page_font_target - typography::BODY_UNDERFILLED_FONT_GROW_LOW_FONT_SKIP_DELTA_PT {
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
    if min_font <= 0.0 || max_font / min_font > typography::BODY_UNDERFILLED_FONT_HARMONIZE_MAX_RATIO {
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
    for payload in body_payloads {
        if !is_underfilled_growth_candidate(payload) {
            continue;
        }
        if payload_density(payload, None, None) >= typography::BODY_UNDERFILLED_DENSITY_FLOOR_TRIGGER {
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
    (typography::BODY_UNDERFILLED_FONT_GROW_DENSITY_LIMIT + 0.08).max(density_limit_for_payload(payload))
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
    if payload_is_continuation_member(payload) {
        return false;
    }
    if payload_bool(payload, "dense_small_box") || payload_bool(payload, "heavy_dense_small_box") {
        return false;
    }
    if payload_string(payload, "render_kind", "") != "markdown" || payload_bool(payload, "prefer_typst_fit") {
        return false;
    }
    required_lines(payload) <= typography::BODY_UNDERFILLED_FONT_GROW_MAX_LINES as i64
}

fn target_font_for_payload(payload: &Value, page_font_target: f64, density: f64, page_underfill_ratio: f64) -> f64 {
    let line_count = required_lines(payload);
    let slack_ratio = density_slack_ratio(density);
    let source_line_weight = source_line_rich_weight(payload, line_count);
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    if line_count < typography::BODY_UNDERFILLED_FONT_GROW_MIN_LINES as i64
        || payload_height_of(payload) < typography::BODY_UNDERFILLED_FONT_GROW_MIN_HEIGHT_PT
    {
        let context_cap = page_font_target.min(current_font + typography::BODY_UNDERFILLED_FONT_GROW_SHORT_MAX_PT);
        return context_cap.min(current_font + typography::BODY_UNDERFILLED_FONT_GROW_SHORT_MAX_PT);
    }
    let recovery_font = font_for_recovery_density(payload, density);
    let mut growth_budget = typography::BODY_UNDERFILLED_FONT_GROW_MAX_PT;
    growth_budget += typography::BODY_UNDERFILLED_FONT_GROW_SHORT_LINE_BONUS * short_line_weight(line_count);
    growth_budget += typography::BODY_UNDERFILLED_FONT_GROW_TALL_SLACK_BONUS * height_slack_weight(payload, line_count);
    growth_budget += typography::BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_BONUS_PT * source_line_weight;
    let eased_growth = growth_budget * (1.0 - (-typography::BODY_UNDERFILLED_FONT_GROW_EXP_RATE * slack_ratio).exp());
    let mut context_cap = page_font_target.max(recovery_font);
    context_cap += typography::BODY_UNDERFILLED_FONT_GROW_PAGE_BONUS_PT * page_underfill_ratio;
    context_cap += typography::BODY_UNDERFILLED_FONT_GROW_CONTEXT_BONUS_PT * slack_ratio;
    context_cap += typography::BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_CAP_BONUS_PT * source_line_weight;
    context_cap.min(current_font + eased_growth)
}

fn font_for_recovery_density(payload: &Value, density: f64) -> f64 {
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    if current_font <= 0.0 || density <= 0.0 {
        return current_font;
    }
    let scale = (typography::BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET / density.max(0.01)).sqrt();
    current_font * scale
}

fn recover_payload_density(payload: &mut Value) {
    let target_density = density_recovery_target(payload);
    for _ in 0..typography::BODY_UNDERFILLED_RECOVERY_MAX_ITERATIONS {
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
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    if current_font <= 0.0 {
        return false;
    }
    let mut font_step = typography::BODY_UNDERFILLED_RECOVERY_FONT_STEP_PT;
    if payload_bool(payload, "_body_font_unified") {
        font_step = typography::BODY_UNDERFILLED_UNIFIED_FONT_MAX_STEP_PT;
    }
    if font_step <= 0.0 {
        return false;
    }
    let target_font = font_for_recovery_density(payload, payload_density(payload, None, None)).min(current_font + font_step);
    let best = largest_font_within_density(payload, current_font, target_font, Some(typography::BODY_UNDERFILLED_DENSITY_SAFE_MAX));
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
    let current_leading = payload_f64(payload, "leading_em", 0.0);
    if current_leading <= 0.0 {
        return false;
    }
    let target_leading = leading_cap_for_recovery(payload)
        .min(current_leading + typography::BODY_UNDERFILLED_RECOVERY_LEADING_STEP_EM);
    let best = largest_leading_within_density(payload, current_leading, target_leading, typography::BODY_UNDERFILLED_DENSITY_SAFE_MAX);
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
    let line_count = required_lines(payload);
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    let source_lines = item.lines.len();
    if line_count <= 2 || source_lines == 0 {
        return typography::BODY_COMFORT_LOW_SOURCE_LINE_LEADING_MAX;
    }
    if source_lines as i64 <= typography::BODY_COMFORT_LOW_SOURCE_LINE_COUNT_MAX {
        return typography::BODY_COMFORT_LOW_SOURCE_LINE_LEADING_MAX;
    }
    let source_line_weight = source_line_rich_weight(payload, line_count);
    0.82 + 0.20 * source_line_weight
}

fn density_recovery_target(payload: &Value) -> f64 {
    let line_count = required_lines(payload);
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    let has_source_lines = !item.lines.is_empty();
    if line_count <= 2 {
        return typography::BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET_SHORT;
    }
    if !has_source_lines {
        return typography::BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET_NO_SOURCE;
    }
    typography::BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET
}

fn density_limit_for_payload(payload: &Value) -> f64 {
    let line_count = required_lines(payload);
    let source_line_bonus =
        typography::BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_DENSITY_BONUS * source_line_rich_weight(payload, line_count);
    if line_count <= 4 {
        return typography::BODY_UNDERFILLED_FONT_GROW_DENSITY_LIMIT + source_line_bonus;
    }
    (typography::BODY_UNDERFILLED_FONT_GROW_DENSITY_LIMIT + source_line_bonus)
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
    let slack = (typography::BODY_UNDERFILLED_FONT_GROW_DENSITY_TRIGGER - density)
        / typography::BODY_UNDERFILLED_FONT_GROW_DENSITY_TRIGGER.max(0.01);
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
    let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
    let source_lines = source_visual_line_count(&item);
    if source_lines <= 0 || line_count <= 0 {
        return 0.0;
    }
    let ratio = source_lines as f64 / line_count.max(1) as f64;
    ((ratio - typography::BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_RATIO_OFFSET)
        / typography::BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_RATIO_RANGE)
        .max(0.0)
        .min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MED_TEXT: &str = "这是一段中等长度的中文文本内容，用于测试正文密度的恢复增长逻辑，它的行数应当能够超过两行以便走常规增长路径。";

    fn payload(font: f64, leading: f64, translated: &str) -> Value {
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
        let mut payloads = vec![payload(11.8, 0.44, MED_TEXT)];
        grow_underfilled_body_payloads(&mut payloads, 12.0, 200.0);
        let font: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        assert!(font > 11.8);
        assert!(payloads[0].get("_body_font_growth_decision").is_some());
    }

    #[test]
    fn skips_dense_candidates() {
        let mut p = payload(11.8, 0.44, MED_TEXT);
        p.as_object_mut().unwrap().insert("dense_small_box".to_string(), json!(true));
        let mut payloads = vec![p];
        grow_underfilled_body_payloads(&mut payloads, 12.0, 200.0);
        assert_eq!(payloads[0]["font_size_pt"], json!(11.8));
        assert!(payloads[0].get("_body_font_growth_decision").is_none());
    }

    #[test]
    fn recovery_steps_leading() {
        let mut payloads = vec![payload(9.0, 0.4, "短文本")];
        recover_underfilled_body_density(&mut payloads);
        let leading: f64 = payloads[0]["leading_em"].as_f64().unwrap();
        assert!(leading >= 0.4);
    }

    #[test]
    fn largest_font_respects_density_limit() {
        let p = payload(10.0, 0.5, "短文本");
        let best = largest_font_within_density(&p, 10.0, 14.0, Some(1.0));
        assert!(best >= 10.0 && best <= 14.0);
    }
}
