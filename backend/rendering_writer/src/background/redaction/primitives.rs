//! Redaction primitives, port of `source/text_redaction.py` +
//! `source/cleanup/item_rects.py` + `source/rects.py` merge logic.

use mupdf::pdf::{PdfPage, PdfRedactImageMethod, PdfRedactLineArtMethod, PdfRedactOptions, PdfRedactTextMethod};
use mupdf::{Error, Rect};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::config::{
    RECT_MERGE_GAP_X_PT, RECT_MERGE_MAX_AREA_GROWTH_RATIO, RECT_MERGE_MAX_VERTICAL_MISALIGN_PT,
    RECT_MERGE_MIN_OVERLAP_RATIO,
};
use super::dto::{build_cleanup_item_plan, ValidRedactionItem};
use super::text_matching::{rect_area, rects_overlap_area};

/// `text_redaction.py::remove_text_under_rects_with_pymupdf_redaction` —
/// redaction annotations over every non-empty rect, then apply with fitz's
/// `images=NONE, graphics=NONE, text=REMOVE`.
pub fn remove_text_under_rects(
    edit_page: &mut PdfPage,
    rects: &[RectTuple],
) -> Result<(), Error> {
    if rects.is_empty() {
        return Ok(());
    }
    for rect in rects {
        if rect[2] <= rect[0] || rect[3] <= rect[1] {
            continue;
        }
        edit_page.add_redact_annotation(Rect::new(
            rect[0] as f32,
            rect[1] as f32,
            rect[2] as f32,
            rect[3] as f32,
        ))?;
    }
    edit_page.apply_redactions_with_options(PdfRedactOptions {
        black_boxes: false,
        image_method: PdfRedactImageMethod::None,
        line_art: PdfRedactLineArtMethod::None,
        text: PdfRedactTextMethod::Remove,
    })?;
    Ok(())
}

/// `item_rects.py::cover_rects_from_valid_items` — merge the mergeable rects
/// (all but `_formula_guard_fragment`) and append the protected fragments
/// unmerged.
pub fn cover_rects_from_valid_items(valid_items: &[ValidRedactionItem]) -> Vec<RectTuple> {
    let mut mergeable: Vec<RectTuple> = Vec::new();
    let mut protected_fragments: Vec<RectTuple> = Vec::new();
    for entry in valid_items {
        if entry.item.formula_guard_fragment {
            protected_fragments.push(entry.rect);
        } else {
            mergeable.push(entry.rect);
        }
    }
    let mut out = merge_rects(&mergeable);
    out.extend(protected_fragments);
    out
}

/// `item_rects.py::text_removal_rects_from_valid_items`.
pub fn text_removal_rects_from_valid_items(valid_items: &[ValidRedactionItem]) -> Vec<RectTuple> {
    valid_items
        .iter()
        .filter(|entry| build_cleanup_item_plan(&entry.item).bbox_text_strip_allowed)
        .map(|entry| entry.rect)
        .collect()
}

/// `rects.py::rect_key` — rounded integer tuple key.
pub fn rect_key(rect: &RectTuple) -> (i64, i64, i64, i64) {
    (
        (rect[0] * 10.0).round() as i64,
        (rect[1] * 10.0).round() as i64,
        (rect[2] * 10.0).round() as i64,
        (rect[3] * 10.0).round() as i64,
    )
}

/// Python `round(value, 2)` — round half to even, for positive values.
fn round_half_even(value: f64) -> f64 {
    let scaled = value * 100.0;
    let floor = scaled.floor();
    let diff = scaled - floor;
    let rounded = if diff < 0.5 {
        floor
    } else if diff > 0.5 {
        floor + 1.0
    } else if floor % 2.0 == 0.0 {
        floor
    } else {
        floor + 1.0
    };
    rounded / 100.0
}

fn rects_should_merge(left: &RectTuple, right: &RectTuple) -> bool {
    let combined_area = rect_area(left) + rect_area(right);
    if combined_area <= 0.0 {
        return false;
    }
    let union_area = rect_area(&rect_union(left, right));
    if union_area / combined_area > RECT_MERGE_MAX_AREA_GROWTH_RATIO {
        return false;
    }
    let same_row = (left[1] - right[1]).abs() <= RECT_MERGE_MAX_VERTICAL_MISALIGN_PT
        && (left[3] - right[3]).abs() <= RECT_MERGE_MAX_VERTICAL_MISALIGN_PT;
    let inter_area = rects_overlap_area(left, right);
    if inter_area > 0.0 {
        let min_area = (rect_area(left).min(rect_area(right))).max(1.0);
        let overlap_ratio = inter_area / min_area;
        return same_row || overlap_ratio >= RECT_MERGE_MIN_OVERLAP_RATIO;
    }
    let horizontal_gap = (left[0].max(right[0]) - left[2].min(right[2])).max(0.0);
    same_row && horizontal_gap <= RECT_MERGE_GAP_X_PT
}

fn rect_union(a: &RectTuple, b: &RectTuple) -> RectTuple {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn merge_sort_key(rect: &RectTuple) -> (i64, i64, i64) {
    (
        (round_half_even(rect[1]) * 100.0).round() as i64,
        (round_half_even(rect[0]) * 100.0).round() as i64,
        (round_half_even(rect[3]) * 100.0).round() as i64,
    )
}

/// `rects.py::merge_rects` — sort, fold in rects_should_merge merges, re-sort.
pub fn merge_rects(rects: &[RectTuple]) -> Vec<RectTuple> {
    let mut ordered: Vec<RectTuple> = rects.to_vec();
    ordered.sort_by_key(merge_sort_key);
    let mut merged: Vec<RectTuple> = Vec::new();
    for rect in ordered {
        let mut current = rect;
        let mut changed = true;
        while changed {
            changed = false;
            let mut kept: Vec<RectTuple> = Vec::new();
            for existing in merged {
                if rects_should_merge(&existing, &current) {
                    current = rect_union(&existing, &current);
                    changed = true;
                } else {
                    kept.push(existing);
                }
            }
            merged = kept;
        }
        merged.push(current);
    }
    merged.sort_by_key(merge_sort_key);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> RectTuple {
        [x0, y0, x1, y1]
    }

    #[test]
    fn merge_close_same_row() {
        // Two rects 2pt apart on the same row -> merged.
        let merged = merge_rects(&[rect(0.0, 0.0, 50.0, 12.0), rect(52.0, 0.0, 100.0, 12.0)]);
        assert_eq!(merged, vec![rect(0.0, 0.0, 100.0, 12.0)]);
    }

    #[test]
    fn merge_overlapping() {
        // Same-row rects with horizontal overlap merge (area growth 0.9 <= 2.4).
        let merged = merge_rects(&[rect(0.0, 0.0, 50.0, 50.0), rect(40.0, 0.0, 90.0, 50.0)]);
        assert_eq!(merged, vec![rect(0.0, 0.0, 90.0, 50.0)]);
    }

    #[test]
    fn merge_keeps_far_rects() {
        let merged = merge_rects(&[rect(0.0, 0.0, 50.0, 12.0), rect(200.0, 0.0, 260.0, 12.0)]);
        assert_eq!(merged, vec![rect(0.0, 0.0, 50.0, 12.0), rect(200.0, 0.0, 260.0, 12.0)]);
    }

    #[test]
    fn merge_dedups_identical() {
        let merged = merge_rects(&[rect(0.0, 0.0, 50.0, 12.0), rect(0.0, 0.0, 50.0, 12.0)]);
        assert_eq!(merged, vec![rect(0.0, 0.0, 50.0, 12.0)]);
    }

    #[test]
    fn round_half_even_matches_python() {
        // Python round(1.005, 2) == 1.0 and round(1.015, 2) == 1.01 (both are
        // the binary-represented values just under the tie, rounded down).
        assert_eq!(round_half_even(0.0), 0.0);
        assert_eq!(round_half_even(1.005), 1.0);
        assert_eq!(round_half_even(1.015), 1.01);
        assert_eq!(round_half_even(50.0), 50.0);
    }
}
