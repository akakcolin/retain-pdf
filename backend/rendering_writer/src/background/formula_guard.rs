//! Formula-region guard protection, port of
//! `policy/formula_guard.py::protect_formula_regions_in_redaction_items` +
//! `source_cleanup/planning/segments.py` (guards-only geometry).
//!
//! Under the pinned layout config (`apply_layout_tuning(
//! source_cleanup_strategy="pikepdf_text_strip",
//! default_text_overlay_cover_fill=False)`, also pinned by
//! `gen_formula_guard_corpus.py`), `build_render_page_policy` yields empty item
//! policies, so `_apply_policy_fields_to_redaction_item` is identity. The port
//! also drops `page_text_source_rects`: production computes `text_rects`, but
//! `expanded_formula_guard` never reads it. Both are documented divergences
//! confined to the pinned path — see `redaction/mod.rs` for the same pinning.

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::redaction::dto::RedactionItem;
use super::redaction::primitives::merge_rects;
use super::stage::item_has_formula_region;

pub const FORMULA_GUARD_VERTICAL_PAD_PT: f64 = 12.0;
pub const FORMULA_GUARD_HORIZONTAL_PAD_PT: f64 = 8.0;
pub const MIN_REDACTION_FRAGMENT_HEIGHT_PT: f64 = 2.0;

fn rect_is_empty(r: &RectTuple) -> bool {
    r[2] <= r[0] || r[3] <= r[1]
}

fn rect_intersect(a: &RectTuple, b: &RectTuple) -> RectTuple {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

/// Python `round(value, 3)` — round half to even, mirroring the 2-decimal
/// `redaction/primitives.rs::round_half_even`.
fn py_round3(value: f64) -> f64 {
    let scaled = value * 1000.0;
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
    rounded / 1000.0
}

/// `geometry.py::rect_list` — `[round(x0,3), round(y0,3), round(x1,3), round(y1,3)]`.
fn rect_list(r: &RectTuple) -> Vec<f64> {
    vec![
        py_round3(r[0]),
        py_round3(r[1]),
        py_round3(r[2]),
        py_round3(r[3]),
    ]
}

/// `geometry.py::item_rect` non-None — a 4-float bbox with a non-empty rect.
fn item_rect(item: &RedactionItem) -> Option<RectTuple> {
    let b = item.bbox.as_ref()?;
    if b.len() != 4 {
        return None;
    }
    let rect = [b[0], b[1], b[2], b[3]];
    if rect_is_empty(&rect) {
        None
    } else {
        Some(rect)
    }
}

/// `formula_guard.py::page_formula_rects`.
fn page_formula_rects(items: &[RedactionItem]) -> Vec<RectTuple> {
    let mut rects = Vec::new();
    for item in items {
        if item_has_formula_region(item) {
            if let Some(rect) = item_rect(item) {
                rects.push(rect);
            }
        }
    }
    rects
}

/// `formula_guard.py::expanded_formula_guard` — production also receives
/// `text_rects` here but never reads them.
fn expanded_formula_guard(formula: &RectTuple) -> RectTuple {
    [
        formula[0] - FORMULA_GUARD_HORIZONTAL_PAD_PT,
        formula[1] - FORMULA_GUARD_VERTICAL_PAD_PT,
        formula[2] + FORMULA_GUARD_HORIZONTAL_PAD_PT,
        formula[3] + FORMULA_GUARD_VERTICAL_PAD_PT,
    ]
}

/// `formula_guard.py::expanded_formula_guards` (text_rects argument dropped).
fn expanded_formula_guards(formula_rects: &[RectTuple]) -> Vec<RectTuple> {
    let expanded: Vec<RectTuple> = formula_rects
        .iter()
        .filter(|r| !rect_is_empty(r))
        .map(expanded_formula_guard)
        .collect();
    merge_rects(&expanded)
}

/// `segments.py::_is_usable_fragment`.
fn is_usable_fragment(rect: &RectTuple, min_width_pt: f64, min_height_pt: f64, min_area_pt2: f64) -> bool {
    !rect_is_empty(rect)
        && rect[2] - rect[0] >= min_width_pt
        && rect[3] - rect[1] >= min_height_pt
        && (rect[2] - rect[0]) * (rect[3] - rect[1]) >= min_area_pt2
}

/// `segments.py::subtract_guard_from_rect` — the four half-plane candidates.
fn subtract_guard_from_rect(
    rect: &RectTuple,
    guard: &RectTuple,
    min_width_pt: f64,
    min_height_pt: f64,
    min_area_pt2: f64,
) -> Vec<RectTuple> {
    let overlap = rect_intersect(rect, guard);
    if rect_is_empty(&overlap) {
        return vec![*rect];
    }
    let candidates = [
        [rect[0], rect[1], rect[2], overlap[1]],
        [rect[0], overlap[3], rect[2], rect[3]],
        [rect[0], overlap[1], overlap[0], overlap[3]],
        [overlap[2], overlap[1], rect[2], overlap[3]],
    ];
    candidates
        .iter()
        .filter(|f| is_usable_fragment(f, min_width_pt, min_height_pt, min_area_pt2))
        .copied()
        .collect()
}

/// `segments.py::split_rect_around_guards`.
fn split_rect_around_guards(
    rect: &RectTuple,
    guards: &[RectTuple],
    min_width_pt: f64,
    min_height_pt: f64,
    min_area_pt2: f64,
) -> Vec<RectTuple> {
    if rect_is_empty(rect) {
        return Vec::new();
    }
    let mut fragments = vec![*rect];
    for guard in guards {
        if rect_is_empty(guard) {
            continue;
        }
        let mut next_fragments: Vec<RectTuple> = Vec::new();
        for fragment in &fragments {
            next_fragments.extend(subtract_guard_from_rect(
                fragment,
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

/// `formula_guard.py::split_rect_away_from_formula_guards` — min_width 1.0,
/// min_height and min_area both `MIN_REDACTION_FRAGMENT_HEIGHT_PT`.
fn split_rect_away_from_formula_guards(rect: &RectTuple, formula_guards: &[RectTuple]) -> Vec<RectTuple> {
    split_rect_around_guards(
        rect,
        formula_guards,
        1.0,
        MIN_REDACTION_FRAGMENT_HEIGHT_PT,
        MIN_REDACTION_FRAGMENT_HEIGHT_PT,
    )
}

/// `formula_guard.py::protect_formula_regions_in_redaction_items`. Policy-field
/// application (`_apply_policy_fields_to_redaction_item`) and the per-item
/// policy lookup are identity under the pinned config, so items pass through
/// as-is and only the bbox geometry changes for split items. A len-4 but empty
/// bbox drops the item; a real split emits fragments with
/// `bbox = rect_list(fragment)`, `_formula_guard_fragment = true`, and the
/// fragment index.
pub fn protect_formula_regions_in_redaction_items(
    redaction_items: Vec<RedactionItem>,
    translated_items: &[RedactionItem],
) -> Vec<RedactionItem> {
    let formula_rects = page_formula_rects(translated_items);
    if formula_rects.is_empty() || redaction_items.is_empty() {
        return redaction_items;
    }
    let formula_guards = expanded_formula_guards(&formula_rects);
    if formula_guards.is_empty() {
        return redaction_items;
    }

    let mut protected_items: Vec<RedactionItem> = Vec::new();
    for source_item in redaction_items {
        let bbox_len = source_item.bbox.as_ref().map(|b| b.len()).unwrap_or(0);
        if bbox_len != 4 {
            protected_items.push(source_item);
            continue;
        }
        let b = source_item.bbox.as_ref().unwrap();
        let rect = [b[0], b[1], b[2], b[3]];
        if rect_is_empty(&rect) {
            continue;
        }
        let fragments = split_rect_away_from_formula_guards(&rect, &formula_guards);
        if fragments.len() == 1 && fragments[0] == rect {
            protected_items.push(source_item);
            continue;
        }
        for (fragment_index, fragment) in fragments.iter().enumerate() {
            let mut fragment_item = source_item.clone();
            fragment_item.bbox = Some(rect_list(fragment));
            fragment_item.formula_guard_fragment = true;
            fragment_item.formula_guard_fragment_index = Some(fragment_index as i32);
            protected_items.push(fragment_item);
        }
    }
    protected_items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(bbox: Option<[f64; 4]>, formula: bool) -> RedactionItem {
        let mut it: RedactionItem = serde_json::from_str(r#"{"translated_text":"x"}"#).unwrap();
        it.bbox = bbox.map(|b| b.to_vec());
        if formula {
            it.block_type = "formula".to_string();
        }
        it
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> RectTuple {
        [x0, y0, x1, y1]
    }

    #[test]
    fn split_quadrants_emit_four_fragments() {
        let formula = item(Some([100.0, 100.0, 200.0, 120.0]), true);
        let target = item(Some([50.0, 50.0, 250.0, 250.0]), false);
        let out = protect_formula_regions_in_redaction_items(vec![target.clone()], &[formula, target]);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].bbox.as_deref().unwrap(), &[50.0, 50.0, 250.0, 88.0]);
        assert_eq!(out[1].bbox.as_deref().unwrap(), &[50.0, 132.0, 250.0, 250.0]);
        assert_eq!(out[2].bbox.as_deref().unwrap(), &[50.0, 88.0, 92.0, 132.0]);
        assert_eq!(out[3].bbox.as_deref().unwrap(), &[208.0, 88.0, 250.0, 132.0]);
        for (i, f) in out.iter().enumerate() {
            assert!(f.formula_guard_fragment);
            assert_eq!(f.formula_guard_fragment_index, Some(i as i32));
        }
    }

    #[test]
    fn fully_contained_item_dropped() {
        let formula = item(Some([100.0, 100.0, 200.0, 120.0]), true);
        let same = item(Some([100.0, 100.0, 200.0, 120.0]), false);
        let out = protect_formula_regions_in_redaction_items(vec![same.clone()], &[formula, same]);
        assert!(out.is_empty());
    }

    #[test]
    fn far_item_unchanged_and_mixed_split() {
        let formula = item(Some([100.0, 300.0, 200.0, 320.0]), true);
        let far = item(Some([50.0, 50.0, 250.0, 100.0]), false);
        let near = item(Some([150.0, 250.0, 350.0, 450.0]), false);
        let out = protect_formula_regions_in_redaction_items(
            vec![far.clone(), near.clone()],
            &[formula, far, near],
        );
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].bbox.as_deref().unwrap(), &[50.0, 50.0, 250.0, 100.0]);
        assert!(!out[0].formula_guard_fragment);
        assert_eq!(out[1].bbox.as_deref().unwrap(), &[150.0, 250.0, 350.0, 288.0]);
        assert_eq!(out[2].bbox.as_deref().unwrap(), &[150.0, 332.0, 350.0, 450.0]);
        assert_eq!(out[3].bbox.as_deref().unwrap(), &[208.0, 288.0, 350.0, 332.0]);
    }

    #[test]
    fn bbox_len_not_four_unchanged() {
        let formula = item(Some([100.0, 100.0, 200.0, 120.0]), true);
        let mut short: RedactionItem = serde_json::from_str(r#"{"translated_text":"a"}"#).unwrap();
        short.bbox = Some(vec![10.0, 10.0, 20.0]);
        let mut empty_bbox: RedactionItem = serde_json::from_str(r#"{"translated_text":"b"}"#).unwrap();
        empty_bbox.bbox = Some(vec![]);
        let out = protect_formula_regions_in_redaction_items(
            vec![short.clone(), empty_bbox.clone()],
            &[formula],
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].bbox.as_deref().unwrap(), &[10.0, 10.0, 20.0]);
        assert!(out[1].bbox.as_deref().unwrap().is_empty());
        assert!(!out[0].formula_guard_fragment);
        assert!(!out[1].formula_guard_fragment);
    }

    #[test]
    fn empty_bbox_dropped_but_other_split() {
        let formula = item(Some([100.0, 100.0, 200.0, 120.0]), true);
        let empty = item(Some([100.0, 100.0, 100.0, 100.0]), false);
        let target = item(Some([50.0, 50.0, 250.0, 250.0]), false);
        let out = protect_formula_regions_in_redaction_items(
            vec![empty.clone(), target.clone()],
            &[formula, empty, target],
        );
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|f| f.formula_guard_fragment));
    }

    #[test]
    fn no_formula_is_identity() {
        let a = item(Some([50.0, 50.0, 250.0, 250.0]), false);
        let out = protect_formula_regions_in_redaction_items(vec![a.clone()], &[a.clone()]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].bbox, a.bbox);
        assert!(!out[0].formula_guard_fragment);
    }

    #[test]
    fn horizontal_only_split() {
        let formula = item(Some([200.0, 50.0, 300.0, 150.0]), true);
        let h = item(Some([150.0, 80.0, 350.0, 120.0]), false);
        let out = protect_formula_regions_in_redaction_items(vec![h.clone()], &[formula, h]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].bbox.as_deref().unwrap(), &[150.0, 80.0, 192.0, 120.0]);
        assert_eq!(out[1].bbox.as_deref().unwrap(), &[308.0, 80.0, 350.0, 120.0]);
    }

    #[test]
    fn vertical_only_split() {
        let formula = item(Some([250.0, 100.0, 300.0, 120.0]), true);
        let v = item(Some([242.0, 50.0, 308.0, 180.0]), false);
        let out = protect_formula_regions_in_redaction_items(vec![v.clone()], &[formula, v]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].bbox.as_deref().unwrap(), &[242.0, 50.0, 308.0, 88.0]);
        assert_eq!(out[1].bbox.as_deref().unwrap(), &[242.0, 132.0, 308.0, 180.0]);
    }

    #[test]
    fn thin_left_fragment_filtered_by_min_width() {
        let formula = item(Some([100.0, 100.0, 200.0, 120.0]), true);
        let m = item(Some([91.5, 50.0, 93.0, 300.0]), false);
        let out = protect_formula_regions_in_redaction_items(vec![m.clone()], &[formula, m]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].bbox.as_deref().unwrap(), &[91.5, 50.0, 93.0, 88.0]);
        assert_eq!(out[1].bbox.as_deref().unwrap(), &[91.5, 132.0, 93.0, 300.0]);
    }

    #[test]
    fn close_guards_merge_into_one() {
        let fa = item(Some([100.0, 100.0, 120.0, 120.0]), true);
        let fb = item(Some([138.0, 100.0, 158.0, 120.0]), true);
        let target = item(Some([50.0, 50.0, 200.0, 200.0]), false);
        let out = protect_formula_regions_in_redaction_items(
            vec![target.clone()],
            &[fa, fb, target.clone()],
        );
        // Merged guard [92,88,166,132] -> no 2pt gap strip between the guards.
        assert_eq!(out.len(), 4);
        assert_eq!(out[2].bbox.as_deref().unwrap(), &[50.0, 88.0, 92.0, 132.0]);
        assert_eq!(out[3].bbox.as_deref().unwrap(), &[166.0, 88.0, 200.0, 132.0]);
    }

    #[test]
    fn rect_list_rounds_half_even() {
        assert_eq!(py_round3(88.5678), 88.568);
        assert_eq!(py_round3(132.4321), 132.432);
        assert_eq!(py_round3(92.1234), 92.123);
        assert_eq!(py_round3(208.9876), 208.988);
        assert_eq!(py_round3(1.0005), 1.0);
        assert_eq!(py_round3(1.0015), 1.002);
    }
}
