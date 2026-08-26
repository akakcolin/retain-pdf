// Differential (golden replay) tests: payload (capacity / continuation_split /
// formula_cost / text_common).

mod common;

use common::*;
use rendering_core::payload::capacity::{
    box_capacity_units, estimated_render_height_pt, estimated_required_lines,
    formula_estimate_discount, text_demand_units,
};
use rendering_core::payload::continuation_split::split_protected_text_for_boxes;
use rendering_core::payload::formula_cost::{approx_formula_visible_text, token_units};
use rendering_core::payload::text_common::{
    layout_density_ratio, normalize_render_text, same_meaningful_render_text,
    strip_formula_placeholders, tokenize_protected_text, translated_zh_char_count,
};
use std::collections::HashMap;

#[test]
fn test_formula_estimate_discount() {
    let c = corpus();
    for case in cases(c, "payload.capacity.formula_estimate_discount") {
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let expected: f64 = from_value(&case.expected);
        let actual = formula_estimate_discount(&text, &to_formula_map(&fm));
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_box_capacity_units() {
    let c = corpus();
    for (_i, case) in cases(c, "payload.capacity.box_capacity_units").iter().enumerate() {
        let inner: Vec<f64> = from_value(&case.input["inner"]);
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let leading: f64 = from_value(&case.input["leading_em"]);
        let visual_lines: Option<i64> = from_value(&case.input["visual_lines"]);
        let expected: f64 = from_value(&case.expected);
        let actual = box_capacity_units(&inner, font, leading, visual_lines);
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_text_demand_units() {
    let c = corpus();
    for case in cases(c, "payload.capacity.text_demand_units") {
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let expected: f64 = from_value(&case.expected);
        let actual = text_demand_units(&text, &to_formula_map(&fm));
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_estimated_required_lines() {
    let c = corpus();
    for (i, case) in cases(c, "payload.capacity.estimated_required_lines").iter().enumerate() {
        let inner: Vec<f64> = from_value(&case.input["inner"]);
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let expected: i64 = from_value(&case.expected);
        let actual = estimated_required_lines(&inner, &text, &to_formula_map(&fm), font);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_estimated_render_height_pt() {
    let c = corpus();
    for (_i, case) in cases(c, "payload.capacity.estimated_render_height_pt").iter().enumerate() {
        let inner: Vec<f64> = from_value(&case.input["inner"]);
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let leading: f64 = from_value(&case.input["leading_em"]);
        let expected: f64 = from_value(&case.expected);
        let actual = estimated_render_height_pt(&inner, &text, &to_formula_map(&fm), font, leading);
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_split_protected_text_for_boxes() {
    let c = corpus();
    for (i, case) in cases(c, "payload.continuation_split.split_protected_text_for_boxes").iter().enumerate() {
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let capacities: Vec<f64> = from_value(&case.input["capacities"]);
        let preferred: Option<Vec<f64>> = from_value(&case.input["preferred_weights"]);
        let direct_math: bool = from_value(&case.input["direct_math_mode"]);
        let expected: Vec<String> = from_value(&case.expected);
        let actual = split_protected_text_for_boxes(
            &text,
            &to_formula_pairs(&fm),
            &capacities,
            preferred.as_deref(),
            direct_math,
        );
        assert_eq!(actual, expected, "chunk mismatch at case {i}");
    }
}

#[test]
fn test_approx_formula_visible_text() {
    let c = corpus();
    for (i, case) in cases(c, "payload.formula_cost.approx_formula_visible_text").iter().enumerate() {
        let formula: String = from_value(&case.input["formula"]);
        let expected: String = from_value(&case.expected);
        let actual = approx_formula_visible_text(&formula);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_token_units() {
    let c = corpus();
    for (_i, case) in cases(c, "payload.formula_cost.token_units").iter().enumerate() {
        let token: String = from_value(&case.input["token"]);
        let lookup: HashMap<String, String> = from_value(&case.input["formula_lookup"]);
        let expected: f64 = from_value(&case.expected);
        let actual = token_units(&token, &lookup);
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_tokenize_protected_text() {
    let c = corpus();
    for (i, case) in cases(c, "payload.text_common.tokenize_protected_text").iter().enumerate() {
        let text: String = from_value(&case.input["text"]);
        let expected: Vec<String> = from_value(&case.expected);
        let actual = tokenize_protected_text(&text);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_strip_formula_placeholders() {
    let c = corpus();
    for (i, case) in cases(c, "payload.text_common.strip_formula_placeholders").iter().enumerate() {
        let text: String = from_value(&case.input["text"]);
        let expected: String = from_value(&case.expected);
        let actual = strip_formula_placeholders(&text);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_normalize_render_text() {
    let c = corpus();
    for (i, case) in cases(c, "payload.text_common.normalize_render_text").iter().enumerate() {
        let text: String = from_value(&case.input["text"]);
        let expected: String = from_value(&case.expected);
        let actual = normalize_render_text(&text);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_same_meaningful_render_text() {
    let c = corpus();
    for (i, case) in cases(c, "payload.text_common.same_meaningful_render_text").iter().enumerate() {
        let a: String = from_value(&case.input["a"]);
        let b: String = from_value(&case.input["b"]);
        let expected: bool = from_value(&case.expected);
        let actual = same_meaningful_render_text(&a, &b);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_translated_zh_char_count() {
    let c = corpus();
    for (i, case) in cases(c, "payload.text_common.translated_zh_char_count").iter().enumerate() {
        let text: String = from_value(&case.input["text"]);
        let expected: usize = from_value(&case.expected);
        let actual = translated_zh_char_count(&text);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_layout_density_ratio() {
    let c = corpus();
    for (_i, case) in cases(c, "payload.text_common.layout_density_ratio").iter().enumerate() {
        let inner: Vec<f64> = from_value(&case.input["inner"]);
        let text: String = from_value(&case.input["text"]);
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let line_step: f64 = from_value(&case.input["line_step_pt"]);
        let expected: f64 = from_value(&case.expected);
        let actual = layout_density_ratio(&inner, &text, font, line_step);
        assert_close_f64(actual, expected);
    }
}
