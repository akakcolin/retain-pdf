// Port of the protected-token helpers that the inline_content chain needs from
// translation/core/payload/formula_protection.py: `wrap_formula_inline_math` and
// `re_protect_restored_formulas`. PROTECTED_TOKEN_RE is the 3-way alternation
// `<[futnvc]\d+-[0-9a-z]{3}/> | [[FORMULA_\d+]] | @@F\d+@@`.

use crate::item::FormulaEntry;

/// `wrap_formula_inline_math`: strip a fully-inline `$...$` wrapper (body must
/// be non-empty and free of `$` / newline), then re-wrap.
pub fn wrap_formula_inline_math(formula_text: &str) -> String {
    let text = formula_text.trim();
    if text.is_empty() {
        return String::new();
    }
    let text = if text.starts_with('$') && text.ends_with('$') && text.len() >= 2 {
        let body = &text[1..text.len() - 1];
        if !body.is_empty() && !body.contains('$') && !body.contains('\n') {
            body.trim()
        } else {
            text
        }
    } else {
        text
    };
    format!("${text}$")
}

/// `_can_replace_raw_formula`: replace the raw (unwrapped) formula text only if
/// it is not pure alphanumerics and carries at least one formula marker.
fn can_replace_raw_formula(formula_text: &str) -> bool {
    let text = formula_text.trim();
    if text.is_empty() || text.chars().count() <= 1 {
        return false;
    }
    if text.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    text.chars().any(|c| matches!(c, '\\' | '_' | '^' | '{' | '}' | '(' | ')' | '[' | ']' | '+' | '-' | '=' | '/' | '*'))
}

/// Match a full PROTECTED_TOKEN_RE token starting at `index`; returns its byte
/// length, or None. Hand-rolled (no regex crate).
fn match_full_protected_token(text: &str, index: usize) -> Option<usize> {
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
    if bytes[index] == b'@' && text[index..].starts_with("@@F") {
        let mut j = index + 3;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > index + 3 && text[j..].starts_with("@@") {
            return Some(j + 2 - index);
        }
    }
    None
}

/// `PROTECTED_TOKEN_RE.split` keeping delimiters (the pattern has no capturing
/// groups, so Python `split` interleaves the matched tokens).
fn split_protected_all(text: &str) -> (Vec<String>, Vec<String>) {
    let mut parts: Vec<String> = Vec::new();
    let mut delimiters: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(len) = match_full_protected_token(text, i) {
            parts.push(text[start..i].to_string());
            delimiters.push(text[i..i + len].to_string());
            start = i + len;
            i = start;
            continue;
        }
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    parts.push(text[start..].to_string());
    (parts, delimiters)
}

/// `re_protect_restored_formulas`: replace formula text that translation already
/// restored inline (wrapped `$...$` form, then the raw form) back with its
/// placeholder, so downstream parts still see protected tokens. Formula entries
/// are processed longest-formula-first so overlapping text wins deterministically.
pub fn re_protect_restored_formulas(text: &str, formula_map: &[FormulaEntry]) -> String {
    let protected = text;
    if protected.is_empty() || formula_map.is_empty() {
        return protected.to_string();
    }
    let (mut parts, delimiters) = split_protected_all(protected);
    let mut entries: Vec<&FormulaEntry> = formula_map.iter().collect();
    entries.sort_by(|a, b| b.formula_text.chars().count().cmp(&a.formula_text.chars().count()));
    for item in entries {
        let formula_text = item.formula_text.trim();
        let placeholder = item.placeholder.trim();
        if formula_text.is_empty() || placeholder.is_empty() {
            continue;
        }
        let wrapped = wrap_formula_inline_math(formula_text);
        for chunk in parts.iter_mut() {
            let mut next = chunk.replace(&wrapped, placeholder);
            if can_replace_raw_formula(formula_text) {
                next = next.replace(formula_text, placeholder);
            }
            *chunk = next;
        }
    }
    let mut rebuilt = String::new();
    for (index, chunk) in parts.iter().enumerate() {
        rebuilt.push_str(chunk);
        if index < delimiters.len() {
            rebuilt.push_str(&delimiters[index]);
        }
    }
    rebuilt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(placeholder: &str, formula_text: &str) -> FormulaEntry {
        FormulaEntry {
            placeholder: placeholder.to_string(),
            formula_text: formula_text.to_string(),
        }
    }

    #[test]
    fn wraps_and_unwraps_inline_math() {
        assert_eq!(wrap_formula_inline_math("x^2"), "$x^2$");
        assert_eq!(wrap_formula_inline_math("$x^2$"), "$x^2$");
        assert_eq!(wrap_formula_inline_math(""), "");
    }

    #[test]
    fn reprotects_wrapped_and_raw_forms() {
        let map = vec![entry("<f1-abc/>", "\\frac{a}{b}")];
        let out = re_protect_restored_formulas("见 $\\frac{a}{b}$ 之后", &map);
        assert_eq!(out, "见 <f1-abc/> 之后");
        let out = re_protect_restored_formulas("见 \\frac{a}{b} 之后", &map);
        assert_eq!(out, "见 <f1-abc/> 之后");
    }

    #[test]
    fn reprotect_preserves_existing_tokens() {
        let map = vec![entry("<f1-abc/>", "E")];
        // "E" is pure alphanumeric -> raw replace disabled; wrapped "$E$" still replaced.
        let out = re_protect_restored_formulas("a $E$ b <f2-xyz/>", &map);
        assert_eq!(out, "a <f1-abc/> b <f2-xyz/>");
    }

    #[test]
    fn reprotect_splits_three_way_tokens() {
        let map = vec![entry("<f1-abc/>", "A+B")];
        let out = re_protect_restored_formulas("<u2-9zz/> $A+B$ [[FORMULA_3]] @@F7@@", &map);
        assert_eq!(out, "<u2-9zz/> <f1-abc/> [[FORMULA_3]] @@F7@@");
    }
}
