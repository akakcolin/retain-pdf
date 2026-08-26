//! Text matching for redaction (Phase 7R-2 shim): `extract_page_text_spans`
//! (span aggregation from the MuPDF structured-text page, matching fitz
//! `get_text("dict")` at the span level) plus the safe-direct path of
//! `source/cleanup/text_matching.py`. The source-text / block / word fallbacks
//! land in 7R-3; for now `item_removable_text_rects` returns `[]` after a
//! safe-direct miss, which drives deterministic whole-bbox covers.

use mupdf::pdf::PdfDocument;
use mupdf::text_page::TextBlockType;
use mupdf::Error;
use mupdf::{Page, Rect, TextPageFlags};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::config::{SAFE_DIRECT_REDACTION_IOU_THRESHOLD, SAFE_DIRECT_REDACTION_SIZE_TOLERANCE};
use super::dto::RedactionItem;
use super::redaction_padding::expand_word_rect;
use super::text_ownership::owned_text_block_entries;

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

/// `text_matching.py::item_removable_text_rects` — Phase 7R-2 shim: safe-direct
/// hit only; the source-text/block/word fallbacks land in 7R-3.
pub fn item_removable_text_rects(
    page: &Page,
    item: &RedactionItem,
    rect: &RectTuple,
    special_math_rects: Option<&[RectTuple]>,
    competing_rects: Option<&[RectTuple]>,
) -> Result<Vec<RectTuple>, Error> {
    let matched = safe_direct_redaction_rect(page, item, rect, competing_rects)?;
    match matched {
        Some(r) => Ok(filter_rects_away_from_special_math(&[r], special_math_rects)),
        None => Ok(Vec::new()),
    }
}

/// Helper bound used by the auto/visual_cover executors (never used in 7R-2
/// corpus pages, but keeps `PdfDocument` import meaningful for future routes).
#[allow(dead_code)]
fn _assert_pdf_document(_doc: &PdfDocument) {}

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
        // rect0 overlaps math by 25 > 0.5 -> dropped; rect1 overlaps by 25 > 0.5 -> dropped
        assert_eq!(filter_rects_away_from_special_math(&rects, Some(&math)), Vec::<RectTuple>::new());
        let math2 = [rect(9.9, 9.9, 20.2, 20.2)];
        // rect0 overlap = 0.01*0.01 = 0.0001 <= 0.5 kept; rect1 overlap = 0.2*0.2=0.04 kept
        assert_eq!(filter_rects_away_from_special_math(&rects, Some(&math2)), rects.to_vec());
        assert_eq!(filter_rects_away_from_special_math(&rects, None), rects.to_vec());
    }
}
