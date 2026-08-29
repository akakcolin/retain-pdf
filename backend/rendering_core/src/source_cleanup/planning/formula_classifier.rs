//! Port of `planning/formula_classifier.py` — textual vs math formula by
//! presence of latin words in the source text.

use serde_json::Value;

use crate::source_cleanup::planning::policy::item_has_formula_region;

/// `formula_text_has_latin_words` — TeX commands stripped, then a 3+ letter
/// latin word is required.
pub fn formula_text_has_latin_words(item: &Value) -> bool {
    let text = formula_source_text(item);
    if text.is_empty() {
        return false;
    }
    let normalized = strip_tex_commands(&text);
    has_latin_word(&normalized)
}

/// `formula_source_text` — first non-empty source-text field.
pub fn formula_source_text(item: &Value) -> String {
    crate::source_cleanup::planning::item_first_str(
        item,
        &[
            "source_text",
            "protected_source_text",
            "translation_unit_protected_source_text",
        ],
    )
}

fn strip_tex_commands(text: &str) -> String {
    // `\\[A-Za-z]+` replaced with a single space (Python `re.sub`).
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            let mut end = index + 1;
            while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                end += 1;
            }
            if end > index + 1 {
                out.push(' ');
                index = end;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

fn has_latin_word(text: &str) -> bool {
    // `[A-Za-z]{3,}` — any run of 3+ latin letters.
    let mut run = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            run += 1;
            if run >= 3 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// `first_formula_cleanup_class` — textual_formula (strips) else math_formula
/// (protects) when a formula region is present.
pub fn first_formula_cleanup_class(item: &Value) -> Option<&'static str> {
    if !item_has_formula_region(item) {
        return None;
    }
    if formula_text_has_latin_words(item) {
        Some("textual_formula")
    } else {
        Some("math_formula")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tex_commands_stripped() {
        assert!(formula_text_has_latin_words(&serde_json::json!({"source_text": "\\sum_{i=1}^n alpha"})));
        assert!(!formula_text_has_latin_words(&serde_json::json!({"source_text": "\\sum_{i=1}^{n} x_2"})));
    }
}
