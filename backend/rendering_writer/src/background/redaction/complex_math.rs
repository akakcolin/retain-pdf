//! Port of `layout/inline_content/complexity.py`'s `item_has_complex_inline_math`
//! — the subset of `layout/text_analysis/service.py`'s inline-math segment
//! detection (via `text_tokens.py::iter_text_tokens`) needed to decide whether a
//! redaction item carries complex inline math. Only INLINE_MATH token bodies
//! matter; the other token kinds exist only to advance the scanner the same way
//! the full tokenizer does. The `COMPLEX_INLINE_MATH_RE` command list is matched
//! manually (a `\command` followed by a non-word char).

use super::dto::RedactionItem;

/// `text_tokens.py::MAX_INLINE_MATH_CHARS`.
const MAX_INLINE_MATH_CHARS: usize = 1200;

/// `COMPLEX_INLINE_MATH_RE` alternative list, in pattern order.
const COMPLEX_INLINE_MATH_COMMANDS: &[&str] = &[
    "sqrt",
    "frac",
    "dfrac",
    "tfrac",
    "cfrac",
    "sum",
    "prod",
    "int",
    "iint",
    "iiint",
    "oint",
    "lim",
    "left",
    "right",
    "begin",
    "overline",
    "underline",
    "underbrace",
    "overbrace",
    "widehat",
    "widetilde",
    "binom",
    "choose",
    "substack",
    "cases",
    "matrix",
    "pmatrix",
    "bmatrix",
    "vmatrix",
];

fn is_ascii_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_formula_suffix_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

fn is_zh_char(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// `text_tokens.py::_is_escaped` — an odd run of backslashes before `index`.
fn is_escaped(text: &[char], index: usize) -> bool {
    let mut backslashes = 0;
    let mut cursor = index;
    while cursor > 0 && text[cursor - 1] == '\\' {
        backslashes += 1;
        cursor -= 1;
    }
    backslashes % 2 == 1
}

fn starts_with(text: &[char], start: usize, pattern: &[char]) -> bool {
    start + pattern.len() <= text.len() && text[start..].starts_with(pattern)
}

fn match_formula_placeholder(text: &[char], index: usize) -> usize {
    for (prefix, suffix) in [
        ("[[FORMULA_", "]]"),
        ("__FORMULA_", "__"),
        ("\u{27e6}FORMULA_", "\u{27e7}"),
        ("<FORMULA_", ">"),
    ] {
        let pc: Vec<char> = prefix.chars().collect();
        if !starts_with(text, index, &pc) {
            continue;
        }
        let mut cursor = index + pc.len();
        let digit_start = cursor;
        while cursor < text.len() && text[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == digit_start {
            continue;
        }
        let sc: Vec<char> = suffix.chars().collect();
        if !starts_with(text, cursor, &sc) {
            continue;
        }
        return cursor + sc.len();
    }
    index
}

fn match_protected_formula(text: &[char], index: usize) -> usize {
    if index + 7 > text.len() || text[index] != '<' {
        return index;
    }
    let mut cursor = index + 1;
    if !matches!(text.get(cursor), Some('f' | 'u' | 't' | 'n' | 'v' | 'c')) {
        return index;
    }
    cursor += 1;
    let digit_start = cursor;
    while cursor < text.len() && text[cursor].is_ascii_digit() {
        cursor += 1;
    }
    if cursor == digit_start || cursor >= text.len() || text[cursor] != '-' {
        return index;
    }
    let suffix_start = cursor + 1;
    let suffix_end = suffix_start + 3;
    if suffix_end > text.len() {
        return index;
    }
    if !text[suffix_start..suffix_end].iter().all(|&c| is_formula_suffix_char(c)) {
        return index;
    }
    if !starts_with(text, suffix_end, &['/', '>']) {
        return index;
    }
    suffix_end + 2
}

fn match_display_math(text: &[char], index: usize) -> usize {
    if !starts_with(text, index, &['$', '$']) || is_escaped(text, index) {
        return index;
    }
    let mut cursor = index + 2;
    while cursor + 1 < text.len() {
        if text[cursor] == '\\' {
            cursor += 2;
            continue;
        }
        if text[cursor] == '$' && text[cursor + 1] == '$' {
            return cursor + 2;
        }
        cursor += 1;
    }
    index
}

fn match_inline_math(text: &[char], index: usize) -> usize {
    if text[index] != '$'
        || starts_with(text, index, &['$', '$'])
        || is_escaped(text, index)
        || index + 1 >= text.len()
    {
        return index;
    }
    let mut cursor = index + 1;
    while cursor < text.len() {
        if cursor - index > MAX_INLINE_MATH_CHARS {
            return index;
        }
        let ch = text[cursor];
        if ch == '\n' {
            return index;
        }
        if ch == '\\' {
            cursor += 2;
            continue;
        }
        if ch == '$' {
            let body_has_content = text[index + 1..cursor].iter().any(|c| !c.is_whitespace());
            return if body_has_content { cursor + 1 } else { index };
        }
        cursor += 1;
    }
    index
}

fn match_whitespace(text: &[char], index: usize) -> usize {
    if !text[index].is_whitespace() {
        return index;
    }
    let mut cursor = index + 1;
    while cursor < text.len() && text[cursor].is_whitespace() {
        cursor += 1;
    }
    cursor
}

fn match_word(text: &[char], index: usize) -> usize {
    if !is_ascii_alnum(text[index]) {
        return index;
    }
    let mut cursor = index + 1;
    while cursor < text.len() && is_ascii_alnum(text[cursor]) {
        cursor += 1;
    }
    while cursor + 1 < text.len() && (text[cursor] == '-' || text[cursor] == '\'')
        && is_ascii_alnum(text[cursor + 1])
    {
        cursor += 2;
        while cursor < text.len() && is_ascii_alnum(text[cursor]) {
            cursor += 1;
        }
    }
    cursor
}

fn match_zh_char(text: &[char], index: usize) -> usize {
    if is_zh_char(text[index]) {
        index + 1
    } else {
        index
    }
}

enum TokenRule {
    FormulaPlaceholder,
    ProtectedFormula,
    DisplayMath,
    InlineMath,
    Whitespace,
    Word,
    ZhChar,
}

/// `text_analysis/service.py::inline_math_segments` — the INLINE_MATH token
/// bodies (`$...$` with the outer `$` stripped and `.strip()`ed), produced by a
/// faithful scan of the whole source with the full `iter_text_tokens` rule
/// order so a `$` that the other rules consume is never misread.
pub fn inline_math_segments(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut segments = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let rules = [
            TokenRule::FormulaPlaceholder,
            TokenRule::ProtectedFormula,
            TokenRule::DisplayMath,
            TokenRule::InlineMath,
            TokenRule::Whitespace,
            TokenRule::Word,
            TokenRule::ZhChar,
        ];
        let mut advanced = false;
        for rule in rules {
            let end = match rule {
                TokenRule::FormulaPlaceholder => match_formula_placeholder(&chars, index),
                TokenRule::ProtectedFormula => match_protected_formula(&chars, index),
                TokenRule::DisplayMath => match_display_math(&chars, index),
                TokenRule::InlineMath => match_inline_math(&chars, index),
                TokenRule::Whitespace => match_whitespace(&chars, index),
                TokenRule::Word => match_word(&chars, index),
                TokenRule::ZhChar => match_zh_char(&chars, index),
            };
            if end > index {
                if matches!(rule, TokenRule::InlineMath) {
                    // `math_token_body` for INLINE_MATH is `value[1:-1].strip()`:
                    // the match end sits after the closing `$`, so drop it.
                    let inner: String = chars[index + 1..end - 1].iter().collect();
                    segments.push(inner.trim().to_string());
                }
                index = end;
                advanced = true;
                break;
            }
        }
        if !advanced {
            // OTHER fallback: consume one char.
            index += 1;
        }
    }
    segments
}

/// `COMPLEX_INLINE_MATH_RE.search` — a `\command` where the following char is a
/// word boundary (non-`[A-Za-z0-9_]`, or end of string).
fn complex_inline_math_re_find(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            let rest: String = chars[i + 1..].iter().collect();
            for cmd in COMPLEX_INLINE_MATH_COMMANDS {
                if let Some(tail) = rest.strip_prefix(cmd) {
                    let after = tail.chars().next();
                    let boundary = match after {
                        None => true,
                        Some(c) => !(c.is_ascii_alphanumeric() || c == '_'),
                    };
                    if boundary {
                        return true;
                    }
                }
            }
        }
        i += 1;
    }
    false
}

/// `complexity.py::has_complex_inline_math_text`.
pub fn has_complex_inline_math_text(text: &str) -> bool {
    inline_math_segments(text)
        .iter()
        .any(|segment| complex_inline_math_re_find(segment))
}

/// `complexity.py::item_has_complex_inline_math` — candidate text fields in the
/// production order; any candidate with complex inline math marks the item.
pub fn item_has_complex_inline_math(item: &RedactionItem) -> bool {
    let candidates = [
        item.render_protected_text.as_str(),
        item.translation_unit_protected_translated_text.as_str(),
        item.protected_translated_text.as_str(),
        item.translated_text.as_str(),
        item.translation_unit_protected_source_text.as_str(),
        item.protected_source_text.as_str(),
        item.source_text.as_str(),
    ];
    candidates.iter().any(|c| has_complex_inline_math_text(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: &str) -> RedactionItem {
        serde_json::from_str(json).expect("parse")
    }

    #[test]
    fn inline_segments_match_python() {
        assert_eq!(inline_math_segments("a $x^2$ b"), vec!["x^2"]);
        assert_eq!(inline_math_segments("$x$"), vec!["x"]);
        assert_eq!(inline_math_segments("  $  x  $  "), vec!["x"]);
        // empty body is not a token
        assert!(inline_math_segments("$  $").is_empty());
        // display math is not inline
        assert!(inline_math_segments("$$x$$").is_empty());
        // escaped dollar does not open
        assert!(inline_math_segments(r"\$x$").is_empty());
        // newline rejects
        assert!(inline_math_segments("$a\nb$").is_empty());
    }

    #[test]
    fn complex_re_find_matches_python() {
        assert!(complex_inline_math_re_find(r"\frac{1}{2}"));
        assert!(complex_inline_math_re_find(r"x\sqrt{y}"));
        assert!(complex_inline_math_re_find(r"\begin{matrix}"));
        // word-boundary: \fracx is not the \frac command
        assert!(!complex_inline_math_re_find(r"\fracx"));
        // trailing command still matches
        assert!(complex_inline_math_re_find(r"a\left"));
        assert!(!complex_inline_math_re_find("no commands here"));
    }

    #[test]
    fn item_detects_complex_math() {
        let it = item(r#"{"translated_text":"$\\frac{a}{b}$"}"#);
        assert!(item_has_complex_inline_math(&it));
        let plain = item(r#"{"translated_text":"hello world"}"#);
        assert!(!item_has_complex_inline_math(&plain));
        let render_field = item(r#"{"render_protected_text":"$x$","translated_text":"p"}"#);
        assert!(!item_has_complex_inline_math(&render_field));
        let unit_field = item(r#"{"translation_unit_protected_source_text":"$\\sum(x)$"}"#);
        assert!(item_has_complex_inline_math(&unit_field));
    }

    #[test]
    fn empty_item_is_not_complex() {
        assert!(!item_has_complex_inline_math(&item(r#"{}"#)));
    }
}
