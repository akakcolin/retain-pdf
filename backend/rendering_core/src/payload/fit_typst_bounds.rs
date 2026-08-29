// Port of services/rendering/layout/payload/fit_typst_bounds.py — the three
// binary-fit bound resolvers consumed by `resolve_typst_binary_fit`.

use crate::payload::fit_common::TYPST_BINARY_FORMULA_RATIO_TRIGGER;
use crate::payload::fit_typst_context::TypstFitContext;

/// `resolve_fit_height_pt`: clamp the effective container height to adjacent
/// availability and the source-text height limit, never below 8pt.
pub fn resolve_fit_height_pt(
    context: &TypstFitContext,
    adjacent_collision_risk: bool,
    adjacent_available_height_pt: Option<f64>,
) -> f64 {
    let mut fit_height_pt = context.effective_container_height_pt;
    if adjacent_collision_risk && adjacent_available_height_pt.is_some() && adjacent_available_height_pt.unwrap() > 0.0 {
        fit_height_pt = fit_height_pt.min(adjacent_available_height_pt.unwrap());
    }
    if context.source_height_limit_pt > 0.0 {
        fit_height_pt = fit_height_pt.min(context.source_height_limit_pt);
    }
    fit_height_pt.max(8.0)
}

/// `resolve_min_font_size_pt`: prefer a page-body-anchored floor, then tighten
/// with a dynamic overflow-relief scale and an inherited short-body floor.
pub fn resolve_min_font_size_pt(
    context: &TypstFitContext,
    font_size_pt: f64,
    page_body_font_size_pt: Option<f64>,
) -> f64 {
    let floor_gap = if context.heavy_dense_small_box {
        1.1
    } else if context.dense_small_box {
        0.86
    } else {
        0.62
    };
    let mut preferred_min_font = if context.is_body && page_body_font_size_pt.is_some() {
        let page_body = page_body_font_size_pt.unwrap();
        let anchor: f64 = if context.heavy_dense_small_box { 7.4 } else { 7.8 };
        anchor.max(font_size_pt.min(page_body - floor_gap))
    } else {
        let fallback_gap = if context.dense_small_box { 0.44 } else { 0.3 };
        8.4_f64.max(font_size_pt - fallback_gap)
    };

    let overflow_excess = (context.effective_overflow_ratio - 1.0).max(0.0);
    let overflow_relief_font = if overflow_excess > 0.0 {
        overflow_excess / (overflow_excess + 0.75)
    } else {
        0.0
    };
    let (min_font_floor, shrink_cap): (f64, f64) = if context.is_body {
        let min_floor = if context.heavy_dense_small_box {
            6.6
        } else if context.dense_small_box {
            6.9
        } else {
            7.1
        };
        let shrink = if context.heavy_dense_small_box {
            0.52
        } else if context.dense_small_box {
            0.48
        } else {
            0.42
        };
        (min_floor, shrink)
    } else {
        (7.0, 0.36)
    };
    let dynamic_font_scale = 1.0 - shrink_cap * overflow_relief_font.powf(1.35);
    let dynamic_min_font = min_font_floor.max(font_size_pt * dynamic_font_scale);
    preferred_min_font = preferred_min_font.min(dynamic_min_font);
    if context.inherited_font_floor > 0.0 {
        preferred_min_font = preferred_min_font.max(font_size_pt.min(context.inherited_font_floor));
    }
    if preferred_min_font >= font_size_pt - 0.04 {
        let fallback_delta = if context.effective_overflow_ratio >= 1.12 || context.heavy_dense_small_box {
            0.18
        } else {
            0.12
        };
        return min_font_floor.max(font_size_pt - fallback_delta);
    }
    preferred_min_font
}

/// `resolve_min_leading_em`: floor leading by body/formula/overflow branches,
/// then apply an overflow-relief dynamic floor and never exceed `leading_em`.
pub fn resolve_min_leading_em(context: &TypstFitContext, leading_em: f64) -> f64 {
    let formula_heavy = context.formula_weight >= TYPST_BINARY_FORMULA_RATIO_TRIGGER;
    let (leading_floor_base, leading_delta): (f64, f64) = if leading_em <= 0.54 && context.is_body {
        let base = if formula_heavy { 0.46 } else { 0.44 };
        (base, 0.01)
    } else if !context.is_body {
        let base = if formula_heavy { 0.22 } else { 0.18 };
        let delta = if context.effective_overflow_ratio >= 1.08 { 0.06 } else { 0.04 };
        (base, delta)
    } else {
        let base = if formula_heavy { 0.56 } else { 0.54 };
        let delta = if formula_heavy {
            0.02
        } else if context.effective_overflow_ratio >= 1.12 {
            0.04
        } else {
            0.03
        };
        (base, delta)
    };

    let mut leading_floor_base = leading_floor_base;
    let mut leading_delta = leading_delta;
    if context.effective_overflow_ratio > 1.0 {
        let overflow_relief = ((context.effective_overflow_ratio - 1.0) / 1.2).min(1.0);
        let dynamic_leading_floor = leading_em - overflow_relief * if context.is_body { 0.08 } else { 0.12 };
        let absolute_leading_floor: f64 = if context.is_body { 0.42 } else { 0.18 };
        leading_floor_base = leading_floor_base.min(absolute_leading_floor.max(dynamic_leading_floor));
        let relief_delta: f64 = 0.02 + overflow_relief * if context.is_body { 0.04 } else { 0.06 };
        leading_delta = leading_delta.max(relief_delta);
    }
    let min_leading = leading_floor_base.max(leading_em - leading_delta);
    min_leading.min(leading_em)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line, Span};
    use crate::payload::fit_typst_context::build_typst_fit_context;

    fn body_context() -> TypstFitContext {
        let item = Item {
            bbox: Some([0.0, 0.0, 300.0, 40.0]),
            lines: vec![
                Line {
                    bbox: Some([0.0, 0.0, 300.0, 20.0]),
                    spans: vec![Span { span_type: "text".into(), content: "hello".into() }],
                },
                Line {
                    bbox: Some([0.0, 20.0, 300.0, 40.0]),
                    spans: vec![Span { span_type: "text".into(), content: "world".into() }],
                },
            ],
            source_text: "hello\nworld".into(),
            is_body_text_candidate: true,
            ..Default::default()
        };
        build_typst_fit_context(&item, &item.source_text, &[], 12.0, 0.4, false, None, 0.0, 0.0).unwrap()
    }

    #[test]
    fn fit_height_clamps_to_source_limit() {
        let mut context = body_context();
        context.effective_container_height_pt = 100.0;
        context.source_height_limit_pt = 60.0;
        assert_eq!(resolve_fit_height_pt(&context, false, None), 60.0);
    }

    #[test]
    fn fit_height_adjacent_wins() {
        let mut context = body_context();
        context.effective_container_height_pt = 100.0;
        context.source_height_limit_pt = 0.0;
        assert_eq!(resolve_fit_height_pt(&context, true, Some(55.0)), 55.0);
    }

    #[test]
    fn min_font_stays_below_source() {
        let context = body_context();
        let min_font = resolve_min_font_size_pt(&context, 14.0, Some(12.0));
        assert!(min_font <= 14.0);
        assert!(min_font > 6.0);
    }

    #[test]
    fn non_body_min_font_floor() {
        let mut context = body_context();
        context.is_body = false;
        let min_font = resolve_min_font_size_pt(&context, 20.0, None);
        assert!(min_font <= 20.0);
        assert!(min_font >= 7.0);
    }

    #[test]
    fn min_leading_never_exceeds_input() {
        let context = body_context();
        let min_leading = resolve_min_leading_em(&context, 0.4);
        assert!(min_leading <= 0.4);
        assert!(min_leading >= 0.42_f64.min(0.4) || (min_leading - 0.4).abs() < 1e-9);
    }

    #[test]
    fn formula_heavy_raises_floor() {
        let mut context = body_context();
        context.formula_weight = 0.2;
        let min_leading = resolve_min_leading_em(&context, 0.6);
        assert!(min_leading >= 0.5);
    }
}
