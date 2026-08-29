//! Port of `planning/segments.py` — splitting strip rects around formula
//! guards and padding segments.

use crate::rect::Rect;

pub const MIN_CLEANUP_SEGMENT_WIDTH_PT: f64 = 1.0;
pub const MIN_CLEANUP_SEGMENT_HEIGHT_PT: f64 = 1.0;
pub const MIN_CLEANUP_SEGMENT_AREA_PT2: f64 = 2.0;
pub const STRIP_SEGMENT_PAD_X_PT: f64 = 1.0;
pub const STRIP_SEGMENT_PAD_Y_PT: f64 = 1.0;
pub const FORMULA_SPLIT_SEGMENT_PAD_X_PT: f64 = 1.0;
pub const FORMULA_SPLIT_SEGMENT_PAD_Y_PT: f64 = 0.0;
pub const BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT: f64 = 1.0;
pub const BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT: f64 = 1.0;

/// `bbox_text_strip_formula_guard_rect` — formula padded by the guard pads.
pub fn bbox_text_strip_formula_guard_rect(formula: &Rect) -> Rect {
    Rect::new(
        formula.x0 - BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT,
        formula.y0 - BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT,
        formula.x1 + BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT,
        formula.y1 + BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT,
    )
}

/// `strip_segments_for_text_rect` — split a text rect around formula guards,
/// then pad each segment (split pads when a split happened, strip pads
/// otherwise).
pub fn strip_segments_for_text_rect(text_rect: &Rect, formula_rects: &[Rect]) -> Vec<Rect> {
    let formula_guards: Vec<Rect> = formula_rects
        .iter()
        .filter(|formula| !formula.is_empty())
        .map(bbox_text_strip_formula_guard_rect)
        .collect();
    let segments = split_rect_around_guards(text_rect, &formula_guards, MIN_CLEANUP_SEGMENT_WIDTH_PT, 2.0, 2.0);
    let was_split_for_formula =
        segments.len() != 1 || segments.first().map(|s| s != text_rect).unwrap_or(false);
    let mut padded: Vec<Rect> = Vec::new();
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let (dx0, dy0, dx1, dy1) = if was_split_for_formula {
            (
                -FORMULA_SPLIT_SEGMENT_PAD_X_PT,
                -FORMULA_SPLIT_SEGMENT_PAD_Y_PT,
                FORMULA_SPLIT_SEGMENT_PAD_X_PT,
                FORMULA_SPLIT_SEGMENT_PAD_Y_PT,
            )
        } else {
            (
                -STRIP_SEGMENT_PAD_X_PT,
                -STRIP_SEGMENT_PAD_Y_PT,
                STRIP_SEGMENT_PAD_X_PT,
                STRIP_SEGMENT_PAD_Y_PT,
            )
        };
        padded.push(segment.padded(dx0, dy0, dx1, dy1));
    }
    padded
}

/// `split_rect_around_guards` — iteratively subtract each guard from the
/// surviving fragments.
pub fn split_rect_around_guards(
    rect: &Rect,
    guards: &[Rect],
    min_width_pt: f64,
    min_height_pt: f64,
    min_area_pt2: f64,
) -> Vec<Rect> {
    if rect.is_empty() {
        return Vec::new();
    }
    let mut fragments: Vec<Rect> = vec![*rect];
    for guard in guards {
        if guard.is_empty() {
            continue;
        }
        let mut next_fragments: Vec<Rect> = Vec::new();
        for fragment in fragments {
            next_fragments.extend(subtract_guard_from_rect(
                &fragment,
                guard,
                min_width_pt,
                min_height_pt,
                min_area_pt2,
            ));
        }
        fragments = next_fragments;
        if fragments.is_empty() {
            break;
        }
    }
    fragments
}

/// `subtract_guard_from_rect` — split `rect` around `guard` into the up-to-four
/// band fragments that pass the usability filter.
pub fn subtract_guard_from_rect(
    rect: &Rect,
    guard: &Rect,
    min_width_pt: f64,
    min_height_pt: f64,
    min_area_pt2: f64,
) -> Vec<Rect> {
    let overlap = rect.intersect(guard);
    if overlap.is_empty() {
        return vec![*rect];
    }
    let candidates = [
        Rect::new(rect.x0, rect.y0, rect.x1, overlap.y0),
        Rect::new(rect.x0, overlap.y1, rect.x1, rect.y1),
        Rect::new(rect.x0, overlap.y0, overlap.x0, overlap.y1),
        Rect::new(overlap.x1, overlap.y0, rect.x1, overlap.y1),
    ];
    let mut usable: Vec<Rect> = Vec::new();
    for fragment in candidates {
        if is_usable_fragment(&fragment, min_width_pt, min_height_pt, min_area_pt2) {
            usable.push(fragment);
        }
    }
    usable
}

fn is_usable_fragment(rect: &Rect, min_width_pt: f64, min_height_pt: f64, min_area_pt2: f64) -> bool {
    !rect.is_empty()
        && rect.width() >= min_width_pt
        && rect.height() >= min_height_pt
        && rect.width() * rect.height() >= min_area_pt2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_removes_guard_band() {
        let rect = Rect::new(0.0, 0.0, 100.0, 20.0);
        let guard = Rect::new(40.0, 5.0, 60.0, 15.0);
        let parts = split_rect_around_guards(&rect, &[guard], 1.0, 1.0, 2.0);
        // vertical bands top (0-5) + bottom (15-20) + left (0-40 x 5-15) + right (60-100 x 5-15)
        assert_eq!(parts.len(), 4);
    }

    #[test]
    fn no_overlap_returns_rect() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let guard = Rect::new(50.0, 50.0, 60.0, 60.0);
        assert_eq!(subtract_guard_from_rect(&rect, &guard, 1.0, 1.0, 2.0), vec![rect]);
    }
}
