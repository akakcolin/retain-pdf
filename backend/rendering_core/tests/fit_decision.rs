// Port of backend/scripts/devtools/tests/text_layout/test_fit_decision.py.

use rendering_core::fit_decision::plan_chinese_body_fit;
use rendering_core::item::FormulaEntry;

fn formula_entry(placeholder: &str, formula_text: &str) -> FormulaEntry {
    FormulaEntry {
        placeholder: placeholder.into(),
        formula_text: formula_text.into(),
    }
}

#[test]
fn formula_heavy_text_limits_chinese_body_shrink() {
    let text = "__FORMULA_1__ 与 __FORMULA_2__ 决定 __FORMULA_3__ 的变化。";
    let formula_map = vec![
        formula_entry("__FORMULA_1__", r"g^{IJ}(R_x)"),
        formula_entry("__FORMULA_2__", r"h^{IJ}(R_x)"),
        formula_entry("__FORMULA_3__", r"\\delta\\mathbf{R}"),
    ];
    let decision = plan_chinese_body_fit(100.0, 24.0, text, &formula_map, 10.4, 0.6, None);
    assert!(decision.font_size_pt >= 10.2);
    assert!(decision.reason_codes.iter().any(|c| c == "formula_weighted"));
    assert!(decision.reason_codes.iter().any(|c| c == "formula_complexity"));
}

#[test]
fn plain_chinese_height_pressure_can_shrink_safely() {
    let text = "这是一个较长的中文正文段落，需要根据宽度估算换行，并在高度不足时只做轻微字号调整。";
    let decision = plan_chinese_body_fit(120.0, 65.0, text, &[], 10.6, 0.6, None);
    assert!(decision.font_size_pt < 10.6);
    assert!(decision.font_size_pt >= 10.0);
    assert!(decision.shrink_pt > 0.0);
    assert_eq!(decision.mode, "continuous_fit");
}

#[test]
fn underfill_growth_is_gradual() {
    let text = "这是较短的正文。";
    let low_fill = plan_chinese_body_fit(180.0, 90.0, text, &[], 10.0, 0.6, Some(10.35));
    let medium_fill = plan_chinese_body_fit(180.0, 52.0, text, &[], 10.0, 0.6, Some(10.35));
    assert!(low_fill.growth_pt > medium_fill.growth_pt);
    assert!(medium_fill.growth_pt > 0.0 && medium_fill.growth_pt < 0.35);
    assert!(low_fill.font_size_pt <= 10.35);
}

#[test]
fn formula_ratio_reduces_underfill_growth() {
    let plain = plan_chinese_body_fit(180.0, 90.0, "这是较短的正文。", &[], 10.0, 0.6, Some(10.35));
    let formula_map = vec![
        formula_entry("__FORMULA_1__", r"g^{IJ}(R_x)"),
        formula_entry("__FORMULA_2__", r"h^{IJ}(R_x)"),
    ];
    let formula = plan_chinese_body_fit(
        180.0,
        90.0,
        "__FORMULA_1__ 决定 __FORMULA_2__。",
        &formula_map,
        10.0,
        0.6,
        Some(10.35),
    );
    assert!(formula.growth_pt < plain.growth_pt);
}

#[test]
fn many_simple_inline_formulas_do_not_shrink_aggressively() {
    let text = "__FORMULA_1__、__FORMULA_2__、__FORMULA_3__ 与 __FORMULA_4__ 描述了势能面。";
    let formula_map = vec![
        formula_entry("__FORMULA_1__", r"x_1"),
        formula_entry("__FORMULA_2__", r"x_2"),
        formula_entry("__FORMULA_3__", r"E_1"),
        formula_entry("__FORMULA_4__", r"E_2"),
    ];
    let decision = plan_chinese_body_fit(112.0, 24.0, text, &formula_map, 10.4, 0.6, None);
    assert!(decision.font_size_pt >= 10.1);
    assert!(decision.reason_codes.iter().any(|c| c == "formula_count_weighted"));
}

#[test]
fn complex_formula_count_reduces_estimate_trust_more_than_simple_count() {
    let simple = plan_chinese_body_fit(
        180.0,
        90.0,
        "__FORMULA_1__ 决定 __FORMULA_2__。",
        &[
            formula_entry("__FORMULA_1__", r"x_1"),
            formula_entry("__FORMULA_2__", r"x_2"),
        ],
        10.0,
        0.6,
        Some(10.35),
    );
    let complex_formula = plan_chinese_body_fit(
        180.0,
        90.0,
        "__FORMULA_1__ 决定 __FORMULA_2__。",
        &[
            formula_entry("__FORMULA_1__", r"\\frac{\\partial E}{\\partial R}"),
            formula_entry("__FORMULA_2__", r"\\sqrt{\\delta\\mathbf{R}^{IJ}}"),
        ],
        10.0,
        0.6,
        Some(10.35),
    );
    assert!(complex_formula.confidence < simple.confidence);
    assert!(complex_formula.growth_pt < simple.growth_pt);
}
