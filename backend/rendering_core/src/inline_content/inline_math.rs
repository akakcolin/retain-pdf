// Port of services/rendering/layout/inline_content/core/inline_math.py.
// Hand-rolled matchers (no regex crate); token analysis reuses
// crate::text::{analysis,tokens}.

use crate::text::analysis::{analyze_text, normalize_direct_typst_math_boundaries};
use crate::text::latex_normalizer::normalize_formula_for_latex_math;
use crate::text::tokens::{math_token_body, replace_non_formula_segments, RAW_MATH_TOKEN_KINDS, TextToken, TextTokenKind};

pub const TEXT_HEAVY_INLINE_MATH_MIN_TEXT_CHARS: usize = 10;

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `re.sub(r"[ \t]{2,}", " ", s)`: collapse runs of 2+ spaces/tabs.
fn collapse_double_space(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        let b = s.as_bytes()[i];
        if b == b' ' || b == b'\t' {
            let run_start = i;
            while i < s.len() && (s.as_bytes()[i] == b' ' || s.as_bytes()[i] == b'\t') {
                i += 1;
            }
            if i - run_start >= 2 {
                out.push(' ');
            } else {
                out.push(b as char);
            }
        } else {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// `re.sub(r"\s+", " ", s).strip()` semantics via split_whitespace.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
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

/// `escape_markdown_literal_asterisks`: `(text or "").replace("*", r"\*")`.
pub fn escape_markdown_literal_asterisks(text: &str) -> String {
    text.replace('*', r"\*")
}

/// `MARKDOWN_EMPHASIS_RE` match starting at byte `start`: returns
/// `(body_start, match_end)` for the shortest valid body. The regex is
/// `(?<![\\*])(?P<marker>\*\*|\*)(?=\S)(?P<body>[^*\n]*?\S)(?P=marker)(?!\*)`.
fn match_emphasis(text: &str, start: usize) -> Option<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if start > 0 {
        let prev_idx = chars.partition_point(|(b, _)| *b < start).checked_sub(1)?;
        let prev = chars[prev_idx].1;
        if prev == '\\' || prev == '*' {
            return None;
        }
    }
    let marker_len = if text[start..].starts_with("**") {
        2
    } else if text[start..].starts_with('*') {
        1
    } else {
        return None;
    };
    let body_start = start + marker_len;
    if body_start >= text.len() {
        return None;
    }
    if text[body_start..].chars().next().unwrap().is_whitespace() {
        return None;
    }
    // 按字符位置推进（Python 以字符索引），避免多字节字符上字节步进 panic。
    // start/marker 均为 ASCII，故 body_start 必在字符边界。
    let body_char = chars.partition_point(|(b, _)| *b < body_start);
    let char_len = chars.len();
    let mut k = body_char + 1;
    while k <= char_len {
        let last_char = chars[k - 1].1;
        if last_char == '*' || last_char == '\n' {
            return None;
        }
        if !last_char.is_whitespace() {
            if marker_len == 2 {
                if k + 1 < char_len && chars[k].1 == '*' && chars[k + 1].1 == '*' {
                    let after = if k + 2 < char_len { chars[k + 2].1 } else { '\0' };
                    if after != '*' {
                        return Some((body_start, chars[k + 1].0 + 1));
                    }
                }
            } else if k < char_len && chars[k].1 == '*' {
                let after = if k + 1 < char_len { chars[k + 1].1 } else { '\0' };
                if after != '*' {
                    return Some((body_start, chars[k].0 + 1));
                }
            }
        }
        k += 1;
    }
    None
}

/// `escape_literal_asterisks_preserving_emphasis`: escape `*` outside of
/// non-overlapping MARKDOWN_EMPHASIS_RE matches.
pub fn escape_literal_asterisks_preserving_emphasis(text: &str) -> String {
    let source = text;
    if !source.contains('*') {
        return source.to_string();
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut last_end = 0usize;
    let mut i = 0usize;
    while i < source.len() {
        if let Some((_body_start, match_end)) = match_emphasis(source, i) {
            chunks.push(escape_markdown_literal_asterisks(&source[last_end..i]));
            chunks.push(source[i..match_end].to_string());
            last_end = match_end;
            i = match_end;
            continue;
        }
        let ch = source[i..].chars().next().unwrap();
        i += ch.len_utf8();
    }
    chunks.push(escape_markdown_literal_asterisks(&source[last_end..]));
    chunks.concat()
}

/// `surround_inline_math_with_spaces`: add a space around raw inline/display
/// math unless the neighbor is a no-space punctuation char.
pub fn surround_inline_math_with_spaces(markdown: &str) -> String {
    let text = markdown;
    if text.is_empty() {
        return String::new();
    }
    let left_no_space = "([{\"'“‘（【「『";
    let right_no_space = ".,;:!?)]}，。！？；：、（）【】「」『』";
    let mut chunks: Vec<String> = Vec::new();
    for token in analyze_text(text).tokens {
        if !RAW_MATH_TOKEN_KINDS.contains(&token.kind) {
            chunks.push(token.value);
            continue;
        }
        let expr = token.value;
        let prev_char = text[..token.start].chars().next_back().unwrap_or('\0');
        let next_char = text[token.end..].chars().next().unwrap_or('\0');
        let prefix = if prev_char != '\0'
            && !prev_char.is_whitespace()
            && !left_no_space.contains(prev_char)
        {
            " "
        } else {
            ""
        };
        let suffix = if next_char != '\0'
            && !next_char.is_whitespace()
            && !right_no_space.contains(next_char)
        {
            " "
        } else {
            ""
        };
        chunks.push(format!("{prefix}{expr}{suffix}"));
    }
    collapse_double_space(&chunks.concat()).trim().to_string()
}

/// `normalize_direct_typst_inline_math_whitespace`: collapse newlines inside
/// `$...$` inline math to a single space (handles `$$` escapes).
pub fn normalize_direct_typst_inline_math_whitespace(text: &str) -> String {
    let source: Vec<char> = text.chars().collect();
    if source.is_empty() {
        return String::new();
    }
    let mut chunks: Vec<char> = Vec::new();
    let mut index = 0usize;
    let mut in_inline_math = false;
    while index < source.len() {
        let ch = source[index];
        let prev_char = if index > 0 { source[index - 1] } else { '\0' };
        let next_char = if index + 1 < source.len() { source[index + 1] } else { '\0' };
        if ch == '$' && prev_char != '\\' {
            if next_char == '$' {
                chunks.extend(['$', '$']);
                index += 2;
                continue;
            }
            in_inline_math = !in_inline_math;
            chunks.push(ch);
            index += 1;
            continue;
        }
        if in_inline_math && (ch == '\r' || ch == '\n') {
            if chunks.last().copied() != Some(' ') {
                chunks.push(' ');
            }
            index += 1;
            while index < source.len() && matches!(source[index], '\r' | '\n' | '\t' | ' ') {
                index += 1;
            }
            continue;
        }
        chunks.push(ch);
        index += 1;
    }
    chunks.into_iter().collect()
}

/// `_scan_latex_text_blocks`: split `\text{...}` bodies (brace-depth aware,
/// honoring `\` escapes) out of a math expression into `("text", body)` /
/// `("math", segment)` parts.
fn scan_latex_text_blocks(expr: &str) -> Vec<(String, String)> {
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut index = 0usize;
    while index < expr.len() {
        let start = match expr[index..].find(r"\text{") {
            Some(rel) => index + rel,
            None => {
                if index < expr.len() {
                    parts.push(("math".into(), expr[index..].to_string()));
                }
                break;
            }
        };
        if start > index {
            parts.push(("math".into(), expr[index..start].to_string()));
        }
        let mut cursor = start + r"\text{".len();
        let mut depth = 1usize;
        let body_start = cursor;
        let mut closed = false;
        while cursor < expr.len() {
            let ch = expr[cursor..].chars().next().unwrap();
            if ch == '\\' {
                cursor += 1;
                if cursor < expr.len() {
                    cursor += expr[cursor..].chars().next().unwrap().len_utf8();
                }
                continue;
            }
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    parts.push(("text".into(), expr[body_start..cursor].to_string()));
                    cursor += 1;
                    closed = true;
                    break;
                }
            }
            cursor += ch.len_utf8();
        }
        if !closed {
            parts.push(("math".into(), expr[start..].to_string()));
            break;
        }
        index = cursor;
    }
    parts
}

/// `_plain_text_from_latex_text`: unescape `\{`/`\}` and collapse whitespace.
fn plain_text_from_latex_text(body: &str) -> String {
    let mut s = String::with_capacity(body.len());
    let bytes = body.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && (bytes[i + 1] == b'{' || bytes[i + 1] == b'}') {
            s.push(bytes[i + 1] as char);
            i += 2;
        } else {
            let ch = body[i..].chars().next().unwrap();
            s.push(ch);
            i += ch.len_utf8();
        }
    }
    collapse_ws(&s)
}

/// `re.fullmatch(r"[,;:，。！？、()\[\]{}（）]+", text)`.
fn is_pure_punctuation(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| matches!(c, ',' | ';' | ':' | '，' | '。' | '！' | '？' | '、' | '(' | ')' | '[' | ']' | '{' | '}' | '（' | '）'))
}

/// `re.search(r"\b[A-Za-z]\b", text)`.
fn has_single_letter_word(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() {
            let prev_ok = i == 0 || !is_word_char(chars[i - 1]);
            let next_ok = i + 1 >= chars.len() || !is_word_char(chars[i + 1]);
            if prev_ok && next_ok {
                return true;
            }
        }
    }
    false
}

/// `_math_chunk_needs_math`.
fn math_chunk_needs_math(chunk: &str) -> bool {
    let text = collapse_ws(chunk);
    if text.is_empty() {
        return false;
    }
    if is_pure_punctuation(&text) {
        return false;
    }
    text.contains('\\')
        || text.chars().any(|c| matches!(c, '_' | '^' | '=' | '|' | '<' | '>' | '+' | '-' | '*' | '/'))
        || has_single_letter_word(&text)
        || text.chars().any(|c| ('\u{0391}'..='\u{03A9}').contains(&c) || ('\u{03B1}'..='\u{03C9}').contains(&c))
}

/// `_normalize_math_punctuation_chunk`: apply `\s*,\s*`→`, `, `\s*\)\s*`→`) `,
/// `\s*\(\s*`→` (` then collapse 2+ spaces/tabs.
fn normalize_math_punctuation_chunk(chunk: &str) -> String {
    let text = collapse_ws(chunk.trim());
    let text = sub_ws_wrapped(&text, ',', ", ");
    let text = sub_ws_wrapped(&text, ')', ") ");
    let text = sub_ws_wrapped(&text, '(', " (");
    collapse_double_space(&text).trim().to_string()
}

/// `re.sub(r"\s*<target>\s*", replacement, s)` — replace target and any
/// surrounding whitespace with `replacement`.
fn sub_ws_wrapped(s: &str, target: char, replacement: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let n = chars.len();
    while i < n {
        if chars[i] == target {
            let mut j = i + 1;
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            out.push_str(replacement);
            i = j;
            continue;
        }
        if chars[i].is_whitespace() {
            let mut j = i;
            while j < n && chars[j].is_whitespace() {
                j += 1;
            }
            if j < n && chars[j] == target {
                i = j;
                continue;
            }
            while i < j {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `_append_demoted_math_chunk`.
fn append_demoted_math_chunk(chunks: &mut Vec<String>, chunk: &str) {
    let mut text = collapse_ws(chunk.trim());
    if text.is_empty() {
        return;
    }
    let mut leading = String::new();
    let mut trailing = String::new();
    let lead_set = "([{（";
    let trail_set = ",.;:，。；：)]）";
    while let Some(c) = text.chars().next() {
        if lead_set.contains(c) {
            leading.push(c);
            text = text[c.len_utf8()..].trim().to_string();
        } else {
            break;
        }
    }
    while let Some(c) = text.chars().last() {
        if trail_set.contains(c) {
            trailing.insert(0, c);
            text = text[..text.len() - c.len_utf8()].trim().to_string();
        } else {
            break;
        }
    }
    if !leading.is_empty() {
        chunks.push(leading);
    }
    if !text.is_empty() {
        if math_chunk_needs_math(&text) {
            chunks.push(format!("${text}$"));
        } else {
            let punct = normalize_math_punctuation_chunk(&text);
            if !punct.is_empty() {
                chunks.push(punct);
            }
        }
    }
    if !trailing.is_empty() {
        chunks.push(trailing);
    }
}

/// `_demote_text_heavy_inline_math_expr`.
fn demote_text_heavy_inline_math_expr(expr: &str) -> Option<String> {
    let parts = scan_latex_text_blocks(expr);
    let text_char_count: usize = parts
        .iter()
        .filter(|(kind, _)| kind == "text")
        .map(|(_, value)| plain_text_from_latex_text(value).chars().count())
        .sum();
    if text_char_count < TEXT_HEAVY_INLINE_MATH_MIN_TEXT_CHARS {
        return None;
    }
    let mut chunks: Vec<String> = Vec::new();
    for (kind, value) in parts {
        if kind == "text" {
            let plain = plain_text_from_latex_text(&value);
            if !plain.is_empty() {
                chunks.push(plain);
            }
            continue;
        }
        let math = collapse_ws(value.trim());
        if math.is_empty() {
            continue;
        }
        append_demoted_math_chunk(&mut chunks, &math);
    }
    let joined = collapse_double_space(&chunks.join(" ")).trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// `demote_text_heavy_inline_math`: unwrap `\text{...}`-heavy inline math whose
/// text parts exceed the char threshold, splitting the remainder into math /
/// punctuation chunks.
pub fn demote_text_heavy_inline_math(text: &str) -> String {
    let mut chunks: Vec<String> = Vec::new();
    for token in analyze_text(text).tokens {
        if token.kind != TextTokenKind::InlineMath {
            chunks.push(token.value);
            continue;
        }
        let expr = math_token_body(&token);
        match demote_text_heavy_inline_math_expr(&expr) {
            Some(replacement) => chunks.push(replacement),
            None => chunks.push(token.value),
        }
    }
    chunks.concat()
}

/// `ANGLE_EXPECTATION_RE` / `BARE_ANGLE_EXPECTATION_RE` script matcher:
/// `_\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}` (max one nesting level).
fn match_angle_script(text: &str, pos: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if pos + 1 >= bytes.len() || bytes[pos] != b'_' || bytes[pos + 1] != b'{' {
        return None;
    }
    let mut i = pos + 2;
    let mut depth = 1usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'{' {
            depth += 1;
            if depth > 2 {
                return None;
            }
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

/// One pass of the angle-bracket expectation substitution. When
/// `require_script` the `_{...}` suffix is mandatory (ANGLE_EXPECTATION_RE),
/// otherwise the bare `\langle...\rangle` form is rewritten (BARE..._RE).
fn sub_angle_brackets(s: &str, require_script: bool) -> String {
    let source = s;
    let mut out = String::new();
    let mut i = 0usize;
    while i < source.len() {
        if source[i..].starts_with(r"\langle") {
            let body_start = skip_ws(source, i + r"\langle".len());
            let mut r = body_start;
            let mut matched: Option<usize> = None; // consume-end
            while r < source.len() {
                if source[r..].starts_with(r"\rangle") {
                    let body = &source[body_start..r];
                    if !body.is_empty() && !body.contains('$') {
                        let after_rangle = r + r"\rangle".len();
                        if require_script {
                            if let Some(script_end) = match_angle_script(source, after_rangle) {
                                matched = Some(script_end);
                                break;
                            }
                        } else {
                            matched = Some(after_rangle);
                            break;
                        }
                    }
                }
                r += source[r..].chars().next().unwrap().len_utf8();
            }
            if let Some(consume_end) = matched {
                out.push('⟨');
                out.push_str(source[body_start..r].trim());
                out.push('⟩');
                if require_script {
                    let after_rangle = r + r"\rangle".len();
                    out.push_str(&source[after_rangle..consume_end]);
                }
                i = consume_end;
                continue;
            }
        }
        let ch = source[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `normalize_angle_bracket_expectation_for_mitex`.
pub fn normalize_angle_bracket_expectation_for_mitex(expr: &str) -> String {
    let normalized = sub_angle_brackets(expr, true);
    sub_angle_brackets(&normalized, false)
}

/// `normalize_sized_delimiters_for_mitex`: map `\left`/`\right` delimiter
/// tokens to their plain/empty form.
pub fn normalize_sized_delimiters_for_mitex(expr: &str) -> String {
    let source = expr;
    let mut out = String::new();
    let mut i = 0usize;
    while i < source.len() {
        let is_left = source[i..].starts_with(r"\left");
        let is_right = !is_left && source[i..].starts_with(r"\right");
        if is_left || is_right {
            let prefix_len = if is_left { r"\left".len() } else { r"\right".len() };
            let j = skip_ws(source, i + prefix_len);
            if source[j..].starts_with(r"\langle") {
                out.push('⟨');
                i = j + r"\langle".len();
                continue;
            }
            if source[j..].starts_with(r"\rangle") {
                out.push('⟩');
                i = j + r"\rangle".len();
                continue;
            }
            if source[j..].starts_with(r"\{") || source[j..].starts_with(r"\}") {
                let brace = source[j + 1..].chars().next().unwrap();
                out.push(brace);
                i = j + 2;
                continue;
            }
            let ch = source[j..].chars().next().unwrap();
            if matches!(ch, '⟨' | '⟩' | '|' | '(' | ')' | '[' | ']' | '{' | '}' | '.') {
                if ch != '.' {
                    out.push(ch);
                }
                i = j + ch.len_utf8();
                continue;
            }
        }
        let ch = source[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `re.fullmatch(r"\\([A-Za-z]{1,3})\\([0-9]{1,7})", expr)` — spreadsheet-cell
/// formula like `\A1\23`.
fn match_spreadsheet_cell(expr: &str) -> Option<(String, String)> {
    let bytes = expr.as_bytes();
    if bytes.is_empty() || bytes[0] != b'\\' {
        return None;
    }
    let mut i = 1usize;
    let mut letters = 0usize;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() && letters < 3 {
        letters += 1;
        i += 1;
    }
    if letters < 1 || i >= bytes.len() || bytes[i] != b'\\' {
        return None;
    }
    let mut j = i + 1;
    let mut digits = 0usize;
    while j < bytes.len() && bytes[j].is_ascii_digit() && digits < 7 {
        digits += 1;
        j += 1;
    }
    if digits < 1 || j != bytes.len() {
        return None;
    }
    Some((expr[1..i].to_string(), expr[i + 1..j].to_string()))
}

/// `re.sub(r"\\{2,}(?=[A-Za-z])", r"\\", expr)`.
fn collapse_multi_backslash(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let run_start = i;
            while i < bytes.len() && bytes[i] == b'\\' {
                i += 1;
            }
            let count = i - run_start;
            let next_is_letter = i < bytes.len() && bytes[i].is_ascii_alphabetic();
            if count >= 2 && next_is_letter {
                out.push('\\');
            } else {
                for _ in 0..count {
                    out.push('\\');
                }
            }
        } else {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

/// `re.sub(r"\\angle(?=[A-Za-z])", r"\\angle ", expr)`.
fn sub_angle_lookahead(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && s[i..].starts_with(r"\angle") {
            let after = i + r"\angle".len();
            if after < bytes.len() && bytes[after].is_ascii_alphabetic() {
                out.push_str(r"\angle ");
                i = after;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `re.sub(r"\\varPhi(?=[^A-Za-z]|$)", r"\\Phi", expr)`.
fn sub_varphi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && s[i..].starts_with(r"\varPhi") {
            let after = i + r"\varPhi".len();
            let next_is_letter = after < bytes.len() && bytes[after].is_ascii_alphabetic();
            if !next_is_letter {
                out.push_str(r"\Phi");
                i = after;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `re.sub(r"\\<cmd>\b", replacement, s)` — replace a latex command followed by
/// a word boundary (not followed by a word char).
fn sub_word_bounded(s: &str, needle: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < s.len() {
        if s[i..].starts_with(needle) {
            let after = i + needle.len();
            let next_is_word = after < s.len() && is_word_char(s[after..].chars().next().unwrap());
            if !next_is_word {
                out.push_str(replacement);
                i = after;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `re.sub(r"\\circled\s*\{\s*\\<body>\s*\}", replacement, s)`.
fn sub_circled_body(s: &str, body_needle: &str, replacement: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < s.len() {
        if s[i..].starts_with(r"\circled") {
            let mut j = skip_ws(s, i + r"\circled".len());
            if j < s.len() && s[j..].starts_with('{') {
                j = skip_ws(s, j + 1);
                if s[j..].starts_with(body_needle) {
                    let k = skip_ws(s, j + body_needle.len());
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

/// `re.sub(r"\\circled\s*\{\s*([^{}]+?)\s*\}", r"\1", expr)`.
fn circled_unwrap(s: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < s.len() {
        if s[i..].starts_with(r"\circled") {
            let mut j = skip_ws(s, i + r"\circled".len());
            if j < s.len() && s[j..].starts_with('{') {
                j = skip_ws(s, j + 1);
                let inner_start = j;
                let mut cursor = j;
                let mut inner = None;
                while cursor < s.len() {
                    let ch = s[cursor..].chars().next().unwrap();
                    if ch == '}' {
                        let candidate = s[inner_start..cursor].trim();
                        if !candidate.is_empty() && !candidate.contains('{') && !candidate.contains('}') {
                            inner = Some(candidate.to_string());
                        }
                        break;
                    }
                    cursor += ch.len_utf8();
                }
                if let Some(value) = inner {
                    out.push_str(&value);
                    i = cursor + 1;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `_sanitize_token` — one RAW_MATH token rewritten for mitex compatibility.
fn sanitize_token(token: &TextToken) -> String {
    let is_display = token.kind == TextTokenKind::DisplayMath;
    let expr = math_token_body(token);
    if expr.is_empty() {
        return token.value.clone();
    }
    if matches!(expr.as_str(), "^®" | "^{®}" | r"^\circled{R}" | r"^\textcircled{R}") {
        return "®".to_string();
    }
    if let Some((cmd, num)) = match_spreadsheet_cell(&expr) {
        return format!("{cmd}{num}");
    }
    let mut expr = collapse_multi_backslash(&expr);
    expr = sub_word_bounded(&expr, r"\langlen", r"\langle n");
    expr = sub_angle_lookahead(&expr);
    expr = sub_word_bounded(&expr, r"\mathscr", r"\mathcal");
    expr = sub_varphi(&expr);
    expr = sub_word_bounded(&expr, r"\hbar", "ℏ");
    expr = sub_word_bounded(&expr, r"\partial", "∂");
    expr = sub_word_bounded(&expr, r"\otimes", "⊗");
    expr = normalize_sized_delimiters_for_mitex(&expr);
    expr = normalize_angle_bracket_expectation_for_mitex(&expr);
    expr = sub_word_bounded(&expr, r"\langle", "⟨");
    expr = sub_word_bounded(&expr, r"\rangle", "⟩");
    expr = normalize_sized_delimiters_for_mitex(&expr);
    expr = sub_circled_body(&expr, r"\times", r"\otimes");
    expr = sub_circled_body(&expr, r"\parallel", r"\circ");
    expr = circled_unwrap(&expr);
    if is_display {
        expr = normalize_formula_for_latex_math(&expr);
    }
    format!("${expr}$")
}

/// `sanitize_direct_typst_inline_math`.
pub fn sanitize_direct_typst_inline_math(text: &str) -> String {
    let mut chunks: Vec<String> = Vec::new();
    for token in analyze_text(text).tokens {
        if RAW_MATH_TOKEN_KINDS.contains(&token.kind) {
            chunks.push(sanitize_token(&token));
        } else {
            chunks.push(token.value);
        }
    }
    chunks.concat()
}

/// `build_direct_typst_passthrough_markdown`.
pub fn build_direct_typst_passthrough_markdown(text: &str) -> String {
    let normalized = normalize_direct_typst_math_boundaries(text.trim());
    let normalized = normalize_direct_typst_inline_math_whitespace(&normalized);
    let markdown = replace_non_formula_segments(&normalized, &escape_literal_asterisks_preserving_emphasis);
    let markdown = sanitize_direct_typst_inline_math(&markdown);
    surround_inline_math_with_spaces(&markdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_literal_asterisks() {
        assert_eq!(escape_markdown_literal_asterisks("a*b"), r"a\*b");
    }

    #[test]
    fn preserves_emphasis_asterisks() {
        assert_eq!(escape_literal_asterisks_preserving_emphasis("*bold* and *x*"), "*bold* and *x*");
        assert_eq!(escape_literal_asterisks_preserving_emphasis("**strong**"), r"**strong**");
        assert_eq!(escape_literal_asterisks_preserving_emphasis("plain"), "plain");
        assert_eq!(escape_literal_asterisks_preserving_emphasis("a**b"), r"a\*\*b");
    }

    // 回归：`*` 后紧跟多字节字符（如中文 `（`）时不能 panic。
    // Python 原版按字符索引，Rust 移植曾用字节步进 k+=1 导致切片落在
    // 非字符边界 → pyo3 panic。期望值取自 Python inline_math.py 逐字比对。
    #[test]
    fn does_not_panic_on_multibyte_after_asterisk() {
        assert_eq!(escape_literal_asterisks_preserving_emphasis("a*（b）"), r"a\*（b）");
        assert_eq!(escape_literal_asterisks_preserving_emphasis("a*b（"), r"a\*b（");
        assert_eq!(escape_literal_asterisks_preserving_emphasis("*（中文加粗）*"), "*（中文加粗）*");
        assert_eq!(escape_literal_asterisks_preserving_emphasis("a*（b）*c"), "a*（b）*c");
        assert_eq!(
            escape_literal_asterisks_preserving_emphasis(
                "袁萌*（IEEE会员），曾轩*（IEEE高级会员）"
            ),
            "袁萌*（IEEE会员），曾轩*（IEEE高级会员）"
        );
        assert_eq!(
            escape_literal_asterisks_preserving_emphasis(
                "袁萌，董志轩，王玉燕，严长浩*（IEEE会员），毕兆瑞（IEEE会员），朱克伦（IEEE会员），王胜国（IEEE终身高级会员），周电（IEEE高级会员），曾轩*（IEEE高级会员）"
            ),
            "袁萌，董志轩，王玉燕，严长浩*（IEEE会员），毕兆瑞（IEEE会员），朱克伦（IEEE会员），王胜国（IEEE终身高级会员），周电（IEEE高级会员），曾轩*（IEEE高级会员）"
        );
    }

    #[test]
    fn surrounds_math_with_spaces() {
        assert_eq!(surround_inline_math_with_spaces("a$x$b"), "a $x$ b");
        assert_eq!(surround_inline_math_with_spaces("( $x$ )"), "( $x$ )");
        assert_eq!(surround_inline_math_with_spaces("x$x$,"), "x $x$,");
    }

    #[test]
    fn normalizes_inline_math_whitespace() {
        assert_eq!(normalize_direct_typst_inline_math_whitespace("$a\n b$"), "$a b$");
        assert_eq!(normalize_direct_typst_inline_math_whitespace("$$x$$"), "$$x$$");
    }

    #[test]
    fn demotes_text_heavy_inline_math() {
        let out = demote_text_heavy_inline_math(
            r"$x \text{some long english sentence about chemistry} y$",
        );
        assert_eq!(out, "$x$ some long english sentence about chemistry $y$");
    }

    #[test]
    fn keeps_short_inline_math() {
        let expr = r"\text{ok}";
        assert_eq!(demote_text_heavy_inline_math_expr(expr), None);
    }

    #[test]
    fn normalizes_angle_expectations() {
        assert_eq!(
            normalize_angle_bracket_expectation_for_mitex(r"\langle a \rangle_{x}"),
            "⟨a⟩_{x}"
        );
        assert_eq!(
            normalize_angle_bracket_expectation_for_mitex(r"\langle b \rangle"),
            "⟨b⟩"
        );
    }

    #[test]
    fn normalizes_sized_delimiters() {
        assert_eq!(
            normalize_sized_delimiters_for_mitex(r"\left( \frac12 \right)"),
            "( \\frac12 )"
        );
        assert_eq!(normalize_sized_delimiters_for_mitex(r"\left\langle x \right\rangle"), "⟨ x ⟩");
        assert_eq!(normalize_sized_delimiters_for_mitex(r"\left."), "");
    }

    #[test]
    fn sanitizes_direct_typst_math() {
        assert_eq!(sanitize_direct_typst_inline_math(r"$\circled{R}$"), r"$R$");
        assert_eq!(sanitize_direct_typst_inline_math(r"$\frac{a}{b}$"), r"$\frac{a}{b}$");
        assert_eq!(sanitize_direct_typst_inline_math(r"$\langlen\rangle$"), r"$⟨n⟩$");
    }

    #[test]
    fn builds_passthrough_markdown() {
        let out = build_direct_typst_passthrough_markdown("$x$ and *y*");
        assert!(out.contains("$x$"));
    }
}
