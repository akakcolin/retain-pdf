//! Port of `planning/mixed_content.py` — unresolved embedded-formula detection
//! (display-math markers + formula-span lines).

use serde_json::Value;

pub const DISPLAY_MATH_MARKERS: [&str; 7] = [
    "$$",
    "\\[",
    "\\]",
    "\\begin{equation",
    "\\begin{align",
    "\\begin{gather",
    "\\begin{multline",
];
pub const FORMULA_SPAN_TYPES: [&str; 4] = ["formula", "math", "inline_formula", "display_formula"];

pub fn item_has_unresolved_embedded_formula(item: &Value) -> bool {
    source_text_has_display_math(&item_source_text(item)) || lines_have_formula_spans(item)
}

pub fn source_text_has_display_math(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    DISPLAY_MATH_MARKERS.iter().any(|marker| text.contains(marker))
        || text.lines().any(line_is_standalone_math)
}

pub fn line_is_standalone_math(line: &str) -> bool {
    let stripped = line.trim();
    if stripped.len() < 3 || !stripped.starts_with('$') || !stripped.ends_with('$') {
        return false;
    }
    let words = latin_words(stripped);
    words <= 2
}

pub fn lines_have_formula_spans(item: &Value) -> bool {
    let lines = crate::source_cleanup::planning::item_lines(item);
    lines.iter().filter(|line| line.is_object()).any(line_has_formula_role)
}

pub fn line_has_formula_role(line: &Value) -> bool {
    if value_is_formula_role(&role_value(line)) {
        return true;
    }
    match line.get("spans") {
        Some(Value::Array(spans)) => {
            spans.iter().filter(|span| span.is_object()).any(span_has_formula_role)
        }
        _ => false,
    }
}

pub fn span_has_formula_role(span: &Value) -> bool {
    value_is_formula_role(&role_value(span))
}

pub fn value_is_formula_role(value: &str) -> bool {
    FORMULA_SPAN_TYPES.contains(&value.trim().to_lowercase().as_str())
}

fn role_value(value: &Value) -> String {
    crate::source_cleanup::planning::item_role_value(value, &["type", "kind", "role"])
}

fn item_source_text(item: &Value) -> String {
    crate::source_cleanup::planning::item_first_str(
        item,
        &[
            "source_text",
            "protected_source_text",
            "translation_unit_protected_source_text",
        ],
    )
}

fn latin_words(text: &str) -> usize {
    let mut count = 0usize;
    let mut run = 0usize;
    let mut chars: Vec<char> = text.chars().collect();
    chars.push(' ');
    for ch in chars {
        if ch.is_ascii_alphabetic() {
            run += 1;
        } else {
            if run >= 3 {
                count += 1;
            }
            run = 0;
        }
    }
    count
}
