//! Port of `services/rendering/layout/text_tokens.py`: the full text tokenizer
//! that the passthrough chain and formula extraction are built on.

use crate::util::is_py_whitespace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextTokenKind {
    DisplayMath,
    InlineMath,
    ProtectedFormula,
    FormulaPlaceholder,
    Whitespace,
    Word,
    ZhChar,
    Other,
}

impl TextTokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TextTokenKind::DisplayMath => "display_math",
            TextTokenKind::InlineMath => "inline_math",
            TextTokenKind::ProtectedFormula => "protected_formula",
            TextTokenKind::FormulaPlaceholder => "formula_placeholder",
            TextTokenKind::Whitespace => "whitespace",
            TextTokenKind::Word => "word",
            TextTokenKind::ZhChar => "zh_char",
            TextTokenKind::Other => "other",
        }
    }
}

pub const FORMULA_TOKEN_KINDS: [TextTokenKind; 4] = [
    TextTokenKind::DisplayMath,
    TextTokenKind::InlineMath,
    TextTokenKind::ProtectedFormula,
    TextTokenKind::FormulaPlaceholder,
];

pub const RAW_MATH_TOKEN_KINDS: [TextTokenKind; 2] =
    [TextTokenKind::DisplayMath, TextTokenKind::InlineMath];

pub const MAX_INLINE_MATH_CHARS: usize = 1200;

pub fn is_formula_kind(kind: TextTokenKind) -> bool {
    FORMULA_TOKEN_KINDS.contains(&kind)
}

pub fn is_raw_math_kind(kind: TextTokenKind) -> bool {
    RAW_MATH_TOKEN_KINDS.contains(&kind)
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextToken {
    pub kind: TextTokenKind,
    pub value: String,
    pub start: usize,
    pub end: usize,
}

fn starts_with(chars: &[char], index: usize, prefix: &str) -> bool {
    let pchars: Vec<char> = prefix.chars().collect();
    index + pchars.len() <= chars.len() && chars[index..index + pchars.len()] == pchars[..]
}

fn _is_ascii_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn _is_formula_suffix_char(c: char) -> bool {
    ('a'..='z').contains(&c) || ('0'..='9').contains(&c)
}

fn _is_zh_char(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn _is_escaped(chars: &[char], index: usize) -> bool {
    let mut backslashes = 0usize;
    let mut cursor = index as isize - 1;
    while cursor >= 0 && chars[cursor as usize] == '\\' {
        backslashes += 1;
        cursor -= 1;
    }
    backslashes % 2 == 1
}

fn _match_formula_placeholder(chars: &[char], index: usize) -> usize {
    for (prefix, suffix) in [
        ("[[FORMULA_", "]]"),
        ("__FORMULA_", "__"),
        ("⟦FORMULA_", "⟧"),
        ("<FORMULA_", ">"),
    ] {
        if !starts_with(chars, index, prefix) {
            continue;
        }
        let mut cursor = index + prefix.chars().count();
        let digit_start = cursor;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == digit_start || !starts_with(chars, cursor, suffix) {
            continue;
        }
        return cursor + suffix.chars().count();
    }
    index
}

fn _match_protected_formula(chars: &[char], index: usize) -> usize {
    if index + 7 > chars.len() || chars[index] != '<' {
        return index;
    }
    let mut cursor = index + 1;
    if !"futnvc".contains(chars[cursor]) {
        return index;
    }
    cursor += 1;
    let digit_start = cursor;
    while cursor < chars.len() && chars[cursor].is_ascii_digit() {
        cursor += 1;
    }
    if cursor == digit_start || cursor >= chars.len() || chars[cursor] != '-' {
        return index;
    }
    let suffix_start = cursor + 1;
    let suffix_end = suffix_start + 3;
    if suffix_end > chars.len()
        || !(0..3).all(|k| _is_formula_suffix_char(chars[suffix_start + k]))
        || !starts_with(chars, suffix_end, "/>")
    {
        return index;
    }
    suffix_end + 2
}

fn _match_display_math(chars: &[char], index: usize) -> usize {
    if !starts_with(chars, index, "$$") || _is_escaped(chars, index) {
        return index;
    }
    let mut cursor = index + 2;
    while cursor + 1 < chars.len() {
        if chars[cursor] == '\\' {
            cursor += 2;
            continue;
        }
        if starts_with(chars, cursor, "$$") {
            return cursor + 2;
        }
        cursor += 1;
    }
    index
}

fn _match_inline_math(chars: &[char], index: usize) -> usize {
    if chars[index] != '$'
        || starts_with(chars, index, "$$")
        || _is_escaped(chars, index)
        || index + 1 >= chars.len()
    {
        return index;
    }
    let mut cursor = index + 1;
    while cursor < chars.len() {
        if cursor - index > MAX_INLINE_MATH_CHARS {
            return index;
        }
        let c = chars[cursor];
        if c == '\n' {
            return index;
        }
        if c == '\\' {
            cursor += 2;
            continue;
        }
        if c == '$' {
            let body: String = chars[index + 1..cursor].iter().collect();
            let body = body.trim();
            return if body.is_empty() { index } else { cursor + 1 };
        }
        cursor += 1;
    }
    index
}

fn _match_whitespace(chars: &[char], index: usize) -> usize {
    if !chars[index].is_whitespace() {
        return index;
    }
    let mut cursor = index + 1;
    while cursor < chars.len() && chars[cursor].is_whitespace() {
        cursor += 1;
    }
    cursor
}

fn _match_word(chars: &[char], index: usize) -> usize {
    if !_is_ascii_alnum(chars[index]) {
        return index;
    }
    let mut cursor = index + 1;
    while cursor < chars.len() && _is_ascii_alnum(chars[cursor]) {
        cursor += 1;
    }
    while cursor + 1 < chars.len()
        && (chars[cursor] == '-' || chars[cursor] == '\'')
        && _is_ascii_alnum(chars[cursor + 1])
    {
        cursor += 2;
        while cursor < chars.len() && _is_ascii_alnum(chars[cursor]) {
            cursor += 1;
        }
    }
    cursor
}

fn _match_zh_char(chars: &[char], index: usize) -> usize {
    if _is_zh_char(chars[index]) {
        index + 1
    } else {
        index
    }
}

type Matcher = fn(&[char], usize) -> usize;

const TOKEN_RULES: &[(TextTokenKind, Matcher)] = &[
    (TextTokenKind::FormulaPlaceholder, _match_formula_placeholder),
    (TextTokenKind::ProtectedFormula, _match_protected_formula),
    (TextTokenKind::DisplayMath, _match_display_math),
    (TextTokenKind::InlineMath, _match_inline_math),
    (TextTokenKind::Whitespace, _match_whitespace),
    (TextTokenKind::Word, _match_word),
    (TextTokenKind::ZhChar, _match_zh_char),
];

pub fn iter_text_tokens(text: &str) -> Vec<TextToken> {
    let source: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < source.len() {
        let mut matched = false;
        for (kind, matcher) in TOKEN_RULES {
            let end = matcher(&source, index);
            if end > index {
                let value: String = source[index..end].iter().collect();
                tokens.push(TextToken {
                    kind: *kind,
                    value,
                    start: index,
                    end,
                });
                index = end;
                matched = true;
                break;
            }
        }
        if !matched {
            tokens.push(TextToken {
                kind: TextTokenKind::Other,
                value: source[index].to_string(),
                start: index,
                end: index + 1,
            });
            index += 1;
        }
    }
    tokens
}

/// Port of `math_token_body`: strip the `$$…$$` / `$…$` delimiters.
pub fn math_token_body(value: &str, kind: TextTokenKind) -> String {
    let chars: Vec<char> = value.chars().collect();
    if kind == TextTokenKind::DisplayMath
        && chars.len() >= 4
        && chars[0] == '$'
        && chars[1] == '$'
        && chars[chars.len() - 2] == '$'
        && chars[chars.len() - 1] == '$'
    {
        return chars[2..chars.len() - 2].iter().collect::<String>().trim().to_string();
    }
    if kind == TextTokenKind::InlineMath
        && chars.len() >= 2
        && chars[0] == '$'
        && chars[chars.len() - 1] == '$'
    {
        return chars[1..chars.len() - 1].iter().collect::<String>().trim().to_string();
    }
    value.to_string()
}

/// Port of `replace_non_formula_segments` (tokenizer variant).
pub fn replace_non_formula_segments<F: Fn(&str) -> String>(text: &str, replacer: F) -> String {
    let mut chunks: Vec<String> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    for token in iter_text_tokens(text) {
        if is_raw_math_kind(token.kind) {
            if !plain.is_empty() {
                chunks.push(replacer(&plain.concat()));
                plain.clear();
            }
            chunks.push(token.value);
            continue;
        }
        plain.push(token.value);
    }
    if !plain.is_empty() {
        chunks.push(replacer(&plain.concat()));
    }
    chunks.concat()
}

pub fn is_py_ws(c: char) -> bool {
    is_py_whitespace(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<TextTokenKind> {
        iter_text_tokens(text).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn inline_math_detection() {
        assert_eq!(kinds("a $x$ b"), vec![
            TextTokenKind::Word,
            TextTokenKind::Whitespace,
            TextTokenKind::InlineMath,
            TextTokenKind::Whitespace,
            TextTokenKind::Word,
        ]);
    }

    #[test]
    fn escaped_dollar_is_not_math() {
        // `\$` -> OTHER; the following `x` is a word; the trailing `$` has no
        // close so it is OTHER too.
        assert_eq!(kinds(r"a \$x$"), vec![
            TextTokenKind::Word,
            TextTokenKind::Whitespace,
            TextTokenKind::Other,
            TextTokenKind::Other,
            TextTokenKind::Word,
            TextTokenKind::Other,
        ]);
    }

    #[test]
    fn empty_inline_math_not_matched() {
        assert!(kinds("$$").contains(&TextTokenKind::Other) || kinds("$$").len() == 2);
    }

    #[test]
    fn display_math_detection() {
        assert_eq!(kinds("$$x+y$$"), vec![TextTokenKind::DisplayMath]);
    }

    #[test]
    fn placeholder_and_protected() {
        assert_eq!(kinds("[[FORMULA_12]]"), vec![TextTokenKind::FormulaPlaceholder]);
        assert_eq!(kinds("<f12-abc/>"), vec![TextTokenKind::ProtectedFormula]);
    }

    #[test]
    fn math_body_strips_delimiters() {
        assert_eq!(math_token_body("$$ x $$", TextTokenKind::DisplayMath), "x");
        assert_eq!(math_token_body("$ x $", TextTokenKind::InlineMath), "x");
        assert_eq!(math_token_body("plain", TextTokenKind::Word), "plain");
    }

    #[test]
    fn replace_non_formula() {
        let out = replace_non_formula_segments("a $x$ b", |plain| plain.to_uppercase());
        assert_eq!(out, "A $x$ B");
    }
}
