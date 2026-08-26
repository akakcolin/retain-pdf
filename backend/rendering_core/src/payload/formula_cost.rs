// Port of services/rendering/layout/payload/formula_cost.py.

use crate::text::latex_normalizer::aggressively_simplify_formula_for_latex_math;
use crate::text::tokens::is_formula_token;
use std::collections::HashMap;

const STYLE_ONLY_LATEX_COMMANDS: [&str; 13] = [
    "left", "right", "mathrm", "mathbf", "mathit", "mathsf", "mathtt", "text", "operatorname",
    "displaystyle", "textstyle", "scriptstyle", "scriptscriptstyle",
];

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Remove `STYLE_ONLY_LATEX_COMMAND_RE` matches (a `\` + style command at a word
/// boundary, with no trailing word char).
fn strip_style_commands(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let mut removed = false;
            for name in STYLE_ONLY_LATEX_COMMANDS {
                if expr[i + 1..].starts_with(name) {
                    let after = i + 1 + name.len();
                    let boundary = after >= bytes.len()
                        || !is_word_char(expr[after..].chars().next().unwrap());
                    if boundary {
                        i = after;
                        removed = true;
                        break;
                    }
                }
            }
            if removed {
                continue;
            }
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Replace `\cmd` (backslash + letters) with "x" (`GENERIC_LATEX_COMMAND_RE.sub`).
fn replace_generic_commands(expr: &str) -> String {
    let bytes = expr.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            out.push('x');
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            continue;
        }
        let ch = expr[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn approx_formula_visible_text(formula_text: &str) -> String {
    let expr = aggressively_simplify_formula_for_latex_math(formula_text);
    if expr.is_empty() {
        return String::new();
    }
    let expr = strip_style_commands(&expr);
    let expr = replace_generic_commands(&expr);
    let expr: String = expr.chars().filter(|c| *c != '{' && *c != '}').collect();
    let expr: String = expr.chars().filter(|c| *c != '~').collect();
    expr.chars().filter(|c| !c.is_whitespace()).collect()
}

fn is_single_zh_char(token: &str) -> bool {
    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => ('\u{4e00}'..='\u{9fff}').contains(&c),
        _ => false,
    }
}

fn is_word_token(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut i = 0usize;
    if i >= bytes.len() || !bytes[i].is_ascii_alphanumeric() {
        return false;
    }
    i += 1;
    while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
        i += 1;
    }
    while i < bytes.len() {
        if bytes[i] != b'-' && bytes[i] != b'\'' {
            return false;
        }
        if i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_alphanumeric() {
            return false;
        }
        i += 2;
        while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
            i += 1;
        }
    }
    true
}

pub fn token_units(token: &str, formula_lookup: &HashMap<String, String>) -> f64 {
    if token.is_empty() {
        return 0.0;
    }
    if token.chars().all(|c| c.is_whitespace()) {
        return (token.chars().count() as f64 * 0.25).max(0.2);
    }
    if is_formula_token(token) {
        let formula_text = match formula_lookup.get(token) {
            Some(f) => f.clone(),
            None => token.trim_start_matches('$').trim_end_matches('$').to_string(),
        };
        let mut normalized = approx_formula_visible_text(&formula_text);
        if normalized.is_empty() {
            normalized = formula_text.chars().filter(|c| !c.is_whitespace()).collect();
        }
        return (normalized.chars().count() as f64 * 0.42).max(1.35);
    }
    if is_single_zh_char(token) {
        return 1.0;
    }
    if is_word_token(token) {
        return (token.chars().count() as f64 * 0.55).max(1.0);
    }
    0.45
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_units_basics() {
        let empty = HashMap::new();
        assert_eq!(token_units("", &empty), 0.0);
        assert_eq!(token_units("   ", &empty), 0.75);
        assert_eq!(token_units("图", &empty), 1.0);
        assert!((token_units("g-h", &empty) - 1.65).abs() < 1e-9);
        assert_eq!(token_units("，", &empty), 0.45);
    }

    #[test]
    fn token_units_formula_placeholder() {
        let mut lookup = HashMap::new();
        lookup.insert("__FORMULA_1__".to_string(), "x_1".to_string());
        let units = token_units("__FORMULA_1__", &lookup);
        // approx visible text of "x_1" → "x1" → len 2 → 0.84 → floor 1.35
        assert_eq!(units, 1.35);
    }
}
