// Ports of services/rendering/layout/payload/fit_metrics.py + fit_vertical.py
// + title_binary_fit.py (with the fit_common.py constants each consumes):
// `fit_translated_block_metrics` (block-fit step solver), `fit_block_to_vertical_limit`
// (adjacent-collision vertical budget solver), and `solve_title_fit` (title
// binary-search box fit).

use crate::item::{FormulaEntry, Item};
use crate::layout::render_item::fit_inner_bbox;
use crate::payload::capacity::{
    box_capacity_units, estimated_render_height_pt, estimated_required_lines, text_demand_units,
};
use crate::payload::text_common::{
    build_plain_text_from_text, layout_density_ratio, translation_density_ratio, COMPACT_TRIGGER_RATIO,
    LAYOUT_COMPACT_TRIGGER_RATIO,
};
use crate::semantics::{is_title_like_block, layout_role, structure_role};
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


pub const TITLE_FIT_WIDTH_SAFETY: f64 = 0.92;
pub const TITLE_FIT_HEIGHT_SAFETY: f64 = 0.94;
pub const TITLE_FIT_MIN_FONT_SIZE_PT: f64 = 5.2;
pub const TITLE_FIT_EPS_PT: f64 = 0.03;
pub const TITLE_FIT_ITERATIONS: usize = 18;

#[derive(Debug, Clone, PartialEq)]
pub struct TitleFitDecision {
    pub font_size_pt: f64,
    pub leading_em: f64,
    pub fit_to_box: bool,
    pub fit_single_line: bool,
    pub fit_min_font_size_pt: f64,
    pub fit_max_font_size_pt: f64,
    pub fit_min_leading_em: f64,
    pub fit_max_height_pt: f64,
    pub fit_target_width_pt: f64,
    pub fit_target_height_pt: f64,
}

/// `_title_kind`: `layout_role(item) or structure_role(item)`; "title" when the
/// role is "title", else "heading".
fn title_kind(item: &Item) -> String {
    let role = layout_role(item);
    let role = if role.is_empty() { structure_role(item) } else { role };
    if role == "title" { "title".to_string() } else { "heading".to_string() }
}

fn title_leading_em(kind: &str, font_size_pt: f64, base_font_size_pt: f64) -> f64 {
    let mut base = if kind == "title" { 0.28 } else { 0.34 };
    if font_size_pt < base_font_size_pt * 0.88 {
        base = if kind == "title" { 0.34 } else { 0.4 };
    }
    if font_size_pt < 8.0 {
        base = if kind == "title" { 0.4 } else { 0.46 };
    }
    py_round(base, 2)
}

fn single_line_width_fits(
    inner: &[f64],
    protected_text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
) -> bool {
    if inner.len() != 4 {
        return false;
    }
    let width_pt = (inner[2] - inner[0]).max(1.0);
    let demand = text_demand_units(protected_text, formula_map);
    demand * font_size_pt <= width_pt * TITLE_FIT_WIDTH_SAFETY
}

fn fits_title_box(
    inner: &[f64],
    protected_text: &str,
    formula_map: &[FormulaEntry],
    kind: &str,
    base_font_size_pt: f64,
    font_size_pt: f64,
) -> bool {
    if inner.len() != 4 || font_size_pt <= 0.0 {
        return false;
    }
    let width_pt = (inner[2] - inner[0]).max(1.0) * TITLE_FIT_WIDTH_SAFETY;
    let height_pt = (inner[3] - inner[1]).max(1.0) * TITLE_FIT_HEIGHT_SAFETY;
    if width_pt <= 0.0 || height_pt <= 0.0 {
        return false;
    }
    let leading_em = title_leading_em(kind, font_size_pt, base_font_size_pt);
    let estimated_height = estimated_render_height_pt(
        &[inner[0], inner[1], inner[0] + width_pt, inner[1] + height_pt],
        protected_text,
        formula_map,
        font_size_pt,
        leading_em,
    );
    estimated_height <= height_pt
}

pub fn solve_title_fit(
    item: &Item,
    protected_text: &str,
    formula_map: &[FormulaEntry],
    base_font_size_pt: f64,
    base_leading_em: f64,
    max_font_size_pt: f64,
) -> Option<TitleFitDecision> {
    if !is_title_like_block(item) || protected_text.is_empty() {
        return None;
    }
    let inner = fit_inner_bbox(item);
    if inner.len() != 4 {
        return None;
    }

    let width_pt = (inner[2] - inner[0]).max(8.0);
    let height_pt = (inner[3] - inner[1]).max(8.0);
    let kind = title_kind(item);
    let plain_text = build_plain_text_from_text(protected_text);
    let text_len = plain_text.chars().count();
    let eff_max = if max_font_size_pt == 0.0 { base_font_size_pt } else { max_font_size_pt };
    let max_size = eff_max.max(1.0);
    let low = TITLE_FIT_MIN_FONT_SIZE_PT.min(base_font_size_pt).min(max_size).max(1.0);
    let high = low.max(max_size);
    let mut best = low;

    if fits_title_box(&inner, protected_text, formula_map, &kind, base_font_size_pt, high) {
        best = high;
    } else {
        let mut low = low;
        let mut high = high;
        for _ in 0..TITLE_FIT_ITERATIONS {
            let mid = low + (high - low) / 2.0;
            if fits_title_box(&inner, protected_text, formula_map, &kind, base_font_size_pt, mid) {
                best = mid;
                low = mid;
            } else {
                high = mid;
            }
            if high - low <= TITLE_FIT_EPS_PT {
                break;
            }
        }
    }

    let font_size_pt = py_round(best.max(1.0), 2);
    let leading_em = title_leading_em(&kind, font_size_pt, base_font_size_pt);
    let required_lines = estimated_required_lines(&inner, protected_text, formula_map, font_size_pt);
    let single_line = required_lines <= 1
        && text_len <= 42
        && single_line_width_fits(&inner, protected_text, formula_map, font_size_pt);
    let shrink_pressure = font_size_pt < base_font_size_pt - 0.08 || required_lines > 1;
    let title_ratio = if kind == "title" { 0.72 } else { 0.78 };
    let fit_min_font = font_size_pt.min(base_font_size_pt).min(font_size_pt * title_ratio).max(1.0);
    let leading_floor = if shrink_pressure { 0.08 } else { 0.0 };
    let fit_min_leading = leading_em.min(base_leading_em).min(leading_em - leading_floor).max(0.16);

    Some(TitleFitDecision {
        font_size_pt,
        leading_em,
        fit_to_box: true,
        fit_single_line: single_line,
        fit_min_font_size_pt: py_round(fit_min_font, 2),
        fit_max_font_size_pt: py_round(font_size_pt.max(max_size), 2),
        fit_min_leading_em: py_round(fit_min_leading, 2),
        fit_max_height_pt: py_round(height_pt, 2),
        fit_target_width_pt: py_round(width_pt, 2),
        fit_target_height_pt: py_round(height_pt, 2),
    })
}



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


#[cfg(test)]
mod tests_title {
    use super::*;

    fn title_item() -> Item {
        Item {
            block_kind: Some("title".into()),
            layout_role: Some("title".into()),
            bbox: Some([0.0, 0.0, 300.0, 60.0]),
            translated_text: "A Short Heading".into(),
            source_text: "A Short Heading".into(),
            lines: vec![crate::item::Line { bbox: Some([0.0, 0.0, 300.0, 20.0]), spans: vec![] }],
            ..Default::default()
        }
    }

    #[test]
    fn non_title_returns_none() {
        let item = Item { block_kind: Some("text".into()), ..Default::default() };
        assert!(solve_title_fit(&item, "x", &[], 10.0, 0.4, 12.0).is_none());
    }

    #[test]
    fn title_fits_at_high_font() {
        let decision = solve_title_fit(&title_item(), "A Short Heading", &[], 10.0, 0.4, 14.0).unwrap();
        assert!(decision.fit_to_box);
        assert!(decision.font_size_pt > 0.0);
        assert!(decision.fit_target_width_pt > 0.0);
    }
}
