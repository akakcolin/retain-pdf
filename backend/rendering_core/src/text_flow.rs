// Port of services/document_schema/text_flow.py — the line-flow classification
// helpers the structured-line-break seed path consumes. Value-level (raw item
// dicts), matching the Python `lines`/`text` shapes.
//
// `\d` (markers/glossary bodies) is approximated as ASCII + fullwidth decimal
// digits (the realistic set in CJK source text); Python `re` matches the full
// Unicode Nd category.

use serde_json::Value;

pub const TEXT_FLOW_PRESERVE_LINES: &str = "preserve_lines";
pub const TEXT_FLOW_FLOW: &str = "flow";

const SOFT_WORDS: [&str; 11] = [
    "and", "or", "of", "for", "to", "in", "on", "with", "the", "a", "an",
];

/// `" ".join(s.split())` — collapse every whitespace run to a single space.
pub fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Python `\d`: ASCII + fullwidth decimal digits (Nd subset).
pub(crate) fn is_decimal_digit(c: char) -> bool {
    c.is_ascii_digit() || ('\u{ff10}'..='\u{ff19}').contains(&c)
}

/// Numeric value of a decimal digit (ASCII or fullwidth), mirroring `int()`.
pub(crate) fn decimal_digit_value(c: char) -> i64 {
    if c.is_ascii_digit() {
        c as i64 - '0' as i64
    } else {
        c as i64 - '\u{ff10}' as i64
    }
}

pub(crate) fn char_at(text: &str, i: usize) -> Option<char> {
    text.get(i..)?.chars().next()
}

pub(crate) fn next_index(text: &str, i: usize) -> usize {
    match text.get(i..).and_then(|s| s.chars().next()) {
        Some(c) => i + c.len_utf8(),
        None => i,
    }
}

/// Python `[.!?。！？]\s*$` `.search` — the string (trailing whitespace
/// stripped) ends with a sentence-end punctuation char.
pub(crate) fn ends_with_sentence_end(line: &str) -> bool {
    let trimmed = line.trim_end();
    let Some(last) = trimmed.chars().last() else {
        return false;
    };
    matches!(last, '.' | '!' | '?' | '。' | '！' | '？')
}

/// Python `(?:[-,，;；:]|(?:\b(?:and|or|of|for|to|in|on|with|the|a|an)\b))\s*$`
/// (IGNORECASE) `.search`.
fn ends_with_soft_continuation(line: &str) -> bool {
    let trimmed = line.trim_end();
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let last = trimmed.chars().last().unwrap();
    if matches!(last, '-' | ',' | '，' | ';' | '；' | ':') {
        return true;
    }
    // Trailing ASCII-alpha word must be a soft word preceded by a non-word char.
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_alphabetic() {
        end -= 1;
    }
    let word = trimmed[end..].to_lowercase();
    if !SOFT_WORDS.contains(&word.as_str()) {
        return false;
    }
    if end == 0 {
        return true;
    }
    // Python `\b` treats CJK chars as word chars: decode the actual char before
    // the word (not a lone UTF-8 byte) so `中文and` has no boundary before `and`.
    let prev = trimmed[..end].chars().next_back().unwrap();
    !(prev.is_alphanumeric() || prev == '_')
}

/// Python `[A-Za-z0-9]+(?:[-'][A-Za-z0-9]+)*` `.findall` — count words in a line.
pub(crate) fn word_count(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphanumeric() {
            count += 1;
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            while i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'\'') {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                if j == i + 1 {
                    break;
                }
                i = j;
            }
        } else {
            i += 1;
        }
    }
    count
}

pub(crate) fn skip_ws(text: &str, mut i: usize) -> usize {
    while let Some(c) = char_at(text, i) {
        if !c.is_whitespace() {
            break;
        }
        i = next_index(text, i);
    }
    i
}

pub(crate) fn is_marker_suffix(c: char) -> bool {
    matches!(c, '.' | ')' | '、')
}

fn is_bullet_char(c: char) -> bool {
    matches!(c, '-' | '*' | '•' | '‣' | '◦')
}

/// `^\s*(?:\d{1,4}|[A-Za-z])\s*[\.)、]\s+\S+` `.match` — an ordered list marker
/// line (1-4 digits or a single ASCII letter, a `.)、` suffix, then `\s+\S`).
pub(crate) fn ordered_marker_match(line: &str) -> bool {
    let start = skip_ws(line, 0);
    // Digit form, trying 4..1 digit prefix lengths (Python regex backtracks).
    let mut i = start;
    let mut digits = 0usize;
    while digits < 4 {
        match char_at(line, i) {
            Some(c) if is_decimal_digit(c) => {
                i = next_index(line, i);
                digits += 1;
            }
            _ => break,
        }
    }
    while digits >= 1 {
        let j = skip_ws(line, i);
        if let Some(suffix) = char_at(line, j) {
            if is_marker_suffix(suffix) {
                let after = next_index(line, j);
                let k = skip_ws(line, after);
                if k > after && char_at(line, k).is_some() {
                    return true;
                }
            }
        }
        // Backtrack one digit.
        let mut pos = start;
        let mut consumed = 0usize;
        while consumed < digits - 1 {
            pos = next_index(line, pos);
            consumed += 1;
        }
        i = pos;
        digits -= 1;
    }
    // Letter form.
    if let Some(c) = char_at(line, start) {
        if c.is_ascii_alphabetic() {
            let mut j = next_index(line, start);
            j = skip_ws(line, j);
            if let Some(suffix) = char_at(line, j) {
                if is_marker_suffix(suffix) {
                    let after = next_index(line, j);
                    let k = skip_ws(line, after);
                    if k > after && char_at(line, k).is_some() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// `^\s*(?:[-*•‣◦])\s+\S+` `.match` — a bullet marker line.
pub(crate) fn bullet_marker_match(line: &str) -> bool {
    let start = skip_ws(line, 0);
    let Some(c) = char_at(line, start) else {
        return false;
    };
    if !is_bullet_char(c) {
        return false;
    }
    let after = next_index(line, start);
    let k = skip_ws(line, after);
    k > after && char_at(line, k).is_some()
}

/// `_line_marked_ratio`: fraction of lines starting with an ordered or bullet
/// marker.
fn line_marked_ratio(lines: &[String]) -> f64 {
    if lines.is_empty() {
        return 0.0;
    }
    let count = lines
        .iter()
        .filter(|line| ordered_marker_match(line) || bullet_marker_match(line))
        .count();
    count as f64 / lines.len() as f64
}

/// `line_text`: a str line is whitespace-collapsed; a dict line uses its `text`
/// or the concatenation of its `spans`/`segments` content.
pub fn line_text(line: &Value) -> String {
    match line {
        Value::String(s) => collapse_ws(s),
        Value::Object(_) => {
            let explicit = line.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if !explicit.trim().is_empty() {
                return collapse_ws(explicit.trim());
            }
            let spans = line
                .get("spans")
                .or_else(|| line.get("segments"))
                .and_then(|v| v.as_array());
            let mut chunks: Vec<String> = Vec::new();
            if let Some(spans) = spans {
                for span in spans {
                    if !span.is_object() {
                        continue;
                    }
                    let content = ["content", "text"]
                        .iter()
                        .find_map(|k| span.get(*k).and_then(|v| v.as_str()))
                        .unwrap_or("")
                        .trim();
                    if !content.is_empty() {
                        chunks.push(content.to_string());
                    }
                }
            }
            collapse_ws(&chunks.join(" "))
        }
        _ => String::new(),
    }
}

/// `line_texts_from_lines`: per-line `line_text`, dropping empty results.
pub fn line_texts_from_lines(lines: &Value) -> Vec<String> {
    let Some(lines) = lines.as_array() else {
        return vec![];
    };
    lines
        .iter()
        .filter_map(|line| {
            let text = line_text(line);
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        })
        .collect()
}

fn line_bboxes(lines: &Value) -> Vec<[f64; 4]> {
    let Some(lines) = lines.as_array() else {
        return vec![];
    };
    let mut out: Vec<[f64; 4]> = Vec::new();
    for line in lines {
        let Some(bbox) = line.get("bbox") else {
            continue;
        };
        let Some(arr) = bbox.as_array() else {
            continue;
        };
        if arr.len() != 4 {
            continue;
        }
        let mut vals = [0.0f64; 4];
        let mut ok = true;
        for (i, v) in arr.iter().enumerate() {
            match v.as_f64() {
                Some(f) => vals[i] = f,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            out.push(vals);
        }
    }
    out
}

/// `line_geometry_is_regular`: >=4 lines with a consistent left edge and pitch
/// (median-height-scaled tolerances).
pub fn line_geometry_is_regular(lines: &Value) -> bool {
    let bboxes = line_bboxes(lines);
    if bboxes.len() < 4 {
        return false;
    }
    let heights: Vec<f64> = bboxes.iter().map(|b| (b[3] - b[1]).max(0.0)).collect();
    let tops: Vec<f64> = bboxes.iter().map(|b| b[1]).collect();
    let pitches: Vec<f64> = tops
        .windows(2)
        .filter_map(|w| if w[1] > w[0] { Some(w[1] - w[0]) } else { None })
        .collect();
    if heights.is_empty() || pitches.is_empty() {
        return false;
    }
    let mut sorted_heights = heights.clone();
    sorted_heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut sorted_pitches = pitches.clone();
    sorted_pitches.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_height = sorted_heights[heights.len() / 2];
    let median_pitch = sorted_pitches[pitches.len() / 2];
    if median_height <= 0.0 || median_pitch <= 0.0 {
        return false;
    }
    let aligned_left = bboxes
        .iter()
        .map(|b| (b[0] - bboxes[0][0]).abs())
        .fold(0.0f64, f64::max)
        <= (3.0f64).max(median_height * 0.4);
    let regular_pitch = pitches
        .iter()
        .map(|p| (p - median_pitch).abs())
        .fold(0.0f64, f64::max)
        <= (3.0f64).max(median_pitch * 0.18);
    aligned_left && regular_pitch
}

/// `looks_like_preserved_line_flow`: many marked explicit lines, or >=6 source
/// lines with regular geometry and few sentence/soft ends and short words.
pub fn looks_like_preserved_line_flow(text: &str, lines: &Value) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let explicit_lines: Vec<String> = py_splitlines(text)
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if explicit_lines.len() >= 3 && line_marked_ratio(&explicit_lines) >= 0.6 {
        return true;
    }
    let line_texts = line_texts_from_lines(lines);
    if line_texts.len() < 6 {
        return false;
    }
    if !line_geometry_is_regular(lines) {
        return false;
    }
    let sentence_end_count = line_texts[..line_texts.len() - 1]
        .iter()
        .filter(|line| ends_with_sentence_end(line))
        .count();
    let soft_end_count = line_texts[..line_texts.len() - 1]
        .iter()
        .filter(|line| ends_with_soft_continuation(line))
        .count();
    let avg_words = line_texts.iter().map(|line| word_count(line) as f64).sum::<f64>()
        / line_texts.len().max(1) as f64;
    sentence_end_count <= (line_texts.len() / 8).max(1)
        && soft_end_count <= (line_texts.len() / 4).max(3)
        && avg_words <= 9.5
}

/// `looks_like_structured_line_flow`: >=2 explicit lines or source line texts,
/// at least 3 total, with a marked majority or preserved-line flow.
pub fn looks_like_structured_line_flow(text: &str, lines: &Value) -> bool {
    let explicit_lines: Vec<String> = py_splitlines(text)
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let line_texts = if explicit_lines.len() >= 2 {
        explicit_lines
    } else {
        line_texts_from_lines(lines)
    };
    if line_texts.len() < 3 {
        return false;
    }
    if line_marked_ratio(&line_texts) >= 0.6 {
        return true;
    }
    looks_like_preserved_line_flow(text, lines)
}

/// `classify_text_flow`.
pub fn classify_text_flow(text: &str, lines: &Value) -> &'static str {
    if looks_like_structured_line_flow(text, lines) {
        TEXT_FLOW_PRESERVE_LINES
    } else {
        TEXT_FLOW_FLOW
    }
}

/// `classify_text_flow_for_role`.
pub fn classify_text_flow_for_role(
    text: &str,
    lines: &Value,
    semantic_role: &str,
    structure_role: &str,
) -> &'static str {
    let role = semantic_role.trim().to_lowercase();
    let structure = structure_role.trim().to_lowercase();
    if structure == "table_of_contents" {
        return TEXT_FLOW_PRESERVE_LINES;
    }
    if (role == "body" || role == "abstract") && !looks_like_structured_line_flow(text, lines) {
        return TEXT_FLOW_FLOW;
    }
    let explicit_lines: Vec<String> = py_splitlines(text)
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if explicit_lines.len() >= 3 {
        return TEXT_FLOW_PRESERVE_LINES;
    }
    classify_text_flow(text, lines)
}

/// Python `str.splitlines()` over `\r\n`, `\r`, `\n` boundaries.
pub fn py_splitlines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            out.push(std::mem::take(&mut current));
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
        } else if c == '\n' {
            out.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    out.push(current);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collapse_and_line_text() {
        assert_eq!(collapse_ws("a\n b   c"), "a b c");
        assert_eq!(line_text(&json!("  a\n b  ")), "a b");
        assert_eq!(
            line_text(&json!({"spans": [{"content": "  A"}, {"text": "B "}]})),
            "A B"
        );
        assert_eq!(line_text(&json!({"text": "  x  y "})), "x y");
        assert_eq!(line_text(&json!(42)), "");
    }

    #[test]
    fn sentence_and_soft_ends() {
        assert!(ends_with_sentence_end("第一句。"));
        assert!(ends_with_sentence_end("yes! "));
        assert!(!ends_with_sentence_end("no comma"));
        assert!(ends_with_soft_continuation("and"));
        assert!(ends_with_soft_continuation("with the "));
        assert!(!ends_with_soft_continuation("stand"));
        assert!(!ends_with_soft_continuation("中文and"));
        assert!(ends_with_soft_continuation("列表，"));
        assert!(ends_with_soft_continuation("foo :"));
    }

    #[test]
    fn word_count_matches() {
        assert_eq!(word_count("foo bar-baz qux's"), 3);
        assert_eq!(word_count("中文 abc １２３"), 1);
    }

    #[test]
    fn markers() {
        assert!(ordered_marker_match("1. 条目"));
        assert!(ordered_marker_match("  12) 条目"));
        assert!(ordered_marker_match("b、 条目"));
        assert!(!ordered_marker_match("1.2. 条目"));
        assert!(!ordered_marker_match("12345. 条目"));
        assert!(!ordered_marker_match("普通段落文字"));
        assert!(bullet_marker_match("- 条目"));
        assert!(bullet_marker_match("• 条目"));
        assert!(!bullet_marker_match("-条目"));
    }

    #[test]
    fn line_geometry_regular() {
        let lines = json!([
            {"bbox": [0.0, 0.0, 100.0, 20.0]},
            {"bbox": [0.0, 30.0, 100.0, 50.0]},
            {"bbox": [0.5, 60.0, 100.0, 80.0]},
            {"bbox": [0.0, 90.0, 100.0, 110.0]},
        ]);
        assert!(line_geometry_is_regular(&lines));
    }

    #[test]
    fn flow_classification() {
        let lines = json!([]);
        assert_eq!(classify_text_flow("", &lines), TEXT_FLOW_FLOW);
        // Two marked lines -> not enough for structured flow (needs 3).
        assert_eq!(classify_text_flow("1. 甲\n2. 乙", &lines), TEXT_FLOW_FLOW);
        // Three marked lines -> structured.
        assert_eq!(
            classify_text_flow("1. 甲\n2. 乙\n3. 丙", &lines),
            TEXT_FLOW_PRESERVE_LINES
        );
    }
}
