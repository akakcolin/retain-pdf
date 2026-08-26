//! Python-`re`-compatible regex helpers.
//!
//! Python `re` treats `\s` / `\S` as ASCII whitespace only (`[ \t\n\r\f\v]`),
//! while Rust regex engines default to Unicode White_Space. To keep the
//! byte-exact emit differential meaningful we translate every `\s`/`\S` to the
//! ASCII classes before compiling. `\b`, `\w`, `\d` stay Unicode (matching
//! Python's default str-mode `re`).
//!
//! `fancy-regex` backs this because the ported patterns use look-around and one
//! backreference, which the `regex` crate does not support.

use fancy_regex::Regex;

/// Compile a Python-`re` pattern with Python ASCII `\s`/`\S` semantics.
pub fn compile_py(pattern: &str) -> Regex {
    Regex::new(&translate_py_whitespace(pattern)).expect("invalid py-regex")
}

/// Translate `\s`/`\S` to Python's ASCII-whitespace classes.
fn translate_py_whitespace(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0;
    let mut in_class = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            if i + 1 < chars.len() && matches!(chars[i + 1], 's' | 'S') {
                if in_class {
                    // `\s` inside a character class expands to the members.
                    out.push_str(" \t\n\r\u{0c}\u{0b}");
                } else if chars[i + 1] == 's' {
                    out.push_str("[ \t\n\r\u{0c}\u{0b}]");
                } else {
                    out.push_str("[^ \t\n\r\u{0c}\u{0b}]");
                }
                i += 2;
                continue;
            }
            out.push(c);
            if i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if c == '[' && !in_class {
            in_class = true;
        } else if c == ']' && in_class {
            in_class = false;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Compile-once wrapper: `let re = py_re!(r"...");` yields `&'static Regex`.
#[macro_export]
macro_rules! py_re {
    ($pat:literal) => {{
        static RE: ::std::sync::OnceLock<::fancy_regex::Regex> = ::std::sync::OnceLock::new();
        RE.get_or_init(|| $crate::pyre::compile_py($pat))
    }};
}

/// `Regex::is_match` that panics on backtracking errors (never for our inputs).
pub fn py_is_match(re: &Regex, text: &str) -> bool {
    re.is_match(text).expect("fancy-regex is_match")
}

/// `Regex::captures` that panics on backtracking errors.
pub fn py_captures<'t>(re: &Regex, text: &'t str) -> Option<fancy_regex::Captures<'t, str>> {
    re.captures(text).expect("fancy-regex captures")
}

/// `Regex::find_iter` that panics per-match on backtracking errors.
pub fn py_find_iter<'t>(re: &Regex, text: &'t str) -> Vec<fancy_regex::Match<'t>> {
    re.find_iter(text)
        .map(|m| m.expect("fancy-regex find_iter"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_whitespace_semantics() {
        // \u3000 (ideographic space) is NOT Python `\s`.
        let re = py_re!(r"\s+");
        assert!(py_is_match(re, "   "));
        assert!(py_is_match(re, "\t\n\r\u{0c}\u{0b}"));
        assert!(!py_is_match(re, "\u{3000}"));
    }

    #[test]
    fn whitespace_inside_class() {
        let re = py_re!(r"\\[A-Za-z]+|[{}\s]");
        assert!(py_is_match(re, " "));
        assert!(py_is_match(re, "{"));
        assert!(py_is_match(re, "\\frac"));
    }

    #[test]
    fn non_whitespace_class() {
        let re = py_re!(r"[^*\n]*?\S");
        assert!(py_is_match(re, "abc"));
        assert!(!py_is_match(re, "   "));
    }

    #[test]
    fn unicode_word_boundary() {
        // \b stays Unicode like Python str-mode re.
        let re = py_re!(r"\\langlen\b");
        assert!(py_is_match(re, r"\langlen x"));
        assert!(!py_is_match(re, r"\langlen中")); // CJK is a word char -> no boundary
    }
}
