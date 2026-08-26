// Port of services/rendering/layout/fit_decision/{curves,models,planner}.py.

use crate::chinese_body_fit::{estimate_chinese_body_height_pt, ChineseBodyFitConfig};
use crate::item::FormulaEntry;
use crate::util::py_round;

// curves.py
fn clamp(value: f64, low: f64, high: f64) -> f64 {
    value.max(low).min(high)
}

fn sigmoid(value: f64, slope: f64) -> f64 {
    1.0 / (1.0 + (-slope * value).exp())
}

fn centered_sigmoid_force(value: f64, slope: f64) -> f64 {
    clamp((sigmoid(value, slope) - 0.5) * 2.0, 0.0, 1.0)
}

fn exp_decay(value: f64, strength: f64, floor: f64) -> f64 {
    ((-strength * value.max(0.0)).exp()).max(floor)
}

// models.py
#[derive(Debug, Clone)]
pub struct FitFeatures {
    pub bbox_width_pt: f64,
    pub bbox_height_pt: f64,
    pub font_size_pt: f64,
    pub leading_em: f64,
    pub estimated_height_pt: f64,
    pub estimated_overflow_ratio: f64,
    pub formula_ratio: f64,
    pub confidence: f64,
    pub max_safe_shrink_pt: f64,
    pub formula_complexity: f64,
    pub inline_formula_count: i64,
    pub complex_formula_count: i64,
    pub formula_count_ratio: f64,
    pub height_estimate_discount: f64,
    pub trust: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FitDecision {
    pub font_size_pt: f64,
    pub mode: String,
    pub confidence: f64,
    pub reason_codes: Vec<String>,
    pub estimated_height_pt: f64,
    pub overflow_ratio: f64,
    pub formula_ratio: f64,
    pub growth_pt: f64,
    pub shrink_pt: f64,
}

// planner.py
const TARGET_FILL_RATIO: f64 = 0.86;
const MAX_GROWTH_PT: f64 = 0.35;
const MAX_SHRINK_PT: f64 = 0.6;
const FORMULA_COMPLEXITY_COMMANDS: [&str; 14] = [
    "frac", "dfrac", "tfrac", "sqrt", "sum", "prod", "int", "delta", "Delta", "partial", "mathbf",
    "begin", "overline", "underline",
];

fn count_complex_commands(formula: &str) -> usize {
    let bytes = formula.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let mut matched = false;
            for name in FORMULA_COMPLEXITY_COMMANDS {
                if formula[i + 1..].starts_with(name) {
                    count += 1;
                    i += 1 + name.len();
                    matched = true;
                    break;
                }
            }
            if !matched {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    count
}

fn nonspace_len(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

struct FormulaFeatures {
    inline_count: i64,
    complex_count: i64,
    count_ratio: f64,
    complexity: f64,
}

fn formula_features(formula_map: &[FormulaEntry], text: &str) -> FormulaFeatures {
    if formula_map.is_empty() {
        return FormulaFeatures {
            inline_count: 0,
            complex_count: 0,
            count_ratio: 0.0,
            complexity: 0.0,
        };
    }
    let inline_count = formula_map.len() as i64;
    let mut complex_count = 0i64;
    let mut complexity = 0.0;
    for entry in formula_map {
        let formula = &entry.formula_text;
        let command_count = count_complex_commands(formula);
        let script_count = formula.matches('^').count() + formula.matches('_').count();
        if command_count > 0 {
            complex_count += 1;
        }
        complexity += command_count as f64 * 0.16;
        complexity += (script_count as f64 * 0.025).min(0.12);
        complexity += (((formula.chars().count() as i64) - 48).max(0) as f64 * 0.0015).min(0.06);
    }
    let text_units = (nonspace_len(text) as f64).max(1.0);
    let count_ratio = inline_count as f64 / (inline_count as f64 + text_units / 12.0);
    FormulaFeatures {
        inline_count,
        complex_count,
        count_ratio: py_round(clamp(count_ratio, 0.0, 1.0), 3),
        complexity: py_round(clamp(complexity, 0.0, 1.0), 3),
    }
}

fn formula_trust(formula_ratio: f64, features: &FormulaFeatures) -> f64 {
    let simple_count = (features.inline_count - features.complex_count).max(0);
    let formula_force = formula_ratio * 0.65
        + features.count_ratio * 0.55
        + simple_count as f64 * 0.055
        + features.complex_count as f64 * 0.26
        + features.complexity * 0.45;
    exp_decay(formula_force, 1.35, 0.22)
}

fn height_estimate_discount(formula_ratio: f64, features: &FormulaFeatures) -> f64 {
    let uncertainty =
        formula_ratio * 0.42 + features.count_ratio * 0.38 + features.complex_count as f64 * 0.05;
    1.0 - 0.18 * centered_sigmoid_force(uncertainty, 3.0)
}

fn features(
    bbox_width_pt: f64,
    bbox_height_pt: f64,
    text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
    leading_em: f64,
) -> FitFeatures {
    let estimate = estimate_chinese_body_height_pt(
        bbox_width_pt,
        text,
        Some(formula_map),
        font_size_pt,
        leading_em,
        &ChineseBodyFitConfig::default(),
    );
    let formula_features = formula_features(formula_map, text);
    let trust = formula_trust(estimate.formula_ratio, &formula_features);
    let height_discount = height_estimate_discount(estimate.formula_ratio, &formula_features);
    let estimated_height_pt = estimate.estimated_height_pt * height_discount;
    FitFeatures {
        bbox_width_pt,
        bbox_height_pt,
        font_size_pt,
        leading_em,
        estimated_height_pt,
        estimated_overflow_ratio: estimated_height_pt / bbox_height_pt.max(1.0),
        formula_ratio: estimate.formula_ratio,
        confidence: estimate.confidence,
        max_safe_shrink_pt: estimate.max_safe_shrink_pt,
        formula_complexity: formula_features.complexity,
        inline_formula_count: formula_features.inline_count,
        complex_formula_count: formula_features.complex_count,
        formula_count_ratio: formula_features.count_ratio,
        height_estimate_discount: height_discount,
        trust,
    }
}

fn reason_codes(features: &FitFeatures, growth_pt: f64, shrink_pt: f64) -> Vec<String> {
    let mut reasons: Vec<String> = Vec::new();
    if growth_pt > 0.0 {
        reasons.push("underfilled_body".to_string());
    }
    if shrink_pt > 0.0 {
        reasons.push("height_pressure".to_string());
    }
    if features.formula_ratio > 0.0 {
        reasons.push("formula_weighted".to_string());
    }
    if features.formula_complexity > 0.0 {
        reasons.push("formula_complexity".to_string());
    }
    if features.inline_formula_count > 0 {
        reasons.push("formula_count_weighted".to_string());
    }
    reasons
}

fn delta_font_pt(features: &FitFeatures, max_growth_font_size_pt: Option<f64>) -> (f64, f64) {
    let fill_ratio = features.estimated_overflow_ratio;
    let underfill_force = centered_sigmoid_force((TARGET_FILL_RATIO - fill_ratio) / TARGET_FILL_RATIO, 5.0);
    let overflow_force = centered_sigmoid_force(fill_ratio - TARGET_FILL_RATIO, 4.2);
    let mut growth_cap = MAX_GROWTH_PT;
    if let Some(max_growth) = max_growth_font_size_pt {
        growth_cap = growth_cap.min((max_growth - features.font_size_pt).max(0.0));
    }
    let growth_pt = growth_cap * underfill_force * features.trust;
    let shrink_pt =
        MAX_SHRINK_PT.min(features.max_safe_shrink_pt) * overflow_force * features.trust * features.trust;
    (py_round(growth_pt, 3), py_round(shrink_pt, 3))
}

pub fn plan_chinese_body_fit(
    bbox_width_pt: f64,
    bbox_height_pt: f64,
    text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
    leading_em: f64,
    max_growth_font_size_pt: Option<f64>,
) -> FitDecision {
    let features = features(bbox_width_pt, bbox_height_pt, text, formula_map, font_size_pt, leading_em);
    let (growth_pt, shrink_pt) = delta_font_pt(&features, max_growth_font_size_pt);
    let mut next_font_size = py_round(font_size_pt + growth_pt - shrink_pt, 2);
    if let Some(max_growth) = max_growth_font_size_pt {
        next_font_size = next_font_size.min(max_growth);
    }
    next_font_size = py_round(next_font_size.max(7.8), 2);
    let reason_codes = reason_codes(&features, growth_pt, shrink_pt);
    let mode = if (next_font_size - font_size_pt).abs() >= 0.005 {
        "continuous_fit"
    } else {
        "fast_estimate"
    };
    FitDecision {
        font_size_pt: next_font_size,
        mode: mode.to_string(),
        confidence: features.trust,
        reason_codes,
        estimated_height_pt: features.estimated_height_pt,
        overflow_ratio: features.estimated_overflow_ratio,
        formula_ratio: features.formula_ratio,
        growth_pt: py_round(growth_pt, 2),
        shrink_pt: py_round(shrink_pt, 2),
    }
}
