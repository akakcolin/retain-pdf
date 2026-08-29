// Port of services/rendering/layout/payload/text_common.py (pure subset).
// The `get_render_*` re-exports live in `crate::layout::render_text`.

use crate::item::Item;
use crate::semantics::is_plain_bodylike_block;
use crate::text::analysis::{analyze_text, tokenize_text};

pub const WORD_RE: &str = r"[A-Za-z0-9]+(?:[-'][A-Za-z0-9]+)*";
pub const ZH_CHAR_RE: &str = r"[\u4e00-\u9fff]";
pub const SPLIT_PUNCTUATION: [&str; 12] = [
    ".", "。", "!", "！", "?", "？", ";", "；", ":", "：", ",", "，",
];
pub const COMPACT_TRIGGER_RATIO: f64 = 0.9;
pub const COMPACT_SCALE: f64 = 0.9;
pub const HEAVY_COMPACT_RATIO: f64 = 1.0;
pub const LAYOUT_COMPACT_TRIGGER_RATIO: f64 = 0.9;
pub const LAYOUT_HEAVY_COMPACT_RATIO: f64 = 1.04;

pub fn tokenize_protected_text(text: &str) -> Vec<String> {
    tokenize_text(text)
}

pub fn strip_formula_placeholders(text: &str) -> String {
    crate::text::analysis::strip_formula_tokens(text, " ")
}

pub fn normalize_render_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn same_meaningful_render_text(source_text: &str, translated_text: &str) -> bool {
    normalize_render_text(source_text) == normalize_render_text(translated_text)
}

pub fn source_word_count(item: &Item) -> usize {
    let source_text = first_source_text(item);
    analyze_text(&source_text).word_count()
}

/// `render_source_text or protected_source_text or source_text`.
fn first_source_text(item: &Item) -> String {
    if !item.render_source_text.is_empty() {
        item.render_source_text.clone()
    } else if !item.protected_source_text.is_empty() {
        item.protected_source_text.clone()
    } else {
        item.source_text.clone()
    }
}

/// `build_plain_text(item)`: `(translated_text or source_text)` then plain-text
/// normalization.
pub fn build_plain_text(item: &Item) -> String {
    let text = if !item.translated_text.is_empty() {
        &item.translated_text
    } else {
        &item.source_text
    };
    build_plain_text_from_text(text)
}

/// `build_plain_text_from_text`: split on `\n`, collapse horizontal whitespace
/// runs to a single space per line, drop empty lines, rejoin with `\n`.
pub fn build_plain_text_from_text(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.trim().split('\n') {
        let collapsed = collapse_horizontal_whitespace(line);
        let stripped = collapsed.trim();
        if !stripped.is_empty() {
            lines.push(stripped.to_string());
        }
    }
    lines.join("\n")
}

fn collapse_horizontal_whitespace(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut pending_space = false;
    for ch in line.chars() {
        if matches!(ch, ' ' | '\t' | '\r' | '\x0c' | '\x0b') {
            pending_space = true;
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
    }
    if pending_space {
        out.push(' ');
    }
    out
}

/// `is_flag_like_plain_text_block`: a single-line `-item` block that is neither
/// body-like nor formula-heavy nor prose-long enough to be a real sentence.
pub fn is_flag_like_plain_text_block(item: &Item) -> bool {
    let plain = build_plain_text(item);
    let text = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return false;
    }
    if !item.formula_map.is_empty() {
        return false;
    }
    if is_plain_bodylike_block(item) {
        return false;
    }
    if item.lines.len() > 1 {
        return false;
    }
    if !text.starts_with('-') {
        return false;
    }
    let body = text[1..].trim();
    if body.is_empty() {
        return false;
    }
    for mark in [".", "。", "!", "！", "?", "？", ";", "；"] {
        if body.contains(mark) {
            return false;
        }
    }
    if body.chars().count() > 32 {
        return false;
    }
    if analyze_text(body).word_count() > 6 {
        return false;
    }
    if analyze_text(body).zh_char_count() > 18 {
        return false;
    }
    true
}

pub fn translated_zh_char_count(protected_text: &str) -> usize {
    analyze_text(protected_text).zh_char_count()
}

pub fn translation_density_ratio(item: &Item, protected_text: &str) -> f64 {
    let source_words = source_word_count(item);
    if source_words == 0 {
        return 0.0;
    }
    let zh_chars = translated_zh_char_count(protected_text);
    if zh_chars == 0 {
        return 0.0;
    }
    zh_chars as f64 / source_words as f64
}

pub fn layout_density_ratio(
    inner: &[f64],
    protected_text: &str,
    font_size_pt: f64,
    line_step_pt: f64,
) -> f64 {
    if inner.len() != 4 || font_size_pt <= 0.0 || line_step_pt <= 0.0 {
        return 0.0;
    }
    let width = (inner[2] - inner[0]).max(8.0);
    let height = (inner[3] - inner[1]).max(8.0);
    let zh_chars = translated_zh_char_count(protected_text);
    if zh_chars == 0 {
        return 0.0;
    }
    let approx_char_width = (font_size_pt * 0.92).max(1.0);
    let chars_per_line = (width / approx_char_width).max(4.0);
    let required_lines = (zh_chars as f64 / chars_per_line).max(1.0);
    let occupied_height = required_lines * line_step_pt;
    occupied_height / height
}

pub fn trim_joined_tokens(tokens: &[String]) -> String {
    tokens.concat().trim().to_string()
}
