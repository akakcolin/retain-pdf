//! `source/cleanup/text_ownership.py::owned_text_entries` — an entry is owned
//! by `rect` when its center lies inside `rect` and no competing rect closer to
//! that center also contains it.

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::text_matching::{rect_center, rect_contains_point};

fn squared_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    dx * dx + dy * dy
}

fn owned_text_entries(
    rect: &RectTuple,
    entries: &[(RectTuple, String)],
    competing_rects: Option<&[RectTuple]>,
) -> Vec<(RectTuple, String)> {
    if entries.is_empty() {
        return Vec::new();
    }
    let competing: Vec<RectTuple> = competing_rects
        .map(|r| {
            r.iter()
                .copied()
                .filter(|c| c[2] > c[0] && c[3] > c[1])
                .collect()
        })
        .unwrap_or_default();
    let mut owned: Vec<(RectTuple, String)> = Vec::new();
    for (entry_rect, text) in entries {
        let (cx, cy) = rect_center(entry_rect);
        if !rect_contains_point(rect, cx, cy) {
            continue;
        }
        let owners: Vec<RectTuple> = competing
            .iter()
            .copied()
            .filter(|c| rect_contains_point(c, cx, cy))
            .collect();
        if !owners.is_empty() {
            let best = owners
                .iter()
                .min_by(|a, b| {
                    squared_distance(rect_center(a), (cx, cy))
                        .partial_cmp(&squared_distance(rect_center(b), (cx, cy)))
                        .expect("finite f64")
                })
                .copied();
            if best != Some(*rect) {
                continue;
            }
        }
        owned.push((*entry_rect, text.clone()));
    }
    owned
}

/// `owned_text_block_entries` (spans/blocks share the same ownership rule).
pub fn owned_text_block_entries(
    rect: &RectTuple,
    entries: &[(RectTuple, String)],
    competing_rects: Option<&[RectTuple]>,
) -> Vec<(RectTuple, String)> {
    owned_text_entries(rect, entries, competing_rects)
}

/// `owned_word_entries` (words share the same ownership rule).
pub fn owned_word_entries(
    rect: &RectTuple,
    entries: &[(RectTuple, String)],
    competing_rects: Option<&[RectTuple]>,
) -> Vec<(RectTuple, String)> {
    owned_text_entries(rect, entries, competing_rects)
}
