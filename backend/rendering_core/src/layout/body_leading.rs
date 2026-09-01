// Ports of services/rendering/layout/payload/body_leading_policy.py,
// body_leading_solver.py, and body_smoothing_policy.py — the body-leading
// restore/refit + solver + adjacent-smoothing policy family (C3-N3, after font
// unify). Operate on the raw JSON block-payload dicts, mutating in place.

use serde_json::Value;

use crate::item::Item;
use crate::layout::body_context::{
    is_same_column_adjacent_body_pair, payload_center_x, payload_density, payload_inner_bottom,
    payload_inner_top, payload_is_continuation_member, required_lines, smooth_adjacent_body_pair,
};
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::layout::typography_capacity::{
    font_growth_grew_pt, font_growth_seed_font_pt, font_growth_slack_ratio, leading_refit_after_font_unify,
    set_leading_decision, set_vertical_budget, LeadingDecision, VerticalBudget,
};
use crate::layout::typography_capacity as typography;
use crate::leading_fit::{BODY_LEADING_MAX, BODY_LEADING_MIN};
use crate::typography::line_count::visual_line_count;
use crate::typography::line_metrics::{local_line_pitch, median_line_pitch};
use crate::util::{median_f64, py_round};

/// `restore_comfort_body_leading`: spend remaining vertical slack on body
/// leading, recording the vertical-budget annotation and the leading decision.
pub fn restore_comfort_body_leading(body_payloads: &mut Vec<Value>) {
    let baseline = page_body_leading_baseline(body_payloads);
    for payload in body_payloads {
        if !eligible_for_body_leading(payload) {
            continue;
        }
        ensure_multiline_comfort_floor(payload);
        let Some(solution) = solve_body_leading(payload, baseline) else {
            continue;
        };
        if payload_f64(payload, "leading_em", 0.0) >= solution.leading_em {
            continue;
        }
        annotate_body_vertical_budget(payload, &solution);
        payload
            .as_object_mut()
            .expect("payload is an object")
            .insert("leading_em".to_string(), Value::from(solution.leading_em));
        set_leading_decision(
            payload,
            &LeadingDecision::new(solution.leading_em, solution.target_density, solution.leading_cap_em),
        );
    }
}

/// `refit_body_leading_after_font_unify`: identical to the restore pass but
/// records `refit_after_font_unify` on the decision (the font-unify path).
pub fn refit_body_leading_after_font_unify(body_payloads: &mut Vec<Value>) {
    let baseline = page_body_leading_baseline(body_payloads);
    for payload in body_payloads {
        if !eligible_for_body_leading(payload) {
            continue;
        }
        ensure_multiline_comfort_floor(payload);
        let Some(solution) = solve_body_leading(payload, baseline) else {
            continue;
        };
        if payload_f64(payload, "leading_em", 0.0) >= solution.leading_em {
            continue;
        }
        annotate_body_vertical_budget(payload, &solution);
        payload
            .as_object_mut()
            .expect("payload is an object")
            .insert("leading_em".to_string(), Value::from(solution.leading_em));
        set_leading_decision(
            payload,
            &LeadingDecision {
                leading_em: solution.leading_em,
                target_density: solution.target_density,
                leading_cap_em: solution.leading_cap_em,
                refit_after_font_unify: true,
            },
        );
    }
}

fn eligible_for_body_leading(payload: &Value) -> bool {
    if payload_string(payload, "render_kind", "") != "markdown" {
        return false;
    }
    if payload_bool(payload, "dense_small_box")
        || payload_bool(payload, "heavy_dense_small_box")
        || payload_bool(payload, "prefer_typst_fit")
    {
        return false;
    }
    true
}

fn page_body_leading_baseline(body_payloads: &[Value]) -> Option<f64> {
    let mut values = Vec::new();
    for payload in body_payloads {
        if !eligible_for_body_leading(payload) {
            continue;
        }
        let leading = payload_f64(payload, "leading_em", 0.0);
        if leading <= 0.0 || leading_refit_after_font_unify(payload) {
            continue;
        }
        values.push(leading);
    }
    if values.len() < 2 {
        return None;
    }
    Some(py_round(median_f64(&values), 2))
}

fn ensure_multiline_comfort_floor(payload: &mut Value) {
    let budget = typography::CapacityBudget::body_comfort_leading();
    if required_lines(payload) < 3 {
        return;
    }
    let current = payload_f64(payload, "leading_em", 0.0);
    if current >= budget.leading_min {
        return;
    }
    let obj = payload.as_object_mut().expect("payload is an object");
    obj.insert(
        "leading_em".to_string(),
        Value::from(py_round(budget.leading_min, 2)),
    );
}

fn clamp(value: f64, low: f64, high: f64) -> f64 {
    low.max(high.min(value))
}

fn exp_response(value: f64, rate: f64) -> f64 {
    1.0 - (-rate * value.max(0.0)).exp()
}

/// `BodyLeadingContext`: derived inputs the solver reads from a body payload.
#[derive(Debug, Clone)]
pub struct BodyLeadingContext {
    current_density: f64,
    line_count: i64,
    source_lines: i64,
    has_source_line_geometry: bool,
    source_leading_em: f64,
    font_growth_pt: f64,
    #[allow(dead_code)]
    font_growth_score: f64,
    #[allow(dead_code)]
    page_baseline_leading_em: Option<f64>,
}

impl BodyLeadingContext {
    pub fn from_payload(payload: &Value, page_baseline_leading_em: Option<f64>) -> Self {
    let budget = typography::CapacityBudget::body_comfort_leading();
        let item = Item::from_json_value(payload.get("item").unwrap_or(&Value::Null));
        let font_size = payload_f64(payload, "font_size_pt", 0.0).max(0.1);
        let pitch = {
            let lp = local_line_pitch(&item);
            if lp > 0.0 {
                lp
            } else {
                median_line_pitch(&item)
            }
        };
        let source_leading = if pitch > 0.0 {
            pitch / font_size - 1.0
        } else {
            0.0
        };
        let grew = font_growth_grew_pt(payload);
        let slack_ratio = font_growth_slack_ratio(payload);
        BodyLeadingContext {
            current_density: payload_density(payload, None, None),
            line_count: required_lines(payload),
            source_lines: visual_line_count(&item),
            has_source_line_geometry: has_source_line_geometry(&item),
            source_leading_em: clamp(source_leading, 0.0, budget.source_line_leading_max),
            font_growth_pt: grew.max(0.0),
            font_growth_score: clamp(
                (grew / budget.font_growth_normalizer_pt) * slack_ratio.max(0.0),
                0.0,
                1.0,
            ),
            page_baseline_leading_em,
        }
    }

    fn source_line_ratio(&self) -> f64 {
        self.source_lines as f64 / (self.line_count as f64).max(1.0)
    }
}

/// `BodyLeadingSolution`: the fitted leading plus the density/cap the decision
/// DTOs record.
#[derive(Debug, Clone, Copy)]
pub struct BodyLeadingSolution {
    pub leading_em: f64,
    pub target_density: f64,
    pub leading_cap_em: f64,
}

/// `solve_body_leading`: None when the payload is already dense enough to leave
/// alone; otherwise the leading that lifts density toward the target.
pub fn solve_body_leading(payload: &Value, page_baseline_leading_em: Option<f64>) -> Option<BodyLeadingSolution> {
    let budget = typography::CapacityBudget::body_comfort_leading();
    let ctx = BodyLeadingContext::from_payload(payload, page_baseline_leading_em);
    if ctx.current_density > budget.leading_density_max {
        return None;
    }
    if ctx.current_density >= budget.density_floor_trigger {
        return None;
    }

    let leading_cap = leading_cap(&ctx);
    let low = payload_f64(payload, "leading_em", BODY_LEADING_MIN);
    let high = low.max(leading_cap);
    let paired_low = font_paired_min_leading(&ctx, low, high);
    let target_density = budget.leading_density_max.min(target_density(&ctx));
    if payload_density(payload, None, Some(paired_low)) >= target_density {
        return None;
    }
    let budget_high = paired_low.max(high.min(low + leading_growth_budget(&ctx)));

    let best = solve_leading_em(payload, paired_low, budget_high, target_density);

    Some(BodyLeadingSolution {
        leading_em: py_round(payload_f64(payload, "leading_em", 0.0).max(best), 2),
        target_density: py_round(target_density, 3),
        leading_cap_em: py_round(leading_cap, 2),
    })
}

/// `annotate_body_vertical_budget`: record the font/leading growth spent against
/// the vertical slack budget.
pub fn annotate_body_vertical_budget(payload: &mut Value, solution: &BodyLeadingSolution) {
    let seed_font = font_growth_seed_font_pt(payload, payload_f64(payload, "font_size_pt", 0.0));
    let current_font = payload_f64(payload, "font_size_pt", 0.0);
    let current_leading = payload_f64(payload, "leading_em", 0.0);
    set_vertical_budget(
        payload,
        &VerticalBudget {
            font_growth_pt: (current_font - seed_font).max(0.0),
            leading_growth_em: (solution.leading_em - current_leading).max(0.0),
            target_density: solution.target_density,
            leading_cap_em: solution.leading_cap_em,
        },
    );
}

fn solve_leading_em(payload: &Value, mut low: f64, mut high: f64, target_density: f64) -> f64 {
    let budget = typography::CapacityBudget::body_comfort_leading();
    let mut best = low;
    let mut low_residual = target_density - payload_density(payload, None, Some(low));
    let mut high_residual = target_density - payload_density(payload, None, Some(high));

    for _ in 0..budget.solver_max_iterations {
        if high - low <= budget.solver_min_bracket_width {
            break;
        }
        let trial = residual_extrapolated_leading(low, high, low_residual, high_residual);
        let density = payload_density(payload, None, Some(trial));
        let residual = target_density - density;
        if residual.abs() <= budget.solver_density_tolerance {
            if density <= budget.leading_density_max && residual >= 0.0 {
                best = trial;
            }
            break;
        }
        if density <= budget.leading_density_max && residual >= 0.0 {
            best = trial;
            low = trial;
            low_residual = residual;
        } else {
            high = trial;
            high_residual = residual;
        }
    }
    best
}

fn residual_extrapolated_leading(low: f64, high: f64, low_residual: f64, high_residual: f64) -> f64 {
    let budget = typography::CapacityBudget::body_comfort_leading();
    let residual_span = low_residual - high_residual;
    if residual_span.abs() <= 1e-9 {
        return (low + high) / 2.0;
    }
    let fraction = clamp(
        low_residual / residual_span,
        budget.solver_extrapolation_min_fraction,
        budget.solver_extrapolation_max_fraction,
    );
    low + (high - low) * fraction
}

fn target_density(ctx: &BodyLeadingContext) -> f64 {
    let budget = typography::CapacityBudget::body_comfort_leading();
    if ctx.line_count <= 2 {
        return budget.density_recovery_target_short;
    }
    if !ctx.has_source_line_geometry {
        return budget.density_recovery_target_no_source;
    }
    if 0 < ctx.source_lines && ctx.source_lines <= budget.low_source_line_count_max {
        return budget.density_recovery_target
            .min(budget.low_source_line_target_fill_max);
    }
    budget.density_recovery_target
}

fn leading_cap(ctx: &BodyLeadingContext) -> f64 {
    let budget = typography::CapacityBudget::body_comfort_leading();
    if ctx.has_source_line_geometry
        && 0 < ctx.source_lines
        && ctx.source_lines <= budget.low_source_line_count_max
    {
        return clamp(
            budget.low_source_line_leading_max,
            budget.leading_min,
            budget.source_line_leading_max,
        );
    }
    if ctx.line_count <= 1 {
        return BODY_LEADING_MAX;
    }

    let line_extra = (ctx.line_count - budget.long_line_threshold + 1).max(0) as f64;
    let mut long_cap_target = budget.long_leading_max;
    if ctx.source_lines <= 0 {
        long_cap_target = budget.no_source_long_leading_max;
    }
    let long_line_cap = BODY_LEADING_MAX
        + exp_response(line_extra, budget.long_line_cap_response_rate)
            * (long_cap_target - BODY_LEADING_MAX);

    let source_gap = (ctx.source_line_ratio() - 1.0).max(0.0);
    let source_cap = BODY_LEADING_MAX
        + exp_response(source_gap, budget.source_line_cap_response_rate)
            * (budget.source_line_leading_max - BODY_LEADING_MAX);

    let pitch_cap = BODY_LEADING_MAX
        + exp_response(ctx.source_leading_em, budget.source_pitch_cap_response_rate)
            * (budget.source_line_leading_max - BODY_LEADING_MAX);

    let cap = BODY_LEADING_MAX.max(long_line_cap).max(source_cap).max(pitch_cap);
    clamp(cap, budget.leading_min, budget.source_line_leading_max)
}

fn font_paired_min_leading(ctx: &BodyLeadingContext, current_leading: f64, leading_cap: f64) -> f64 {
    let budget = typography::CapacityBudget::body_comfort_leading();
    if ctx.font_growth_pt <= 0.0 {
        return current_leading;
    }
    let line_weight =
        exp_response((ctx.line_count - 1).max(0) as f64, budget.multi_line_weight_rate);
    let paired_gain = budget.font_growth_min_leading_gain_max
        * exp_response(ctx.font_growth_pt, budget.font_growth_min_leading_rate)
        * line_weight;
    clamp(current_leading + paired_gain, current_leading, leading_cap)
}

fn leading_growth_budget(ctx: &BodyLeadingContext) -> f64 {
    let budget = typography::CapacityBudget::body_comfort_leading();
    let line_weight =
        exp_response((ctx.line_count - 1).max(0) as f64, budget.multi_line_weight_rate);
    let long_text_bonus = budget.leading_growth_long_text_max
        * exp_response(
            (ctx.line_count as f64 - budget.line_count_base).max(0.0),
            budget.line_count_response_rate,
        );
    let mut source_line_pressure = 0.0;
    if ctx.has_source_line_geometry {
        source_line_pressure = exp_response(
            (ctx.source_lines as f64 - budget.source_line_volume_base).max(0.0),
            budget.source_line_volume_response_rate,
        );
    }
    let source_pitch_pressure = exp_response(
        (ctx.source_leading_em - budget.leading_min).max(0.0),
        budget.source_pitch_response_rate,
    );
    let source_bonus = budget.leading_growth_source_max
        * source_line_pressure.max(source_pitch_pressure);
    let font_spend_penalty = budget.leading_font_spend_penalty_max
        * exp_response(ctx.font_growth_pt, budget.font_growth_min_leading_rate);
    let floor = if ctx.font_growth_pt > 0.0 {
        budget.leading_growth_min_after_font_growth
    } else {
        0.0
    };
    (budget.leading_growth_base_max * line_weight
        + long_text_bonus
        + source_bonus
        - font_spend_penalty)
        .max(floor)
}

/// `_has_source_line_geometry`: the item carries a non-empty `lines` array.
fn has_source_line_geometry(item: &Item) -> bool {
    !item.lines.is_empty()
}

/// `smooth_adjacent_body_payloads`: for each body block, smooth it against the
/// best same-column neighbor directly below it, once per pair.
pub fn smooth_adjacent_body_payloads(body_payloads: &mut Vec<Value>, page_text_width_med: f64) {
    let top_left = |idx: usize| {
        let bbox = inner_bbox4(&body_payloads[idx]).unwrap_or([0.0; 4]);
        (bbox[1], bbox[0])
    };
    let mut order: Vec<usize> = (0..body_payloads.len()).collect();
    order.sort_by(|&a, &b| {
        let (ta, la) = top_left(a);
        let (tb, lb) = top_left(b);
        ta.partial_cmp(&tb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut smoothed: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for i in 0..order.len() {
        let current_idx = order[i];
        if payload_is_continuation_member(&body_payloads[current_idx]) {
            continue;
        }
        let mut best_next: Option<usize> = None;
        let mut best_key: Option<(f64, f64)> = None;
        for &nxt_idx in &order[i + 1..] {
            if payload_is_continuation_member(&body_payloads[nxt_idx]) {
                continue;
            }
            if !is_same_column_adjacent_body_pair(&body_payloads[current_idx], &body_payloads[nxt_idx], page_text_width_med) {
                continue;
            }
            let gap = (-4.0_f64).max(payload_inner_top(&body_payloads[nxt_idx]) - payload_inner_bottom(&body_payloads[current_idx]));
            let center_delta = (payload_center_x(&body_payloads[current_idx]) - payload_center_x(&body_payloads[nxt_idx])).abs();
            let key = (gap, center_delta);
            if best_key.is_none() || key < best_key.unwrap() {
                best_key = Some(key);
                best_next = Some(nxt_idx);
            }
        }
        let Some(best_next) = best_next else {
            continue;
        };
        let pair_key = (current_idx.min(best_next), current_idx.max(best_next));
        if smoothed.contains(&pair_key) {
            continue;
        }
        let (lo, hi) = if current_idx < best_next {
            (current_idx, best_next)
        } else {
            (best_next, current_idx)
        };
        let (left, right) = body_payloads.split_at_mut(hi);
        let a = &mut left[lo];
        let b = &mut right[0];
        smooth_adjacent_body_pair(a, b);
        smoothed.insert(pair_key);
    }
}

#[cfg(test)]
mod tests_policy {
    use super::*;
    use serde_json::json;

    fn body_payload(leading_em: f64, translated: &str) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 350.0, 120.0],
            "translated_text": translated,
            "formula_map": [],
            "font_size_pt": 12.0,
            "leading_em": leading_em,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {
                "bbox": [50.0, 0.0, 350.0, 120.0],
                "lines": [
                    {"bbox": [50.0, 0.0, 350.0, 20.0], "spans": [{"type": "text", "text": "第一行"}]},
                    {"bbox": [50.0, 24.0, 350.0, 44.0], "spans": [{"type": "text", "text": "第二行"}]},
                    {"bbox": [50.0, 48.0, 350.0, 68.0], "spans": [{"type": "text", "text": "第三行"}]},
                ],
            },
        })
    }

    #[test]
    fn restore_spends_slack_on_short_text() {
        let mut payloads = vec![body_payload(0.44, "短文本")];
        restore_comfort_body_leading(&mut payloads);
        let leading: f64 = payloads[0]["leading_em"].as_f64().unwrap();
        assert!(leading > 0.44);
        assert!(payloads[0].get("_body_leading_decision").is_some());
        assert!(payloads[0]["_body_vertical_budget"]["target_density"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn skips_dense_payload() {
        let mut payload = body_payload(0.44, "短文本");
        payload.as_object_mut().unwrap().insert("dense_small_box".to_string(), json!(true));
        let mut payloads = vec![payload];
        restore_comfort_body_leading(&mut payloads);
        assert_eq!(payloads[0]["leading_em"], json!(0.44));
        assert!(payloads[0].get("_body_leading_decision").is_none());
    }

    #[test]
    fn refit_marks_decision_flag() {
        let mut payloads = vec![body_payload(0.44, "短文本")];
        refit_body_leading_after_font_unify(&mut payloads);
        assert_eq!(payloads[0]["_body_leading_decision"]["refit_after_font_unify"], json!(true));
    }

    #[test]
    fn baseline_requires_two_values() {
        assert!(page_body_leading_baseline(&[body_payload(0.44, "短文本")]).is_none());
        let two = vec![body_payload(0.44, "短文本"), body_payload(0.5, "另一段")];
        let baseline = page_body_leading_baseline(&two).unwrap();
        assert!(baseline > 0.44);
    }

    #[test]
    fn multiline_comfort_floor_applied() {
        // Estimated 4 lines with a low leading: floor lifts to BODY_COMFORT_LEADING_MIN.
        const MULTILINE: &str =
            "这是一段足够长的中文段落文字内容，它需要能够在给定的宽度之内被排版成多行文字才能满足段落的自然阅读习惯。这里再补充一部分内容让文本继续变长，直到它能够稳定的产生三行以上的估计行数。";
        let mut payloads = vec![body_payload(0.44, MULTILINE)];
        ensure_multiline_comfort_floor(&mut payloads[0]);
        assert_eq!(payloads[0]["leading_em"], json!(0.56));
    }
}

#[cfg(test)]
mod tests_solver {
    use super::*;
    use serde_json::json;

    fn body_payload(leading_em: f64, translated: &str) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 350.0, 120.0],
            "translated_text": translated,
            "formula_map": [],
            "font_size_pt": 12.0,
            "leading_em": leading_em,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {
                "bbox": [50.0, 0.0, 350.0, 120.0],
                "lines": [
                    {"bbox": [50.0, 0.0, 350.0, 20.0], "spans": [{"type": "text", "text": "第一行"}]},
                    {"bbox": [50.0, 24.0, 350.0, 44.0], "spans": [{"type": "text", "text": "第二行"}]},
                    {"bbox": [50.0, 48.0, 350.0, 68.0], "spans": [{"type": "text", "text": "第三行"}]},
                ],
            },
        })
    }

    const DENSE_TEXT: &str =
        "段落文字内容足够长的时候会占满整个高度并且还有更多的文本需要换行排列这样才能触发碰撞拟合逻辑的正常路径执行。这段内容再次重复一遍以确保它足够长，能够稳定地超出相邻块可用的垂直高度预算从而真正进入碰撞处理的分支而不是提前返回。再加上更多的文字让段落变得足够高，最终拟合时字体尺寸一定会被压缩到更小的数值以满足垂直空间的限制要求。";

    #[test]
    fn dense_payload_returns_none() {
        // A long text at leading 0.68 on this box is dense and left alone.
        let payload = body_payload(0.68, DENSE_TEXT);
        assert!(solve_body_leading(&payload, None).is_none());
    }

    #[test]
    fn underfilled_payload_produces_solution() {
        // Low density (short text in a tall box) should yield a leading bump.
        let payload = body_payload(0.44, "短文本");
        let solution = solve_body_leading(&payload, None);
        assert!(solution.is_some());
        let solution = solution.unwrap();
        assert!(solution.leading_em >= 0.44);
        assert!(solution.leading_cap_em > 0.0);
    }

    #[test]
    fn annotate_records_budget() {
        let mut payload = body_payload(0.44, "短文本");
        let solution = BodyLeadingSolution {
            leading_em: 0.6,
            target_density: 0.8,
            leading_cap_em: 0.7,
        };
        annotate_body_vertical_budget(&mut payload, &solution);
        assert_eq!(payload["_body_vertical_budget"]["leading_growth_em"], json!(0.16));
        assert_eq!(payload["_body_vertical_budget"]["target_density"], json!(0.8));
    }
}

#[cfg(test)]
mod tests_smoothing {
    use super::*;
    use serde_json::json;

    fn payload(top: f64, bottom: f64, font: f64, leading: f64, text: &str) -> Value {
        json!({
            "inner_bbox": [50.0, top, 250.0, bottom],
            "translated_text": text,
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

    const ZH_A: &str = "这是一段足够长的中文文本内容，用于测试相邻正文块之间的字号和行距平滑。";
    const ZH_B: &str = "这是第二段足够长的中文文本内容，它的密度与前面一段略有差异以便观察平滑效果。";

    #[test]
    fn smooths_adjacent_same_column_pair() {
        let mut payloads = vec![payload(0.0, 40.0, 11.0, 0.44, ZH_A), payload(41.0, 90.0, 12.5, 0.58, ZH_B)];
        smooth_adjacent_body_payloads(&mut payloads, 200.0);
        let font0: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        let font1: f64 = payloads[1]["font_size_pt"].as_f64().unwrap();
        assert!((font0 - font1).abs() < 1.0);
        assert!(font0 > 11.0);
    }

    #[test]
    fn different_column_pair_untouched() {
        let mut payloads = vec![
            payload(0.0, 40.0, 11.0, 0.44, ZH_A),
            json!({
                "inner_bbox": [400.0, 41.0, 600.0, 90.0],
                "translated_text": ZH_B,
                "formula_map": [],
                "font_size_pt": 12.5,
                "leading_em": 0.58,
                "render_kind": "markdown",
                "is_body": true,
                "dense_small_box": false,
                "heavy_dense_small_box": false,
                "item": {},
            }),
        ];
        smooth_adjacent_body_payloads(&mut payloads, 200.0);
        assert_eq!(payloads[0]["font_size_pt"], json!(11.0));
        assert_eq!(payloads[1]["font_size_pt"], json!(12.5));
    }
}
