// Port of services/rendering/layout/payload/text_common.py (pure subset).
// `is_flag_like_plain_text_block` and the `get_render_*` re-exports depend on
// markdown/render_text and are not ported (Phase 1 does not need them).

use crate::item::Item;
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
    analyze_text(&item.source_text).word_count()
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
