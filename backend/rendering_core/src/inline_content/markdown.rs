// Port of services/rendering/layout/inline_content/core/markdown.py.

use std::collections::HashMap;

use serde_json::Value;

use crate::item::FormulaEntry;
use crate::text::analysis::{analyze_text, FormulaSegment};
use crate::text::latex_normalizer::normalize_formula_for_latex_math;
use crate::text::tokens::replace_non_formula_segments;

use super::inline_math::{
    demote_text_heavy_inline_math, escape_markdown_literal_asterisks,
    surround_inline_math_with_spaces,
};

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

/// Skip a run of Python-regex `\s` whitespace starting at byte `i`.
fn skip_ws(s: &str, mut i: usize) -> usize {
    while i < s.len() {
        let ch = s[i..].chars().next().unwrap();
        if !ch.is_whitespace() {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

/// `_normalize_text_chunk`.
fn normalize_text_chunk(text: &str) -> String {
    let mut normalized_lines: Vec<String> = Vec::new();
    for line in text.trim().split('\n') {
        let collapsed = collapse_hspace(line);
        let stripped = collapsed.trim();
        if !stripped.is_empty() {
            normalized_lines.push(stripped.to_string());
        }
    }
    normalized_lines.join("\n")
}

/// `_sanitize_existing_inline_math_for_markdown`.
fn sanitize_existing_inline_math_for_markdown(text: &str) -> String {
    let mut chunks: Vec<String> = Vec::new();
    let analysis = analyze_text(text);
    let inline_by_start: HashMap<usize, &FormulaSegment> =
        analysis.inline_math_segments.iter().map(|s| (s.start, s)).collect();
    for token in &analysis.tokens {
        let segment = inline_by_start.get(&token.start).copied();
        match segment {
            None => chunks.push(token.value.clone()),
            Some(seg) => {
                let expr = seg.body.clone();
                if expr.is_empty() {
                    chunks.push(token.value.clone());
                } else {
                    let normalized = normalize_formula_for_latex_math(&expr);
                    chunks.push(format!("${normalized}$"));
                }
            }
        }
    }
    chunks.concat()
}

/// `\textcircled\s*\{\s*\scriptsize\s*\{\s*\parallel\s*\}\s*\}` → `$\circ$`.
fn sub_textcircled_scriptsize_parallel(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < s.len() {
        if s[i..].starts_with(r"\textcircled") {
            let mut j = skip_ws(s, i + r"\textcircled".len());
            if j < s.len() && s[j..].starts_with('{') {
                j = skip_ws(s, j + 1);
                if s[j..].starts_with(r"\scriptsize") {
                    let k = skip_ws(s, j + r"\scriptsize".len());
                    if k < s.len() && s[k..].starts_with('{') {
                        let m = skip_ws(s, k + 1);
                        if s[m..].starts_with(r"\parallel") {
                            let p = skip_ws(s, m + r"\parallel".len());
                            if p < s.len() && s[p..].starts_with('}') {
                                let q = skip_ws(s, p + 1);
                                if q < s.len() && s[q..].starts_with('}') {
                                    out.push_str(r"$\circ$");
                                    i = q + 1;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `\textcircled\s*\{\s*\<inner>\s*\}` → replacement (single-level).
fn sub_textcircled(s: &str, inner: &str, replacement: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < s.len() {
        if s[i..].starts_with(r"\textcircled") {
            let mut j = skip_ws(s, i + r"\textcircled".len());
            if j < s.len() && s[j..].starts_with('{') {
                j = skip_ws(s, j + 1);
                if s[j..].starts_with(inner) {
                    let k = skip_ws(s, j + inner.len());
                    if k < s.len() && s[k..].starts_with('}') {
                        out.push_str(replacement);
                        i = k + 1;
                        continue;
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `build_markdown_from_direct_text`.
pub fn build_markdown_from_direct_text(text: &str, normalize_existing_inline_math: bool) -> String {
    let mut markdown = normalize_text_chunk(text);
    markdown = demote_text_heavy_inline_math(&markdown);
    markdown = replace_non_formula_segments(&markdown, &escape_markdown_literal_asterisks);
    if normalize_existing_inline_math {
        markdown = sanitize_existing_inline_math_for_markdown(&markdown);
    }
    markdown = surround_inline_math_with_spaces(&markdown);
    markdown = sub_textcircled_scriptsize_parallel(&markdown);
    markdown = sub_textcircled(&markdown, r"\parallel", r"$\circ$");
    markdown = sub_textcircled(&markdown, r"\times", r"$\otimes$");
    markdown
}

/// `build_direct_typst_passthrough_text`.
pub fn build_direct_typst_passthrough_text(text: &str) -> String {
    super::inline_math::build_direct_typst_passthrough_markdown(text)
}

/// `build_plain_text_from_text` — reused from `crate::payload::text_common`
/// (identical split-on-`\n` + horizontal-whitespace collapse).
pub use crate::payload::text_common::build_plain_text_from_text;

fn formula_map_from_value(item: &Value) -> Vec<FormulaEntry> {
    crate::item::formula_map(item.get("formula_map"))
}

/// `build_markdown_paragraph`.
pub fn build_markdown_paragraph(item: &Value) -> String {
    let protected = item
        .get("protected_translated_text")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| item.get("protected_source_text").and_then(|v| v.as_str()))
        .unwrap_or("");
    let formula_map = formula_map_from_value(item);
    super::build_item_render_markdown(item, protected, &formula_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_text_chunk() {
        assert_eq!(normalize_text_chunk("  a \t b \n\n c  "), "a b\nc");
        assert_eq!(normalize_text_chunk(""), "");
    }

    #[test]
    fn builds_plain_text() {
        assert_eq!(build_plain_text_from_text("  a\tb \n\n c "), "a b\nc");
    }

    #[test]
    fn direct_markdown_escapes_asterisks() {
        let out = build_markdown_from_direct_text("*not emphasis*", false);
        assert!(out.contains(r"\*not emphasis\*"));
    }

    #[test]
    fn textcircled_substitutions() {
        let out = build_markdown_from_direct_text(r"a \textcircled{\times} b", false);
        assert!(out.contains(r"a $\otimes$ b"));
        let out = build_markdown_from_direct_text(r"a \textcircled{\scriptsize{\parallel}} b", false);
        assert!(out.contains(r"a $\circ$ b"));
    }

    #[test]
    fn markdown_paragraph_reads_item() {
        let item = json!({
            "protected_translated_text": "你好 $x$",
            "formula_map": [],
            "math_mode": "placeholder",
        });
        let out = build_markdown_paragraph(&item);
        assert!(out.contains("你好"));
    }
}
