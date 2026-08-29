// Port of services/rendering/layout/payload/fit_metrics.py plus the
// fit_common.py constants the function consumes.

use crate::item::{FormulaEntry, Item};
use crate::layout::render_item::fit_inner_bbox;
use crate::payload::capacity::{box_capacity_units, text_demand_units};
use crate::payload::text_common::{
    layout_density_ratio, translation_density_ratio, COMPACT_TRIGGER_RATIO, LAYOUT_COMPACT_TRIGGER_RATIO,
};
use crate::typography::line_count::visual_line_count;
use crate::util::py_round;

pub const LAYOUT_DENSITY_SAFE_MAX: f64 = 0.89;
pub const AGGRESSIVE_DEMAND_RATIO: f64 = 1.16;
pub const AGGRESSIVE_LAYOUT_DENSITY_MARGIN: f64 = 0.12;

pub fn fit_translated_block_metrics(
    item: &Item,
    protected_text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
    leading_em: f64,
    page_body_font_size_pt: Option<f64>,
) -> (f64, f64) {
    let demand = text_demand_units(protected_text, formula_map);
    let box_ = fit_inner_bbox(item);
    if box_.len() != 4 {
        return (font_size_pt, leading_em);
    }
    let line_step = (font_size_pt * 1.02).max(font_size_pt * (1.0 + leading_em));
    let length_density_ratio = translation_density_ratio(item, protected_text);
    let layout_density = layout_density_ratio(&box_, protected_text, font_size_pt, line_step);
    let is_dense_block =
        length_density_ratio >= COMPACT_TRIGGER_RATIO || layout_density >= LAYOUT_COMPACT_TRIGGER_RATIO;
    let heavy_dense_small_box = item.heavy_dense_small_box;
    let dense_small_box = item.dense_small_box;
    let wide_aspect_body_text = item.wide_aspect_body_text;
    let visual_lines = visual_line_count(item);

    let mut font_size_pt = font_size_pt;
    if item.is_body_text_candidate && page_body_font_size_pt.is_some() {
        let page_body = page_body_font_size_pt.unwrap();
        let floor_gap: f64 = if heavy_dense_small_box {
            0.58
        } else if dense_small_box {
            0.34
        } else {
            0.12
        };
        let floor_gap = if wide_aspect_body_text {
            (floor_gap - 0.1).max(0.0)
        } else {
            floor_gap
        };
        font_size_pt = py_round(font_size_pt.max(page_body - floor_gap), 2);
    }
    if demand <= 0.0 {
        return (font_size_pt, leading_em);
    }

    let capacity = box_capacity_units(&box_, font_size_pt, leading_em, Some(visual_lines));
    let safe_capacity_ratio = if wide_aspect_body_text { 1.0 } else { 0.96 };
    let safe_layout_density = if wide_aspect_body_text {
        LAYOUT_DENSITY_SAFE_MAX + 0.03
    } else {
        LAYOUT_DENSITY_SAFE_MAX
    };
    if capacity <= 0.0
        || (demand <= capacity * safe_capacity_ratio && layout_density < safe_layout_density)
    {
        return (font_size_pt, leading_em);
    }

    let aggressive_fit = heavy_dense_small_box
        || (dense_small_box
            && capacity > 0.0
            && demand > capacity * 1.04
            && layout_density >= LAYOUT_DENSITY_SAFE_MAX + 0.03)
        || (capacity > 0.0
            && demand > capacity * (AGGRESSIVE_DEMAND_RATIO + 0.1)
            && layout_density >= LAYOUT_DENSITY_SAFE_MAX + AGGRESSIVE_LAYOUT_DENSITY_MARGIN);
    let mut best_font = font_size_pt;
    let best_leading = leading_em;

    let max_steps = if item.is_body_text_candidate {
        if wide_aspect_body_text {
            if aggressive_fit {
                1
            } else {
                0
            }
        } else if aggressive_fit {
            2
        } else if is_dense_block {
            1
        } else {
            0
        }
    } else if aggressive_fit {
        4
    } else if is_dense_block {
        2
    } else {
        1
    };
    let mut min_font = {
        let base: f64 = if dense_small_box || is_dense_block { 8.45 } else { 8.75 };
        match page_body_font_size_pt {
            Some(page_body) => {
                let floor: f64 = if heavy_dense_small_box {
                    0.62
                } else if dense_small_box {
                    0.4
                } else {
                    0.18
                };
                base.max(page_body - floor)
            }
            None => base,
        }
    };
    if wide_aspect_body_text {
        min_font = min_font.max(font_size_pt - 0.06);
    }
    for step in 1..=max_steps {
        let candidate_font = py_round(min_font.max(font_size_pt - step as f64 * 0.12), 2);
        let candidate_capacity = box_capacity_units(&box_, candidate_font, leading_em, Some(visual_lines));
        if demand <= candidate_capacity * 0.98 {
            return (candidate_font, leading_em);
        }
        best_font = candidate_font;
    }

    if item.is_body_text_candidate {
        if !aggressive_fit {
            return (best_font, best_leading);
        }
        let emergency_leading_base: f64 = if dense_small_box || is_dense_block { 0.54 } else { 0.56 };
        let emergency_leading = py_round(emergency_leading_base.max(leading_em - 0.01), 2);
        let emergency_min_font = {
            let base: f64 = if dense_small_box || is_dense_block { 7.8 } else { 8.2 };
            match page_body_font_size_pt {
                Some(page_body) => {
                    let floor: f64 = if heavy_dense_small_box {
                        1.25
                    } else if dense_small_box {
                        0.95
                    } else {
                        0.7
                    };
                    base.max(page_body - floor)
                }
                None => base,
            }
        };
        let emergency_steps = if dense_small_box || is_dense_block { 8 } else { 5 };
        for step in 1..emergency_steps {
            let candidate_font = py_round(emergency_min_font.max(best_font - step as f64 * 0.14), 2);
            let candidate_capacity = box_capacity_units(&box_, candidate_font, emergency_leading, Some(visual_lines));
            if demand <= candidate_capacity * 0.98 {
                return (candidate_font, emergency_leading);
            }
            best_font = candidate_font;
        }
        return (best_font, emergency_leading);
    }

    (best_font, best_leading)
}
