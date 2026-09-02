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

/// True for a "narrow" content glyph: ASCII letter/digit, Hiragana, Katakana
/// (full + halfwidth), or Hangul (syllables + Jamo). CJK ideographs
/// (U+4E00–U+9FFF) are intentionally excluded — they are counted as `ZhChar`.
fn is_narrow_content_glyph(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || ('\u{3040}'..='\u{309f}').contains(&c)
        || ('\u{30a0}'..='\u{30ff}').contains(&c)
        || ('\u{ff66}'..='\u{ff9f}').contains(&c)
        || ('\u{ac00}'..='\u{d7af}').contains(&c)
        || ('\u{1100}'..='\u{11ff}').contains(&c)
        || ('\u{3130}'..='\u{318f}').contains(&c)
}

/// Number of narrow content glyphs (latin/Hiragana/Katakana/Hangul) after
/// stripping formula placeholders/math, so `__FORMULA_1__` / `<f1-abc/>` tokens
/// do not inflate the count. Only consulted when the text has zero CJK
/// ideographs (`zh_char_count == 0`).
pub fn translated_narrow_glyph_count(protected_text: &str) -> usize {
    strip_formula_placeholders(protected_text)
        .chars()
        .filter(|&c| is_narrow_content_glyph(c))
        .count()
}

/// Glyph mass for density accounting: CJK ideographs count 1.0 unit each,
/// narrow glyphs (latin/kana/Hangul) count 0.5 each. When any ideograph is
/// present the value is exactly the ideograph count (narrow glyphs ignored) so
/// the golden zh/kanji outputs stay bit-identical to the zh-only arithmetic.
pub fn content_glyph_units(protected_text: &str) -> f64 {
    let zh_chars = translated_zh_char_count(protected_text);
    if zh_chars > 0 {
        zh_chars as f64
    } else {
        translated_narrow_glyph_count(protected_text) as f64 * 0.5
    }
}

pub fn translation_density_ratio(item: &Item, protected_text: &str) -> f64 {
    let source_words = source_word_count(item);
    if source_words == 0 {
        return 0.0;
    }
    let units = content_glyph_units(protected_text);
    if units <= 0.0 {
        return 0.0;
    }
    units / source_words as f64
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
    if zh_chars > 0 {
        // EXACT legacy zh arithmetic — golden zh/kanji parity must be preserved.
        occupied_height_ratio(
            zh_chars as f64,
            width,
            height,
            (font_size_pt * 0.92).max(1.0),
            line_step_pt,
        )
    } else {
        let narrow_chars = translated_narrow_glyph_count(protected_text);
        if narrow_chars == 0 {
            return 0.0;
        }
        // Narrow glyphs advance ~half an ideograph (~0.46em), so roughly twice
        // as many fit per line as the zh model assumes per em.
        occupied_height_ratio(
            narrow_chars as f64,
            width,
            height,
            (font_size_pt * 0.46).max(1.0),
            line_step_pt,
        )
    }
}

/// `occupied_height / height` for `glyph_count` glyphs at the given advance
/// width per line. Shared by the zh and narrow branches of `layout_density_ratio`.
fn occupied_height_ratio(
    glyph_count: f64,
    width: f64,
    height: f64,
    approx_char_width: f64,
    line_step_pt: f64,
) -> f64 {
    let chars_per_line = (width / approx_char_width).max(4.0);
    let required_lines = (glyph_count / chars_per_line).max(1.0);
    let occupied_height = required_lines * line_step_pt;
    occupied_height / height
}

pub fn trim_joined_tokens(tokens: &[String]) -> String {
    tokens.concat().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::Item;

    fn latin_source_item() -> Item {
        // 13 Word tokens ("a"..="m").
        Item {
            source_text: "a b c d e f g h i j k l m".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn english_translation_density_is_positive() {
        let item = latin_source_item();
        let text = "This English translation is substantially longer than the source sentence";
        assert_eq!(translated_zh_char_count(text), 0);
        let density = translation_density_ratio(&item, text);
        assert!(density > 0.0);
        let units = translated_narrow_glyph_count(text) as f64 * 0.5;
        assert_eq!(density, units / source_word_count(&item) as f64);
    }

    #[test]
    fn hangul_translation_density_is_positive() {
        let item = latin_source_item();
        let text = "이것은 한국어 번역 문장입니다";
        assert_eq!(translated_zh_char_count(text), 0);
        assert!(translation_density_ratio(&item, text) > 0.0);
    }

    #[test]
    fn kana_only_translation_density_is_positive() {
        let item = latin_source_item();
        let text = "これはひらがなとカタカナだけのテキストです";
        assert_eq!(translated_zh_char_count(text), 0);
        assert!(translation_density_ratio(&item, text) > 0.0);
    }

    #[test]
    fn zh_density_matches_legacy_formula_exactly() {
        let item = latin_source_item();
        let text = "Hello 世界 これはテスト kanji 漢字 mixed text";
        assert!(translated_zh_char_count(text) > 0);
        let legacy = translated_zh_char_count(text) as f64 / source_word_count(&item) as f64;
        assert_eq!(translation_density_ratio(&item, text), legacy);
    }

    #[test]
    fn layout_density_zh_matches_legacy_formula_exactly() {
        let inner: Vec<f64> = vec![10.0, 20.0, 210.0, 120.0];
        let font: f64 = 12.0;
        let line_step: f64 = 16.0;
        let text = "你好世界 这是一段中文 Latin mixed 123 内容";
        let zh = translated_zh_char_count(text);
        assert!(zh > 0);
        let width = (inner[2] - inner[0]).max(8.0);
        let height = (inner[3] - inner[1]).max(8.0);
        let approx = (font * 0.92).max(1.0);
        let cpl = (width / approx).max(4.0);
        let legacy = ((zh as f64 / cpl).max(1.0) * line_step) / height;
        assert_eq!(layout_density_ratio(&inner, text, font, line_step), legacy);
    }

    #[test]
    fn layout_density_narrow_is_positive_when_overflowing() {
        let inner = vec![0.0, 0.0, 60.0, 30.0];
        let text = "This is a fairly long English paragraph that should overflow a small box";
        assert_eq!(translated_zh_char_count(text), 0);
        let density = layout_density_ratio(&inner, text, 12.0, 14.0);
        assert!(density > 0.0);
    }

    #[test]
    fn layout_density_hangul_is_positive_when_overflowing() {
        let inner = vec![0.0, 0.0, 40.0, 20.0];
        let text = "이것은 짧은 상자에 넘칠 만큼 긴 한국어 문장입니다";
        assert_eq!(translated_zh_char_count(text), 0);
        assert!(layout_density_ratio(&inner, text, 11.0, 13.0) > 0.0);
    }

    #[test]
    fn narrow_count_ignores_formula_placeholders() {
        assert_eq!(translated_narrow_glyph_count("ab __FORMULA_1__ cd"), 4);
        assert_eq!(translated_narrow_glyph_count("__FORMULA_1__"), 0);
        assert_eq!(translated_narrow_glyph_count("a <f1-abc/> b"), 2);
        assert_eq!(translated_narrow_glyph_count("[[FORMULA_12]] $x_1$"), 0);
    }

    #[test]
    fn content_glyph_units_ignores_narrow_when_zh_present() {
        assert_eq!(content_glyph_units("a 世界 b 漢字"), 4.0);
        assert!(content_glyph_units("only latin and kana カナ") > 0.0);
        assert_eq!(content_glyph_units(""), 0.0);
    }
}
