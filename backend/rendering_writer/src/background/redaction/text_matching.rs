//! Text matching for redaction, port of `source/cleanup/text_matching.py` plus
//! the span/block/word extractors it uses (`text_extract.py`), the ownership
//! rules (`text_ownership.py`), the redaction-rect builders (`text_rects.py`),
//! and the word normalizer (`source/items.py::normalize_words`).
//!
//! Phase 7R-3 is the full matcher: safe-direct first, then the block layer
//! (`_matched_text_block_rects`), then the word layer (word entries, owned
//! words, `_word_overlap_passes`), then the whole-bbox fallback. The
//! caller-supplied `page_words` cache is always `None` here — extraction reads
//! the live page, matching the corpus path.

use std::collections::HashSet;

use mupdf::text_page::TextBlockType;
use mupdf::{Error, Page, Rect, TextExtractOptions, TextPageFlags};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::config::{SAFE_DIRECT_REDACTION_IOU_THRESHOLD, SAFE_DIRECT_REDACTION_SIZE_TOLERANCE};
use super::dto::RedactionItem;
use super::primitives::rect_key;
use super::redaction_padding::{expand_item_rect, expand_word_rect};
use super::text_ownership::{owned_text_block_entries, owned_word_entries};

/// `rects.py::rect_area` — non-negative area of `[x0, y0, x1, y1]`.
pub fn rect_area(r: &RectTuple) -> f64 {
    (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0)
}

/// `rects.py::rects_overlap_area` — area of the intersection, `0.0` when empty.
pub fn rects_overlap_area(a: &RectTuple, b: &RectTuple) -> f64 {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = a[2].min(b[2]);
    let y1 = a[3].min(b[3]);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    (x1 - x0) * (y1 - y0)
}

/// `text_extract.py::rect_contains_point` — inclusive bounds test.
pub fn rect_contains_point(rect: &RectTuple, x: f64, y: f64) -> bool {
    rect[0] <= x && x <= rect[2] && rect[1] <= y && y <= rect[3]
}

/// `text_extract.py::rect_center`.
pub fn rect_center(rect: &RectTuple) -> (f64, f64) {
    ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0)
}

/// `rects.py::clip_rect` — `rect` inflated by 1pt (the word-extraction clip).
pub fn clip_rect(rect: &RectTuple) -> RectTuple {
    [rect[0] - 1.0, rect[1] - 1.0, rect[2] + 1.0, rect[3] + 1.0]
}

/// `text_extract.py::extract_page_text_spans` — span rects + stripped text from
/// the structured-text page. Spans are grouped per line by (font name, size),
/// which matches fitz `get_text("dict")` for the single-font/single-size corpus
/// pages; color/flag grouping is a documented divergence (irrelevant here).
pub fn extract_page_text_spans(page: &Page) -> Result<Vec<(RectTuple, String)>, Error> {
    let text_page = page.to_text_page(TextPageFlags::empty())?;
    let mut spans: Vec<(RectTuple, String)> = Vec::new();
    for block in text_page.blocks() {
        if block.r#type() != TextBlockType::Text {
            continue;
        }
        for line in block.lines() {
            let mut current: Option<(String, f32, Vec<Rect>)> = None;
            let mut current_text = String::new();
            for ch in line.chars() {
                let font_name = ch.font().map(|f| f.name().to_string()).unwrap_or_default();
                let size = ch.size();
                let rect = Rect::from(ch.quad());
                match &mut current {
                    Some((name, sz, rects)) if *name == font_name && (*sz - size).abs() < 1e-6 => {
                        rects.push(rect);
                        if let Some(c) = ch.char() {
                            current_text.push(c);
                        }
                    }
                    _ => {
                        flush_span(&mut spans, &mut current, &mut current_text);
                        current = Some((font_name, size, vec![rect]));
                        if let Some(c) = ch.char() {
                            current_text.push(c);
                        }
                    }
                }
            }
            flush_span(&mut spans, &mut current, &mut current_text);
        }
    }
    Ok(spans)
}

fn flush_span(
    spans: &mut Vec<(RectTuple, String)>,
    current: &mut Option<(String, f32, Vec<Rect>)>,
    current_text: &mut String,
) {
    if let Some((_, _, rects)) = current.take() {
        let text = current_text.trim().to_string();
        if !rects.is_empty() && !text.is_empty() {
            let mut rect = Rect::new(f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
            for r in &rects {
                rect.x0 = rect.x0.min(r.x0);
                rect.y0 = rect.y0.min(r.y0);
                rect.x1 = rect.x1.max(r.x1);
                rect.y1 = rect.y1.max(r.y1);
            }
            if !rect.is_empty() {
                spans.push(([rect.x0 as f64, rect.y0 as f64, rect.x1 as f64, rect.y1 as f64], text));
            }
        }
        current_text.clear();
    }
}

/// `text_extract.py::extract_page_text_blocks` — text blocks as (bbox, text),
/// matching fitz `get_text("blocks")` (lines joined by newlines, trimmed).
pub fn extract_page_text_blocks(page: &Page) -> Result<Vec<(RectTuple, String)>, Error> {
    let text_page = page.to_text_page(TextPageFlags::empty())?;
    let mut blocks: Vec<(RectTuple, String)> = Vec::new();
    for block in text_page.blocks() {
        if block.r#type() != TextBlockType::Text {
            continue;
        }
        let bounds = block.bounds();
        if bounds.is_empty() {
            continue;
        }
        let mut text = String::new();
        for line in block.lines() {
            if !text.is_empty() {
                text.push('\n');
            }
            for ch in line.chars() {
                if let Some(c) = ch.char() {
                    text.push(c);
                }
            }
        }
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        blocks.push((
            [bounds.x0 as f64, bounds.y0 as f64, bounds.x1 as f64, bounds.y1 as f64],
            text,
        ));
    }
    Ok(blocks)
}

/// `text_extract.py::extract_item_word_entries` — words whose bbox overlaps the
/// inflated item clip, lowercased. `page_words` is always `None` (live page).
pub fn extract_item_word_entries(page: &Page, rect: &RectTuple) -> Result<Vec<(RectTuple, String)>, Error> {
    let clip = clip_rect(rect);
    let options = TextExtractOptions {
        flags: TextPageFlags::PRESERVE_LIGATURES,
    };
    let words = page.words(options)?;
    let mut entries: Vec<(RectTuple, String)> = Vec::new();
    for w in words {
        let candidate = [w.bounds.x0 as f64, w.bounds.y0 as f64, w.bounds.x1 as f64, w.bounds.y1 as f64];
        if rects_overlap_area(&clip, &candidate) <= 0.0 {
            continue;
        }
        let token = w.text.trim().to_lowercase();
        if token.is_empty() {
            continue;
        }
        entries.push((candidate, token));
    }
    Ok(entries)
}

/// `text_safe_direct.py::rect_iou`.
pub fn rect_iou(a: &RectTuple, b: &RectTuple) -> f64 {
    let inter = rects_overlap_area(a, b);
    if inter <= 0.0 {
        return 0.0;
    }
    let union = rect_area(a) + rect_area(b) - inter;
    if union <= 0.0 {
        return 0.0;
    }
    inter / union
}

/// `text_safe_direct.py::rect_center_contains` — center of `target` inside `rect`.
pub fn rect_center_contains(rect: &RectTuple, target: &RectTuple) -> bool {
    let (cx, cy) = rect_center(target);
    rect_contains_point(rect, cx, cy)
}

/// `text_safe_direct.py::relative_size_error`.
pub fn relative_size_error(expected: f64, actual: f64) -> f64 {
    let baseline = expected.max(1.0);
    (actual - expected).abs() / baseline
}

/// `text_safe_direct.py::safe_direct_redaction_rect` — the single-span path.
/// `_item` mirrors production's unused `item` argument.
pub fn safe_direct_redaction_rect(
    page: &Page,
    _item: &RedactionItem,
    rect: &RectTuple,
    competing_rects: Option<&[RectTuple]>,
) -> Result<Option<RectTuple>, Error> {
    if rect[2] <= rect[0] || rect[3] <= rect[1] {
        return Ok(None);
    }
    let raw_bbox = *rect;
    let span_entries = extract_page_text_spans(page)?;
    if span_entries.is_empty() {
        return Ok(None);
    }
    let owned_spans = owned_text_block_entries(&raw_bbox, &span_entries, competing_rects);
    if owned_spans.is_empty() {
        return Ok(None);
    }
    let mut matched: Vec<RectTuple> = Vec::new();
    for (span_rect, _span_text) in owned_spans {
        if !rect_center_contains(&span_rect, &raw_bbox) {
            continue;
        }
        let width_error = relative_size_error(raw_bbox[2] - raw_bbox[0], span_rect[2] - span_rect[0]);
        let height_error = relative_size_error(raw_bbox[3] - raw_bbox[1], span_rect[3] - span_rect[1]);
        let iou = rect_iou(&raw_bbox, &span_rect);
        if width_error > SAFE_DIRECT_REDACTION_SIZE_TOLERANCE {
            continue;
        }
        if height_error > SAFE_DIRECT_REDACTION_SIZE_TOLERANCE {
            continue;
        }
        if iou < SAFE_DIRECT_REDACTION_IOU_THRESHOLD {
            continue;
        }
        matched.push(span_rect);
    }
    if matched.len() != 1 {
        return Ok(None);
    }
    Ok(Some(expand_word_rect(&matched[0])))
}

/// `text_math_guard.py::filter_rects_away_from_special_math`.
pub fn filter_rects_away_from_special_math(
    rects: &[RectTuple],
    special_math_rects: Option<&[RectTuple]>,
) -> Vec<RectTuple> {
    if rects.is_empty() {
        return Vec::new();
    }
    let Some(math) = special_math_rects else {
        return rects.to_vec();
    };
    if math.is_empty() {
        return rects.to_vec();
    }
    rects
        .iter()
        .copied()
        .filter(|r| !math.iter().any(|m| rects_overlap_area(r, m) > 0.5))
        .collect()
}

/// `text_rects.py::word_entries_to_redaction_rects` — expanded word rects,
/// deduped by `rect_key`.
pub fn word_entries_to_redaction_rects(entries: &[(RectTuple, String)]) -> Vec<RectTuple> {
    let mut rects: Vec<RectTuple> = Vec::new();
    let mut seen: HashSet<(i64, i64, i64, i64)> = HashSet::new();
    for (word_rect, _token) in entries {
        let expanded = expand_word_rect(word_rect);
        if !seen.insert(rect_key(&expanded)) {
            continue;
        }
        rects.push(expanded);
    }
    rects
}

/// `text_rects.py::item_bbox_redaction_rect` — the whole item bbox expanded.
pub fn item_bbox_redaction_rect(rect: &RectTuple) -> Vec<RectTuple> {
    let expanded = expand_item_rect(rect);
    if expanded[2] <= expanded[0] || expanded[3] <= expanded[1] {
        Vec::new()
    } else {
        vec![expanded]
    }
}

/// `items.py::normalize_words` — `WORD_RE`-equivalent tokenizer (ASCII word runs
/// with `-./` separators plus CJK runs), lowercased. No regex crate needed.
pub fn normalize_words(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut words: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if is_cjk(c) {
            let mut j = i + 1;
            while j < n && is_cjk(chars[j]) {
                j += 1;
            }
            words.push(chars[i..j].iter().collect::<String>().to_lowercase());
            i = j;
        } else if c.is_ascii_alphanumeric() {
            let mut j = i + 1;
            while j < n && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            loop {
                if j < n && is_sep(chars[j]) {
                    let mut w = j + 1;
                    while w < n && chars[w].is_ascii_alphanumeric() {
                        w += 1;
                    }
                    if w > j + 1 {
                        j = w;
                        continue;
                    }
                }
                break;
            }
            words.push(chars[i..j].iter().collect::<String>().to_lowercase());
            i = j;
        } else {
            i += 1;
        }
    }
    words
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn is_sep(c: char) -> bool {
    matches!(c, '-' | '.' | '/')
}

/// `text_matching.py::_word_overlap_passes`.
fn word_overlap_passes(source_words: &[String], candidate_words: &[String]) -> bool {
    let source_set: HashSet<&str> = source_words.iter().map(|s| s.as_str()).collect();
    let candidate_set: HashSet<&str> = candidate_words.iter().map(|s| s.as_str()).collect();
    let overlap = candidate_set.iter().filter(|w| source_set.contains(*w)).count();
    if source_set.is_empty() {
        return candidate_words.len() >= 2;
    }
    let source_len = source_words.len();
    if source_len <= 3 {
        return overlap >= 1;
    }
    if source_len <= 8 {
        return overlap >= 2;
    }
    overlap >= std::cmp::max(2, (source_len as f64 * 0.3) as usize)
}

/// `text_matching.py::_matched_text_block_rects` — owned blocks whose normalized
/// words overlap the source words, expanded and deduped.
fn matched_text_block_rects(
    page: &Page,
    rect: &RectTuple,
    source_words: &[String],
    competing_rects: Option<&[RectTuple]>,
) -> Result<Vec<RectTuple>, Error> {
    let block_entries = extract_page_text_blocks(page)?;
    if block_entries.is_empty() {
        return Ok(Vec::new());
    }
    let owned = owned_text_block_entries(rect, &block_entries, competing_rects);
    let mut matched: Vec<RectTuple> = Vec::new();
    let mut seen: HashSet<(i64, i64, i64, i64)> = HashSet::new();
    for (block_rect, block_text) in owned {
        let block_words = normalize_words(&block_text);
        if block_words.is_empty() || !word_overlap_passes(source_words, &block_words) {
            continue;
        }
        let expanded = expand_word_rect(&block_rect);
        if !seen.insert(rect_key(&expanded)) {
            continue;
        }
        matched.push(expanded);
    }
    Ok(matched)
}

/// `text_matching.py::item_removable_text_rects` — full Phase 7R-3 matcher:
/// safe-direct, then block, then word, then whole-bbox fallback.
#[allow(clippy::too_many_arguments)]
pub fn item_removable_text_rects(
    page: &Page,
    item: &RedactionItem,
    rect: &RectTuple,
    special_math_rects: Option<&[RectTuple]>,
    competing_rects: Option<&[RectTuple]>,
) -> Result<Vec<RectTuple>, Error> {
    let matched = safe_direct_redaction_rect(page, item, rect, competing_rects)?;
    if let Some(r) = matched {
        return Ok(filter_rects_away_from_special_math(&[r], special_math_rects));
    }

    let source_text = source_text_for(item);
    if source_text.is_empty() {
        return Ok(Vec::new());
    }

    let source_words = normalize_words(&source_text);
    let block_rects = matched_text_block_rects(page, rect, &source_words, competing_rects)?;
    if !block_rects.is_empty() {
        let filtered = filter_rects_away_from_special_math(&block_rects, special_math_rects);
        if !filtered.is_empty() {
            return Ok(filtered);
        }
    }

    let word_entries = extract_item_word_entries(page, rect)?;
    if word_entries.is_empty() {
        return Ok(Vec::new());
    }
    let owned = owned_word_entries(rect, &word_entries, competing_rects);
    if owned.is_empty() {
        return Ok(Vec::new());
    }

    let mut pdf_words: Vec<String> = Vec::new();
    for (_word_rect, token) in &owned {
        pdf_words.extend(normalize_words(token));
    }
    if pdf_words.is_empty() {
        return Ok(Vec::new());
    }

    if source_words.is_empty() {
        if pdf_words.len() < 2 {
            return Ok(Vec::new());
        }
        return Ok(filter_rects_away_from_special_math(
            &item_bbox_redaction_rect(rect),
            special_math_rects,
        ));
    }

    if !word_overlap_passes(&source_words, &pdf_words) {
        return Ok(Vec::new());
    }
    Ok(filter_rects_away_from_special_math(
        &word_entries_to_redaction_rects(&owned),
        special_math_rects,
    ))
}

/// `item.get("source_text") or item.get("protected_source_text") or ""` then
/// `.strip()`.
fn source_text_for(item: &RedactionItem) -> String {
    let raw = if !item.source_text.is_empty() {
        item.source_text.as_str()
    } else {
        item.protected_source_text.as_str()
    };
    raw.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> RectTuple {
        [x0, y0, x1, y1]
    }

    #[test]
    fn rect_geometry_matches_python() {
        assert_eq!(rect_area(&rect(0.0, 0.0, 10.0, 20.0)), 200.0);
        assert_eq!(rect_area(&rect(10.0, 10.0, 5.0, 5.0)), 0.0);
        assert_eq!(rects_overlap_area(&rect(0.0, 0.0, 10.0, 10.0), &rect(5.0, 5.0, 15.0, 15.0)), 25.0);
        assert_eq!(rects_overlap_area(&rect(0.0, 0.0, 10.0, 10.0), &rect(20.0, 20.0, 30.0, 30.0)), 0.0);
        assert!(rect_contains_point(&rect(0.0, 0.0, 10.0, 10.0), 5.0, 5.0));
        assert!(rect_contains_point(&rect(0.0, 0.0, 10.0, 10.0), 10.0, 10.0));
        assert!(!rect_contains_point(&rect(0.0, 0.0, 10.0, 10.0), 10.1, 10.0));
    }

    #[test]
    fn rect_iou_matches_python() {
        assert_eq!(rect_iou(&rect(0.0, 0.0, 10.0, 10.0), &rect(5.0, 5.0, 15.0, 15.0)), 25.0 / 175.0);
        assert_eq!(rect_iou(&rect(0.0, 0.0, 10.0, 10.0), &rect(20.0, 20.0, 30.0, 30.0)), 0.0);
        assert_eq!(rect_iou(&rect(0.0, 0.0, 10.0, 10.0), &rect(0.0, 0.0, 10.0, 10.0)), 1.0);
    }

    #[test]
    fn relative_size_error_matches_python() {
        assert_eq!(relative_size_error(50.0, 52.0), 2.0 / 50.0);
        assert!((relative_size_error(0.5, 0.6) - 0.1).abs() < 1e-12);
    }

    #[test]
    fn filter_math_removes_overlaps_only() {
        let rects = [rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 20.0, 30.0, 30.0)];
        let math = [rect(5.0, 5.0, 25.0, 25.0)];
        assert_eq!(filter_rects_away_from_special_math(&rects, Some(&math)), Vec::<RectTuple>::new());
        let math2 = [rect(9.9, 9.9, 20.2, 20.2)];
        assert_eq!(filter_rects_away_from_special_math(&rects, Some(&math2)), rects.to_vec());
        assert_eq!(filter_rects_away_from_special_math(&rects, None), rects.to_vec());
    }

    #[test]
    fn normalize_words_matches_python() {
        assert_eq!(normalize_words("The quick-brown 123.45 fox"), ["the", "quick-brown", "123.45", "fox"]);
        assert_eq!(normalize_words("foo--bar baz-"), ["foo", "bar", "baz"]);
        assert_eq!(normalize_words("中文hello世界"), ["中文", "hello", "世界"]);
        assert_eq!(normalize_words("a/b.c-d"), ["a/b.c-d"]);
        assert_eq!(normalize_words(""), Vec::<String>::new());
        assert_eq!(normalize_words("!!!---"), Vec::<String>::new());
    }

    #[test]
    fn word_overlap_passes_matches_python() {
        let source = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // source_len 2 <= 3: needs overlap >= 1
        assert!(word_overlap_passes(&source(&["beta"]), &source(&["alpha", "beta", "gamma"])));
        assert!(!word_overlap_passes(&source(&["zzzz", "qqq"]), &source(&["alpha", "beta", "gamma"])));
        // empty source: needs candidate >= 2 words
        assert!(word_overlap_passes(&source(&[]), &source(&["a", "b"])));
        assert!(!word_overlap_passes(&source(&[]), &source(&["a"])));
        // source_len 4-8: needs overlap >= 2
        assert!(word_overlap_passes(
            &source(&["a", "b", "c", "d"]),
            &source(&["a", "b", "x", "y"])
        ));
        assert!(!word_overlap_passes(
            &source(&["a", "b", "c", "d"]),
            &source(&["a", "x", "y", "z"])
        ));
    }
}
