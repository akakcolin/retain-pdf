// Port of services/rendering/layout/chinese_body_fit.py.

use crate::item::FormulaEntry;
use crate::text::analysis::analyze_text;
use crate::text::tokens::TextTokenKind;
use crate::util::py_round;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct ChineseBodyFitConfig {
    pub chinese_char_width_em: f64,
    pub ascii_char_width_em: f64,
    pub punctuation_width_em: f64,
    pub space_width_em: f64,
    pub formula_base_width_em: f64,
    pub formula_char_width_em: f64,
    pub formula_max_width_em: f64,
    pub formula_height_scale: f64,
    pub line_width_safety: f64,
    pub line_height_floor_em: f64,
    pub min_font_size_pt: f64,
    pub search_precision_pt: f64,
}

impl Default for ChineseBodyFitConfig {
    fn default() -> Self {
        ChineseBodyFitConfig {
            chinese_char_width_em: 1.0,
            ascii_char_width_em: 0.55,
            punctuation_width_em: 0.62,
            space_width_em: 0.32,
            formula_base_width_em: 0.65,
            formula_char_width_em: 0.24,
            formula_max_width_em: 4.8,
            formula_height_scale: 1.04,
            line_width_safety: 0.98,
            line_height_floor_em: 1.02,
            min_font_size_pt: 7.8,
            search_precision_pt: 0.04,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChineseBodyFitResult {
    pub font_size_pt: f64,
    pub estimated_height_pt: f64,
    pub line_count: i64,
    pub overflow_ratio: f64,
    pub formula_ratio: f64,
    pub confidence: f64,
    pub max_safe_shrink_pt: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChineseBodyToken {
    pub text: String,
    pub units: f64,
    pub formula: bool,
}

fn formula_lookup(formula_map: Option<&[FormulaEntry]>) -> HashMap<String, String> {
    let mut lookup = HashMap::new();
    for entry in formula_map.into_iter().flatten() {
        if !entry.placeholder.is_empty() {
            lookup.insert(entry.placeholder.clone(), entry.formula_text.clone());
        }
    }
    lookup
}

/// `len(re.sub(r"\\[A-Za-z]+|[\s{}]", "", formula_text or ""))` — count the
/// chars that survive stripping latex commands, whitespace and braces.
fn formula_visible_len(formula_text: &str) -> usize {
    let bytes = formula_text.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            continue;
        }
        let ch = formula_text[i..].chars().next().unwrap();
        if ch == '{' || ch == '}' || ch.is_whitespace() {
            i += ch.len_utf8();
            continue;
        }
        count += 1;
        i += ch.len_utf8();
    }
    count
}

fn formula_units(formula_text: &str, config: &ChineseBodyFitConfig) -> f64 {
    let visible_len = formula_visible_len(formula_text);
    (config.formula_base_width_em + visible_len as f64 * config.formula_char_width_em)
        .min(config.formula_max_width_em)
}

fn is_zh_char(c: char) -> bool {
    ('\u{3400}'..='\u{4dbf}').contains(&c)
        || ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{f900}'..='\u{faff}').contains(&c)
}

fn is_punctuation(c: char) -> bool {
    matches!(
        c,
        '，' | '。' | '！' | '？' | '；' | '：' | '、' | ',' | '.' | '!' | '?' | ';' | ':' | '(' | ')' | '['
            | ']' | '{' | '}' | '<' | '>' | '《' | '》' | '“' | '”' | '‘' | '’' | '"' | '\''
    )
}

pub fn tokenize_chinese_body_text(
    text: &str,
    formula_map: Option<&[FormulaEntry]>,
    config: &ChineseBodyFitConfig,
) -> Vec<ChineseBodyToken> {
    let formula_lookup = formula_lookup(formula_map);
    let mut tokens: Vec<ChineseBodyToken> = Vec::new();
    for token in analyze_text(text).tokens {
        if matches!(
            token.kind,
            TextTokenKind::DisplayMath
                | TextTokenKind::InlineMath
                | TextTokenKind::ProtectedFormula
                | TextTokenKind::FormulaPlaceholder
        ) {
            let formula_text = match formula_lookup.get(&token.value) {
                Some(f) => f.clone(),
                None => token.value.trim_start_matches('$').trim_end_matches('$').to_string(),
            };
            tokens.push(ChineseBodyToken {
                text: token.value.clone(),
                units: formula_units(&formula_text, config),
                formula: true,
            });
            continue;
        }
        if token.kind == TextTokenKind::Word {
            let word = token.value.clone();
            let char_count = word.chars().count();
            tokens.push(ChineseBodyToken {
                text: word,
                units: (char_count as f64 * config.ascii_char_width_em).max(0.8),
                formula: false,
            });
            continue;
        }
        for ch in token.value.chars() {
            let (units, formula) = if ch.is_whitespace() {
                (config.space_width_em, false)
            } else if is_zh_char(ch) {
                (config.chinese_char_width_em, false)
            } else if is_punctuation(ch) {
                (config.punctuation_width_em, false)
            } else {
                (config.ascii_char_width_em, false)
            };
            tokens.push(ChineseBodyToken { text: ch.to_string(), units, formula });
        }
    }
    tokens
}

pub fn formula_unit_ratio(
    text: &str,
    formula_map: Option<&[FormulaEntry]>,
    config: &ChineseBodyFitConfig,
) -> f64 {
    let tokens = tokenize_chinese_body_text(text, formula_map, config);
    let total_units: f64 = tokens.iter().map(|t| t.units).sum();
    if total_units <= 0.0 {
        return 0.0;
    }
    let formula_units: f64 = tokens.iter().filter(|t| t.formula).map(|t| t.units).sum();
    formula_units / total_units
}

pub fn confidence_for_formula_ratio(formula_ratio: f64) -> f64 {
    if formula_ratio <= 0.15 {
        1.0
    } else if formula_ratio <= 0.35 {
        0.6
    } else {
        0.25
    }
}

pub fn max_safe_shrink_for_formula_ratio(formula_ratio: f64) -> f64 {
    if formula_ratio <= 0.15 {
        0.6
    } else if formula_ratio <= 0.35 {
        0.4
    } else {
        0.2
    }
}

pub fn estimate_chinese_body_lines(
    bbox_width_pt: f64,
    text: &str,
    formula_map: Option<&[FormulaEntry]>,
    font_size_pt: f64,
    config: &ChineseBodyFitConfig,
) -> (i64, i64) {
    if bbox_width_pt <= 0.0 || font_size_pt <= 0.0 {
        return (1, 0);
    }
    let line_capacity_units = ((bbox_width_pt * config.line_width_safety) / font_size_pt).max(1.0);
    let mut line_count = 1i64;
    let mut formula_line_count = 0i64;
    let mut current_units = 0.0;
    let mut current_has_formula = false;
    for token in tokenize_chinese_body_text(text, formula_map, config) {
        let token_units = token.units.max(0.01);
        if current_units > 0.0 && current_units + token_units > line_capacity_units {
            if current_has_formula {
                formula_line_count += 1;
            }
            line_count += 1;
            current_units = 0.0;
            current_has_formula = false;
        }
        if token_units > line_capacity_units {
            let wrapped_lines = ((token_units / line_capacity_units).ceil() as i64).max(1);
            line_count += wrapped_lines - 1;
            current_units = token_units % line_capacity_units;
        } else {
            current_units += token_units;
        }
        current_has_formula = current_has_formula || token.formula;
    }
    if current_has_formula {
        formula_line_count += 1;
    }
    (line_count.max(1), formula_line_count)
}

pub fn estimate_chinese_body_height_pt(
    bbox_width_pt: f64,
    text: &str,
    formula_map: Option<&[FormulaEntry]>,
    font_size_pt: f64,
    leading_em: f64,
    config: &ChineseBodyFitConfig,
) -> ChineseBodyFitResult {
    let formula_ratio = formula_unit_ratio(text, formula_map, config);
    let (line_count, formula_line_count) =
        estimate_chinese_body_lines(bbox_width_pt, text, formula_map, font_size_pt, config);
    let line_step =
        (font_size_pt * config.line_height_floor_em).max(font_size_pt * (1.0 + leading_em));
    let extra_formula_height =
        formula_line_count as f64 * font_size_pt * (config.formula_height_scale - 1.0).max(0.0);
    let estimated_height = line_count as f64 * line_step + extra_formula_height;
    ChineseBodyFitResult {
        font_size_pt: py_round(font_size_pt, 2),
        estimated_height_pt: py_round(estimated_height, 2),
        line_count,
        overflow_ratio: 0.0,
        formula_ratio: py_round(formula_ratio, 3),
        confidence: confidence_for_formula_ratio(formula_ratio),
        max_safe_shrink_pt: max_safe_shrink_for_formula_ratio(formula_ratio),
    }
}

pub fn solve_chinese_body_font_size_pt(
    bbox_width_pt: f64,
    bbox_height_pt: f64,
    text: &str,
    formula_map: Option<&[FormulaEntry]>,
    leading_em: f64,
    min_font_size_pt: f64,
    max_font_size_pt: f64,
    config: &ChineseBodyFitConfig,
) -> ChineseBodyFitResult {
    let mut low = config.min_font_size_pt.max(min_font_size_pt);
    let mut high = low.max(max_font_size_pt);
    let mut best = estimate_chinese_body_height_pt(bbox_width_pt, text, formula_map, low, leading_em, config);
    while high - low > config.search_precision_pt {
        let mid = (low + high) / 2.0;
        let candidate = estimate_chinese_body_height_pt(bbox_width_pt, text, formula_map, mid, leading_em, config);
        if candidate.estimated_height_pt <= bbox_height_pt {
            best = candidate;
            low = mid;
        } else {
            high = mid;
        }
    }
    let overflow_ratio = best.estimated_height_pt / bbox_height_pt.max(1.0);
    ChineseBodyFitResult {
        font_size_pt: py_round(best.font_size_pt, 2),
        estimated_height_pt: best.estimated_height_pt,
        line_count: best.line_count,
        overflow_ratio: py_round(overflow_ratio, 3),
        formula_ratio: best.formula_ratio,
        confidence: best.confidence,
        max_safe_shrink_pt: best.max_safe_shrink_pt,
    }
}
