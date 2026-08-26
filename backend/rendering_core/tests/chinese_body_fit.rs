// Port of backend/scripts/devtools/tests/text_layout/test_chinese_body_fit.py.

use rendering_core::chinese_body_fit::{
    estimate_chinese_body_height_pt, estimate_chinese_body_lines, formula_unit_ratio,
    solve_chinese_body_font_size_pt, tokenize_chinese_body_text, ChineseBodyFitConfig,
};
use rendering_core::item::FormulaEntry;

fn config() -> ChineseBodyFitConfig {
    ChineseBodyFitConfig::default()
}

#[test]
fn chinese_body_line_count_uses_bbox_width() {
    let text = "这是一个用于测试中文正文宽度估算的段落，宽度越窄换行越多。";
    let (wide_lines, _) = estimate_chinese_body_lines(240.0, text, None, 10.0, &config());
    let (narrow_lines, _) = estimate_chinese_body_lines(80.0, text, None, 10.0, &config());
    assert!(narrow_lines > wide_lines);
}

#[test]
fn chinese_body_solver_reduces_font_when_height_is_tight() {
    let text = "这是一个较长的中文正文段落，需要根据 bbox 宽度估算换行数，再根据高度反推出可用字号。";
    let loose = solve_chinese_body_font_size_pt(
        180.0, 90.0, text, None, 0.6, 8.0, 11.0, &config(),
    );
    let tight = solve_chinese_body_font_size_pt(
        180.0, 42.0, text, None, 0.6, 8.0, 11.0, &config(),
    );
    assert!(loose.font_size_pt > tight.font_size_pt);
    assert!(tight.estimated_height_pt <= 42.0);
}

#[test]
fn formula_lines_add_height_pressure() {
    let text = "其中 __FORMULA_1__ 表示能量差，后续正文继续解释该条件。";
    let formula_map = vec![FormulaEntry {
        placeholder: "__FORMULA_1__".into(),
        formula_text: r"\\Delta E_{IJ}(\\mathbf{R}) = 0".into(),
    }];
    let without_formula =
        estimate_chinese_body_height_pt(160.0, &text.replace("__FORMULA_1__", "能量差"), None, 10.0, 0.6, &config());
    let with_formula =
        estimate_chinese_body_height_pt(160.0, text, Some(&formula_map), 10.0, 0.6, &config());
    assert!(with_formula.estimated_height_pt > without_formula.estimated_height_pt);
    assert!(with_formula.formula_ratio > 0.0);
}

#[test]
fn tokenizer_distinguishes_chinese_ascii_punctuation_and_formula() {
    let formula_map = vec![FormulaEntry { placeholder: "__FORMULA_1__".into(), formula_text: "g-h".into() }];
    let tokens = tokenize_chinese_body_text("图3. g-h 平面 __FORMULA_1__", Some(&formula_map), &config());
    assert!(tokens.iter().any(|t| t.formula));
    assert!(tokens.iter().any(|t| t.text == "图"));
    assert!(tokens.iter().any(|t| t.text == "g-h"));
}

#[test]
fn tokenizer_treats_dollar_inline_math_as_formula() {
    let tokens = tokenize_chinese_body_text(r"沿 $g-h(\mathbf{R}_x)$ 路径变化。", None, &config());
    let formula_tokens: Vec<_> = tokens.iter().filter(|t| t.formula).collect();
    assert_eq!(formula_tokens.len(), 1);
    assert_eq!(formula_tokens[0].text, r"$g-h(\mathbf{R}_x)$");
}

#[test]
fn formula_heavy_text_lowers_fit_confidence() {
    let text = "__FORMULA_1__ 与 __FORMULA_2__ 决定 __FORMULA_3__ 的变化。";
    let formula_map = vec![
        FormulaEntry { placeholder: "__FORMULA_1__".into(), formula_text: r"g^{IJ}(R_x)".into() },
        FormulaEntry { placeholder: "__FORMULA_2__".into(), formula_text: r"h^{IJ}(R_x)".into() },
        FormulaEntry { placeholder: "__FORMULA_3__".into(), formula_text: r"\\delta\\mathbf{R}".into() },
    ];
    let result = estimate_chinese_body_height_pt(160.0, text, Some(&formula_map), 10.0, 0.6, &config());
    assert!(formula_unit_ratio(text, Some(&formula_map), &config()) > 0.35);
    assert!(result.confidence < 0.5);
    assert!(result.max_safe_shrink_pt <= 0.2);
}
