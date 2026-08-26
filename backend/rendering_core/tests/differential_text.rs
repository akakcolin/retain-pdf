// Differential (golden replay) tests: text (tokens / analysis / latex_normalizer).

mod common;

use common::*;
use rendering_core::text::analysis::analyze_text;
use rendering_core::text::latex_normalizer::{
    aggressively_simplify_formula_for_latex_math, normalize_formula_for_latex_math,
};
use rendering_core::text::tokens::{is_formula_token, tokenize_text};

#[test]
fn test_tokenize_text() {
    let c = corpus();
    for (i, case) in cases(c, "text.tokens.tokenize_text").iter().enumerate() {
        let text: String = from_value(&case.input["text"]);
        let expected: Vec<String> = from_value(&case.expected);
        let actual = tokenize_text(&text);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_is_formula_token() {
    let c = corpus();
    for (i, case) in cases(c, "text.tokens.is_formula_token").iter().enumerate() {
        let token: String = from_value(&case.input["token"]);
        let expected: bool = from_value(&case.expected);
        let actual = is_formula_token(&token);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn analyze_text_stats() {
    let c = corpus();
    for (i, case) in cases(c, "text.analysis.analyze_text_stats").iter().enumerate() {
        let text: String = from_value(&case.input["text"]);
        let e: AnalyzeStatsExpected = from_value(&case.expected);
        let stats = &analyze_text(&text).stats;
        assert_eq!(stats.word_count, e.word_count as usize, "word_count at case {i}");
        assert_eq!(stats.zh_char_count, e.zh_char_count as usize, "zh_char_count at case {i}");
        assert_eq!(stats.formula_count, e.formula_count as usize, "formula_count at case {i}");
        assert_eq!(stats.raw_math_count, e.raw_math_count as usize, "raw_math_count at case {i}");
        assert_eq!(stats.latex_command_count, e.latex_command_count as usize, "latex_command_count at case {i}");
        assert_eq!(stats.placeholder_count, e.placeholder_count as usize, "placeholder_count at case {i}");
        assert_eq!(stats.protected_formula_count, e.protected_formula_count as usize, "protected_formula_count at case {i}");
    }
}

#[test]
fn test_normalize_formula_for_latex_math() {
    let c = corpus();
    for (i, case) in cases(c, "text.latex_normalizer.normalize_formula_for_latex_math").iter().enumerate() {
        let formula: String = from_value(&case.input["formula"]);
        let expected: String = from_value(&case.expected);
        let actual = normalize_formula_for_latex_math(&formula);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}

#[test]
fn test_aggressively_simplify_formula_for_latex_math() {
    let c = corpus();
    for (i, case) in cases(c, "text.latex_normalizer.aggressively_simplify_formula_for_latex_math")
        .iter()
        .enumerate()
    {
        let formula: String = from_value(&case.input["formula"]);
        let expected: String = from_value(&case.expected);
        let actual = aggressively_simplify_formula_for_latex_math(&formula);
        assert_eq!(actual, expected, "mismatch at case {i}");
    }
}
