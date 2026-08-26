//! Port of the parts of `text_analysis/service.py` the Typst emitter needs:
//! the analyzed-text view (tokens + formula segments), `formula_texts_for_render`,
//! and `normalize_direct_typst_math_boundaries`.

use crate::dto::MathMapEntry;
use crate::text_tokens::{
    is_formula_kind, iter_text_tokens, math_token_body, TextToken, TextTokenKind,
};

#[derive(Debug, Clone, PartialEq)]
pub struct FormulaSegment {
    pub kind: TextTokenKind,
    pub value: String,
    pub body: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzedText {
    pub tokens: Vec<TextToken>,
    pub formula_segments: Vec<FormulaSegment>,
}

pub fn analyze_text(text: &str) -> AnalyzedText {
    let tokens = iter_text_tokens(text);
    let formula_segments = tokens
        .iter()
        .filter(|t| is_formula_kind(t.kind))
        .map(|t| FormulaSegment {
            kind: t.kind,
            value: t.value.clone(),
            body: math_token_body(&t.value, t.kind),
            start: t.start,
            end: t.end,
        })
        .collect();
    AnalyzedText {
        tokens,
        formula_segments,
    }
}

/// Port of `formula_texts_for_render`: math_map entries first, then any
/// `$…$`/`$$…$$` formulas found in the text itself.
pub fn formula_texts_for_render(text: &str, formula_map: &[MathMapEntry]) -> Vec<String> {
    let mut formulas: Vec<String> = formula_map
        .iter()
        .map(|entry| {
            let raw = if !entry.formula_text.is_empty() {
                &entry.formula_text
            } else {
                &entry.latex
            };
            raw.trim().to_string()
        })
        .collect();
    formulas.extend(
        analyze_text(text)
            .formula_segments
            .into_iter()
            .map(|segment| segment.body),
    );
    formulas.retain(|f| !f.is_empty());
    formulas
}

pub fn normalize_direct_typst_math_boundaries(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    _wrap_parenthesized_inline_math(text)
}

/// Port of `_wrap_parenthesized_inline_math`: `($x$)` becomes `$(x)$`.
fn _wrap_parenthesized_inline_math(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let tokens = analyze_text(source).tokens;
    let mut chunks: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    for token in &tokens {
        if token.kind != TextTokenKind::InlineMath {
            continue;
        }
        while cursor < token.start {
            chunks.push(chars[cursor].to_string());
            cursor += 1;
        }
        let mut open_index = token.start as isize - 1;
        while open_index >= 0 && chars[open_index as usize].is_whitespace() {
            open_index -= 1;
        }
        let mut close_index = token.end;
        while close_index < chars.len() && chars[close_index].is_whitespace() {
            close_index += 1;
        }
        if open_index >= 0
            && close_index < chars.len()
            && chars[open_index as usize] == '('
            && chars[close_index] == ')'
        {
            let expr = math_token_body(&token.value, token.kind);
            while chunks.len() > 0 && cursor > open_index as usize {
                chunks.pop();
                cursor -= 1;
            }
            chunks.push(format!("$({expr})$"));
            cursor = close_index + 1;
            continue;
        }
        chunks.push(token.value.clone());
        cursor = token.end;
    }
    chunks.push(chars[cursor..].iter().collect());
    chunks.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_parenthesized_inline_math() {
        assert_eq!(
            normalize_direct_typst_math_boundaries("($x$)"),
            "$(x)$"
        );
        assert_eq!(
            normalize_direct_typst_math_boundaries("a ($x$) b"),
            "a $(x)$ b"
        );
        assert_eq!(
            normalize_direct_typst_math_boundaries("a $x$ b"),
            "a $x$ b"
        );
    }

    #[test]
    fn formula_texts_from_map_and_text() {
        let map = vec![
            MathMapEntry {
                formula_text: "F".to_string(),
                latex: String::new(),
            },
            MathMapEntry {
                formula_text: String::new(),
                latex: "L".to_string(),
            },
        ];
        assert_eq!(
            formula_texts_for_render("see $a+b$ $$c$$", &map),
            vec!["F", "L", "a+b", "c"]
        );
    }
}
