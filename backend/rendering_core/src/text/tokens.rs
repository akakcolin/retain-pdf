// Port of services/rendering/layout/text_tokens.py — a hand-written tokenizer
// (no regex) with a small set of ordered match rules.

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

pub const RAW_MATH_TOKEN_KINDS: [TextTokenKind; 2] = [TextTokenKind::DisplayMath, TextTokenKind::InlineMath];

pub const MAX_INLINE_MATH_CHARS: usize = 1200;

#[derive(Debug, Clone, PartialEq)]
pub struct TextToken {
    pub kind: TextTokenKind,
    pub value: String,
    pub start: usize,
    pub end: usize,
}

fn is_ascii_alnum(c: char) -> bool {
    c.is_ascii_alphabetic() || c.is_ascii_digit()
}

fn is_formula_suffix_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

fn is_zh_char(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// Count consecutive backslashes before `index`; escaped if the count is odd.
fn is_escaped(text: &str, index: usize) -> bool {
    let mut backslashes = 0usize;
    let mut cursor = index;
    while cursor > 0 {
        cursor -= 1;
        if text.as_bytes()[cursor] == b'\\' {
            backslashes += 1;
        } else {
            break;
        }
    }
    backslashes % 2 == 1
}

fn match_formula_placeholder(text: &str, index: usize) -> usize {
    const PREFIXES: [(&str, &str); 4] = [
        ("[[FORMULA_", "]]"),
        ("__FORMULA_", "__"),
        ("⟦FORMULA_", "⟧"),
        ("<FORMULA_", ">"),
    ];
    let bytes = text.as_bytes();
    for (prefix, suffix) in PREFIXES {
        if !text[index..].starts_with(prefix) {
            continue;
        }
        let mut cursor = index + prefix.len();
        let digit_start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == digit_start || !text[cursor..].starts_with(suffix) {
            continue;
        }
        return cursor + suffix.len();
    }
    index
}

fn match_protected_formula(text: &str, index: usize) -> usize {
    let bytes = text.as_bytes();
    if index + 7 > bytes.len() || bytes[index] != b'<' {
        return index;
    }
    let mut cursor = index + 1;
    if !matches!(bytes[cursor], b'f' | b'u' | b't' | b'n' | b'v' | b'c') {
        return index;
    }
    cursor += 1;
    let digit_start = cursor;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
        cursor += 1;
    }
    if cursor == digit_start || cursor >= bytes.len() || bytes[cursor] != b'-' {
        return index;
    }
    let suffix_start = cursor + 1;
    let suffix_end = suffix_start + 3;
    if suffix_end > bytes.len() {
        return index;
    }
    let suffix_ok = bytes[suffix_start..suffix_end]
        .iter()
        .all(|&b| is_formula_suffix_char(b as char));
    if !suffix_ok || !text[suffix_end..].starts_with("/>") {
        return index;
    }
    suffix_end + 2
}

fn match_display_math(text: &str, index: usize) -> usize {
    let bytes = text.as_bytes();
    if index + 1 >= bytes.len() || !text[index..].starts_with("$$") || is_escaped(text, index) {
        return index;
    }
    let mut cursor = index + 2;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor += 2;
            continue;
        }
        if text[cursor..].starts_with("$$") {
            return cursor + 2;
        }
        cursor += 1;
    }
    index
}

fn match_inline_math(text: &str, index: usize) -> usize {
    let bytes = text.as_bytes();
    if bytes[index] != b'$'
        || text[index..].starts_with("$$")
        || is_escaped(text, index)
        || index + 1 >= bytes.len()
    {
        return index;
    }
    let mut cursor = index + 1;
    while cursor < bytes.len() {
        if cursor - index > MAX_INLINE_MATH_CHARS {
            return index;
        }
        let c = bytes[cursor];
        if c == b'\n' {
            return index;
        }
        if c == b'\\' {
            cursor += 2;
            continue;
        }
        if c == b'$' {
            let body = text[index + 1..cursor].trim();
            if body.is_empty() {
                return index;
            }
            return cursor + 1;
        }
        cursor += 1;
    }
    index
}

fn match_whitespace(text: &str, index: usize) -> usize {
    let bytes = text.as_bytes();
    if !text[index..].chars().next().unwrap().is_whitespace() {
        return index;
    }
    let mut cursor = index;
    while cursor < bytes.len() {
        let ch = text[cursor..].chars().next().unwrap();
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn match_word(text: &str, index: usize) -> usize {
    let bytes = text.as_bytes();
    if !is_ascii_alnum(text[index..].chars().next().unwrap()) {
        return index;
    }
    let mut cursor = index + 1;
    while cursor < bytes.len() && is_ascii_alnum(text[cursor..].chars().next().unwrap()) {
        cursor += 1;
    }
    while cursor + 1 < bytes.len() {
        let c = bytes[cursor];
        if c != b'-' && c != b'\'' {
            break;
        }
        let next_is_alnum = is_ascii_alnum(text[cursor + 1..].chars().next().unwrap());
        if !next_is_alnum {
            break;
        }
        cursor += 2;
        while cursor < bytes.len() && is_ascii_alnum(text[cursor..].chars().next().unwrap()) {
            cursor += 1;
        }
    }
    cursor
}

fn match_zh_char(text: &str, index: usize) -> usize {
    let ch = text[index..].chars().next().unwrap();
    if is_zh_char(ch) {
        index + ch.len_utf8()
    } else {
        index
    }
}

/// Ordered rules; the first rule whose matcher advances wins (mirrors
/// Python's TOKEN_RULES tuple).
fn match_end(kind: TextTokenKind, text: &str, index: usize) -> usize {
    match kind {
        TextTokenKind::FormulaPlaceholder => match_formula_placeholder(text, index),
        TextTokenKind::ProtectedFormula => match_protected_formula(text, index),
        TextTokenKind::DisplayMath => match_display_math(text, index),
        TextTokenKind::InlineMath => match_inline_math(text, index),
        TextTokenKind::Whitespace => match_whitespace(text, index),
        TextTokenKind::Word => match_word(text, index),
        TextTokenKind::ZhChar => match_zh_char(text, index),
        TextTokenKind::Other => index,
    }
}

/// True if `text` contains any of the three protected-token patterns
/// (`PROTECTED_TOKEN_RE`): `<[futnvc]\d+-[0-9a-z]{3}/>`, `[[FORMULA_\d+]]`, or
/// `@@F\d+@@`. Hand-rolled (no regex crate) and intentionally narrower than
/// `match_formula_placeholder` — the render path gates only on these exact
/// patterns.
pub fn has_protected_token(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'<' {
            if i + 7 <= bytes.len() && matches!(bytes[i + 1], b'f' | b'u' | b't' | b'n' | b'v' | b'c') {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 2 && j + 5 <= bytes.len() && bytes[j] == b'-' {
                    let suffix_ok = bytes[j + 1..j + 4]
                        .iter()
                        .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit());
                    if suffix_ok && text[j + 4..].starts_with("/>") {
                        return true;
                    }
                }
            }
        } else if b == b'[' && text[i..].starts_with("[[FORMULA_") {
            let mut j = i + 10;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 10 && text[j..].starts_with("]]") {
                return true;
            }
        } else if b == b'@' && text[i..].starts_with("@@F") {
            let mut j = i + 3;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 3 && text[j..].starts_with("@@") {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub fn iter_text_tokens(text: &str) -> Vec<TextToken> {
    let source = text;
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let rule_order = [
        TextTokenKind::FormulaPlaceholder,
        TextTokenKind::ProtectedFormula,
        TextTokenKind::DisplayMath,
        TextTokenKind::InlineMath,
        TextTokenKind::Whitespace,
        TextTokenKind::Word,
        TextTokenKind::ZhChar,
    ];
    while index < source.len() {
        let mut matched = false;
        for &kind in &rule_order {
            let end = match_end(kind, source, index);
            if end > index {
                tokens.push(TextToken {
                    kind,
                    value: source[index..end].to_string(),
                    start: index,
                    end,
                });
                index = end;
                matched = true;
                break;
            }
        }
        if !matched {
            let ch = source[index..].chars().next().unwrap();
            let end = index + ch.len_utf8();
            tokens.push(TextToken {
                kind: TextTokenKind::Other,
                value: source[index..end].to_string(),
                start: index,
                end,
            });
            index = end;
        }
    }
    tokens
}

pub fn tokenize_text(text: &str) -> Vec<String> {
    iter_text_tokens(text).into_iter().map(|t| t.value).collect()
}

pub fn iter_formula_tokens(text: &str) -> Vec<TextToken> {
    iter_text_tokens(text)
        .into_iter()
        .filter(|t| FORMULA_TOKEN_KINDS.contains(&t.kind))
        .collect()
}

pub fn strip_formula_tokens(text: &str, replacement: &str) -> String {
    let mut out = String::new();
    for token in iter_text_tokens(text) {
        if FORMULA_TOKEN_KINDS.contains(&token.kind) {
            out.push_str(replacement);
        } else {
            out.push_str(&token.value);
        }
    }
    out
}

pub fn classify_token_value(token: &str) -> TextTokenKind {
    let tokens = iter_text_tokens(token);
    if tokens.len() == 1 && tokens[0].value == *token {
        tokens[0].kind
    } else {
        TextTokenKind::Other
    }
}

pub fn is_formula_token(token: &str) -> bool {
    FORMULA_TOKEN_KINDS.contains(&classify_token_value(token))
}

pub fn math_token_body(token: &TextToken) -> String {
    let value = &token.value;
    let kind = token.kind;
    if kind == TextTokenKind::DisplayMath && value.starts_with("$$") && value.ends_with("$$") {
        return value[2..value.len() - 2].trim().to_string();
    }
    if kind == TextTokenKind::InlineMath && value.starts_with('$') && value.ends_with('$') {
        return value[1..value.len() - 1].trim().to_string();
    }
    value.clone()
}

pub fn replace_non_formula_segments(text: &str, replacer: &dyn Fn(&str) -> String) -> String {
    let mut chunks: Vec<String> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    for token in iter_text_tokens(text) {
        if RAW_MATH_TOKEN_KINDS.contains(&token.kind) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<TextTokenKind> {
        iter_text_tokens(text).into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn zh_char_split() {
        assert_eq!(
            kinds("中文"),
            vec![TextTokenKind::ZhChar, TextTokenKind::ZhChar]
        );
    }

    #[test]
    fn word_with_hyphen() {
        assert_eq!(kinds("g-h"), vec![TextTokenKind::Word]);
    }

    #[test]
    fn dollar_inline_math() {
        assert_eq!(
            kinds(r"沿 $g-h(\mathbf{R}_x)$ 路径"),
            vec![
                TextTokenKind::ZhChar,
                TextTokenKind::Whitespace,
                TextTokenKind::InlineMath,
                TextTokenKind::Whitespace,
                TextTokenKind::ZhChar,
                TextTokenKind::ZhChar,
            ]
        );
        let tokens = iter_text_tokens(r"$g-h(\mathbf{R}_x)$");
        assert_eq!(math_token_body(&tokens[0]), r"g-h(\mathbf{R}_x)");
    }

    #[test]
    fn formula_placeholder() {
        let tokens = iter_text_tokens("__FORMULA_1__ 与 __FORMULA_3__");
        let formula: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TextTokenKind::FormulaPlaceholder)
            .map(|t| t.value.clone())
            .collect();
        assert_eq!(formula, vec!["__FORMULA_1__", "__FORMULA_3__"]);
    }

    #[test]
    fn escaped_dollar_is_not_math() {
        // Python: [OTHER, OTHER, WORD, OTHER, OTHER] — `\` and `$` are separate
        // `other` tokens; the escaped `$` never opens inline math.
        assert_eq!(
            kinds(r"\$notmath\$"),
            vec![
                TextTokenKind::Other,
                TextTokenKind::Other,
                TextTokenKind::Word,
                TextTokenKind::Other,
                TextTokenKind::Other,
            ]
        );
    }

    #[test]
    fn strip_formula_replaces_with_space() {
        let stripped = strip_formula_tokens("a __FORMULA_1__ b", " ");
        assert_eq!(stripped, "a   b");
    }

    #[test]
    fn protected_token_detection() {
        assert!(has_protected_token("a <f1-abc/> b"));
        assert!(has_protected_token("a <u3-9zz/> b"));
        assert!(has_protected_token("[[FORMULA_12]]"));
        assert!(has_protected_token("x @@F7@@ y"));
        assert!(!has_protected_token("plain text"));
        assert!(!has_protected_token("<f1-abc>"));
        assert!(!has_protected_token("<f-a/>"));
        assert!(!has_protected_token("[[FORMULA_x]]"));
        assert!(!has_protected_token("@@Fx@@"));
        // Broad tokenizer placeholder forms must NOT trip the render gate.
        assert!(!has_protected_token("__FORMULA_1__"));
        assert!(!has_protected_token("<FORMULA_1>"));
    }
}
