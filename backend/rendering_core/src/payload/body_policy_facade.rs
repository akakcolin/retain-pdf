// Port of services/rendering/layout/payload/body_policy_facade.py — the uniform
// stage signature the body pipeline uses to call each body policy leaf. The
// caller keeps `body_payloads` (clones of the body entries) and `ordered_payloads`
// (the real payload vec) in sync around each stage call.

use serde_json::Value;

use crate::layout::annotation_font_policy as annotation;
use crate::layout::body_fit_policy;
use crate::layout::body_font_dense_policy as dense;
use crate::layout::body_font_harmonize_policy as harmonize;
use crate::layout::body_font_inheritance_policy as inheritance;
use crate::layout::body_font_underfill_policy as underfill;
use crate::layout::body_font_unify_policy as unify;
use crate::layout::body_leading_policy as leading;
use crate::layout::body_page_anchor_policy as page_anchor;
use crate::layout::body_smoothing_policy as smoothing;

pub fn tighten_body_payloads(
    body_payloads: &mut Vec<Value>,
    body_font_median: f64,
    body_density_target: f64,
    body_pressure_median: f64,
    _ordered_payloads: &mut Vec<Value>,
    _page_text_width_med: f64,
) {
    dense::tighten_body_payloads(body_payloads, body_font_median, body_density_target, body_pressure_median);
}

pub fn mark_force_fit_dense_outliers(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    _ordered_payloads: &mut Vec<Value>,
    _page_text_width_med: f64,
) {
    dense::mark_force_fit_dense_outliers(body_payloads);
}

pub fn grow_underfilled_body_payloads(
    body_payloads: &mut Vec<Value>,
    body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    _ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    underfill::grow_underfilled_body_payloads(body_payloads, body_font_median, page_text_width_med);
}

pub fn recover_underfilled_body_density(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    _ordered_payloads: &mut Vec<Value>,
    _page_text_width_med: f64,
) {
    underfill::recover_underfilled_body_density(body_payloads);
}

pub fn restore_comfort_body_leading(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    _ordered_payloads: &mut Vec<Value>,
    _page_text_width_med: f64,
) {
    leading::restore_comfort_body_leading(body_payloads);
}

pub fn refit_body_leading_after_font_unify(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    _ordered_payloads: &mut Vec<Value>,
    _page_text_width_med: f64,
) {
    leading::refit_body_leading_after_font_unify(body_payloads);
}

pub fn harmonize_underfilled_body_fonts(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    underfill::harmonize_underfilled_body_fonts(body_payloads, ordered_payloads, page_text_width_med);
}

pub fn apply_page_body_font_anchor(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    page_anchor::apply_page_body_font_anchor(body_payloads, ordered_payloads, page_text_width_med);
}

pub fn inherit_short_body_fonts(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    inheritance::inherit_short_body_fonts(body_payloads, ordered_payloads, page_text_width_med);
}

pub fn inherit_low_height_body_fonts(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    inheritance::inherit_low_height_body_fonts(body_payloads, ordered_payloads, page_text_width_med);
}

pub fn unify_similar_body_fonts(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
    book_body_font_target: Option<f64>,
) {
    unify::unify_similar_body_fonts(body_payloads, ordered_payloads, page_text_width_med, book_body_font_target);
}

pub fn relax_short_body_context_heights(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    body_fit_policy::relax_short_body_context_heights(body_payloads, ordered_payloads, page_text_width_med);
}

pub fn harmonize_long_body_payloads(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    _ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    harmonize::harmonize_long_body_payloads(body_payloads, page_text_width_med);
}

pub fn smooth_adjacent_body_payloads(
    body_payloads: &mut Vec<Value>,
    _body_font_median: f64,
    _body_density_target: f64,
    _body_pressure_median: f64,
    _ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    smoothing::smooth_adjacent_body_payloads(body_payloads, page_text_width_med);
}

/// `unify_annotation_fonts`: caption/footnote font unify (non-body pipeline).
pub fn unify_annotation_fonts(ordered_payloads: &mut Vec<Value>) {
    annotation::unify_annotation_fonts(ordered_payloads);
}

/// `recover_underfilled_annotation_density`: caption/footnote density recovery.
pub fn recover_underfilled_annotation_density(ordered_payloads: &mut Vec<Value>) {
    annotation::recover_underfilled_annotation_density(ordered_payloads);
}
