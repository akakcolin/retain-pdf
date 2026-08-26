// Port of backend/scripts/devtools/tests/text_layout/test_payload_capacity.py.

use rendering_core::item::FormulaEntry;
use rendering_core::payload::capacity::{estimated_render_height_pt, formula_estimate_discount};

fn formula_entry(placeholder: &str, formula_text: &str) -> FormulaEntry {
    FormulaEntry {
        placeholder: placeholder.into(),
        formula_text: formula_text.into(),
    }
}

#[test]
fn formula_estimate_discount_keeps_plain_text_unchanged() {
    assert_eq!(formula_estimate_discount("这是一段普通正文。", &[]), 1.0);
}

#[test]
fn formula_estimate_discount_is_continuous_and_bounded() {
    let formula_map = vec![
        formula_entry("[[FORMULA_1]]", r"x_1"),
        formula_entry("[[FORMULA_2]]", r"\\frac{\\partial E}{\\partial R}"),
        formula_entry("[[FORMULA_3]]", r"\\sqrt{\\delta R}"),
    ];
    let discount = formula_estimate_discount(
        "[[FORMULA_1]] 与 [[FORMULA_2]] 以及 [[FORMULA_3]] 共同决定结果。",
        &formula_map,
    );
    assert!(discount >= 0.86 && discount < 1.0);
}

#[test]
fn dollar_inline_math_contributes_to_formula_discount() {
    let discount = formula_estimate_discount(
        r"沿 $g-h(\mathbf{R}_x)$ 和 $\\frac{\\partial E}{\\partial R}$ 路径变化。",
        &[],
    );
    assert!(discount >= 0.86 && discount < 1.0);
}

#[test]
fn formula_heavy_height_estimate_is_less_aggressive_than_plain_line_count() {
    let inner = [0.0, 0.0, 120.0, 30.0];
    let formula_map = vec![
        formula_entry("[[FORMULA_1]]", r"x_1"),
        formula_entry("[[FORMULA_2]]", r"\\frac{\\partial E}{\\partial R}"),
        formula_entry("[[FORMULA_3]]", r"\\sqrt{\\delta R}"),
    ];
    let formula_text = "[[FORMULA_1]] 与 [[FORMULA_2]] 以及 [[FORMULA_3]] 共同决定结果。";
    let plain_text = "变量一与偏导能量以及平方根扰动共同决定结果。";
    let formula_height = estimated_render_height_pt(&inner, formula_text, &formula_map, 10.4, 0.6);
    let plain_height = estimated_render_height_pt(&inner, plain_text, &[], 10.4, 0.6);
    assert!(formula_height < plain_height * 1.35);
}
