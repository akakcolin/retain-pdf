// Port of services/rendering/layout/title_binary_fit.py.

use crate::item::{FormulaEntry, Item};
use crate::layout::render_item::fit_inner_bbox;
use crate::payload::capacity::{estimated_render_height_pt, estimated_required_lines, text_demand_units};
use crate::payload::text_common::build_plain_text_from_text;
use crate::semantics::{is_title_like_block, layout_role, structure_role};
use crate::util::py_round;

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

#[cfg(test)]
mod tests {
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
