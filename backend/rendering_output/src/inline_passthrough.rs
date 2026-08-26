//! Port of `inline_content/core/markdown.py` + `inline_content/core/inline_math.py`
//! for the direct-Typst passthrough pipeline used by `build_typst_block` and the
//! preserved-line / TOC builders.

use crate::latex_normalizer::normalize_formula_for_latex_math;
use crate::py_re;
use crate::pyre::{py_captures, py_find_iter};
use crate::text_analysis::{analyze_text, normalize_direct_typst_math_boundaries};
use crate::text_tokens::{is_raw_math_kind, math_token_body, replace_non_formula_segments, TextTokenKind};

/// `escape_markdown_literal_asterisks`.
pub fn escape_markdown_literal_asterisks(text: &str) -> String {
    text.replace('*', r"\*")
}

/// `escape_literal_asterisks_preserving_emphasis`: escape literal `*` outside
/// of matched emphasis spans (`MARKDOWN_EMPHASIS_RE`, with the `(?P=marker)`
/// backreference).
pub fn escape_literal_asterisks_preserving_emphasis(text: &str) -> String {
    let source = text;
    if !source.contains('*') {
        return source.to_string();
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut last_end = 0usize;
    let re = py_re!(r"(?<![\\*])(?P<marker>\*\*|\*)(?=\S)(?P<body>[^*\n]*?\S)(?P=marker)(?!\*)");
    for m in py_find_iter(re, source) {
        chunks.push(escape_markdown_literal_asterisks(&source[last_end..m.start()]));
        chunks.push(m.as_str().to_string());
        last_end = m.end();
    }
    chunks.push(escape_markdown_literal_asterisks(&source[last_end..]));
    chunks.concat()
}

/// `surround_inline_math_with_spaces`.
pub fn surround_inline_math_with_spaces(markdown: &str) -> String {
    let text = markdown;
    if text.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let left_no_space: Vec<char> = "([{\"'“‘（【「『".chars().collect();
    let right_no_space: Vec<char> = ".,;:!?)]}，。！？；：、（）【】「」『』".chars().collect();
    let mut chunks: Vec<String> = Vec::new();
    for token in analyze_text(text).tokens {
        if !is_raw_math_kind(token.kind) {
            chunks.push(token.value);
            continue;
        }
        let prev_char = if token.start > 0 { chars[token.start - 1] } else { '\0' };
        let next_char = if token.end < chars.len() { chars[token.end] } else { '\0' };
        let mut prefix = "";
        let mut suffix = "";
        if prev_char != '\0' && !prev_char.is_whitespace() && !left_no_space.contains(&prev_char) {
            prefix = " ";
        }
        if next_char != '\0' && !next_char.is_whitespace() && !right_no_space.contains(&next_char) {
            suffix = " ";
        }
        chunks.push(format!("{prefix}{}{suffix}", token.value));
    }
    let collapsed = py_re!(r"[ \t]{2,}").replace_all(&chunks.concat(), " ").into_owned();
    collapsed.trim().to_string()
}

/// `normalize_direct_typst_inline_math_whitespace`.
pub fn normalize_direct_typst_inline_math_whitespace(text: &str) -> String {
    let source: Vec<char> = text.chars().collect();
    if source.is_empty() {
        return String::new();
    }
    let mut chunks: Vec<char> = Vec::new();
    let mut index = 0usize;
    let mut in_inline_math = false;
    while index < source.len() {
        let c = source[index];
        let prev_char = if index > 0 { source[index - 1] } else { '\0' };
        let next_char = if index + 1 < source.len() { source[index + 1] } else { '\0' };
        if c == '$' && prev_char != '\\' {
            if next_char == '$' {
                chunks.push('$');
                chunks.push('$');
                index += 2;
                continue;
            }
            in_inline_math = !in_inline_math;
            chunks.push(c);
            index += 1;
            continue;
        }
        if in_inline_math && (c == '\r' || c == '\n') {
            if chunks.last() != Some(&' ') {
                chunks.push(' ');
            }
            index += 1;
            while index < source.len() && matches!(source[index], '\r' | '\n' | '\t' | ' ') {
                index += 1;
            }
            continue;
        }
        chunks.push(c);
        index += 1;
    }
    chunks.into_iter().collect()
}

/// `normalize_angle_bracket_expectation_for_mitex`.
pub fn normalize_angle_bracket_expectation_for_mitex(expr: &str) -> String {
    let normalized = py_re!(
        r"\\langle\s*(?P<body>[^$]+?)\s*\\rangle(?P<script>_\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\})"
    )
    .replace_all(expr, |caps: &fancy_regex::Captures<'_, str>| {
        let body = caps.name("body").unwrap().as_str().trim();
        let script = caps.name("script").unwrap().as_str();
        format!("⟨{body}⟩{script}")
    })
    .into_owned();
    py_re!(r"\\langle\s*(?P<body>[^$]+?)\s*\\rangle")
        .replace_all(&normalized, |caps: &fancy_regex::Captures<'_, str>| {
            let body = caps.name("body").unwrap().as_str().trim();
            format!("⟨{body}⟩")
        })
        .into_owned()
}

/// `normalize_sized_delimiters_for_mitex`.
pub fn normalize_sized_delimiters_for_mitex(expr: &str) -> String {
    py_re!(r"\\(?:left|right)\s*(?P<delimiter>\\langle|\\rangle|[⟨⟩|()\[\]{}.]|\\[{}])")
        .replace_all(expr, |caps: &fancy_regex::Captures<'_, str>| {
            let delimiter = caps.name("delimiter").unwrap().as_str();
            match delimiter {
                r"\langle" | "⟨" => "⟨".to_string(),
                r"\rangle" | "⟩" => "⟩".to_string(),
                "." => String::new(),
                r"\{" => "{".to_string(),
                r"\}" => "}".to_string(),
                _ => delimiter.to_string(),
            }
        })
        .into_owned()
}

/// `sanitize_direct_typst_inline_math`.
pub fn sanitize_direct_typst_inline_math(text: &str) -> String {
    let mut chunks: Vec<String> = Vec::new();
    for token in analyze_text(text).tokens {
        if is_raw_math_kind(token.kind) {
            chunks.push(sanitize_token(&token));
        } else {
            chunks.push(token.value);
        }
    }
    chunks.concat()
}

fn sanitize_token(token: &crate::text_tokens::TextToken) -> String {
    let is_display = token.kind == TextTokenKind::DisplayMath;
    let expr = math_token_body(&token.value, token.kind);
    if expr.is_empty() {
        return token.value.clone();
    }
    if matches!(expr.as_str(), "^®" | "^{®}" | r"^\circled{R}" | r"^\textcircled{R}") {
        return "®".to_string();
    }
    if let Some(caps) = py_captures(py_re!(r"\\([A-Za-z]{1,3})\\([0-9]{1,7})"), &expr) {
        if caps.get(0).unwrap().start() == 0 && caps.get(0).unwrap().end() == expr.len()
        {
            let a = caps.get(1).unwrap().as_str();
            let b = caps.get(2).unwrap().as_str();
            return format!("{a}{b}");
        }
    }
    let mut expr = py_re!(r"\\{2,}(?=[A-Za-z])").replace_all(&expr, r"\\").into_owned();
    expr = py_re!(r"\\langlen\b").replace_all(&expr, r"\\langle n").into_owned();
    expr = py_re!(r"\\angle(?=[A-Za-z])").replace_all(&expr, r"\\angle ").into_owned();
    expr = py_re!(r"\\mathscr\b").replace_all(&expr, r"\\mathcal").into_owned();
    expr = py_re!(r"\\varPhi(?=[^A-Za-z]|$)").replace_all(&expr, r"\\Phi").into_owned();
    expr = py_re!(r"\\hbar\b").replace_all(&expr, "ℏ").into_owned();
    expr = py_re!(r"\\partial\b").replace_all(&expr, "∂").into_owned();
    expr = py_re!(r"\\otimes\b").replace_all(&expr, "⊗").into_owned();
    expr = normalize_sized_delimiters_for_mitex(&expr);
    expr = normalize_angle_bracket_expectation_for_mitex(&expr);
    expr = py_re!(r"\\langle\b").replace_all(&expr, "⟨").into_owned();
    expr = py_re!(r"\\rangle\b").replace_all(&expr, "⟩").into_owned();
    expr = normalize_sized_delimiters_for_mitex(&expr);
    expr = py_re!(r"\\circled\s*\{\s*\\times\s*\}")
        .replace_all(&expr, r"\\otimes")
        .into_owned();
    expr = py_re!(r"\\circled\s*\{\s*\\parallel\s*\}")
        .replace_all(&expr, r"\\circ")
        .into_owned();
    expr = py_re!(r"\\circled\s*\{\s*([^{}]+?)\s*\}")
        .replace_all(&expr, "$1")
        .into_owned();
    if is_display {
        expr = normalize_formula_for_latex_math(&expr);
    }
    format!("${expr}$")
}

/// `build_direct_typst_passthrough_markdown`.
pub fn build_direct_typst_passthrough_markdown(text: &str) -> String {
    let normalized = normalize_direct_typst_math_boundaries(text.trim());
    let normalized = normalize_direct_typst_inline_math_whitespace(&normalized);
    let markdown = replace_non_formula_segments(&normalized, escape_literal_asterisks_preserving_emphasis);
    let markdown = sanitize_direct_typst_inline_math(&markdown);
    surround_inline_math_with_spaces(&markdown)
}

/// `build_direct_typst_passthrough_text`.
pub fn build_direct_typst_passthrough_text(text: &str) -> String {
    build_direct_typst_passthrough_markdown(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_literal_asterisks_only() {
        assert_eq!(escape_literal_asterisks_preserving_emphasis("a*b"), r"a\*b");
        // Emphasis span preserved.
        assert_eq!(
            escape_literal_asterisks_preserving_emphasis("a *bold* b"),
            "a *bold* b"
        );
    }

    #[test]
    fn surrounds_math_with_spaces() {
        assert_eq!(surround_inline_math_with_spaces("a$x$b"), "a $x$ b");
        // No space before opening paren.
        assert_eq!(surround_inline_math_with_spaces("($x$)"), "($x$)");
    }

    #[test]
    fn sanitizes_sized_delimiters() {
        assert_eq!(normalize_sized_delimiters_for_mitex(r"\left\langle x \right\rangle"), "⟨ x ⟩");
    }

    #[test]
    fn angle_expectation_uses_unicode() {
        assert_eq!(
            normalize_angle_bracket_expectation_for_mitex(r"\langle x \rangle"),
            "⟨x⟩"
        );
        assert_eq!(
            normalize_angle_bracket_expectation_for_mitex(r"\langle n \rangle_n"),
            "⟨n⟩_n"
        );
    }

    #[test]
    fn passthrough_normalizes_inline_math() {
        assert_eq!(build_direct_typst_passthrough_markdown("a $x$ b"), "a $x$ b");
    }

    #[test]
    fn passthrough_handles_spreadsheet_cell() {
        // Python collapses `\AB\12` (spreadsheet cell) to plain "AB12".
        assert_eq!(build_direct_typst_passthrough_markdown(r"$\AB\12$"), "AB12");
    }
}
