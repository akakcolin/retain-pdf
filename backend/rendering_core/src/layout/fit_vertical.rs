// Port of services/rendering/layout/payload/fit_vertical.py — the block-fit
// solver that shrinks font/leading until an adjacent-collision block fits its
// vertical budget.

use crate::item::{FormulaEntry, Item};
use crate::layout::render_item::fit_inner_bbox;
use crate::payload::capacity::estimated_render_height_pt;
use crate::payload::text_common::{
    layout_density_ratio, translation_density_ratio, COMPACT_TRIGGER_RATIO, LAYOUT_COMPACT_TRIGGER_RATIO,
};
use crate::util::py_round;

/// `fit_block_to_vertical_limit`: reduce `(font_size_pt, leading_em)` so the
/// estimated render height fits `max_height_pt`. Returns the fitted pair; early
/// returns the inputs unchanged when already within budget or the bbox is invalid.
pub fn fit_block_to_vertical_limit(
    item: &Item,
    protected_text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
    leading_em: f64,
    max_height_pt: f64,
    page_body_font_size_pt: Option<f64>,
) -> (f64, f64) {
    let inner = fit_inner_bbox(item);
    if inner.len() != 4 || max_height_pt <= 0.0 {
        return (font_size_pt, leading_em);
    }
    let mut estimated_height =
        estimated_render_height_pt(&inner, protected_text, formula_map, font_size_pt, leading_em);
    if estimated_height <= max_height_pt * 1.02 {
        return (font_size_pt, leading_em);
    }

    let line_step = (font_size_pt * 1.02).max(font_size_pt * (1.0 + leading_em));
    let length_density_ratio = translation_density_ratio(item, protected_text);
    let layout_density = layout_density_ratio(&inner, protected_text, font_size_pt, line_step);
    let is_dense_block =
        length_density_ratio >= COMPACT_TRIGGER_RATIO || layout_density >= LAYOUT_COMPACT_TRIGGER_RATIO;
    let is_body = item.is_body_text_candidate;
    let inherited_font_floor = item.short_body_inherited_font_floor_pt;

    let mut min_font: f64 = if is_dense_block { 8.45 } else { 8.75 };
    if is_body {
        if let Some(page_body) = page_body_font_size_pt {
            min_font = min_font.min(page_body - 0.5);
        }
    }
    let body_floor: f64 = if is_body { 7.4 } else { 8.0 };
    min_font = body_floor.max(min_font);
    if inherited_font_floor > 0.0 {
        min_font = min_font.max(font_size_pt.min(inherited_font_floor));
    }

    let mut best_font = font_size_pt;
    let mut best_leading = leading_em;
    for _ in 0..10 {
        if estimated_height <= max_height_pt * 1.01 {
            return (py_round(best_font, 2), py_round(best_leading, 2));
        }
        if best_font > min_font {
            let step: f64 = if is_dense_block { 0.1 } else { 0.08 };
            best_font = min_font.max(best_font - step);
        } else if best_leading > (if is_body { 0.52 } else { 0.3 }) {
            let floor: f64 = if is_body { 0.52 } else { 0.3 };
            best_leading = floor.max(best_leading - 0.01);
        } else {
            break;
        }
        estimated_height = estimated_render_height_pt(&inner, protected_text, formula_map, best_font, best_leading);
    }

    let overflow_ratio = estimated_height / max_height_pt.max(1.0);
    if overflow_ratio > 1.02 {
        let severe_overflow = overflow_ratio > 1.22;
        let extreme_overflow = overflow_ratio > 1.5;
        let compressed_leading_boost = 1.10;
        let mut floor_leading: f64 = if is_body { 0.52 } else { 0.28 };
        if is_dense_block {
            floor_leading = if is_body { 0.5 } else { 0.24 };
        }
        if severe_overflow {
            floor_leading = floor_leading.min(if is_body { 0.44 } else { 0.22 });
        }
        if extreme_overflow {
            floor_leading = floor_leading.min(if is_body { 0.38 } else { 0.16 });
        }
        floor_leading = leading_em.min(floor_leading * compressed_leading_boost);

        let mut dense_min_font = min_font;
        if is_body {
            if let Some(page_body) = page_body_font_size_pt {
                let gap: f64 = if severe_overflow { 2.6 } else { 1.6 };
                dense_min_font = 6.2_f64.max(dense_min_font.min(page_body - gap));
            }
        } else {
            dense_min_font = 6.4_f64.max(dense_min_font - (if severe_overflow { 1.8 } else { 1.0 }));
        }
        if inherited_font_floor > 0.0 {
            dense_min_font = dense_min_font.max(font_size_pt.min(inherited_font_floor));
        }
        for _ in 0..18 {
            if estimated_height <= max_height_pt * 1.01 {
                break;
            }
            if best_font > dense_min_font {
                let step: f64 = if severe_overflow { 0.22 } else { 0.16 };
                best_font = dense_min_font.max(best_font - step);
            } else if best_leading > floor_leading {
                let step: f64 = if severe_overflow { 0.03 } else { 0.02 };
                best_leading = floor_leading.max(best_leading - step);
            }
            estimated_height = estimated_render_height_pt(&inner, protected_text, formula_map, best_font, best_leading);
        }
    }

    (py_round(best_font, 2), py_round(best_leading, 2))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{FormulaEntry, Item};

    fn dense_paragraph_item() -> Item {
        Item {
            bbox: Some([0.0, 0.0, 200.0, 100.0]),
            source_text: "a b c d e f g h i j".to_string(),
            is_body_text_candidate: true,
            ..Default::default()
        }
    }

    #[test]
    fn fits_within_budget_returns_inputs() {
        let item = dense_paragraph_item();
        assert_eq!(fit_block_to_vertical_limit(&item, "你好世界", &[], 12.0, 0.5, 200.0, None), (12.0, 0.5));
    }

    #[test]
    fn shrinks_when_tight() {
        let item = dense_paragraph_item();
        let (font, leading) = fit_block_to_vertical_limit(&item, "你好世界很长的一段文字内容", &[], 12.0, 0.5, 30.0, None);
        assert!(font <= 12.0);
        assert!(leading <= 0.5);
    }

    #[test]
    fn uses_formula_entries() {
        let item = Item {
            bbox: Some([0.0, 0.0, 40.0, 100.0]),
            source_text: "a b c d e f g h i j".to_string(),
            is_body_text_candidate: true,
            ..Default::default()
        };
        let formulas = vec![FormulaEntry { placeholder: "@f@".to_string(), formula_text: "x^2".to_string() }];
        let (font, _) = fit_block_to_vertical_limit(&item, "你好世界很长的一段文字内容 @f@", &formulas, 12.0, 0.5, 20.0, None);
        assert!(font < 12.0);
    }

    #[test]
    fn inherited_font_floor_caps_shrink() {
        let item = Item {
            bbox: Some([0.0, 0.0, 40.0, 100.0]),
            source_text: "a b c d e f g h i j".to_string(),
            is_body_text_candidate: true,
            short_body_inherited_font_floor_pt: 10.0,
            ..Default::default()
        };
        let (font, _) = fit_block_to_vertical_limit(&item, "你好世界很长的一段文字内容", &[], 12.0, 0.5, 20.0, None);
        assert!(font >= 10.0);
    }
}
