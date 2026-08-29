// Port of services/rendering/layout/payload/body_leading_solver.py — the solver
// that spends remaining vertical slack on body line spacing. Operates on the raw
// JSON block-payload dicts and the leading/target DTOs from `typography_decision`.

use serde_json::Value;

use crate::item::Item;
use crate::layout::body_common::{payload_density, required_lines};
use crate::layout::payload_dict::payload_f64;
use crate::layout::typography_decision::{
    font_growth_grew_pt, font_growth_seed_font_pt, font_growth_slack_ratio, set_vertical_budget,
    VerticalBudget,
};
use crate::layout::typography_policy as typography;
use crate::leading_fit::{BODY_LEADING_MAX, BODY_LEADING_MIN};
use crate::typography::line_count::visual_line_count;
use crate::typography::line_metrics::{local_line_pitch, median_line_pitch};
use crate::util::py_round;

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
            source_leading_em: clamp(source_leading, 0.0, typography::BODY_COMFORT_SOURCE_LINE_LEADING_MAX),
            font_growth_pt: grew.max(0.0),
            font_growth_score: clamp(
                (grew / typography::BODY_COMFORT_FONT_GROWTH_NORMALIZER_PT) * slack_ratio.max(0.0),
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
    let ctx = BodyLeadingContext::from_payload(payload, page_baseline_leading_em);
    if ctx.current_density > typography::BODY_COMFORT_LEADING_DENSITY_MAX {
        return None;
    }
    if ctx.current_density >= typography::BODY_COMFORT_DENSITY_FLOOR_TRIGGER {
        return None;
    }

    let leading_cap = leading_cap(&ctx);
    let low = payload_f64(payload, "leading_em", BODY_LEADING_MIN);
    let high = low.max(leading_cap);
    let paired_low = font_paired_min_leading(&ctx, low, high);
    let target_density = typography::BODY_COMFORT_LEADING_DENSITY_MAX.min(target_density(&ctx));
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
    let mut best = low;
    let mut low_residual = target_density - payload_density(payload, None, Some(low));
    let mut high_residual = target_density - payload_density(payload, None, Some(high));

    for _ in 0..typography::BODY_COMFORT_SOLVER_MAX_ITERATIONS {
        if high - low <= typography::BODY_COMFORT_SOLVER_MIN_BRACKET_WIDTH {
            break;
        }
        let trial = residual_extrapolated_leading(low, high, low_residual, high_residual);
        let density = payload_density(payload, None, Some(trial));
        let residual = target_density - density;
        if residual.abs() <= typography::BODY_COMFORT_SOLVER_DENSITY_TOLERANCE {
            if density <= typography::BODY_COMFORT_LEADING_DENSITY_MAX && residual >= 0.0 {
                best = trial;
            }
            break;
        }
        if density <= typography::BODY_COMFORT_LEADING_DENSITY_MAX && residual >= 0.0 {
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
    let residual_span = low_residual - high_residual;
    if residual_span.abs() <= 1e-9 {
        return (low + high) / 2.0;
    }
    let fraction = clamp(
        low_residual / residual_span,
        typography::BODY_COMFORT_SOLVER_EXTRAPOLATION_MIN_FRACTION,
        typography::BODY_COMFORT_SOLVER_EXTRAPOLATION_MAX_FRACTION,
    );
    low + (high - low) * fraction
}

fn target_density(ctx: &BodyLeadingContext) -> f64 {
    if ctx.line_count <= 2 {
        return typography::BODY_COMFORT_DENSITY_RECOVERY_TARGET_SHORT;
    }
    if !ctx.has_source_line_geometry {
        return typography::BODY_COMFORT_DENSITY_RECOVERY_TARGET_NO_SOURCE;
    }
    if 0 < ctx.source_lines && ctx.source_lines <= typography::BODY_COMFORT_LOW_SOURCE_LINE_COUNT_MAX {
        return typography::BODY_COMFORT_DENSITY_RECOVERY_TARGET
            .min(typography::BODY_COMFORT_LOW_SOURCE_LINE_TARGET_FILL_MAX);
    }
    typography::BODY_COMFORT_DENSITY_RECOVERY_TARGET
}

fn leading_cap(ctx: &BodyLeadingContext) -> f64 {
    if ctx.has_source_line_geometry
        && 0 < ctx.source_lines
        && ctx.source_lines <= typography::BODY_COMFORT_LOW_SOURCE_LINE_COUNT_MAX
    {
        return clamp(
            typography::BODY_COMFORT_LOW_SOURCE_LINE_LEADING_MAX,
            typography::BODY_COMFORT_LEADING_MIN,
            typography::BODY_COMFORT_SOURCE_LINE_LEADING_MAX,
        );
    }
    if ctx.line_count <= 1 {
        return BODY_LEADING_MAX;
    }

    let line_extra = (ctx.line_count - typography::BODY_COMFORT_LONG_LINE_THRESHOLD + 1).max(0) as f64;
    let mut long_cap_target = typography::BODY_COMFORT_LONG_LEADING_MAX;
    if ctx.source_lines <= 0 {
        long_cap_target = typography::BODY_COMFORT_NO_SOURCE_LONG_LEADING_MAX;
    }
    let long_line_cap = BODY_LEADING_MAX
        + exp_response(line_extra, typography::BODY_COMFORT_LONG_LINE_CAP_RESPONSE_RATE)
            * (long_cap_target - BODY_LEADING_MAX);

    let source_gap = (ctx.source_line_ratio() - 1.0).max(0.0);
    let source_cap = BODY_LEADING_MAX
        + exp_response(source_gap, typography::BODY_COMFORT_SOURCE_LINE_CAP_RESPONSE_RATE)
            * (typography::BODY_COMFORT_SOURCE_LINE_LEADING_MAX - BODY_LEADING_MAX);

    let pitch_cap = BODY_LEADING_MAX
        + exp_response(ctx.source_leading_em, typography::BODY_COMFORT_SOURCE_PITCH_CAP_RESPONSE_RATE)
            * (typography::BODY_COMFORT_SOURCE_LINE_LEADING_MAX - BODY_LEADING_MAX);

    let cap = BODY_LEADING_MAX.max(long_line_cap).max(source_cap).max(pitch_cap);
    clamp(cap, typography::BODY_COMFORT_LEADING_MIN, typography::BODY_COMFORT_SOURCE_LINE_LEADING_MAX)
}

fn font_paired_min_leading(ctx: &BodyLeadingContext, current_leading: f64, leading_cap: f64) -> f64 {
    if ctx.font_growth_pt <= 0.0 {
        return current_leading;
    }
    let line_weight =
        exp_response((ctx.line_count - 1).max(0) as f64, typography::BODY_COMFORT_MULTI_LINE_WEIGHT_RATE);
    let paired_gain = typography::BODY_COMFORT_FONT_GROWTH_MIN_LEADING_GAIN_MAX
        * exp_response(ctx.font_growth_pt, typography::BODY_COMFORT_FONT_GROWTH_MIN_LEADING_RATE)
        * line_weight;
    clamp(current_leading + paired_gain, current_leading, leading_cap)
}

fn leading_growth_budget(ctx: &BodyLeadingContext) -> f64 {
    let line_weight =
        exp_response((ctx.line_count - 1).max(0) as f64, typography::BODY_COMFORT_MULTI_LINE_WEIGHT_RATE);
    let long_text_bonus = typography::BODY_COMFORT_LEADING_GROWTH_LONG_TEXT_MAX
        * exp_response(
            (ctx.line_count as f64 - typography::BODY_COMFORT_LINE_COUNT_BASE).max(0.0),
            typography::BODY_COMFORT_LINE_COUNT_RESPONSE_RATE,
        );
    let mut source_line_pressure = 0.0;
    if ctx.has_source_line_geometry {
        source_line_pressure = exp_response(
            (ctx.source_lines as f64 - typography::BODY_COMFORT_SOURCE_LINE_VOLUME_BASE).max(0.0),
            typography::BODY_COMFORT_SOURCE_LINE_VOLUME_RESPONSE_RATE,
        );
    }
    let source_pitch_pressure = exp_response(
        (ctx.source_leading_em - typography::BODY_COMFORT_LEADING_MIN).max(0.0),
        typography::BODY_COMFORT_SOURCE_PITCH_RESPONSE_RATE,
    );
    let source_bonus = typography::BODY_COMFORT_LEADING_GROWTH_SOURCE_MAX
        * source_line_pressure.max(source_pitch_pressure);
    let font_spend_penalty = typography::BODY_COMFORT_LEADING_FONT_SPEND_PENALTY_MAX
        * exp_response(ctx.font_growth_pt, typography::BODY_COMFORT_FONT_GROWTH_MIN_LEADING_RATE);
    let floor = if ctx.font_growth_pt > 0.0 {
        typography::BODY_COMFORT_LEADING_GROWTH_MIN_AFTER_FONT_GROWTH
    } else {
        0.0
    };
    (typography::BODY_COMFORT_LEADING_GROWTH_BASE_MAX * line_weight
        + long_text_bonus
        + source_bonus
        - font_spend_penalty)
        .max(floor)
}

/// `_has_source_line_geometry`: the item carries a non-empty `lines` array.
fn has_source_line_geometry(item: &Item) -> bool {
    !item.lines.is_empty()
}

#[cfg(test)]
mod tests {
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
