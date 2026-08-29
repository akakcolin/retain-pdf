// Port of services/rendering/layout/payload/line_structure.py — the
// `seed_render_fields` boundary (structured-line detection, source-line
// splitting, `maybe_preserve_structured_line_breaks`, `source_line_texts`) plus
// the emit/seed paths already present (`fit_preserved_line_block_metrics`,
// `preserved_line_boxes_for_item`).

use serde_json::{json, Value};

use crate::item::Item;
use crate::text::tokens::RAW_MATH_TOKEN_KINDS;
use crate::text_flow;
use crate::util::py_round;

pub const PRESERVED_LINE_LEADING_CANDIDATES: [f64; 6] = [0.12, 0.16, 0.2, 0.24, 0.28, 0.32];
pub const PRESERVED_LINE_HEIGHT_FILL: f64 = 0.96;
pub const PRESERVED_LINE_MIN_FONT_PT: f64 = 7.2;
pub const PRESERVED_LINE_IDEAL_LEADING: f64 = 0.22;
pub const PRESERVED_LINE_IDEAL_FONT_PT: f64 = 10.6;
pub const CAPTION_PRESERVE_LINE_MAX_UNITS: f64 = 52.0;
pub const CAPTION_PRESERVE_LINE_MAX_CHARS: usize = 80;

/// `source_line_texts`: the explicit `source_line_texts` list (stripped, empty
/// dropped) when present, else `line_texts_from_lines(item["lines"])`.
pub fn source_line_texts(item: &Value) -> Vec<String> {
    if let Some(arr) = item.get("source_line_texts").and_then(|v| v.as_array()) {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
    }
    text_flow::line_texts_from_lines(item.get("lines").unwrap_or(&json!([])))
}

fn string_or_layout_role(item: &Value) -> String {
    let semantic = item.get("semantic_role").and_then(|v| v.as_str()).unwrap_or("");
    if !semantic.trim().is_empty() {
        semantic.trim().to_string()
    } else {
        item.get("layout_role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    }
}

fn protected_source_text_or_source(item: &Value) -> String {
    let protected = item.get("protected_source_text").and_then(|v| v.as_str()).unwrap_or("");
    if !protected.is_empty() {
        protected.to_string()
    } else {
        item.get("source_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }
}

fn has_line_contract(item: &Value) -> bool {
    source_line_texts(item).len() >= 2
}

fn is_caption_like_block(item: &Value) -> bool {
    crate::semantics::is_caption_like_block(&Item::from_json_value(item))
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn is_punctuation(c: char) -> bool {
    "，。！？；：、,.!?;:()[]{}<>《》“”‘’\"'".contains(c)
}

/// Python `[A-Za-z0-9]+(?:[-'][A-Za-z0-9]+)*` fullmatch.
fn is_word_token(token: &str) -> bool {
    let bytes = token.as_bytes();
    let mut i = 0usize;
    if bytes.first().map_or(false, |b| !b.is_ascii_alphanumeric()) {
        return false;
    }
    while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
        i += 1;
    }
    loop {
        if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'\'') {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j == i + 1 {
                return false;
            }
            i = j;
        } else {
            break;
        }
    }
    i == bytes.len()
}

/// Python `[\u4e00-\u9fff]|[A-Za-z0-9]+(?:[-'][A-Za-z0-9]+)*|[^\S\r\n]+|.`
/// `.findall`. `\n` matches nothing (Python `.` excludes it and `[^\S\r\n]`
/// excludes it); `\r` matches as a lone char.
fn token_re_findall(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = text[i..].chars().next().unwrap();
        let cw = c.len_utf8();
        if is_cjk(c) {
            out.push(c.to_string());
            i += cw;
        } else if c.is_ascii_alphanumeric() {
            let start = i;
            let mut j = i + cw;
            while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            loop {
                if j < bytes.len() && (bytes[j] == b'-' || bytes[j] == b'\'') {
                    let mut k = j + 1;
                    while k < bytes.len() && bytes[k].is_ascii_alphanumeric() {
                        k += 1;
                    }
                    if k > j + 1 {
                        j = k;
                        continue;
                    }
                }
                break;
            }
            out.push(text[start..j].to_string());
            i = j;
        } else if c.is_whitespace() && c != '\r' && c != '\n' {
            let start = i;
            let mut j = i + cw;
            while j < bytes.len() {
                let nc = text[j..].chars().next().unwrap();
                if nc.is_whitespace() && nc != '\r' && nc != '\n' {
                    j += nc.len_utf8();
                } else {
                    break;
                }
            }
            out.push(text[start..j].to_string());
            i = j;
        } else if c == '\n' {
            i += cw;
        } else {
            out.push(c.to_string());
            i += cw;
        }
    }
    out
}

/// `_token_units`.
fn token_units(token: &str) -> f64 {
    if token.is_empty() {
        return 0.0;
    }
    let char_count = token.chars().count();
    if token.chars().all(|c| c.is_whitespace()) {
        return 0.12f64.max(char_count as f64 * 0.18);
    }
    if char_count == 1 && is_cjk(token.chars().next().unwrap()) {
        return 1.0;
    }
    if is_word_token(token) {
        return 0.8f64.max(char_count as f64 * 0.55);
    }
    if char_count == 1 && is_punctuation(token.chars().next().unwrap()) {
        return 0.5;
    }
    0.55
}

/// `_text_units`.
fn text_units(text: &str) -> f64 {
    token_re_findall(text).iter().map(|t| token_units(t)).sum()
}

fn has_long_caption_line(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        let text = line.trim();
        text.chars().count() > CAPTION_PRESERVE_LINE_MAX_CHARS
            || text_units(text) > CAPTION_PRESERVE_LINE_MAX_UNITS
    })
}

/// `_should_disable_caption_preserve_lines(item, translated_lines)`: caption
/// blocks with a long line lose their line structure.
fn should_disable_caption_preserve_lines(item: &Value, lines: &[String]) -> bool {
    is_caption_like_block(item) && has_long_caption_line(lines)
}

fn glossary_term_line_matches(line: &str) -> bool {
    let start = text_flow::skip_ws(line, 0);
    let bytes = line.as_bytes();
    if start >= bytes.len() {
        return false;
    }
    let first = line[start..].chars().next().unwrap();
    if !first.is_ascii_uppercase() {
        return false;
    }
    let i = start + first.len_utf8();
    // `[A-Z][A-Z0-9-]{1,12}` alternative, then `\s+\S`.
    let mut j = i;
    let mut n = 0usize;
    while j < bytes.len() && n < 12 {
        let c = bytes[j] as char;
        if c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-' {
            j += 1;
            n += 1;
        } else {
            break;
        }
    }
    if n >= 1 && has_ws_then_nonws(line, j) {
        return true;
    }
    // `[A-Z]\d{1,3}[A-Z]*` alternative.
    let mut k = i;
    let mut nd = 0usize;
    while k < bytes.len() && nd < 3 && (bytes[k] as char).is_ascii_digit() {
        k += 1;
        nd += 1;
    }
    if nd >= 1 {
        while k < bytes.len() && (bytes[k] as char).is_ascii_uppercase() {
            k += 1;
        }
        if has_ws_then_nonws(line, k) {
            return true;
        }
    }
    false
}

/// `\s+\S` from byte index `i` (at least one whitespace then a non-ws char).
fn has_ws_then_nonws(line: &str, i: usize) -> bool {
    let k = text_flow::skip_ws(line, i);
    if k <= i {
        return false;
    }
    text_flow::char_at(line, k).is_some()
}

/// `_glossary_lines_are_short_entries`.
fn glossary_lines_are_short_entries(lines: &[String]) -> bool {
    lines.iter().all(|line| {
        line.chars().count() <= 88
            && !text_flow::ends_with_sentence_end(line)
            && text_flow::word_count(line) <= 9
            && glossary_term_line_matches(line)
    })
}

/// `BODY_LIST_BLOCK_RE.fullmatch` over the `\n`-joined trimmed lines: every
/// line starts with an ordered or bullet marker followed by content.
fn body_list_block_matches(expanded: &str) -> bool {
    let lines: Vec<&str> = expanded.split('\n').collect();
    if lines.len() < 2 {
        return false;
    }
    lines
        .iter()
        .all(|line| text_flow::ordered_marker_match(line) || text_flow::bullet_marker_match(line))
}

/// `BODY_GLOSSARY_BLOCK_RE.fullmatch`: every line is a glossary term line.
fn body_glossary_block_matches(expanded: &str) -> bool {
    let lines: Vec<&str> = expanded.split('\n').collect();
    if lines.len() < 2 {
        return false;
    }
    lines.iter().all(|line| glossary_term_line_matches(line))
}

/// `_body_lines_match_preserve_whitelist`.
fn body_lines_match_preserve_whitelist(lines: &[String]) -> bool {
    let materialized: Vec<String> = lines
        .iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if materialized.len() < 2 {
        return false;
    }
    let expanded = materialized.join("\n");
    if body_list_block_matches(&expanded) {
        return true;
    }
    if body_glossary_block_matches(&expanded) {
        return glossary_lines_are_short_entries(&materialized);
    }
    false
}

/// `looks_like_structured_line_block(item, lines)`: explicit preserve_lines
/// contract, body/abstract gating, then `classify_text_flow` on the source.
pub fn looks_like_structured_line_block(item: &Value, lines: Option<&[String]>) -> bool {
    let semantic_role = string_or_layout_role(item).to_lowercase();
    let structure_role = item
        .get("structure_role")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let explicit_preserve = item
        .get("text_flow")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase()
        == text_flow::TEXT_FLOW_PRESERVE_LINES;
    if explicit_preserve && has_line_contract(item) {
        if (semantic_role == "body" || semantic_role == "abstract")
            && structure_role != "table_of_contents"
        {
            let candidate = match lines {
                Some(l) => l.to_vec(),
                None => source_line_texts(item),
            };
            return body_lines_match_preserve_whitelist(&candidate);
        }
        return true;
    }
    if structure_role != "table_of_contents" && (semantic_role == "body" || semantic_role == "abstract")
    {
        return false;
    }
    let source_text = protected_source_text_or_source(item);
    let raw_lines = item.get("lines").cloned().unwrap_or_else(|| json!([]));
    text_flow::classify_text_flow(&source_text, &raw_lines) == text_flow::TEXT_FLOW_PRESERVE_LINES
}

/// `_clean_line`: `re.sub(r"\s+", " ", "".join(tokens)).strip()`.
fn clean_line(tokens: &[String]) -> String {
    text_flow::collapse_ws(&tokens.concat())
}

/// `_line_split_tokens`: raw-math token values kept verbatim, other tokens
/// re-split by `TOKEN_RE`.
fn line_split_tokens(text: &str) -> Vec<String> {
    let analysis = crate::text::analysis::analyze_text(text);
    let mut out: Vec<String> = Vec::new();
    for token in &analysis.tokens {
        if RAW_MATH_TOKEN_KINDS.contains(&token.kind) {
            out.push(token.value.clone());
        } else {
            out.extend(token_re_findall(&token.value));
        }
    }
    out.retain(|t| !t.is_empty());
    out
}

/// `^\s*((?:\d{1,4}|[A-Za-z])\s*[\.)、])\s+` — extract the (whitespace-stripped)
/// ordered marker at the line start.
fn extract_marker(line: &str) -> Option<String> {
    let start = text_flow::skip_ws(line, 0);
    let mut i = start;
    let mut digits = 0usize;
    while digits < 4 {
        match text_flow::char_at(line, i) {
            Some(c) if text_flow::is_decimal_digit(c) => {
                i = text_flow::next_index(line, i);
                digits += 1;
            }
            _ => break,
        }
    }
    while digits >= 1 {
        let j = text_flow::skip_ws(line, i);
        if let Some(suffix) = text_flow::char_at(line, j) {
            if text_flow::is_marker_suffix(suffix) {
                let after = text_flow::next_index(line, j);
                let k = text_flow::skip_ws(line, after);
                if k > after {
                    let end = text_flow::next_index(line, j);
                    let marker: String = line[start..end]
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    return Some(marker);
                }
            }
        }
        let mut pos = start;
        let mut consumed = 0usize;
        while consumed < digits - 1 {
            pos = text_flow::next_index(line, pos);
            consumed += 1;
        }
        i = pos;
        digits -= 1;
    }
    if let Some(c) = text_flow::char_at(line, start) {
        if c.is_ascii_alphabetic() {
            let mut j = text_flow::next_index(line, start);
            j = text_flow::skip_ws(line, j);
            if let Some(suffix) = text_flow::char_at(line, j) {
                if text_flow::is_marker_suffix(suffix) {
                    let after = text_flow::next_index(line, j);
                    let k = text_flow::skip_ws(line, after);
                    if k > after {
                        let end = text_flow::next_index(line, j);
                        let marker: String = line[start..end]
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect();
                        return Some(marker);
                    }
                }
            }
        }
    }
    None
}

/// `(?<!\S)` + whitespace-tolerant marker chars + `\s+` — first match position.
fn find_marker_position(text: &str, marker: &str) -> Option<usize> {
    let mchars: Vec<char> = marker.chars().collect();
    let mut i = 0usize;
    while i < text.len() {
        if i > 0 {
            let prev = text[..i].chars().next_back().unwrap();
            if !prev.is_whitespace() {
                i = text_flow::next_index(text, i);
                continue;
            }
        }
        let Some(c0) = text_flow::char_at(text, i) else {
            break;
        };
        if c0 != mchars[0] {
            i = text_flow::next_index(text, i);
            continue;
        }
        let mut j = i + c0.len_utf8();
        let mut matched = true;
        for &mc in &mchars[1..] {
            while j < text.len() {
                let c = text[j..].chars().next().unwrap();
                if c.is_whitespace() {
                    j += c.len_utf8();
                } else {
                    break;
                }
            }
            if text_flow::char_at(text, j) != Some(mc) {
                matched = false;
                break;
            }
            j += mc.len_utf8();
        }
        if !matched {
            i = text_flow::next_index(text, i);
            continue;
        }
        // `\s+` after the last marker char.
        if j >= text.len() {
            i = text_flow::next_index(text, i);
            continue;
        }
        if !text[j..].chars().next().unwrap().is_whitespace() {
            i = text_flow::next_index(text, i);
            continue;
        }
        return Some(i);
    }
    None
}

/// `_ordered_line_markers_are_increasing`: all markers share one kind and the
/// values strictly increase.
fn ordered_line_markers_are_increasing(markers: &[String]) -> bool {
    let mut parsed: Vec<(String, i64)> = Vec::new();
    for marker in markers {
        let chars: Vec<char> = marker.chars().collect();
        if chars.len() < 2 {
            return false;
        }
        let suffix = chars[chars.len() - 1];
        if !text_flow::is_marker_suffix(suffix) {
            return false;
        }
        let body: String = chars[..chars.len() - 1].iter().collect();
        let body_chars: Vec<char> = body.chars().collect();
        let digit_count = body_chars.iter().filter(|c| text_flow::is_decimal_digit(**c)).count();
        if digit_count == body_chars.len() && digit_count >= 1 && digit_count <= 4 {
            let value = body_chars.iter().fold(0i64, |acc, c| acc * 10 + text_flow::decimal_digit_value(*c));
            parsed.push((format!("number:{suffix}"), value));
        } else if body_chars.len() == 1 {
            let c = body_chars[0];
            if c.is_ascii_alphabetic() {
                let kind = format!(
                    "alpha:{suffix}:{}",
                    if c.is_ascii_uppercase() { "upper" } else { "lower" }
                );
                let value = (c.to_ascii_lowercase() as i64) - ('a' as i64) + 1;
                parsed.push((kind, value));
            } else {
                return false;
            }
        } else {
            return false;
        }
    }
    if parsed.is_empty() {
        return false;
    }
    let kind0 = parsed[0].0.clone();
    if parsed.iter().any(|(k, _)| k != &kind0) {
        return false;
    }
    parsed.windows(2).all(|w| w[1].1 > w[0].1)
}

/// `_split_text_by_source_line_markers`.
fn split_text_by_source_line_markers(
    translated_text: &str,
    source_lines: &[String],
) -> Option<Vec<String>> {
    let materialized: Vec<String> = source_lines
        .iter()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if materialized.len() < 2 {
        return None;
    }
    let mut markers: Vec<String> = Vec::new();
    for line in &materialized {
        match extract_marker(line) {
            Some(m) => markers.push(m),
            None => return None,
        }
    }
    let mut seen: std::collections::HashSet<&String> = std::collections::HashSet::new();
    for marker in &markers {
        if !seen.insert(marker) {
            return None;
        }
    }
    if !ordered_line_markers_are_increasing(&markers) {
        return None;
    }
    let text = translated_text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let mut positions: Vec<usize> = Vec::new();
    for marker in &markers {
        match find_marker_position(&text, marker) {
            Some(p) => positions.push(p),
            None => return None,
        }
    }
    let mut sorted = positions.clone();
    sorted.sort();
    if positions != sorted || positions[0] > 2 {
        return None;
    }
    let mut chunks: Vec<String> = Vec::new();
    for (idx, &start) in positions.iter().enumerate() {
        let end = if idx + 1 < positions.len() {
            positions[idx + 1]
        } else {
            text.len()
        };
        let chunk = text[start..end].trim().to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
    }
    if chunks.len() == markers.len() {
        Some(chunks)
    } else {
        None
    }
}

/// `split_text_by_source_line_weights`: split the translated text into as many
/// chunks as weighted source lines.
pub fn split_text_by_source_line_weights(translated_text: &str, source_lines: &[String]) -> Vec<String> {
    let source_weights: Vec<f64> = source_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| 1.0f64.max(text_units(line)))
        .collect();
    if source_weights.is_empty() {
        let trimmed = translated_text.trim();
        return if trimmed.is_empty() {
            vec![]
        } else {
            vec![trimmed.to_string()]
        };
    }
    if let Some(chunks) = split_text_by_source_line_markers(translated_text, source_lines) {
        return chunks;
    }
    let tokens = line_split_tokens(translated_text);
    if tokens.is_empty() {
        return vec![];
    }
    let token_units: Vec<f64> = tokens.iter().map(|t| token_units(t)).collect();
    let total_units: f64 = token_units.iter().sum();
    if total_units <= 0.0 {
        return vec![clean_line(&tokens)];
    }
    let total_source_weight: f64 = source_weights.iter().sum();
    let mut chunks: Vec<String> = Vec::new();
    let mut start = 0usize;
    let mut running_units = 0.0f64;
    let mut source_running = 0.0f64;
    let mut token_index = 0usize;
    for &sw in &source_weights[..source_weights.len() - 1] {
        source_running += sw;
        let target_units = total_units * (source_running / total_source_weight);
        while token_index < tokens.len() && running_units + token_units[token_index] < target_units {
            running_units += token_units[token_index];
            token_index += 1;
        }
        let mut split_index = (start + 1).max(token_index.min(tokens.len()));
        while split_index < tokens.len() && tokens[split_index].chars().all(|c| c.is_whitespace()) {
            split_index += 1;
        }
        chunks.push(clean_line(&tokens[start..split_index]));
        start = split_index;
    }
    chunks.push(clean_line(&tokens[start..]));
    chunks.retain(|c| !c.is_empty());
    chunks
}

/// `re.sub(r"[ \t]*[\r\n]+[ \t]*", " ", text).strip()`.
fn collapse_newlines(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\r' || bytes[i] == b'\n' {
            while out.ends_with(' ') || out.ends_with('\t') {
                out.pop();
            }
            while i < bytes.len() && (bytes[i] == b'\r' || bytes[i] == b'\n') {
                i += 1;
            }
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            out.push(' ');
        } else {
            let c = text[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out.trim().to_string()
}

/// `maybe_preserve_structured_line_breaks`: join explicit newlines into a single
/// flow, or split a single-line text by the weighted source lines and flag the
/// block for preserved line boxes.
pub fn maybe_preserve_structured_line_breaks(item: &mut Value, translated_text: &str) -> String {
    let text = translated_text.trim().to_string();
    if text.is_empty() {
        return text;
    }
    if text.contains('\n') {
        let text_lines: Vec<String> = text_flow::py_splitlines(&text)
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if should_disable_caption_preserve_lines(item, &text_lines) {
            return collapse_newlines(&text);
        }
        if looks_like_structured_line_block(item, None) {
            item["_render_preserve_line_breaks"] = json!(true);
            item["_render_line_structure"] = json!("structured_lines");
            return text;
        }
        return collapse_newlines(&text);
    }
    let lines = source_line_texts(item);
    if should_disable_caption_preserve_lines(item, &lines) {
        return text;
    }
    if !looks_like_structured_line_block(item, Some(&lines)) {
        return text;
    }
    let chunks = split_text_by_source_line_weights(&text, &lines);
    if chunks.len() < 2 {
        return text;
    }
    item["_render_preserve_line_breaks"] = json!(true);
    item["_render_line_structure"] = json!("structured_lines");
    chunks.join("\n")
}

/// `fit_preserved_line_block_metrics`: pick the leading candidate that best
/// packs `line_count` preserved lines into `inner`, scoring pitch fit, leading
/// preference and font-size deviation.
pub fn fit_preserved_line_block_metrics(
    inner: &[f64],
    protected_text: &str,
    font_size_pt: f64,
    leading_em: f64,
) -> (f64, f64) {
    if inner.len() != 4 {
        return (font_size_pt, leading_em);
    }
    let line_count = protected_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        .max(1);
    if line_count <= 1 {
        return (font_size_pt, leading_em);
    }
    let height = (inner[3] - inner[1]).max(1.0);
    if height <= 0.0 {
        return (font_size_pt, leading_em);
    }

    let mut best: Option<(f64, f64, f64)> = None; // (score, font, leading)
    let source_font_hint = font_size_pt.max(PRESERVED_LINE_IDEAL_FONT_PT);
    for &candidate_leading in &PRESERVED_LINE_LEADING_CANDIDATES {
        let candidate_font = height * PRESERVED_LINE_HEIGHT_FILL
            / (line_count as f64 * (1.0 + candidate_leading)).max(1.0);
        if candidate_font < PRESERVED_LINE_MIN_FONT_PT {
            continue;
        }
        let line_pitch = candidate_font * (1.0 + candidate_leading);
        let target_pitch = height / (line_count as f64).max(1.0);
        let pitch_error = (line_pitch - target_pitch).abs() / target_pitch.max(1.0);
        let leading_error = (candidate_leading - PRESERVED_LINE_IDEAL_LEADING).abs() * 0.9;
        let font_error = (candidate_font - source_font_hint.min(PRESERVED_LINE_IDEAL_FONT_PT + 1.0)).abs() / 18.0;
        let score = pitch_error + leading_error + font_error;
        if best.is_none() || score < best.unwrap().0 {
            best = Some((score, candidate_font, candidate_leading));
        }
    }

    match best {
        Some((_score, font, leading)) => (py_round(font, 2), py_round(leading, 2)),
        None => {
            let fallback_leading = PRESERVED_LINE_LEADING_CANDIDATES[0];
            let fallback_font = height * PRESERVED_LINE_HEIGHT_FILL
                / (line_count as f64 * (1.0 + fallback_leading)).max(1.0);
            (py_round(fallback_font.max(PRESERVED_LINE_MIN_FONT_PT), 2), fallback_leading)
        }
    }
}

/// `preserved_line_boxes_for_item`: zip translated text lines with the source
/// `lines` bboxes into `RenderLineBox` DTOs for preserve-line-break blocks.
/// Any shape mismatch aborts the whole list (matches the Python early-returns).
pub fn preserved_line_boxes_for_item(item: &Value, translated_text: &str) -> Vec<Value> {
    let preserve = item
        .get("_render_preserve_line_breaks")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !preserve {
        return vec![];
    }
    let text_lines: Vec<&str> = translated_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let raw_lines = item.get("lines").and_then(|v| v.as_array());
    let Some(raw_lines) = raw_lines else {
        return vec![];
    };
    if text_lines.is_empty() || raw_lines.len() < text_lines.len() {
        return vec![];
    }
    let mut boxes: Vec<Value> = Vec::new();
    for (text, raw_line) in text_lines.iter().zip(raw_lines.iter()) {
        let bbox_value = raw_line.get("bbox");
        let Some(bbox) = bbox_value.and_then(|v| v.as_array()) else {
            return vec![];
        };
        if bbox.len() != 4 {
            return vec![];
        }
        let mut line_bbox: Vec<f64> = Vec::with_capacity(4);
        for value in bbox {
            match value.as_f64() {
                Some(v) => line_bbox.push(v),
                None => return vec![],
            }
        }
        if line_bbox[2] <= line_bbox[0] || line_bbox[3] <= line_bbox[1] {
            return vec![];
        }
        boxes.push(json!({"text": text, "bbox": line_bbox}));
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn single_line_returns_unchanged() {
        assert_eq!(fit_preserved_line_block_metrics(&[0.0, 0.0, 100.0, 20.0], "single", 10.0, 0.4), (10.0, 0.4));
    }

    #[test]
    fn short_box_forces_fallback() {
        // Three lines in a 24pt-tall box: every candidate font lands below 7.2.
        let (font, leading) = fit_preserved_line_block_metrics(&[0.0, 0.0, 100.0, 24.0], "a\nb\nc", 10.6, 0.4);
        assert_eq!(leading, 0.12);
        assert_eq!(font, 7.2);
    }

    #[test]
    fn preserved_boxes_zip_text_and_raw_lines() {
        let item = json!({
            "_render_preserve_line_breaks": true,
            "lines": [
                {"bbox": [0.0, 0.0, 100.0, 20.0]},
                {"bbox": [0.0, 20.0, 100.0, 40.0]},
            ],
        });
        let boxes = preserved_line_boxes_for_item(&item, "one\ntwo");
        assert_eq!(
            boxes,
            vec![
                json!({"text": "one", "bbox": [0.0, 0.0, 100.0, 20.0]}),
                json!({"text": "two", "bbox": [0.0, 20.0, 100.0, 40.0]}),
            ]
        );
    }

    #[test]
    fn preserved_boxes_require_flag_and_shape() {
        assert_eq!(preserved_line_boxes_for_item(&json!({}), "x"), Vec::<Value>::new());
        let item = json!({"_render_preserve_line_breaks": true, "lines": []});
        assert_eq!(preserved_line_boxes_for_item(&item, "x"), Vec::<Value>::new());
        let item = json!({
            "_render_preserve_line_breaks": true,
            "lines": [{"bbox": [0.0, 0.0, 100.0, 20.0]}],
        });
        // Two translated lines but only one source line -> abort whole list.
        assert_eq!(preserved_line_boxes_for_item(&item, "a\nb"), Vec::<Value>::new());
        // Degenerate bbox aborts the list.
        let item = json!({
            "_render_preserve_line_breaks": true,
            "lines": [{"bbox": [0.0, 0.0, 0.0, 20.0]}],
        });
        assert_eq!(preserved_line_boxes_for_item(&item, "a"), Vec::<Value>::new());
    }

    #[test]
    fn source_line_texts_explicit_and_fallback() {
        let item = json!({"source_line_texts": ["  a  ", "", "b"]});
        assert_eq!(source_line_texts(&item), vec!["a", "b"]);
        let item = json!({"lines": [{"spans": [{"content": "x"}]}, {"text": "  y"}]});
        assert_eq!(source_line_texts(&item), vec!["x", "y"]);
        assert_eq!(source_line_texts(&json!({})), Vec::<String>::new());
    }

    #[test]
    fn token_units_and_text_units() {
        assert_eq!(text_units("第一句，二。"), 5.0); // 4 CJK units + 2 punct
        assert_eq!(text_units("  "), 0.36); // whitespace run
        assert_eq!(text_units("a"), 0.8); // single-char word floors at 0.8
        assert_eq!(text_units("word"), 2.2); // 4-char word at 0.55/char
        assert_eq!(text_units("中文"), 2.0); // two CJK chars, 1.0 each
    }

    #[test]
    fn structured_block_explicit_preserve_contract() {
        let item = json!({"text_flow": "preserve_lines", "source_line_texts": ["a", "b"]});
        assert!(looks_like_structured_line_block(&item, None));
    }

    #[test]
    fn structured_block_body_gating() {
        let item = json!({
            "semantic_role": "body",
            "text_flow": "preserve_lines",
            "source_line_texts": ["1. 甲", "2. 乙"],
        });
        assert!(looks_like_structured_line_block(&item, None));
        // Body block not on the whitelist and not explicitly marked -> gated.
        let item = json!({
            "semantic_role": "body",
            "text_flow": "preserve_lines",
            "source_line_texts": ["普通行一", "普通行二"],
        });
        assert!(!looks_like_structured_line_block(&item, None));
        // toc structure role skips the body gate -> falls through to source classify.
        let item = json!({
            "semantic_role": "body",
            "structure_role": "table_of_contents",
            "source_text": "1. 甲\n2. 乙\n3. 丙",
        });
        assert!(looks_like_structured_line_block(&item, None));
        // Non-toc body block short-circuits before source classify even when structured.
        let item = json!({
            "semantic_role": "body",
            "source_text": "1. 甲\n2. 乙\n3. 丙",
        });
        assert!(!looks_like_structured_line_block(&item, None));
    }

    #[test]
    fn glossary_short_entries_match() {
        let lines = vec!["AB 甲".to_string(), "C2 乙".to_string()];
        assert!(body_lines_match_preserve_whitelist(&lines));
        let lines = vec!["AB 甲".to_string(), "普通句子。" .to_string()];
        assert!(!body_lines_match_preserve_whitelist(&lines));
        assert!(glossary_term_line_matches("AB 甲"));
        assert!(glossary_term_line_matches("C2X 乙"));
        assert!(!glossary_term_line_matches("A 甲"));
    }

    #[test]
    fn caption_long_line_disables_preserve() {
        let item = json!({"semantic_role": "caption", "layout_role": "caption"});
        let long_line = "字".repeat(81);
        assert!(should_disable_caption_preserve_lines(&item, &[long_line]));
        assert!(!should_disable_caption_preserve_lines(&item, &["short".to_string()]));
        assert!(!should_disable_caption_preserve_lines(&json!({}), &["字".repeat(81)]));
    }

    #[test]
    fn split_by_markers_and_weights() {
        let source = vec!["1. 甲".to_string(), "2. 乙".to_string()];
        assert_eq!(
            split_text_by_source_line_weights("1. 甲内容 2. 乙内容", &source),
            vec!["1. 甲内容", "2. 乙内容"]
        );
        // Weights path: no markers, single line split into two weighted chunks.
        let source = vec!["第一段".to_string(), "第二段第二段".to_string()];
        let chunks = split_text_by_source_line_weights("一二三四五六七八九", &source);
        assert_eq!(chunks.len(), 2);
        assert!(!chunks[0].is_empty() && !chunks[1].is_empty());
    }

    #[test]
    fn collapse_newlines_regex() {
        assert_eq!(collapse_newlines("a \n b\n\nc"), "a b c");
        assert_eq!(collapse_newlines(" \t\r\n x"), "x");
        assert_eq!(collapse_newlines("a\r\nb"), "a b");
    }

    #[test]
    fn maybe_preserve_structured_line_breaks_paths() {
        // Empty text returned unchanged.
        assert_eq!(maybe_preserve_structured_line_breaks(&mut json!({}), ""), "");

        // Explicit newlines on a caption-like block with long line -> collapsed.
        let mut item = json!({"layout_role": "caption"});
        let text = "字".repeat(41) + "\n" + &"字".repeat(41);
        let out = maybe_preserve_structured_line_breaks(&mut item, &text);
        assert_eq!(out, text.replace('\n', " "));
        assert_ne!(item.get("_render_preserve_line_breaks"), Some(&json!(true)));

        // Multi-line structured block -> preserved with flag set.
        let mut item = json!({"text_flow": "preserve_lines", "source_line_texts": ["1. 甲", "2. 乙"]});
        let out = maybe_preserve_structured_line_breaks(&mut item, "1. 甲\n2. 乙");
        assert_eq!(out, "1. 甲\n2. 乙");
        assert_eq!(item["_render_preserve_line_breaks"], json!(true));
        assert_eq!(item["_render_line_structure"], json!("structured_lines"));

        // Single-line text with a structured source contract -> split + flagged.
        let mut item = json!({"text_flow": "preserve_lines", "source_line_texts": ["1. 甲", "2. 乙"]});
        let out = maybe_preserve_structured_line_breaks(&mut item, "1. 甲内容 2. 乙内容");
        assert_eq!(out, "1. 甲内容\n2. 乙内容");
        assert_eq!(item["_render_preserve_line_breaks"], json!(true));

        // Single line with no contract -> unchanged.
        let mut item = json!({});
        assert_eq!(maybe_preserve_structured_line_breaks(&mut item, "普通正文"), "普通正文");
        assert_eq!(item.get("_render_preserve_line_breaks"), None);
    }

    #[test]
    fn glossary_term_line_formats() {
        assert!(glossary_term_line_matches("AB-1 内容"));
        assert!(glossary_term_line_matches("X12 内容"));
        assert!(!glossary_term_line_matches("x1 内容"));
        assert!(!glossary_term_line_matches("AB"));
    }
}
