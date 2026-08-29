// Port of services/rendering/layout/payload/fit_typst.py — the `resolve_typst_binary_fit`
// assembler the emit boundary calls for non-title blocks.

use crate::item::{FormulaEntry, Item};
use crate::payload::fit_typst_bounds::{resolve_fit_height_pt, resolve_min_font_size_pt, resolve_min_leading_em};
use crate::payload::fit_typst_context::{build_typst_fit_context, should_apply_typst_fit};
use crate::util::py_round;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypstBinaryFit {
    pub fit_to_box: bool,
    pub min_font_size_pt: f64,
    pub min_leading_em: f64,
    pub max_height_pt: f64,
}

impl Default for TypstBinaryFit {
    fn default() -> Self {
        TypstBinaryFit {
            fit_to_box: false,
            min_font_size_pt: 0.0,
            min_leading_em: 0.0,
            max_height_pt: 0.0,
        }
    }
}

/// `resolve_typst_binary_fit`. The caller must pre-apply the payload overrides
/// to `item` (render_inner_bbox / is_body_text_candidate / dense_small_box /
/// heavy_dense_small_box) before calling; `relaxed_fit_height_pt` and
/// `inherited_font_floor_pt` mirror the raw `_relaxed_fit_height_pt` /
/// `_short_body_inherited_font_floor_pt` payload keys.
pub fn resolve_typst_binary_fit(
    item: &Item,
    protected_text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
    leading_em: f64,
    relaxed_fit_height_pt: f64,
    inherited_font_floor_pt: f64,
    page_body_font_size_pt: Option<f64>,
    prefer_typst_fit: bool,
    adjacent_collision_risk: bool,
    adjacent_available_height_pt: Option<f64>,
) -> TypstBinaryFit {
    let Some(context) = build_typst_fit_context(
        item,
        protected_text,
        formula_map,
        font_size_pt,
        leading_em,
        adjacent_collision_risk,
        adjacent_available_height_pt,
        relaxed_fit_height_pt,
        inherited_font_floor_pt,
    ) else {
        return TypstBinaryFit::default();
    };
    if !should_apply_typst_fit(&context, prefer_typst_fit, adjacent_collision_risk) {
        return TypstBinaryFit::default();
    }
    let fit_height_pt = resolve_fit_height_pt(&context, adjacent_collision_risk, adjacent_available_height_pt);
    let min_font = resolve_min_font_size_pt(&context, font_size_pt, page_body_font_size_pt);
    let min_leading = resolve_min_leading_em(&context, leading_em);
    TypstBinaryFit {
        fit_to_box: true,
        min_font_size_pt: py_round(min_font, 2),
        min_leading_em: py_round(min_leading, 2),
        max_height_pt: py_round(fit_height_pt, 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line, Span};

    #[test]
    fn no_context_or_gate_returns_false() {
        let item = Item::default();
        let fit = resolve_typst_binary_fit(&item, "", &[], 12.0, 0.4, 0.0, 0.0, None, false, false, None);
        assert!(!fit.fit_to_box);
        assert_eq!(fit.min_font_size_pt, 0.0);
    }

    #[test]
    fn prefer_flag_applies_fit() {
        let item = Item {
            bbox: Some([0.0, 0.0, 300.0, 40.0]),
            lines: vec![Line {
                bbox: Some([0.0, 0.0, 300.0, 20.0]),
                spans: vec![Span { span_type: "text".into(), content: "hello".into() }],
            }],
            source_text: "hello".into(),
            translated_text: "你好".into(),
            ..Default::default()
        };
        let fit = resolve_typst_binary_fit(&item, "你好", &[], 12.0, 0.4, 0.0, 0.0, None, true, false, None);
        assert!(fit.fit_to_box);
        assert!(fit.min_font_size_pt > 0.0);
        assert!(fit.min_leading_em > 0.0);
        assert!(fit.max_height_pt >= 8.0);
    }
}
