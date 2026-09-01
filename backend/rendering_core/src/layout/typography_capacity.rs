// Ports of services/rendering/policy/typography_policy.py,
// services/rendering/policy/typography_decision.py, and
// services/rendering/layout/typography_memory/features.py — the body-pipeline
// (C3-N3) typography constants, the decision DTOs the stages write onto payload
// dicts, and the deterministic feature-hash builder. The learning / store
// modules behind typography memory are not ported.

use blake2::digest::{Update, VariableOutput};
use blake2::Blake2bVar;
use serde_json::{json, Value};

use crate::item::formula_map;
use crate::item::Item;
use crate::layout::payload_dict::{inner_bbox4, payload_string};
use crate::payload::capacity::{estimated_required_lines, formula_estimate_discount};
use crate::payload::text_common::{source_word_count, translated_zh_char_count, translation_density_ratio};
use crate::semantics::{block_kind, is_title_like_block, layout_role, semantic_role, structure_role};
use crate::typography::content::formula_ratio;
use crate::typography::line_count::source_visual_line_count;
use crate::typography::line_metrics::{bbox_height, bbox_width};
use crate::util::py_round;

// Body font growth for translated paragraphs that visually underfill their OCR
// bbox. These values define the font-size side of the vertical slack budget.
pub const BODY_UNDERFILLED_DENSITY_FLOOR_TRIGGER: f64 = 0.60;
pub const BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET: f64 = 0.80;
pub const BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET_NO_SOURCE: f64 = 0.68;
pub const BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET_SHORT: f64 = 0.72;
pub const BODY_UNDERFILLED_DENSITY_SAFE_MAX: f64 = 0.98;
pub const BODY_UNDERFILLED_RECOVERY_MAX_ITERATIONS: i32 = 6;
pub const BODY_UNDERFILLED_RECOVERY_FONT_STEP_PT: f64 = 0.28;
pub const BODY_UNDERFILLED_RECOVERY_LEADING_STEP_EM: f64 = 0.06;
pub const BODY_UNDERFILLED_UNIFIED_FONT_MAX_STEP_PT: f64 = 0.0;
pub const BODY_UNDERFILLED_FONT_GROW_DENSITY_TRIGGER: f64 = BODY_UNDERFILLED_DENSITY_FLOOR_TRIGGER;
pub const BODY_UNDERFILLED_FONT_GROW_DENSITY_LIMIT: f64 = BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET;
pub const BODY_UNDERFILLED_FONT_GROW_MAX_PT: f64 = 1.15;
pub const BODY_UNDERFILLED_FONT_GROW_CONTEXT_BONUS_PT: f64 = 0.18;
pub const BODY_UNDERFILLED_FONT_GROW_PAGE_BONUS_PT: f64 = 0.16;
pub const BODY_UNDERFILLED_FONT_GROW_EXP_RATE: f64 = 1.55;
pub const BODY_UNDERFILLED_FONT_GROW_MAX_LINES: i32 = 8;
pub const BODY_UNDERFILLED_FONT_GROW_SHORT_LINE_BONUS: f64 = 0.04;
pub const BODY_UNDERFILLED_FONT_GROW_TALL_SLACK_BONUS: f64 = 0.08;
pub const BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_BONUS_PT: f64 = 0.18;
pub const BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_CAP_BONUS_PT: f64 = 0.16;
pub const BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_DENSITY_BONUS: f64 = 0.01;
pub const BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_RATIO_OFFSET: f64 = 1.25;
pub const BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_RATIO_RANGE: f64 = 2.75;
pub const BODY_UNDERFILLED_FONT_HARMONIZE_MAX_RATIO: f64 = 1.16;
pub const BODY_UNDERFILLED_FONT_GROW_MIN_LINES: i32 = 2;
pub const BODY_UNDERFILLED_FONT_GROW_MIN_HEIGHT_PT: f64 = 22.0;
pub const BODY_UNDERFILLED_FONT_GROW_SHORT_MAX_PT: f64 = 0.0;
pub const BODY_UNDERFILLED_FONT_GROW_LOW_FONT_SKIP_DELTA_PT: f64 = 0.20;

// Page-level body font anchor. Long, stable body paragraphs act as anchors for
// short body blocks on the same page/column so font size does not jump sharply.
pub const PAGE_BODY_FONT_ANCHOR_COUNT: usize = 2;
pub const PAGE_BODY_FONT_ANCHOR_MIN_HEIGHT_PT: f64 = 42.0;
pub const PAGE_BODY_FONT_ANCHOR_MIN_LINES: i64 = 3;
pub const PAGE_BODY_FONT_ANCHOR_MIN_WIDTH_RATIO: f64 = 0.62;
pub const PAGE_BODY_FONT_ANCHOR_TARGET_MAX_RATIO: f64 = 1.12;
pub const PAGE_BODY_FONT_ANCHOR_TARGET_MAX_DELTA_PT: f64 = 1.15;
pub const PAGE_BODY_FONT_ANCHOR_APPLY_DENSITY_LIMIT: f64 = 1.20;
pub const PAGE_BODY_FONT_ANCHOR_SHORT_HEIGHT_RELAX_RATIO: f64 = 1.72;
pub const PAGE_BODY_FONT_ANCHOR_SHORT_HEIGHT_RELAX_MAX_EXTRA_PT: f64 = 14.0;

// Body font unification. Prefer a conservative low-page anchor: one-line OCR
// boxes have no leading budget, so using large blocks as the target can make
// short/tight text jump upward and mislead density checks.
pub const BODY_FONT_UNIFY_ANCHOR_COUNT: usize = 2;
pub const BODY_FONT_UNIFY_ANCHOR_MIN_HEIGHT_PT: f64 = 30.0;
pub const BODY_FONT_UNIFY_ANCHOR_MIN_WIDTH_RATIO: f64 = 0.58;
pub const BODY_FONT_UNIFY_ANCHOR_MAX_DENSITY: f64 = 1.18;
pub const BODY_FONT_UNIFY_TARGET_QUANTILE: f64 = 0.25;
pub const BODY_FONT_UNIFY_EXTREME_SMALL_RATIO: f64 = 0.82;
pub const BODY_FONT_UNIFY_EXTREME_SMALL_DELTA_PT: f64 = 1.6;
pub const BODY_FONT_UNIFY_MIN_FILTERED_COUNT: usize = 2;
pub const BODY_FONT_UNIFY_CANDIDATE_MIN_WIDTH_RATIO: f64 = 0.30;
pub const BODY_FONT_UNIFY_APPLY_TOLERANCE_PT: f64 = 0.08;
pub const BODY_FONT_UNIFY_MAX_SHRINK_PT: f64 = 1.25;
pub const BODY_FONT_UNIFY_GROW_DENSITY_LIMIT: f64 = 1.08;
pub const BODY_FONT_UNIFY_DIRECT_DENSE_MAX_LINES: i64 = 2;
pub const BODY_FONT_PRE_SMOOTH_MIN_LINES: i64 = 2;
pub const BODY_FONT_PRE_SMOOTH_MIN_HEIGHT_PT: f64 = 22.0;

// Annotation font unification. Captions and footnotes follow the same low-anchor
// rule as body text, but their seed fonts are already smaller via role scales.
pub const ANNOTATION_FONT_UNIFY_TARGET_QUANTILE: f64 = 0.25;
pub const ANNOTATION_FONT_UNIFY_EXTREME_SMALL_RATIO: f64 = 0.80;
pub const ANNOTATION_FONT_UNIFY_EXTREME_SMALL_DELTA_PT: f64 = 1.2;
pub const ANNOTATION_FONT_UNIFY_MIN_FILTERED_COUNT: usize = 2;
pub const ANNOTATION_FONT_UNIFY_APPLY_TOLERANCE_PT: f64 = 0.06;
pub const ANNOTATION_FONT_UNIFY_MAX_SHRINK_PT: f64 = 0.9;
pub const CAPTION_BODY_FONT_CAP_RATIO: f64 = 0.88;
pub const CAPTION_FONT_UNIFY_TARGET_BONUS_PT: f64 = 0.0;
pub const CAPTION_FONT_UNIFY_MAX_GROW_PT: f64 = 0.0;
pub const FOOTNOTE_BODY_FONT_CAP_RATIO: f64 = 0.82;
pub const FOOTNOTE_FONT_UNIFY_TARGET_BONUS_PT: f64 = 0.04;
pub const FOOTNOTE_FONT_UNIFY_MAX_GROW_PT: f64 = 0.08;
pub const ANNOTATION_UNDERFILLED_DENSITY_FLOOR_TRIGGER: f64 = BODY_UNDERFILLED_DENSITY_FLOOR_TRIGGER;
pub const ANNOTATION_UNDERFILLED_DENSITY_RECOVERY_TARGET: f64 = BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET;
pub const ANNOTATION_UNDERFILLED_DENSITY_SAFE_MAX: f64 = BODY_UNDERFILLED_DENSITY_SAFE_MAX;
pub const ANNOTATION_UNDERFILLED_RECOVERY_MAX_ITERATIONS: i32 = 5;
pub const CAPTION_UNDERFILLED_RECOVERY_FONT_STEP_PT: f64 = 0.08;
pub const CAPTION_UNDERFILLED_RECOVERY_LEADING_STEP_EM: f64 = 0.025;
pub const CAPTION_UNDERFILLED_RECOVERY_LEADING_CAP_EM: f64 = 0.68;
pub const FOOTNOTE_UNDERFILLED_RECOVERY_FONT_STEP_PT: f64 = 0.12;
pub const FOOTNOTE_UNDERFILLED_RECOVERY_LEADING_STEP_EM: f64 = 0.025;
pub const FOOTNOTE_UNDERFILLED_RECOVERY_LEADING_CAP_EM: f64 = 0.66;

// Body leading recovery. The solver spends remaining vertical slack on line
// spacing after font growth, using source line count/pitch as soft signals.
pub const BODY_COMFORT_LEADING_MIN: f64 = 0.56;
pub const BODY_COMFORT_LEADING_DENSITY_MAX: f64 = 0.985;
pub const BODY_COMFORT_DENSITY_FLOOR_TRIGGER: f64 = BODY_UNDERFILLED_DENSITY_FLOOR_TRIGGER;
pub const BODY_COMFORT_DENSITY_RECOVERY_TARGET: f64 = BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET;
pub const BODY_COMFORT_DENSITY_RECOVERY_TARGET_NO_SOURCE: f64 = BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET_NO_SOURCE;
pub const BODY_COMFORT_DENSITY_RECOVERY_TARGET_SHORT: f64 = BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET_SHORT;
pub const BODY_COMFORT_BASE_FILL: f64 = 0.70;
pub const BODY_COMFORT_TARGET_FILL_MAX: f64 = 0.90;
pub const BODY_COMFORT_LOW_SOURCE_LINE_COUNT_MAX: i64 = 5;
pub const BODY_COMFORT_LOW_SOURCE_LINE_TARGET_FILL_MAX: f64 = 0.78;
pub const BODY_COMFORT_LOW_SOURCE_LINE_LEADING_MAX: f64 = 0.70;
pub const BODY_COMFORT_LONG_LINE_THRESHOLD: i64 = 9;
pub const BODY_COMFORT_LONG_LEADING_MAX: f64 = 0.76;
pub const BODY_COMFORT_NO_SOURCE_LONG_LEADING_MAX: f64 = 0.74;
pub const BODY_COMFORT_SOURCE_LINE_LEADING_MAX: f64 = 1.02;
pub const BODY_COMFORT_SOLVER_MAX_ITERATIONS: i32 = 6;
pub const BODY_COMFORT_SOLVER_DENSITY_TOLERANCE: f64 = 0.002;
pub const BODY_COMFORT_SOLVER_MIN_BRACKET_WIDTH: f64 = 0.005;
pub const BODY_COMFORT_SOLVER_EXTRAPOLATION_MIN_FRACTION: f64 = 0.12;
pub const BODY_COMFORT_SOLVER_EXTRAPOLATION_MAX_FRACTION: f64 = 0.88;
pub const BODY_COMFORT_SLACK_NORMALIZER: f64 = 0.38;
pub const BODY_COMFORT_SOURCE_LINE_RATIO_OFFSET: f64 = 1.0;
pub const BODY_COMFORT_SOURCE_LINE_RATIO_MAX_GAP: f64 = 4.0;
pub const BODY_COMFORT_SOURCE_LEADING_MAX_GAP: f64 = 1.0;
pub const BODY_COMFORT_LINE_COUNT_BASE: f64 = 2.0;
pub const BODY_COMFORT_LINE_COUNT_NORMALIZER: f64 = 12.0;
pub const BODY_COMFORT_SLACK_GAIN_MAX: f64 = 0.08;
pub const BODY_COMFORT_SOURCE_LINE_GAIN_MAX: f64 = 0.12;
pub const BODY_COMFORT_SOURCE_PITCH_GAIN_MAX: f64 = 0.05;
pub const BODY_COMFORT_MULTI_LINE_GAIN_MAX: f64 = 0.10;
pub const BODY_COMFORT_FONT_GROWTH_GAIN_MAX: f64 = 0.03;
pub const BODY_COMFORT_FONT_GROWTH_MIN_LEADING_GAIN_MAX: f64 = 0.12;
pub const BODY_COMFORT_FONT_GROWTH_MIN_LEADING_RATE: f64 = 1.8;
pub const BODY_COMFORT_LEADING_GROWTH_BASE_MAX: f64 = 0.10;
pub const BODY_COMFORT_LEADING_GROWTH_SOURCE_MAX: f64 = 0.28;
pub const BODY_COMFORT_LEADING_GROWTH_LONG_TEXT_MAX: f64 = 0.08;
pub const BODY_COMFORT_LEADING_FONT_SPEND_PENALTY_MAX: f64 = 0.20;
pub const BODY_COMFORT_LEADING_GROWTH_MIN_AFTER_FONT_GROWTH: f64 = 0.04;
pub const BODY_COMFORT_PAGE_BASELINE_DENSITY_WEIGHT: f64 = 0.10;
pub const BODY_COMFORT_PAGE_BASELINE_MAX_DENSITY_DELTA: f64 = 0.20;
pub const BODY_COMFORT_SLACK_RESPONSE_RATE: f64 = 2.0;
pub const BODY_COMFORT_MULTI_LINE_WEIGHT_RATE: f64 = 0.42;
pub const BODY_COMFORT_SOURCE_LINE_RESPONSE_RATE: f64 = 0.78;
pub const BODY_COMFORT_SOURCE_LINE_VOLUME_BASE: f64 = 6.0;
pub const BODY_COMFORT_SOURCE_LINE_VOLUME_RESPONSE_RATE: f64 = 0.45;
pub const BODY_COMFORT_SOURCE_PITCH_RESPONSE_RATE: f64 = 3.2;
pub const BODY_COMFORT_LINE_COUNT_RESPONSE_RATE: f64 = 2.1;
pub const BODY_COMFORT_LONG_LINE_CAP_RESPONSE_RATE: f64 = 0.38;
pub const BODY_COMFORT_SOURCE_LINE_CAP_RESPONSE_RATE: f64 = 0.7;
pub const BODY_COMFORT_SOURCE_PITCH_CAP_RESPONSE_RATE: f64 = 1.1;
pub const BODY_COMFORT_FONT_GROWTH_NORMALIZER_PT: f64 = 1.2;

// Tall OCR bboxes often include source paragraph slack rather than usable text
// height. Use an effective height for density decisions so body rhythm remains
// closer to the page/book font target instead of chasing excessive whitespace.
pub const BODY_TALL_BBOX_MIN_LINES: i64 = 3;
pub const BODY_TALL_BBOX_MIN_HEIGHT_PT: f64 = 35.0;
pub const BODY_TALL_BBOX_HEIGHT_RATIO_TRIGGER: f64 = 1.8;
pub const BODY_TALL_BBOX_PAGE_RATIO_MULTIPLIER: f64 = 1.35;
pub const BODY_TALL_BBOX_EFFECTIVE_NATURAL_MULTIPLIER: f64 = 1.45;
pub const BODY_TALL_BBOX_EFFECTIVE_MIN_ORIGINAL_RATIO: f64 = 0.58;

// Adjacent collision protection. Once body font unification has selected a page
// body font, collision handling should not break that rhythm with a second font
// fit pass. It may only compress leading.
pub const BODY_COLLISION_UNIFIED_MIN_LEADING_EM: f64 = 0.52;
pub const BODY_COLLISION_UNIFIED_FONT_TOLERANCE_PT: f64 = 0.08;

// --- TypographyCapacity capacity model ---------------------------------------
//
// The closed-form counterpart to the density heuristics above. A block's render
// height is `line_count * line_step * formula_discount` with
// `line_step = font_size * max(1.02, 1 + leading_em)`, so density is linear in
// both font and (above the leading floor) leading — giving exact inverse solvers
// instead of the bisection/stepping loops. B1 reparameterization wires every
// strategy family's constants into `CapacityBudget` (presets per family, all
// carrying the current constant values so behavior stays identical); B2 adopts
// the closed-form solvers and lets the presets diverge.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypographyCapacity {
    pub box_height_pt: f64,
    pub line_count: i64,
    pub font_size_pt: f64,
    pub leading_em: f64,
    pub discount: f64,
}

impl TypographyCapacity {
    /// `from_payload`: build the model from a block payload dict. None when the
    /// inner bbox or font is missing.
    pub fn from_payload(payload: &Value) -> Option<Self> {
        let bbox = inner_bbox4(payload)?;
        let effective = payload_f64(payload, "density_effective_height_pt", 0.0);
        let box_height_pt = if effective > 0.0 {
            effective.max(8.0)
        } else {
            (bbox[3] - bbox[1]).max(8.0)
        };
        let font_size_pt = payload_f64(payload, "font_size_pt", 0.0);
        if font_size_pt <= 0.0 || box_height_pt <= 0.0 {
            return None;
        }
        let text = payload_string(payload, "translated_text", "");
        let fm = formula_map(payload.get("formula_map"));
        let line_count = estimated_required_lines(&bbox, &text, &fm, font_size_pt).max(1);
        let discount = formula_estimate_discount(&text, &fm);
        Some(Self {
            box_height_pt,
            line_count,
            font_size_pt,
            leading_em: payload_f64(payload, "leading_em", 0.0),
            discount,
        })
    }

    pub fn line_step_pt(&self) -> f64 {
        self.font_size_pt * (1.02_f64).max(1.0 + self.leading_em)
    }

    pub fn estimated_height_pt(&self) -> f64 {
        self.line_count as f64 * self.line_step_pt() * self.discount
    }

    pub fn density(&self) -> f64 {
        if self.box_height_pt <= 0.0 {
            return 0.0;
        }
        self.estimated_height_pt() / self.box_height_pt
    }

    /// Exact inverse: the font that reaches `target` density holding the leading
    /// floor, line count and discount fixed (density is linear in font).
    pub fn font_for_density(&self, target: f64) -> f64 {
        if self.box_height_pt <= 0.0 || self.line_count <= 0 || target <= 0.0 {
            return self.font_size_pt;
        }
        let denominator = self.line_count as f64 * (1.02_f64).max(1.0 + self.leading_em) * self.discount.max(1e-6);
        if denominator <= 0.0 {
            return self.font_size_pt;
        }
        target * self.box_height_pt / denominator
    }

    /// Exact inverse: the leading that reaches `target` density holding font,
    /// line count and discount fixed. 0 when the leading floor already overfills.
    pub fn leading_for_density(&self, target: f64) -> f64 {
        if self.box_height_pt <= 0.0 || self.font_size_pt <= 0.0 || self.line_count <= 0 || target <= 0.0 {
            return self.leading_em;
        }
        let denominator = self.line_count as f64 * self.font_size_pt * self.discount.max(1e-6);
        if denominator <= 0.0 {
            return self.leading_em;
        }
        let w = target * self.box_height_pt / denominator;
        if w <= 1.02 {
            0.0
        } else {
            (w - 1.0).max(0.0)
        }
    }

    /// Closed-form counterpart to the stepping recovery solver: grow font toward
    /// the recovery target (capped by the grow budget), then leading toward the
    /// target within the safe density limit.
    pub fn fit_with(&self, budget: &CapacityBudget) -> TypographyFit {
        let mut cap = *self;
        let density = cap.density();
        if density > 0.0 && density < budget.font_grow_density_trigger {
            let target_font = cap.font_for_density(budget.density_recovery_target);
            let fitted_font = target_font.min(cap.font_size_pt + budget.font_grow_max_pt).max(cap.font_size_pt);
            cap.font_size_pt = fitted_font;
        }
        if cap.density() < budget.density_recovery_target {
            let target_leading = cap.leading_for_density(budget.density_recovery_target);
            let safe_leading = cap.leading_for_density(budget.density_safe_max);
            cap.leading_em = target_leading.min(safe_leading).max(cap.leading_em);
        }
        TypographyFit {
            font_size_pt: cap.font_size_pt,
            leading_em: cap.leading_em,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypographyFit {
    pub font_size_pt: f64,
    pub leading_em: f64,
}

/// Tuning parameters for the capacity model. Presets carry the current constant
/// values so heuristics degrade into model parameters (B1 reparameterization).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapacityBudget {
    pub density_floor_trigger: f64,
    pub density_recovery_target: f64,
    pub density_recovery_target_no_source: f64,
    pub density_recovery_target_short: f64,
    pub density_safe_max: f64,
    pub recovery_max_iterations: i32,
    pub recovery_font_step_pt: f64,
    pub recovery_leading_step_em: f64,
    pub unified_font_max_step_pt: f64,
    pub font_grow_density_trigger: f64,
    pub font_grow_density_limit: f64,
    pub font_grow_max_pt: f64,
    pub font_grow_context_bonus_pt: f64,
    pub font_grow_page_bonus_pt: f64,
    pub font_grow_exp_rate: f64,
    pub font_grow_max_lines: i32,
    pub font_grow_short_line_bonus: f64,
    pub font_grow_tall_slack_bonus: f64,
    pub font_grow_source_line_bonus_pt: f64,
    pub font_grow_source_line_cap_bonus_pt: f64,
    pub font_grow_source_line_density_bonus: f64,
    pub font_grow_source_line_ratio_offset: f64,
    pub font_grow_source_line_ratio_range: f64,
    pub font_harmonize_max_ratio: f64,
    pub font_grow_min_lines: i32,
    pub font_grow_min_height_pt: f64,
    pub font_grow_short_max_pt: f64,
    pub font_grow_low_font_skip_delta_pt: f64,
    pub leading_min: f64,
    pub leading_density_max: f64,
    pub low_source_line_count_max: i64,
    pub low_source_line_target_fill_max: f64,
    pub low_source_line_leading_max: f64,
    pub long_line_threshold: i64,
    pub long_leading_max: f64,
    pub no_source_long_leading_max: f64,
    pub source_line_leading_max: f64,
    pub solver_max_iterations: i32,
    pub solver_density_tolerance: f64,
    pub solver_min_bracket_width: f64,
    pub solver_extrapolation_min_fraction: f64,
    pub solver_extrapolation_max_fraction: f64,
    pub line_count_base: f64,
    pub line_count_response_rate: f64,
    pub long_line_cap_response_rate: f64,
    pub multi_line_weight_rate: f64,
    pub source_line_cap_response_rate: f64,
    pub source_line_volume_base: f64,
    pub source_line_volume_response_rate: f64,
    pub source_pitch_cap_response_rate: f64,
    pub source_pitch_response_rate: f64,
    pub font_growth_min_leading_gain_max: f64,
    pub font_growth_min_leading_rate: f64,
    pub font_growth_normalizer_pt: f64,
    pub leading_font_spend_penalty_max: f64,
    pub leading_growth_base_max: f64,
    pub leading_growth_long_text_max: f64,
    pub leading_growth_min_after_font_growth: f64,
    pub leading_growth_source_max: f64,
    pub unify_anchor_count: usize,
    pub unify_anchor_min_height_pt: f64,
    pub unify_anchor_min_width_ratio: f64,
    pub unify_anchor_max_density: f64,
    pub unify_target_quantile: f64,
    pub unify_extreme_small_ratio: f64,
    pub unify_extreme_small_delta_pt: f64,
    pub unify_min_filtered_count: usize,
    pub unify_candidate_min_width_ratio: f64,
    pub unify_apply_tolerance_pt: f64,
    pub unify_grow_density_limit: f64,
    pub unify_direct_dense_max_lines: i64,
    pub page_anchor_count: usize,
    pub page_anchor_min_height_pt: f64,
    pub page_anchor_min_lines: i64,
    pub page_anchor_min_width_ratio: f64,
    pub page_anchor_apply_density_limit: f64,
    pub annotation_unify_target_quantile: f64,
    pub annotation_unify_extreme_small_ratio: f64,
    pub annotation_unify_extreme_small_delta_pt: f64,
    pub annotation_unify_min_filtered_count: usize,
    pub annotation_unify_apply_tolerance_pt: f64,
    pub annotation_unify_max_shrink_pt: f64,
    pub caption_body_font_cap_ratio: f64,
    pub caption_unify_target_bonus_pt: f64,
    pub caption_unify_max_grow_pt: f64,
    pub footnote_body_font_cap_ratio: f64,
    pub footnote_unify_target_bonus_pt: f64,
    pub footnote_unify_max_grow_pt: f64,
    pub annotation_density_floor_trigger: f64,
    pub annotation_density_recovery_target: f64,
    pub annotation_density_safe_max: f64,
    pub annotation_recovery_max_iterations: i32,
    pub caption_recovery_font_step_pt: f64,
    pub caption_recovery_leading_step_em: f64,
    pub caption_recovery_leading_cap_em: f64,
    pub footnote_recovery_font_step_pt: f64,
    pub footnote_recovery_leading_step_em: f64,
    pub footnote_recovery_leading_cap_em: f64,
    pub tall_bbox_min_lines: i64,
    pub tall_bbox_min_height_pt: f64,
    pub tall_bbox_height_ratio_trigger: f64,
    pub tall_bbox_page_ratio_multiplier: f64,
    pub tall_bbox_effective_natural_multiplier: f64,
    pub tall_bbox_effective_min_original_ratio: f64,
}

impl Default for CapacityBudget {
    /// Current constants across every strategy family (B1 reparameterization).
    fn default() -> Self {
        Self {
            density_floor_trigger: BODY_UNDERFILLED_DENSITY_FLOOR_TRIGGER,
            density_recovery_target: BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET,
            density_recovery_target_no_source: BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET_NO_SOURCE,
            density_recovery_target_short: BODY_UNDERFILLED_DENSITY_RECOVERY_TARGET_SHORT,
            density_safe_max: BODY_UNDERFILLED_DENSITY_SAFE_MAX,
            recovery_max_iterations: BODY_UNDERFILLED_RECOVERY_MAX_ITERATIONS,
            recovery_font_step_pt: BODY_UNDERFILLED_RECOVERY_FONT_STEP_PT,
            recovery_leading_step_em: BODY_UNDERFILLED_RECOVERY_LEADING_STEP_EM,
            unified_font_max_step_pt: BODY_UNDERFILLED_UNIFIED_FONT_MAX_STEP_PT,
            font_grow_density_trigger: BODY_UNDERFILLED_FONT_GROW_DENSITY_TRIGGER,
            font_grow_density_limit: BODY_UNDERFILLED_FONT_GROW_DENSITY_LIMIT,
            font_grow_max_pt: BODY_UNDERFILLED_FONT_GROW_MAX_PT,
            font_grow_context_bonus_pt: BODY_UNDERFILLED_FONT_GROW_CONTEXT_BONUS_PT,
            font_grow_page_bonus_pt: BODY_UNDERFILLED_FONT_GROW_PAGE_BONUS_PT,
            font_grow_exp_rate: BODY_UNDERFILLED_FONT_GROW_EXP_RATE,
            font_grow_max_lines: BODY_UNDERFILLED_FONT_GROW_MAX_LINES,
            font_grow_short_line_bonus: BODY_UNDERFILLED_FONT_GROW_SHORT_LINE_BONUS,
            font_grow_tall_slack_bonus: BODY_UNDERFILLED_FONT_GROW_TALL_SLACK_BONUS,
            font_grow_source_line_bonus_pt: BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_BONUS_PT,
            font_grow_source_line_cap_bonus_pt: BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_CAP_BONUS_PT,
            font_grow_source_line_density_bonus: BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_DENSITY_BONUS,
            font_grow_source_line_ratio_offset: BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_RATIO_OFFSET,
            font_grow_source_line_ratio_range: BODY_UNDERFILLED_FONT_GROW_SOURCE_LINE_RATIO_RANGE,
            font_harmonize_max_ratio: BODY_UNDERFILLED_FONT_HARMONIZE_MAX_RATIO,
            font_grow_min_lines: BODY_UNDERFILLED_FONT_GROW_MIN_LINES,
            font_grow_min_height_pt: BODY_UNDERFILLED_FONT_GROW_MIN_HEIGHT_PT,
            font_grow_short_max_pt: BODY_UNDERFILLED_FONT_GROW_SHORT_MAX_PT,
            font_grow_low_font_skip_delta_pt: BODY_UNDERFILLED_FONT_GROW_LOW_FONT_SKIP_DELTA_PT,
            leading_min: BODY_COMFORT_LEADING_MIN,
            leading_density_max: BODY_COMFORT_LEADING_DENSITY_MAX,
            low_source_line_count_max: BODY_COMFORT_LOW_SOURCE_LINE_COUNT_MAX,
            low_source_line_target_fill_max: BODY_COMFORT_LOW_SOURCE_LINE_TARGET_FILL_MAX,
            low_source_line_leading_max: BODY_COMFORT_LOW_SOURCE_LINE_LEADING_MAX,
            long_line_threshold: BODY_COMFORT_LONG_LINE_THRESHOLD,
            long_leading_max: BODY_COMFORT_LONG_LEADING_MAX,
            no_source_long_leading_max: BODY_COMFORT_NO_SOURCE_LONG_LEADING_MAX,
            source_line_leading_max: BODY_COMFORT_SOURCE_LINE_LEADING_MAX,
            solver_max_iterations: BODY_COMFORT_SOLVER_MAX_ITERATIONS,
            solver_density_tolerance: BODY_COMFORT_SOLVER_DENSITY_TOLERANCE,
            solver_min_bracket_width: BODY_COMFORT_SOLVER_MIN_BRACKET_WIDTH,
            solver_extrapolation_min_fraction: BODY_COMFORT_SOLVER_EXTRAPOLATION_MIN_FRACTION,
            solver_extrapolation_max_fraction: BODY_COMFORT_SOLVER_EXTRAPOLATION_MAX_FRACTION,
            line_count_base: BODY_COMFORT_LINE_COUNT_BASE,
            line_count_response_rate: BODY_COMFORT_LINE_COUNT_RESPONSE_RATE,
            long_line_cap_response_rate: BODY_COMFORT_LONG_LINE_CAP_RESPONSE_RATE,
            multi_line_weight_rate: BODY_COMFORT_MULTI_LINE_WEIGHT_RATE,
            source_line_cap_response_rate: BODY_COMFORT_SOURCE_LINE_CAP_RESPONSE_RATE,
            source_line_volume_base: BODY_COMFORT_SOURCE_LINE_VOLUME_BASE,
            source_line_volume_response_rate: BODY_COMFORT_SOURCE_LINE_VOLUME_RESPONSE_RATE,
            source_pitch_cap_response_rate: BODY_COMFORT_SOURCE_PITCH_CAP_RESPONSE_RATE,
            source_pitch_response_rate: BODY_COMFORT_SOURCE_PITCH_RESPONSE_RATE,
            font_growth_min_leading_gain_max: BODY_COMFORT_FONT_GROWTH_MIN_LEADING_GAIN_MAX,
            font_growth_min_leading_rate: BODY_COMFORT_FONT_GROWTH_MIN_LEADING_RATE,
            font_growth_normalizer_pt: BODY_COMFORT_FONT_GROWTH_NORMALIZER_PT,
            leading_font_spend_penalty_max: BODY_COMFORT_LEADING_FONT_SPEND_PENALTY_MAX,
            leading_growth_base_max: BODY_COMFORT_LEADING_GROWTH_BASE_MAX,
            leading_growth_long_text_max: BODY_COMFORT_LEADING_GROWTH_LONG_TEXT_MAX,
            leading_growth_min_after_font_growth: BODY_COMFORT_LEADING_GROWTH_MIN_AFTER_FONT_GROWTH,
            leading_growth_source_max: BODY_COMFORT_LEADING_GROWTH_SOURCE_MAX,
            unify_anchor_count: BODY_FONT_UNIFY_ANCHOR_COUNT,
            unify_anchor_min_height_pt: BODY_FONT_UNIFY_ANCHOR_MIN_HEIGHT_PT,
            unify_anchor_min_width_ratio: BODY_FONT_UNIFY_ANCHOR_MIN_WIDTH_RATIO,
            unify_anchor_max_density: BODY_FONT_UNIFY_ANCHOR_MAX_DENSITY,
            unify_target_quantile: BODY_FONT_UNIFY_TARGET_QUANTILE,
            unify_extreme_small_ratio: BODY_FONT_UNIFY_EXTREME_SMALL_RATIO,
            unify_extreme_small_delta_pt: BODY_FONT_UNIFY_EXTREME_SMALL_DELTA_PT,
            unify_min_filtered_count: BODY_FONT_UNIFY_MIN_FILTERED_COUNT,
            unify_candidate_min_width_ratio: BODY_FONT_UNIFY_CANDIDATE_MIN_WIDTH_RATIO,
            unify_apply_tolerance_pt: BODY_FONT_UNIFY_APPLY_TOLERANCE_PT,
            unify_grow_density_limit: BODY_FONT_UNIFY_GROW_DENSITY_LIMIT,
            unify_direct_dense_max_lines: BODY_FONT_UNIFY_DIRECT_DENSE_MAX_LINES,
            page_anchor_count: PAGE_BODY_FONT_ANCHOR_COUNT,
            page_anchor_min_height_pt: PAGE_BODY_FONT_ANCHOR_MIN_HEIGHT_PT,
            page_anchor_min_lines: PAGE_BODY_FONT_ANCHOR_MIN_LINES,
            page_anchor_min_width_ratio: PAGE_BODY_FONT_ANCHOR_MIN_WIDTH_RATIO,
            page_anchor_apply_density_limit: PAGE_BODY_FONT_ANCHOR_APPLY_DENSITY_LIMIT,
            annotation_unify_target_quantile: ANNOTATION_FONT_UNIFY_TARGET_QUANTILE,
            annotation_unify_extreme_small_ratio: ANNOTATION_FONT_UNIFY_EXTREME_SMALL_RATIO,
            annotation_unify_extreme_small_delta_pt: ANNOTATION_FONT_UNIFY_EXTREME_SMALL_DELTA_PT,
            annotation_unify_min_filtered_count: ANNOTATION_FONT_UNIFY_MIN_FILTERED_COUNT,
            annotation_unify_apply_tolerance_pt: ANNOTATION_FONT_UNIFY_APPLY_TOLERANCE_PT,
            annotation_unify_max_shrink_pt: ANNOTATION_FONT_UNIFY_MAX_SHRINK_PT,
            caption_body_font_cap_ratio: CAPTION_BODY_FONT_CAP_RATIO,
            caption_unify_target_bonus_pt: CAPTION_FONT_UNIFY_TARGET_BONUS_PT,
            caption_unify_max_grow_pt: CAPTION_FONT_UNIFY_MAX_GROW_PT,
            footnote_body_font_cap_ratio: FOOTNOTE_BODY_FONT_CAP_RATIO,
            footnote_unify_target_bonus_pt: FOOTNOTE_FONT_UNIFY_TARGET_BONUS_PT,
            footnote_unify_max_grow_pt: FOOTNOTE_FONT_UNIFY_MAX_GROW_PT,
            annotation_density_floor_trigger: ANNOTATION_UNDERFILLED_DENSITY_FLOOR_TRIGGER,
            annotation_density_recovery_target: ANNOTATION_UNDERFILLED_DENSITY_RECOVERY_TARGET,
            annotation_density_safe_max: ANNOTATION_UNDERFILLED_DENSITY_SAFE_MAX,
            annotation_recovery_max_iterations: ANNOTATION_UNDERFILLED_RECOVERY_MAX_ITERATIONS,
            caption_recovery_font_step_pt: CAPTION_UNDERFILLED_RECOVERY_FONT_STEP_PT,
            caption_recovery_leading_step_em: CAPTION_UNDERFILLED_RECOVERY_LEADING_STEP_EM,
            caption_recovery_leading_cap_em: CAPTION_UNDERFILLED_RECOVERY_LEADING_CAP_EM,
            footnote_recovery_font_step_pt: FOOTNOTE_UNDERFILLED_RECOVERY_FONT_STEP_PT,
            footnote_recovery_leading_step_em: FOOTNOTE_UNDERFILLED_RECOVERY_LEADING_STEP_EM,
            footnote_recovery_leading_cap_em: FOOTNOTE_UNDERFILLED_RECOVERY_LEADING_CAP_EM,
            tall_bbox_min_lines: BODY_TALL_BBOX_MIN_LINES,
            tall_bbox_min_height_pt: BODY_TALL_BBOX_MIN_HEIGHT_PT,
            tall_bbox_height_ratio_trigger: BODY_TALL_BBOX_HEIGHT_RATIO_TRIGGER,
            tall_bbox_page_ratio_multiplier: BODY_TALL_BBOX_PAGE_RATIO_MULTIPLIER,
            tall_bbox_effective_natural_multiplier: BODY_TALL_BBOX_EFFECTIVE_NATURAL_MULTIPLIER,
            tall_bbox_effective_min_original_ratio: BODY_TALL_BBOX_EFFECTIVE_MIN_ORIGINAL_RATIO,
        }
    }
}

impl CapacityBudget {
    /// Underfilled body font-growth budget. Today all presets carry the same
    /// constants (B1 keeps behavior identical); they diverge when the capacity
    /// model adopts closed-form fits (B2).
    pub fn body_underfilled() -> Self {
        Self::default()
    }
    /// Body leading-recovery (comfort) budget.
    pub fn body_comfort_leading() -> Self {
        Self::default()
    }
    /// Body font-unify budget.
    pub fn font_unify() -> Self {
        Self::default()
    }
    /// Page body-font anchor budget.
    pub fn page_anchor() -> Self {
        Self::default()
    }
    /// Caption/footnote annotation font-unify + recovery budget.
    pub fn annotation_caption() -> Self {
        Self::default()
    }
    /// Tall-bbox effective-height density budget.
    pub fn body_density() -> Self {
        Self::default()
    }
}

pub const FONT_GROWTH_DECISION_KEY: &str = "_body_font_growth_decision";
pub const LEADING_DECISION_KEY: &str = "_body_leading_decision";
pub const PAGE_ANCHOR_DECISION_KEY: &str = "_page_body_anchor_decision";
pub const VERTICAL_BUDGET_KEY: &str = "_body_vertical_budget";

#[derive(Debug, Clone)]
pub struct FontGrowthDecision {
    pub seed_font_pt: f64,
    pub target_font_pt: f64,
    pub slack_ratio: f64,
    pub reason: String,
}

impl FontGrowthDecision {
    pub fn new(seed_font_pt: f64, target_font_pt: f64, slack_ratio: f64) -> Self {
        Self {
            seed_font_pt,
            target_font_pt,
            slack_ratio,
            reason: "underfilled_body".to_string(),
        }
    }

    pub fn grew_pt(&self) -> f64 {
        (self.target_font_pt - self.seed_font_pt).max(0.0)
    }

    pub fn to_payload(&self) -> Value {
        json!({
            "seed_font_pt": py_round(self.seed_font_pt, 3),
            "target_font_pt": py_round(self.target_font_pt, 3),
            "grew_pt": py_round(self.grew_pt(), 3),
            "slack_ratio": py_round(self.slack_ratio, 3),
            "reason": self.reason,
        })
    }
}

#[derive(Debug, Clone)]
pub struct LeadingDecision {
    pub leading_em: f64,
    pub target_density: f64,
    pub leading_cap_em: f64,
    pub refit_after_font_unify: bool,
}

impl LeadingDecision {
    pub fn new(leading_em: f64, target_density: f64, leading_cap_em: f64) -> Self {
        Self {
            leading_em,
            target_density,
            leading_cap_em,
            refit_after_font_unify: false,
        }
    }

    pub fn to_payload(&self) -> Value {
        json!({
            "leading_em": py_round(self.leading_em, 3),
            "target_density": py_round(self.target_density, 3),
            "leading_cap_em": py_round(self.leading_cap_em, 3),
            "refit_after_font_unify": self.refit_after_font_unify,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PageBodyAnchorDecision {
    pub target_font_pt: f64,
    pub applied: bool,
}

impl PageBodyAnchorDecision {
    pub fn to_payload(&self) -> Value {
        json!({
            "target_font_pt": py_round(self.target_font_pt, 3),
            "applied": self.applied,
        })
    }
}

#[derive(Debug, Clone)]
pub struct VerticalBudget {
    pub font_growth_pt: f64,
    pub leading_growth_em: f64,
    pub target_density: f64,
    pub leading_cap_em: f64,
}

impl VerticalBudget {
    pub fn to_payload(&self) -> Value {
        json!({
            "font_growth_pt": py_round(self.font_growth_pt.max(0.0), 3),
            "leading_growth_em": py_round(self.leading_growth_em.max(0.0), 3),
            "target_density": py_round(self.target_density, 3),
            "leading_cap_em": py_round(self.leading_cap_em, 3),
        })
    }
}

fn payload_f64(payload: &Value, key: &str, default: f64) -> f64 {
    payload.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
}

pub fn set_font_growth_decision(payload: &mut Value, decision: &FontGrowthDecision) {
    let as_obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    as_obj.insert(FONT_GROWTH_DECISION_KEY.to_string(), decision.to_payload());
    as_obj.insert(
        "_body_underfill_seed_font_pt".to_string(),
        json!(py_round(decision.seed_font_pt, 2)),
    );
    as_obj.insert(
        "_body_underfill_font_grew_pt".to_string(),
        json!(py_round(decision.grew_pt(), 2)),
    );
    as_obj.insert(
        "_body_underfill_font_slack_ratio".to_string(),
        json!(py_round(decision.slack_ratio, 3)),
    );
}

fn decision_dict<'a>(payload: &'a Value, key: &str) -> Option<&'a serde_json::Map<String, Value>> {
    payload.get(key).and_then(|v| v.as_object())
}

pub fn font_growth_grew_pt(payload: &Value) -> f64 {
    if let Some(decision) = decision_dict(payload, FONT_GROWTH_DECISION_KEY) {
        return decision.get("grew_pt").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }
    payload_f64(payload, "_body_underfill_font_grew_pt", 0.0)
}

pub fn font_growth_seed_font_pt(payload: &Value, fallback: f64) -> f64 {
    if let Some(decision) = decision_dict(payload, FONT_GROWTH_DECISION_KEY) {
        return decision
            .get("seed_font_pt")
            .and_then(|v| v.as_f64())
            .filter(|v| *v != 0.0)
            .unwrap_or(fallback);
    }
    let direct = payload_f64(payload, "_body_underfill_seed_font_pt", 0.0);
    if direct == 0.0 {
        fallback
    } else {
        direct
    }
}

pub fn font_growth_slack_ratio(payload: &Value) -> f64 {
    if let Some(decision) = decision_dict(payload, FONT_GROWTH_DECISION_KEY) {
        return decision.get("slack_ratio").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }
    payload_f64(payload, "_body_underfill_font_slack_ratio", 0.0)
}

pub fn set_leading_decision(payload: &mut Value, decision: &LeadingDecision) {
    let as_obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    as_obj.insert(LEADING_DECISION_KEY.to_string(), decision.to_payload());
    as_obj.insert(
        "_body_dynamic_leading_cap_em".to_string(),
        json!(py_round(decision.leading_cap_em, 2)),
    );
    as_obj.insert(
        "_body_leading_target_density".to_string(),
        json!(py_round(decision.target_density, 3)),
    );
    if decision.refit_after_font_unify {
        as_obj.insert("_body_leading_refit_after_font_unify".to_string(), json!(true));
    }
}

pub fn leading_refit_after_font_unify(payload: &Value) -> bool {
    if let Some(decision) = decision_dict(payload, LEADING_DECISION_KEY) {
        if decision.get("refit_after_font_unify").and_then(|v| v.as_bool()).unwrap_or(false) {
            return true;
        }
    }
    payload
        .get("_body_leading_refit_after_font_unify")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn set_page_body_anchor_decision(payload: &mut Value, decision: &PageBodyAnchorDecision) {
    let as_obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    as_obj.insert(PAGE_ANCHOR_DECISION_KEY.to_string(), decision.to_payload());
    as_obj.insert(
        "_page_body_anchor_font_pt".to_string(),
        json!(py_round(decision.target_font_pt, 2)),
    );
    if decision.applied {
        as_obj.insert("_page_body_anchor_font_applied".to_string(), json!(true));
    }
}

pub fn set_vertical_budget(payload: &mut Value, budget: &VerticalBudget) {
    let as_obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    as_obj.insert(VERTICAL_BUDGET_KEY.to_string(), budget.to_payload());
}

pub const TYPOGRAPHY_MEMORY_FEATURE_VERSION: &str = "typography_memory_features_v1";

#[derive(Debug, Clone, PartialEq)]
pub struct TypographyFeature {
    pub key: String,
    pub payload: serde_json::Map<String, serde_json::Value>,
}

fn linear_bin(value: f64, step: f64) -> i64 {
    py_round(value.max(0.0) / step.max(0.1), 0) as i64
}

fn log_bin(value: usize) -> i64 {
    py_round((value as f64).max(0.0).ln_1p() * 4.0, 0) as i64
}

fn ratio_bin(value: f64) -> i64 {
    py_round(value.max(0.0).min(12.0) * 10.0, 0) as i64
}

fn blake2b_hex(data: &str, digest_size: usize) -> String {
    let mut hasher = Blake2bVar::new(digest_size).expect("valid digest size");
    hasher.update(data.as_bytes());
    let mut out = vec![0u8; digest_size];
    hasher.finalize_variable(&mut out).expect("hash output fits");
    let mut hex = String::with_capacity(digest_size * 2);
    for byte in out {
        hex.push_str(&format!("{:02x}", byte));
    }
    hex
}

pub fn build_typography_feature(
    item: &Item,
    translated_text: &str,
    font_size_pt: f64,
    leading_em: f64,
    page_width: Option<f64>,
    page_height: Option<f64>,
    page_text_width_med: f64,
    is_body: bool,
    dense_small_box: bool,
    heavy_dense_small_box: bool,
    wide_aspect_body_text: bool,
    preserve_line_breaks: bool,
) -> Option<TypographyFeature> {
    let width = bbox_width(item);
    let height = bbox_height(item);
    if width <= 0.0 || height <= 0.0 || font_size_pt <= 0.0 || leading_em <= 0.0 {
        return None;
    }
    let page_width = page_width.unwrap_or(0.0);
    let page_height = page_height.unwrap_or(0.0);
    let source_words = source_word_count(item);
    let zh_chars = translated_zh_char_count(translated_text);

    let mut payload = serde_json::Map::new();
    payload.insert("version".to_string(), serde_json::Value::String(TYPOGRAPHY_MEMORY_FEATURE_VERSION.to_string()));
    payload.insert("block_kind".to_string(), serde_json::Value::String(block_kind(item)));
    payload.insert("layout_role".to_string(), serde_json::Value::String(layout_role(item)));
    payload.insert("semantic_role".to_string(), serde_json::Value::String(semantic_role(item)));
    payload.insert("structure_role".to_string(), serde_json::Value::String(structure_role(item)));
    payload.insert("title".to_string(), serde_json::Value::Bool(is_title_like_block(item)));
    payload.insert("body".to_string(), serde_json::Value::Bool(is_body));
    payload.insert("dense".to_string(), serde_json::Value::Bool(dense_small_box));
    payload.insert("heavy_dense".to_string(), serde_json::Value::Bool(heavy_dense_small_box));
    payload.insert("wide_body".to_string(), serde_json::Value::Bool(wide_aspect_body_text));
    payload.insert("preserve_lines".to_string(), serde_json::Value::Bool(preserve_line_breaks));
    payload.insert("w_bin".to_string(), serde_json::Value::from(linear_bin(width, 12.0)));
    payload.insert("h_bin".to_string(), serde_json::Value::from(linear_bin(height, 8.0)));
    payload.insert("aspect_bin".to_string(), serde_json::Value::from(ratio_bin(width / height.max(1.0))));
    payload.insert(
        "page_w_bin".to_string(),
        if page_width > 0.0 {
            serde_json::Value::from(linear_bin(page_width, 40.0))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "page_h_bin".to_string(),
        if page_height > 0.0 {
            serde_json::Value::from(linear_bin(page_height, 40.0))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "rel_w_bin".to_string(),
        if page_width > 0.0 {
            serde_json::Value::from(ratio_bin(width / page_width.max(1.0)))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "rel_h_bin".to_string(),
        if page_height > 0.0 {
            serde_json::Value::from(ratio_bin(height / page_height.max(1.0)))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "text_w_bin".to_string(),
        if page_text_width_med > 0.0 {
            serde_json::Value::from(linear_bin(page_text_width_med, 12.0))
        } else {
            serde_json::Value::from(0)
        },
    );
    payload.insert(
        "source_lines_bin".to_string(),
        serde_json::Value::from(12.min(source_visual_line_count(item) as i64)),
    );
    payload.insert("source_words_bin".to_string(), serde_json::Value::from(log_bin(source_words)));
    payload.insert("zh_chars_bin".to_string(), serde_json::Value::from(log_bin(zh_chars)));
    payload.insert(
        "density_bin".to_string(),
        serde_json::Value::from(ratio_bin(translation_density_ratio(item, translated_text))),
    );
    payload.insert("formula_bin".to_string(), serde_json::Value::from(ratio_bin(formula_ratio(item))));

    let raw = serde_json::to_string(&payload).expect("feature payload serializes");
    let key = blake2b_hex(&raw, 16);
    Some(TypographyFeature { key, payload })
}

#[cfg(test)]
mod tests_decision {
    use super::*;
    use serde_json::json;

    #[test]
    fn leading_decision_round_trips() {
        let mut payload = json!({"font_size_pt": 12.0});
        set_leading_decision(
            &mut payload,
            &LeadingDecision {
                leading_em: 0.34,
                target_density: 0.8,
                leading_cap_em: 0.66,
                refit_after_font_unify: true,
            },
        );
        assert!(leading_refit_after_font_unify(&payload));
        assert_eq!(payload["_body_dynamic_leading_cap_em"], json!(0.66));
        assert_eq!(payload["_body_leading_target_density"], json!(0.8));
        assert!(!leading_refit_after_font_unify(&json!({})));
    }

    #[test]
    fn font_growth_reads_written_values() {
        let mut payload = json!({});
        set_font_growth_decision(&mut payload, &FontGrowthDecision::new(11.0, 12.0, 0.5));
        assert_eq!(font_growth_grew_pt(&payload), 1.0);
        assert_eq!(font_growth_seed_font_pt(&payload, 99.0), 11.0);
        assert_eq!(font_growth_slack_ratio(&payload), 0.5);
    }
}

#[cfg(test)]
mod tests_memory {
    use super::*;
    use crate::item::Item;

    fn feature_item() -> Item {
        Item {
            block_kind: Some("text".into()),
            layout_role: Some("paragraph".into()),
            semantic_role: Some("body".into()),
            structure_role: Some("body".into()),
            source_text: "A reasonably long source text block for feature hashing.".into(),
            bbox: Some([40.0, 100.0, 400.0, 160.0]),
            lines: vec![crate::item::Line { bbox: Some([40.0, 100.0, 390.0, 115.0]), spans: vec![] }],
            ..Default::default()
        }
    }

    #[test]
    fn degenerate_dimensions_return_none() {
        assert!(build_typography_feature(&Item::default(), "x", 10.0, 0.4, None, None, 0.0, false, false, false, false, false).is_none());
    }

    #[test]
    fn feature_key_is_deterministic_and_hex() {
        let item = feature_item();
        let a = build_typography_feature(&item, "翻译文本", 11.4, 0.48, Some(595.0), Some(842.0), 300.0, true, false, false, false, false).unwrap();
        let b = build_typography_feature(&item, "翻译文本", 11.4, 0.48, Some(595.0), Some(842.0), 300.0, true, false, false, false, false).unwrap();
        assert_eq!(a.key, b.key);
        assert_eq!(a.key.len(), 32);
        assert!(a.key.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
