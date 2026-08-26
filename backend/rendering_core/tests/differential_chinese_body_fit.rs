// Differential (golden replay) tests: chinese_body_fit + fit_decision.
// Replays backend/rendering_core/tests/corpus.json against the lib.

mod common;

use common::*;
use rendering_core::chinese_body_fit::{
    estimate_chinese_body_height_pt, estimate_chinese_body_lines, formula_unit_ratio,
    solve_chinese_body_font_size_pt, tokenize_chinese_body_text, ChineseBodyFitConfig,
    ChineseBodyToken,
};
use rendering_core::fit_decision::plan_chinese_body_fit;
use serde::Deserialize;

fn cfg() -> ChineseBodyFitConfig {
    ChineseBodyFitConfig::default()
}

#[derive(Deserialize)]
struct TokenExpected {
    text: String,
    units: f64,
    formula: bool,
}

#[test]
fn test_formula_unit_ratio() {
    let c = corpus();
    for case in cases(c, "chinese_body_fit.formula_unit_ratio") {
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let expected: f64 = from_value(&case.expected);
        let actual = formula_unit_ratio(&text, Some(&to_formula_map(&fm)), &cfg());
        assert_close_f64(actual, expected);
    }
}

#[test]
fn test_tokenize_chinese_body_text() {
    let c = corpus();
    for (i, case) in cases(c, "chinese_body_fit.tokenize_chinese_body_text").iter().enumerate() {
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let expected: Vec<TokenExpected> = from_value(&case.expected);
        let actual: Vec<ChineseBodyToken> =
            tokenize_chinese_body_text(&text, Some(&to_formula_map(&fm)), &cfg());
        assert_eq!(
            actual.len(),
            expected.len(),
            "token count mismatch at case {i}"
        );
        for (a, e) in actual.iter().zip(expected.iter()) {
            assert_eq!(a.text, e.text, "token text mismatch at case {i}");
            assert_close_f64(a.units, e.units);
            assert_eq!(a.formula, e.formula, "token formula flag mismatch at case {i}");
        }
    }
}

#[test]
fn test_estimate_chinese_body_lines() {
    let c = corpus();
    for (i, case) in cases(c, "chinese_body_fit.estimate_chinese_body_lines").iter().enumerate() {
        let width: f64 = from_value(&case.input["bbox_width_pt"]);
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let expected: [i64; 2] = from_value(&case.expected);
        let (lines, formula_lines) =
            estimate_chinese_body_lines(width, &text, Some(&to_formula_map(&fm)), font, &cfg());
        assert_eq!(lines, expected[0], "line_count mismatch at case {i}");
        assert_eq!(formula_lines, expected[1], "formula_line_count mismatch at case {i}");
    }
}

#[test]
fn test_estimate_chinese_body_height_pt() {
    let c = corpus();
    for (i, case) in cases(c, "chinese_body_fit.estimate_chinese_body_height_pt").iter().enumerate() {
        let width: f64 = from_value(&case.input["bbox_width_pt"]);
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let leading: f64 = from_value(&case.input["leading_em"]);
        let e: ChineseBodyFitExpected = from_value(&case.expected);
        let r = estimate_chinese_body_height_pt(width, &text, Some(&to_formula_map(&fm)), font, leading, &cfg());
        assert_close_f64(r.font_size_pt, e.font_size_pt);
        assert_close_f64(r.estimated_height_pt, e.estimated_height_pt);
        assert_eq!(r.line_count, e.line_count, "line_count mismatch at case {i}");
        assert_close_f64(r.overflow_ratio, e.overflow_ratio);
        assert_close_f64(r.formula_ratio, e.formula_ratio);
        assert_close_f64(r.confidence, e.confidence);
        assert_close_f64(r.max_safe_shrink_pt, e.max_safe_shrink_pt);
    }
}

#[test]
fn test_solve_chinese_body_font_size_pt() {
    let c = corpus();
    for (i, case) in cases(c, "chinese_body_fit.solve_chinese_body_font_size_pt").iter().enumerate() {
        let width: f64 = from_value(&case.input["bbox_width_pt"]);
        let height: f64 = from_value(&case.input["bbox_height_pt"]);
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let leading: f64 = from_value(&case.input["leading_em"]);
        let min_font: f64 = from_value(&case.input["min_font_size_pt"]);
        let max_font: f64 = from_value(&case.input["max_font_size_pt"]);
        let e: ChineseBodyFitExpected = from_value(&case.expected);
        let r = solve_chinese_body_font_size_pt(
            width,
            height,
            &text,
            Some(&to_formula_map(&fm)),
            leading,
            min_font,
            max_font,
            &cfg(),
        );
        assert_close_f64(r.font_size_pt, e.font_size_pt);
        assert_close_f64(r.estimated_height_pt, e.estimated_height_pt);
        assert_eq!(r.line_count, e.line_count, "line_count mismatch at case {i}");
        assert_close_f64(r.overflow_ratio, e.overflow_ratio);
        assert_close_f64(r.formula_ratio, e.formula_ratio);
        assert_close_f64(r.confidence, e.confidence);
        assert_close_f64(r.max_safe_shrink_pt, e.max_safe_shrink_pt);
    }
}

#[test]
fn test_plan_chinese_body_fit() {
    let c = corpus();
    for (i, case) in cases(c, "fit_decision.plan_chinese_body_fit").iter().enumerate() {
        let width: f64 = from_value(&case.input["bbox_width_pt"]);
        let height: f64 = from_value(&case.input["bbox_height_pt"]);
        let text: String = from_value(&case.input["text"]);
        let fm: Vec<FormulaDto> = from_value(&case.input["formula_map"]);
        let font: f64 = from_value(&case.input["font_size_pt"]);
        let leading: f64 = from_value(&case.input["leading_em"]);
        let growth: Option<f64> = from_value(&case.input["max_growth_font_size_pt"]);
        let e: FitDecisionExpected = from_value(&case.expected);
        let d = plan_chinese_body_fit(width, height, &text, &to_formula_map(&fm), font, leading, growth);
        assert_close_f64(d.font_size_pt, e.font_size_pt);
        assert_eq!(d.mode, e.mode, "mode mismatch at case {i}");
        assert_close_f64(d.confidence, e.confidence);
        assert_eq!(d.reason_codes, e.reason_codes, "reason_codes mismatch at case {i}");
        assert_close_f64(d.estimated_height_pt, e.estimated_height_pt);
        assert_close_f64(d.overflow_ratio, e.overflow_ratio);
        assert_close_f64(d.formula_ratio, e.formula_ratio);
        assert_close_f64(d.growth_pt, e.growth_pt);
        assert_close_f64(d.shrink_pt, e.shrink_pt);
    }
}
