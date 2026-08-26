//! Port of `source/background/detect.py`: identify large background images and
//! tiled background image patterns on a page. mupdf-rs has no `get_image_info`
//! equivalent, so placement rects come from the content-stream CTM at each `Do`
//! (the same walk as `image_compress::max_display_rect_by_xref`). Form-XObject
//! recursion is not performed (consistent with the existing image walk).

use std::collections::HashMap;

use mupdf::pdf::PdfPage;
use mupdf::Error;

use rendering_core::source_cleanup::content_stream::tokenize;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_core::source_cleanup::pdf_math::{transform_rect, Operand};
use rendering_core::source_cleanup::stream_state::ContentStreamState;

use crate::contents::page_contents_bytes;

use super::config::{
    DETECT_PRIMARY_COVERAGE_RATIO, TILED_BACKGROUND_IMAGE_COVERAGE_RATIO,
    TILED_BACKGROUND_IMAGE_MIN_COUNT, TILED_BACKGROUND_IMAGE_MIN_WIDTH_RATIO,
};
use super::patch::{rect_area, rect_intersection, rect_is_empty, rect_union, rect_width};

/// `detect.py::_page_image_infos` equivalent — placement rect per image xref,
/// from the CTM at each `Do` that paints the image (the transformed unit
/// square). Mirrors fitz `get_image_rects` / `get_image_info` bbox.
pub fn image_placement_rects_by_xref(page: &PdfPage) -> Result<HashMap<i32, Vec<RectTuple>>, Error> {
    let images = page.images()?;
    if images.is_empty() {
        return Ok(HashMap::new());
    }
    let name_to_xref: HashMap<String, i32> =
        images.iter().map(|i| (i.name.clone(), i.xref)).collect();
    let stream = page_contents_bytes(page)?;
    let tokens = match tokenize(&stream) {
        Ok(tokens) => tokens,
        Err(_) => return Ok(HashMap::new()),
    };
    let mut state = ContentStreamState::default();
    let mut rects: HashMap<i32, Vec<RectTuple>> = HashMap::new();
    for token in &tokens {
        let op = token.operator.as_str();
        if state.apply_state_operator(op, &token.operands) {
            continue;
        }
        if op == "Do" && !token.operands.is_empty() {
            let Operand::Name(name) = &token.operands[0] else {
                continue;
            };
            let Some(&xref) = name_to_xref.get(name) else {
                continue;
            };
            let rect = transform_rect(&state.ctm, &[0.0, 0.0, 1.0, 1.0]);
            if rect_is_empty(&rect) {
                continue;
            }
            rects.entry(xref).or_default().push(rect);
        }
    }
    Ok(rects)
}

/// Python `round(value, 3)` as an integer-scaled sort key (round-half-even).
fn round3(value: f64) -> i64 {
    (value * 1000.0).round_ties_even() as i64
}

/// `detect.py::_merge_vertical_image_bands` — sort by `(round(y0,3),
/// round(x0,3))` and merge rects whose vertical gap is within `y_tolerance`.
pub fn merge_vertical_image_bands(rects: &[RectTuple], y_tolerance: f64) -> Vec<RectTuple> {
    let mut ordered: Vec<RectTuple> = rects.to_vec();
    ordered.sort_by(|a, b| round3(a[1]).cmp(&round3(b[1])).then(round3(a[0]).cmp(&round3(b[0]))));
    let mut merged: Vec<RectTuple> = Vec::new();
    for rect in ordered {
        if let Some(prev) = merged.last() {
            if rect[1] <= prev[3] + y_tolerance {
                let idx = merged.len() - 1;
                merged[idx] = rect_union(&merged[idx], &rect);
                continue;
            }
        }
        merged.push(rect);
    }
    merged
}

/// `detect.py::_image_rects` — every image placement rect clipped to the page.
fn image_rects_for_page(page: &PdfPage, page_rect: &RectTuple) -> Result<Vec<RectTuple>, Error> {
    let rects_by_xref = image_placement_rects_by_xref(page)?;
    let mut out = Vec::new();
    for rects in rects_by_xref.values() {
        for rect in rects {
            let inter = rect_intersection(rect, page_rect);
            if rect_is_empty(&inter) {
                continue;
            }
            out.push(inter);
        }
    }
    Ok(out)
}

/// `detect.py::pick_primary_background_image` — the largest placement covering
/// at least `coverage_ratio_threshold` of the page; `None` when none qualifies.
/// Candidate tie-breaks are deterministic (xref, then rect origin).
pub fn pick_primary_background_image(
    page: &PdfPage,
    page_rect: &RectTuple,
    coverage_ratio_threshold: f64,
) -> Result<Option<(i32, RectTuple)>, Error> {
    let page_area = rect_area(page_rect).max(1.0);
    let rects_by_xref = image_placement_rects_by_xref(page)?;
    let mut candidates: Vec<(f64, i32, RectTuple)> = Vec::new();
    for (&xref, rects) in &rects_by_xref {
        if xref <= 0 {
            continue;
        }
        for rect in rects {
            if rect_is_empty(rect) {
                continue;
            }
            let coverage_ratio = rect_area(&rect_intersection(rect, page_rect)) / page_area;
            if coverage_ratio < coverage_ratio_threshold {
                continue;
            }
            candidates.push((coverage_ratio, xref, *rect));
        }
    }
    candidates.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then(a.2[0].total_cmp(&b.2[0]))
            .then(a.2[1].total_cmp(&b.2[1]))
    });
    Ok(candidates.into_iter().max_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, xref, rect)| (xref, rect)))
}

/// `detect.py::page_has_tiled_background_images` — at least
/// `TILED_BACKGROUND_IMAGE_MIN_COUNT` page-wide tiles whose merged vertical
/// bands cover `TILED_BACKGROUND_IMAGE_COVERAGE_RATIO` of the page.
pub fn page_has_tiled_background_images(
    page: &PdfPage,
    page_rect: &RectTuple,
) -> Result<bool, Error> {
    let page_area = rect_area(page_rect).max(1.0);
    let page_width = rect_width(page_rect).max(1.0);
    let image_rects = image_rects_for_page(page, page_rect)?;
    if image_rects.len() < TILED_BACKGROUND_IMAGE_MIN_COUNT {
        return Ok(false);
    }
    let page_wide: Vec<RectTuple> = image_rects
        .into_iter()
        .filter(|rect| {
            rect_width(&rect_intersection(rect, page_rect)) / page_width >= TILED_BACKGROUND_IMAGE_MIN_WIDTH_RATIO
        })
        .collect();
    if page_wide.len() < TILED_BACKGROUND_IMAGE_MIN_COUNT {
        return Ok(false);
    }
    let merged = merge_vertical_image_bands(&page_wide, 1.0);
    let covered_area: f64 = merged
        .iter()
        .map(|rect| rect_area(&rect_intersection(rect, page_rect)))
        .sum();
    Ok(covered_area / page_area >= TILED_BACKGROUND_IMAGE_COVERAGE_RATIO)
}

/// `detect.py::page_has_large_background_image` — a primary background image
/// or a tiled background pattern.
pub fn page_has_large_background_image(
    page: &PdfPage,
    page_rect: &RectTuple,
) -> Result<bool, Error> {
    if pick_primary_background_image(page, page_rect, DETECT_PRIMARY_COVERAGE_RATIO)?.is_some() {
        return Ok(true);
    }
    page_has_tiled_background_images(page, page_rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_bands_matches_python() {
        let r = |x0, y0, x1, y1| [x0, y0, x1, y1];
        // non-vertical neighbors with gap <= 1 merge into a union
        let merged = merge_vertical_image_bands(
            &[r(0.0, 0.0, 100.0, 10.0), r(0.0, 10.5, 100.0, 20.0)],
            1.0,
        );
        assert_eq!(merged, vec![r(0.0, 0.0, 100.0, 20.0)]);
        // gap > tolerance keeps separate
        let merged = merge_vertical_image_bands(
            &[r(0.0, 0.0, 100.0, 10.0), r(0.0, 12.0, 100.0, 20.0)],
            1.0,
        );
        assert_eq!(merged, vec![r(0.0, 0.0, 100.0, 10.0), r(0.0, 12.0, 100.0, 20.0)]);
        // unsorted input merges after ordering
        let merged = merge_vertical_image_bands(
            &[r(0.0, 10.0, 100.0, 20.0), r(0.0, 0.0, 100.0, 10.0)],
            1.0,
        );
        assert_eq!(merged, vec![r(0.0, 0.0, 100.0, 20.0)]);
    }
}
