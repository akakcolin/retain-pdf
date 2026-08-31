// Port of services/rendering/layout/text_analysis/{models,service}.py.
// The Python LRU caches are performance-only and are not ported.

use super::tokens::{
    classify_token_value, iter_text_tokens, math_token_body as token_math_body, FORMULA_TOKEN_KINDS,
    RAW_MATH_TOKEN_KINDS, TextToken, TextTokenKind,
};
use crate::item::FormulaEntry;

#[derive(Debug, Clone, PartialEq)]
pub struct TextSegment {
    pub value: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormulaSegment {
    pub kind: TextTokenKind,
    pub value: String,
    pub body: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextAnalysisStats {
    pub word_count: usize,
    pub zh_char_count: usize,
    pub formula_count: usize,
    pub raw_math_count: usize,
    pub latex_command_count: usize,
    pub placeholder_count: usize,
    pub protected_formula_count: usize,
}

#[derive(Debug, Clone)]
pub struct AnalyzedText {
    pub source: String,
    pub tokens: Vec<TextToken>,
    pub plain_text: String,
    pub plain_segments: Vec<TextSegment>,
    pub formula_segments: Vec<FormulaSegment>,
    pub inline_math_segments: Vec<FormulaSegment>,
    pub display_math_segments: Vec<FormulaSegment>,
    pub stats: TextAnalysisStats,
    pub has_complex_inline_math: bool,
    pub formula_visible_units: f64,
}

const COMPLEX_INLINE_MATH_COMMANDS: [&str; 29] = [
    "sqrt", "frac", "dfrac", "tfrac", "cfrac", "sum", "prod", "int", "iint", "iiint", "oint", "lim",
    "left", "right", "begin", "overline", "underline", "underbrace", "overbrace", "widehat",
    "widetilde", "binom", "choose", "substack", "cases", "matrix", "pmatrix", "bmatrix", "vmatrix",
];

impl AnalyzedText {
    pub fn word_count(&self) -> usize {
        self.stats.word_count
    }
    pub fn zh_char_count(&self) -> usize {
        self.stats.zh_char_count
    }
    pub fn formula_count(&self) -> usize {
        self.stats.formula_count
    }
    pub fn raw_math_count(&self) -> usize {
        self.stats.raw_math_count
    }
    pub fn latex_command_count(&self) -> usize {
        self.stats.latex_command_count
    }
    pub fn placeholder_count(&self) -> usize {
        self.stats.placeholder_count
    }
    pub fn protected_formula_count(&self) -> usize {
        self.stats.protected_formula_count
    }
    pub fn has_display_math(&self) -> bool {
        !self.display_math_segments.is_empty()
    }
    pub fn has_inline_math(&self) -> bool {
        !self.inline_math_segments.is_empty()
    }
}

/// Count `\\[A-Za-z]+` occurrences in `source` (non-overlapping), matching
/// `LATEX_COMMAND_RE.findall`.
fn count_latex_commands(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            count += 1;
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    count
}

/// True if `formula_text` contains `\cmd` where cmd is a COMPLEX_INLINE_MATH command.
fn has_complex_inline_command(formula_text: &str) -> bool {
    let bytes = formula_text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            if COMPLEX_INLINE_MATH_COMMANDS.contains(&&formula_text[start..j]) {
                return true;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    false
}

pub fn analyze_text(text: &str) -> AnalyzedText {
    let source = text.to_string();
    let tokens = iter_text_tokens(&source);
    let plain_segments = plain_segments(&tokens);
    let formula_segments: Vec<FormulaSegment> = tokens
        .iter()
        .filter(|t| FORMULA_TOKEN_KINDS.contains(&t.kind))
        .map(|t| FormulaSegment {
            kind: t.kind,
            value: t.value.clone(),
            body: token_math_body(t),
            start: t.start,
            end: t.end,
        })
        .collect();
    let inline_math_segments: Vec<FormulaSegment> = formula_segments
        .iter()
        .filter(|s| s.kind == TextTokenKind::InlineMath)
        .cloned()
        .collect();
    let display_math_segments: Vec<FormulaSegment> = formula_segments
        .iter()
        .filter(|s| s.kind == TextTokenKind::DisplayMath)
        .cloned()
        .collect();
    let stats = TextAnalysisStats {
        word_count: tokens.iter().filter(|t| t.kind == TextTokenKind::Word).count(),
        zh_char_count: tokens.iter().filter(|t| t.kind == TextTokenKind::ZhChar).count(),
        formula_count: formula_segments.len(),
        raw_math_count: tokens
            .iter()
            .filter(|t| RAW_MATH_TOKEN_KINDS.contains(&t.kind))
            .count(),
        latex_command_count: count_latex_commands(&source),
        placeholder_count: tokens
            .iter()
            .filter(|t| t.kind == TextTokenKind::FormulaPlaceholder)
            .count(),
        protected_formula_count: tokens
            .iter()
            .filter(|t| t.kind == TextTokenKind::ProtectedFormula)
            .count(),
    };
    let has_complex_inline_math = inline_math_segments
        .iter()
        .any(|s| is_complex_inline_math(&s.body));
    let formula_visible_units = formula_segments
        .iter()
        .map(|s| formula_visible_units(&s.body))
        .sum();
    AnalyzedText {
        source,
        plain_text: plain_segments.iter().map(|s| s.value.clone()).collect(),
        plain_segments,
        formula_segments,
        inline_math_segments,
        display_math_segments,
        tokens,
        stats,
        has_complex_inline_math,
        formula_visible_units,
    }
}

pub fn analyze_render_item_text(render_protected: &str, source: &str) -> (AnalyzedText, AnalyzedText) {
    (analyze_text(render_protected), analyze_text(source))
}

pub fn tokenize_text(text: &str) -> Vec<String> {
    analyze_text(text).tokens.into_iter().map(|t| t.value).collect()
}

pub fn strip_formula_tokens(text: &str, replacement: &str) -> String {
    let mut out = String::new();
    for token in analyze_text(text).tokens {
        if FORMULA_TOKEN_KINDS.contains(&token.kind) {
            out.push_str(replacement);
        } else {
            out.push_str(&token.value);
        }
    }
    out
}

pub fn is_formula_token(token: &str) -> bool {
    FORMULA_TOKEN_KINDS.contains(&classify_token_value(token))
}

pub fn math_token_body(token: &TextToken) -> String {
    token_math_body(token)
}

pub fn inline_math_segments(text: &str) -> Vec<String> {
    analyze_text(text)
        .inline_math_segments
        .into_iter()
        .map(|s| s.body)
        .collect()
}

/// Mirrors `formula_texts_for_render`: formula_map entries (formula_text / latex)
/// followed by any inline/display math bodies found in `text`.
pub fn formula_texts_for_render(text: &str, formula_map: &[FormulaEntry]) -> Vec<String> {
    let mut formulas: Vec<String> = formula_map
        .iter()
        .map(|e| e.formula_text.trim().to_string())
        .collect();
    formulas.extend(
        analyze_text(text)
            .formula_segments
            .into_iter()
            .map(|s| s.body),
    );
    formulas.retain(|f| !f.is_empty());
    formulas
}

pub fn normalize_direct_typst_math_boundaries(text: &str) -> String {
    let source = text;
    if source.is_empty() {
        return String::new();
    }
    wrap_parenthesized_inline_math(source)
}

fn plain_segments(tokens: &[TextToken]) -> Vec<TextSegment> {
    let mut segments: Vec<TextSegment> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut start = 0usize;
    let mut end = 0usize;
    for token in tokens {
        if FORMULA_TOKEN_KINDS.contains(&token.kind) {
            if !current.is_empty() {
                segments.push(TextSegment {
                    value: current.concat(),
                    start,
                    end,
                });
                current.clear();
            }
            continue;
        }
        if current.is_empty() {
            start = token.start;
        }
        current.push(token.value.clone());
        end = token.end;
    }
    if !current.is_empty() {
        segments.push(TextSegment {
            value: current.concat(),
            start,
            end,
        });
    }
    segments
}

/// `max(0, len(compact) * 0.42)` where compact removes `\{`, `\}`, whitespace
/// and latex commands (`\cmd`).
pub fn formula_visible_units(formula_text: &str) -> f64 {
    let bytes = formula_text.as_bytes();
    let mut len = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
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
        len += 1;
        i += ch.len_utf8();
    }
    (len as f64) * 0.42
}

fn is_complex_inline_math(formula_text: &str) -> bool {
    let expr = formula_text;
    if has_complex_inline_command(expr) {
        return true;
    }
    let has_command = count_latex_commands(expr) > 0;
    if expr.chars().count() >= 48 && has_command {
        return true;
    }
    (expr.contains('_') || expr.contains('^')) && has_command
}

/// Mirrors `_wrap_parenthesized_inline_math`: wraps `$(...)$`-style inline math
/// that sits inside literal parens.
fn wrap_parenthesized_inline_math(source: &str) -> String {
    let mut chunks: Vec<char> = Vec::new();
    let mut cursor = 0usize;
    let tokens = analyze_text(source).tokens;
    for token in &tokens {
        if token.kind != TextTokenKind::InlineMath {
            continue;
        }
        while cursor < token.start {
            chunks.push(source[cursor..].chars().next().unwrap());
            cursor += source[cursor..].chars().next().unwrap().len_utf8();
        }
        let mut open_index = token.start;
        while open_index > 0 {
            let Some(ch) = source[..open_index].chars().next_back() else { break };
            open_index -= ch.len_utf8();
            if !ch.is_whitespace() {
                break;
            }
        }
        let mut close_index = token.end;
        while close_index < source.len() {
            let ch = source[close_index..].chars().next().unwrap();
            if !ch.is_whitespace() {
                break;
            }
            close_index += ch.len_utf8();
        }
        let open_ok = open_index < token.start
            && source[open_index..].chars().next() == Some('(');
        let close_ok = close_index < source.len()
            && source[close_index..].chars().next().unwrap() == ')';
        if open_ok && close_ok {
            let expr = token_math_body(token).trim().to_string();
            // Pop back to just after the '('.
            while !chunks.is_empty() && cursor > open_index {
                let popped = chunks.pop().unwrap();
                cursor -= popped.len_utf8();
            }
            chunks.extend(format!("$({expr})$").chars());
            cursor = close_index + 1;
            continue;
        }
        chunks.extend(token.value.chars());
        cursor = token.end;
    }
    while cursor < source.len() {
        chunks.push(source[cursor..].chars().next().unwrap());
        cursor += source[cursor..].chars().next().unwrap().len_utf8();
    }
    chunks.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_and_zh_counts() {
        let a = analyze_text("hello 中文");
        assert_eq!(a.word_count(), 1);
        assert_eq!(a.zh_char_count(), 2);
    }

    #[test]
    fn formula_segments_and_visible_units() {
        let a = analyze_text("a $x_1$ and $$y$$");
        assert_eq!(a.raw_math_count(), 2);
        assert_eq!(a.formula_count(), 2);
        assert_eq!(a.inline_math_segments.len(), 1);
        assert_eq!(a.display_math_segments.len(), 1);
        assert!(a.formula_visible_units > 0.0);
        assert_eq!(formula_visible_units(r"\frac{ab}{c}"), 3.0 * 0.42);
    }

    #[test]
    fn latex_command_count() {
        assert_eq!(count_latex_commands(r"\frac a \mathbf{R}"), 2);
        assert_eq!(count_latex_commands(r"\\frac"), 1);
    }

    #[test]
    fn complex_inline_math_detection() {
        assert!(is_complex_inline_math(r"\frac{\partial E}{\partial R}"));
        assert!(!is_complex_inline_math(r"x_1"));
        assert!(is_complex_inline_math(r"\delta\mathbf{R}^{IJ}"));
    }

    #[test]
    fn wrap_parenthesized_math() {
        let out = normalize_direct_typst_math_boundaries("( $g-h$ )");
        assert_eq!(out, "$(g-h)$");
    }

    #[test]
    fn wrap_parenthesized_math_cjk_before_paren_no_panic() {
        // Multi-byte CJK char immediately before the token used to panic
        // on a non-char-boundary byte slice (token.start - 1).
        let out = normalize_direct_typst_math_boundaries("应$x_1$");
        assert_eq!(out, "应$x_1$");
        let out = normalize_direct_typst_math_boundaries("应( $x_1$ )");
        assert_eq!(out, "应$(x_1)$");
    }

    #[test]
    fn plain_segments_skip_formulas() {
        let a = analyze_text("ab __FORMULA_1__ cd");
        assert_eq!(a.plain_text, "ab  cd");
        assert_eq!(a.placeholder_count(), 1);
    }
}
