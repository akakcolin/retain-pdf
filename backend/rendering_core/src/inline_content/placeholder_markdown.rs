// Port of services/rendering/layout/inline_content/fallback/placeholder_markdown.py.

use std::collections::HashMap;

use crate::item::FormulaEntry;
use crate::text::latex_normalizer::normalize_formula_for_latex_math;

use super::markdown::build_markdown_from_direct_text;
use super::protected_tokens::re_protect_restored_formulas;

/// `formula_map_lookup`.
pub fn formula_map_lookup(formula_map: &[FormulaEntry]) -> HashMap<String, String> {
    formula_map
        .iter()
        .map(|e| (e.placeholder.clone(), e.formula_text.clone()))
        .collect()
}

/// Match a render protected token at byte `index`: `<[futnvc]\d+-[0-9a-z]{3}/>`
/// or `[[FORMULA_\d+]]`. Returns byte length.
fn match_render_protected_token(text: &str, index: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if index + 7 <= bytes.len()
        && bytes[index] == b'<'
        && matches!(bytes[index + 1], b'f' | b'u' | b't' | b'n' | b'v' | b'c')
    {
        let mut j = index + 2;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > index + 2 && j + 5 <= bytes.len() && bytes[j] == b'-' {
            let suffix_ok = bytes[j + 1..j + 4].iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit());
            if suffix_ok && text[j + 4..].starts_with("/>") {
                return Some(j + 6 - index);
            }
        }
    }
    if bytes[index] == b'[' && text[index..].starts_with("[[FORMULA_") {
        let mut j = index + 10;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > index + 10 && text[j..].starts_with("]]") {
            return Some(j + 2 - index);
        }
    }
    None
}

/// `split_protected_text`: token_re split keeping both text parts and tokens.
pub fn split_protected_text(protected_text: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let bytes = protected_text.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(len) = match_render_protected_token(protected_text, i) {
            if i > start {
                parts.push(protected_text[start..i].to_string());
            }
            parts.push(protected_text[i..i + len].to_string());
            start = i + len;
            i = start;
            continue;
        }
        i += protected_text[i..].chars().next().unwrap().len_utf8();
    }
    if start < bytes.len() {
        parts.push(protected_text[start..].to_string());
    }
    parts
}

/// `re.sub(r"[ \t\r\f\v]+", " ", line)` — collapse horizontal whitespace runs.
fn collapse_hspace(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pending_space = false;
    for ch in line.chars() {
        if matches!(ch, ' ' | '\t' | '\r' | '\x0c' | '\x0b') {
            pending_space = true;
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }
    if pending_space {
        out.push(' ');
    }
    out
}

/// `build_markdown_from_parts`.
pub fn build_markdown_from_parts(protected: &str, formula_map: &[FormulaEntry]) -> String {
    if formula_map.is_empty() {
        return build_markdown_from_direct_text(protected, false);
    }
    let protected = re_protect_restored_formulas(protected, formula_map);
    let parts = split_protected_text(&protected);
    let formula_lookup = formula_map_lookup(formula_map);
    let mut chunks: Vec<String> = Vec::new();
    for part in &parts {
        if let Some(formula_text) = formula_lookup.get(part) {
            let normalized = normalize_formula_for_latex_math(formula_text);
            chunks.push(format!("${normalized}$"));
        } else {
            let mut lines: Vec<String> = Vec::new();
            for raw_line in part.trim().split('\n') {
                let collapsed = collapse_hspace(raw_line);
                let stripped = collapsed.trim();
                if !stripped.is_empty() {
                    lines.push(stripped.to_string());
                }
            }
            let text = lines.join("\n");
            if !text.is_empty() {
                chunks.push(text);
            }
        }
    }
    let markdown = chunks.concat().trim().to_string();
    build_markdown_from_direct_text(&markdown, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::FormulaEntry;

    fn entry(placeholder: &str, formula_text: &str) -> FormulaEntry {
        FormulaEntry {
            placeholder: placeholder.to_string(),
            formula_text: formula_text.to_string(),
        }
    }

    #[test]
    fn splits_protected_text_keeping_tokens() {
        let parts = split_protected_text("a <f1-abc/> b [[FORMULA_2]]");
        assert_eq!(parts, vec!["a ", "<f1-abc/>", " b ", "[[FORMULA_2]]"]);
    }

    #[test]
    fn builds_markdown_from_parts_with_formula_map() {
        let map = vec![entry("<f1-abc/>", "x+y")];
        let out = build_markdown_from_parts("a <f1-abc/> b", &map);
        assert!(out.contains("$x + y$"));
        assert!(!out.contains("<f1-abc/>"));
    }

    #[test]
    fn builds_markdown_from_parts_without_formula_map() {
        let out = build_markdown_from_parts("hello   world", &[]);
        assert_eq!(out, "hello world");
    }
}
