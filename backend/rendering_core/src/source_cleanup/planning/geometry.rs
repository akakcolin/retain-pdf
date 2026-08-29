//! Port of `planning/geometry.py` — formula guard rects and bbox→pdf-coordinate
//! resolution.

use crate::rect::{Matrix, Rect};
use crate::source_cleanup::planning::coordinate_resolver::raw_bbox_rect;
use crate::source_cleanup::planning::segments::bbox_text_strip_formula_guard_rect;
use crate::source_cleanup::planning::segments::split_rect_around_guards;

/// `ocr_bbox_to_pdf_rect_with_ctm` — bbox transformed by the inverse page ctm.
pub fn ocr_bbox_to_pdf_rect_with_ctm(inverse_ctm: &Matrix, bbox: &[f64]) -> Option<Rect> {
    let rect = raw_bbox_rect(bbox)?;
    let pdf_rect = rect.transformed(inverse_ctm);
    if pdf_rect.is_empty() {
        None
    } else {
        Some(pdf_rect)
    }
}

/// `formula_guard_rects` — guarded formulas (non-empty only).
pub fn formula_guard_rects(formula_rects: &[Rect], _strip_rects: Option<&[Rect]>) -> Vec<Rect> {
    formula_rects
        .iter()
        .filter(|rect| !rect.is_empty())
        .map(bbox_text_strip_formula_guard_rect)
        .collect()
}

/// `split_rect_away_from_formulas` — split a rect around formula guards.
pub fn split_rect_away_from_formulas(rect: &Rect, formula_rects: &[Rect]) -> Vec<Rect> {
    let guards: Vec<Rect> = formula_rects.iter().map(bbox_text_strip_formula_guard_rect).collect();
    split_rect_around_guards(rect, &guards, 1.0, 1.0, 2.0)
}

/// `shrink_rect_away_from_formulas` — largest surviving segment, or empty.
pub fn shrink_rect_away_from_formulas(rect: &Rect, formula_rects: &[Rect]) -> Rect {
    let protected_segments = split_rect_away_from_formulas(rect, formula_rects);
    match protected_segments.len() {
        0 => Rect::empty(),
        1 => protected_segments[0],
        _ => protected_segments
            .iter()
            .max_by(|a, b| {
                let area_a = a.width() * a.height();
                let area_b = b.width() * b.height();
                area_a.partial_cmp(&area_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .unwrap_or_else(Rect::empty),
    }
}
