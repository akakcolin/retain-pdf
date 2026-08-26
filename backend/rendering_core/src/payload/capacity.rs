// Port of services/rendering/layout/payload/capacity.py. The Python LRU caches
// are performance-only and are not ported.

use crate::item::FormulaEntry;
use crate::payload::formula_cost::token_units;
use crate::payload::text_common::tokenize_protected_text;
use crate::util::py_round;
use std::collections::HashMap;

const COMPLEX_FORMULA_COMMANDS: [&str; 14] = [
    "frac", "dfrac", "tfrac", "sqrt", "sum", "prod", "int", "begin", "delta", "Delta", "partial",
    "mathbf", "overline", "underline",
];

fn has_complex_command(formula_text: &str) -> bool {
    let bytes = formula_text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            for name in COMPLEX_FORMULA_COMMANDS {
                if formula_text[i + 1..].starts_with(name) {
                    return true;
                }
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    false
}

fn formula_map_key(formula_map: &[FormulaEntry]) -> Vec<(String, String)> {
    formula_map
        .iter()
        .map(|e| (e.placeholder.clone(), e.formula_text.clone()))
        .collect()
}

fn formula_estimate_discount_cached(protected_text: &str, formula_key: &[(String, String)]) -> f64 {
    let tokens = tokenize_protected_text(protected_text);
    let formula_lookup: HashMap<String, String> =
        formula_key.iter().cloned().collect();
    let formula_tokens: Vec<&String> = tokens
        .iter()
        .filter(|token| {
            formula_lookup.contains_key(token.as_str())
                || (token.starts_with('$') && token.ends_with('$'))
        })
        .collect();
    if formula_tokens.is_empty() {
        return 1.0;
    }
    let visible_tokens: Vec<&String> = tokens.iter().filter(|t| !t.trim().is_empty()).collect();
    let formula_count = formula_tokens.len();
    let complex_count = formula_tokens
        .iter()
        .filter(|token| {
            let formula_text = formula_lookup
                .get(token.as_str())
                .cloned()
                .unwrap_or_else(|| token.trim_start_matches('$').trim_end_matches('$').to_string());
            has_complex_command(&formula_text)
        })
        .count();
    let count_ratio = formula_count as f64 / (visible_tokens.len() as f64).max(1.0);
    let uncertainty = formula_count as f64 * 0.06 + complex_count as f64 * 0.06 + count_ratio * 0.9;
    py_round(1.0 - 0.14 * (1.0 - (-1.7 * uncertainty.max(0.0)).exp()), 3)
}

fn text_demand_units_cached(protected_text: &str, formula_key: &[(String, String)]) -> f64 {
    if protected_text.is_empty() {
        return 0.0;
    }
    let formula_lookup: HashMap<String, String> = formula_key.iter().cloned().collect();
    tokenize_protected_text(protected_text)
        .iter()
        .map(|token| token_units(token, &formula_lookup))
        .sum()
}

pub fn formula_estimate_discount(protected_text: &str, formula_map: &[FormulaEntry]) -> f64 {
    formula_estimate_discount_cached(protected_text, &formula_map_key(formula_map))
}

pub fn box_capacity_units(
    inner: &[f64],
    font_size_pt: f64,
    leading_em: f64,
    visual_lines: Option<i64>,
) -> f64 {
    if inner.len() != 4 {
        return 0.0;
    }
    let width = (inner[2] - inner[0]).max(8.0);
    let height = (inner[3] - inner[1]).max(8.0);
    let line_step = (font_size_pt * 1.02).max(font_size_pt * (1.0 + leading_em));
    let mut lines = ((height / line_step) as i64).max(1);
    if let Some(v) = visual_lines {
        if v > 1 {
            lines = lines.min((v + 1).max(1));
        }
    }
    let chars_per_line = (width / (font_size_pt * 0.92).max(1.0)).max(4.0);
    lines as f64 * chars_per_line * 0.98
}

pub fn text_demand_units(protected_text: &str, formula_map: &[FormulaEntry]) -> f64 {
    text_demand_units_cached(protected_text, &formula_map_key(formula_map))
}

pub fn estimated_required_lines(
    inner: &[f64],
    protected_text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
) -> i64 {
    if inner.len() != 4 {
        return 1;
    }
    let width = (inner[2] - inner[0]).max(8.0);
    let chars_per_line = (width / (font_size_pt * 0.92).max(1.0)).max(4.0);
    let demand = text_demand_units(protected_text, formula_map);
    if demand <= 0.0 {
        return 1;
    }
    ((demand / (chars_per_line * 0.98).max(1.0)).ceil() as i64).max(1)
}

pub fn estimated_render_height_pt(
    inner: &[f64],
    protected_text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
    leading_em: f64,
) -> f64 {
    if inner.len() != 4 {
        return 0.0;
    }
    let line_step = (font_size_pt * 1.02).max(font_size_pt * (1.0 + leading_em));
    let required_lines = estimated_required_lines(inner, protected_text, formula_map, font_size_pt);
    required_lines as f64 * line_step * formula_estimate_discount(protected_text, formula_map)
}

pub fn source_layout_density_reference(
    _item: &crate::item::Item,
    _inner: &[f64],
    _font_size_pt: f64,
    _leading_em: f64,
) -> f64 {
    0.0
}
