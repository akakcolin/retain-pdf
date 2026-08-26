//! Port of `source/background/patch.py` (pure geometry subset). All rect math
//! operates on `RectTuple = [x0, y0, x1, y1]` (PyMuPDF order). The PIL image
//! rewrite helpers (`pick_background_patch`, `sample_background_color`,
//! `rewrite_*`) are out of scope for the port.

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::config::{
    BACKGROUND_COVER_SAMPLE_MARGIN_PT, STRICT_VERTICAL_MERGE_GAP_PT,
    STRICT_VERTICAL_MERGE_MIN_WIDTH_OVERLAP_RATIO,
};

/// `Rect & Rect` — intersection (PyMuPDF `&`).
pub fn rect_intersection(a: &RectTuple, b: &RectTuple) -> RectTuple {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}

/// `Rect | Rect` — union (PyMuPDF `|`).
pub fn rect_union(a: &RectTuple, b: &RectTuple) -> RectTuple {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// PyMuPDF `Rect.is_empty`: `x1 <= x0 or y1 <= y0`.
pub fn rect_is_empty(r: &RectTuple) -> bool {
    r[2] <= r[0] || r[3] <= r[1]
}

/// PyMuPDF `Rect.is_infinite` (any coordinate non-finite).
pub fn rect_is_finite(r: &RectTuple) -> bool {
    r.iter().all(|c| c.is_finite())
}

/// `rects.py::rect_area` — non-negative width x height.
pub fn rect_area(r: &RectTuple) -> f64 {
    (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0)
}

pub fn rect_width(r: &RectTuple) -> f64 {
    (r[2] - r[0]).max(0.0)
}

pub fn rect_height(r: &RectTuple) -> f64 {
    (r[3] - r[1]).max(0.0)
}

/// `patch.py::width_overlap_ratio` — shared x-span over the narrower rect.
pub fn width_overlap_ratio(a: &RectTuple, b: &RectTuple) -> f64 {
    let overlap = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let min_width = (a[2] - a[0]).min(b[2] - b[0]).max(1e-6);
    overlap / min_width
}

/// `patch.py::merge_close_vertical_rects` — sort by `(y0, x0)`, merge rects
/// whose vertical gap is within `STRICT_VERTICAL_MERGE_GAP_PT` and whose
/// horizontal overlap is at least `STRICT_VERTICAL_MERGE_MIN_WIDTH_OVERLAP_RATIO`.
pub fn merge_close_vertical_rects(rects: &[RectTuple]) -> Vec<RectTuple> {
    if rects.is_empty() {
        return Vec::new();
    }
    let mut ordered: Vec<RectTuple> = rects.to_vec();
    ordered.sort_by(|a, b| a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0])));
    let mut merged: Vec<RectTuple> = vec![ordered[0]];
    for rect in ordered.into_iter().skip(1) {
        let current = merged.last().unwrap();
        let gap = rect[1] - current[3];
        if (0.0..=STRICT_VERTICAL_MERGE_GAP_PT).contains(&gap)
            && width_overlap_ratio(current, &rect) >= STRICT_VERTICAL_MERGE_MIN_WIDTH_OVERLAP_RATIO
        {
            *merged.last_mut().unwrap() = rect_union(current, &rect);
        } else {
            merged.push(rect);
        }
    }
    merged
}

/// `patch.py::map_rect_to_image` — project a page-space rect onto an image of
/// `image_size` pixels covering `image_rect`. `None` if the rect does not
/// overlap, or the mapped bounds are under 2px in either axis.
pub fn map_rect_to_image(
    image_rect: &RectTuple,
    image_size: (i64, i64),
    rect: &RectTuple,
) -> Option<(i64, i64, i64, i64)> {
    let (width, height) = image_size;
    if width <= 0 || height <= 0 {
        return None;
    }
    let inter = rect_intersection(rect, image_rect);
    if rect_is_empty(&inter) {
        return None;
    }
    let sx = width as f64 / (image_rect[2] - image_rect[0]).max(1e-6);
    let sy = height as f64 / (image_rect[3] - image_rect[1]).max(1e-6);
    let x0 = (((inter[0] - image_rect[0]) * sx) as i64).clamp(0, width);
    let y0 = (((inter[1] - image_rect[1]) * sy) as i64).clamp(0, height);
    let x1 = (((inter[2] - image_rect[0]) * sx) as i64).clamp(0, width);
    let y1 = (((inter[3] - image_rect[1]) * sy) as i64).clamp(0, height);
    if x1 - x0 < 2 || y1 - y0 < 2 {
        return None;
    }
    Some((x0, y0, x1, y1))
}

/// `fill.py::_patch_candidate_rects` — the four strips bordering `rect`,
/// expanded by a size-derived margin, clipped to `page_rect` and kept only when
/// at least 1pt thick in both axes.
pub fn patch_candidate_rects(page_rect: &RectTuple, rect: &RectTuple) -> Vec<RectTuple> {
    let margin = BACKGROUND_COVER_SAMPLE_MARGIN_PT.max(
        18.0_f64.min((4.0_f64).max(rect_width(rect).min(rect_height(rect)) * 0.35)),
    );
    let candidates = [
        [rect[0] - margin, rect[1], rect[0], rect[3]],
        [rect[2], rect[1], rect[2] + margin, rect[3]],
        [rect[0], rect[1] - margin, rect[2], rect[1]],
        [rect[0], rect[3], rect[2], rect[3] + margin],
    ];
    let mut valid = Vec::new();
    for candidate in candidates {
        let clipped = rect_intersection(&candidate, page_rect);
        if rect_is_empty(&clipped) || rect_width(&clipped) <= 1.0 || rect_height(&clipped) <= 1.0 {
            continue;
        }
        valid.push(clipped);
    }
    valid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> RectTuple {
        [x0, y0, x1, y1]
    }

    #[test]
    fn width_overlap_matches_python() {
        assert_eq!(width_overlap_ratio(&rect(0.0, 0.0, 100.0, 50.0), &rect(10.0, 5.0, 90.0, 60.0)), 1.0);
        assert!((width_overlap_ratio(&rect(0.0, 0.0, 100.0, 50.0), &rect(50.0, 5.0, 150.0, 60.0)) - 0.5).abs() < 1e-9);
        assert_eq!(width_overlap_ratio(&rect(0.0, 0.0, 100.0, 50.0), &rect(200.0, 5.0, 300.0, 60.0)), 0.0);
    }

    #[test]
    fn merge_vertical_matches_python() {
        assert_eq!(merge_close_vertical_rects(&[]), Vec::<RectTuple>::new());
        // stacked rects with zero gap merge into one
        assert_eq!(
            merge_close_vertical_rects(&[rect(10.0, 0.0, 50.0, 20.0), rect(10.0, 20.0, 50.0, 40.0), rect(10.0, 40.0, 50.0, 60.0)]),
            vec![rect(10.0, 0.0, 50.0, 60.0)]
        );
        // gap of 5pt is too large → kept separate
        assert_eq!(
            merge_close_vertical_rects(&[rect(10.0, 0.0, 50.0, 20.0), rect(10.0, 25.0, 50.0, 45.0)]),
            vec![rect(10.0, 0.0, 50.0, 20.0), rect(10.0, 25.0, 50.0, 45.0)]
        );
        // horizontal overlap below 0.72 → kept separate
        assert_eq!(
            merge_close_vertical_rects(&[rect(0.0, 0.0, 20.0, 20.0), rect(50.0, 20.0, 70.0, 40.0)]),
            vec![rect(0.0, 0.0, 20.0, 20.0), rect(50.0, 20.0, 70.0, 40.0)]
        );
        // unsorted input still merges after ordering
        assert_eq!(
            merge_close_vertical_rects(&[rect(10.0, 40.0, 50.0, 60.0), rect(10.0, 0.0, 50.0, 20.0), rect(10.0, 20.0, 50.0, 40.0)]),
            vec![rect(10.0, 0.0, 50.0, 60.0)]
        );
    }

    #[test]
    fn map_rect_to_image_matches_python() {
        let image_rect = rect(100.0, 200.0, 500.0, 600.0);
        assert_eq!(
            map_rect_to_image(&image_rect, (400, 400), &rect(150.0, 250.0, 300.0, 350.0)),
            Some((50, 50, 200, 150))
        );
        assert_eq!(
            map_rect_to_image(&image_rect, (400, 400), &rect(50.0, 250.0, 300.0, 700.0)),
            Some((0, 50, 200, 400))
        );
        assert_eq!(map_rect_to_image(&image_rect, (400, 400), &rect(600.0, 600.0, 700.0, 700.0)), None);
        assert_eq!(map_rect_to_image(&image_rect, (400, 400), &rect(100.0, 200.0, 101.0, 201.0)), None);
    }

    #[test]
    fn patch_candidate_rects_matches_python() {
        let page = rect(0.0, 0.0, 612.0, 792.0);
        assert_eq!(
            patch_candidate_rects(&page, &rect(300.0, 300.0, 400.0, 400.0)),
            vec![
                rect(282.0, 300.0, 300.0, 400.0),
                rect(400.0, 300.0, 418.0, 400.0),
                rect(300.0, 282.0, 400.0, 300.0),
                rect(300.0, 400.0, 400.0, 418.0),
            ]
        );
        // corner rect: two strips are clipped away by the page edge
        assert_eq!(
            patch_candidate_rects(&page, &rect(0.0, 0.0, 20.0, 20.0)),
            vec![rect(20.0, 0.0, 27.0, 20.0), rect(0.0, 20.0, 20.0, 27.0)]
        );
        assert_eq!(
            patch_candidate_rects(&page, &rect(300.0, 300.0, 310.0, 310.0)),
            vec![
                rect(294.0, 300.0, 300.0, 310.0),
                rect(310.0, 300.0, 316.0, 310.0),
                rect(300.0, 294.0, 310.0, 300.0),
                rect(300.0, 310.0, 310.0, 316.0),
            ]
        );
    }
}
