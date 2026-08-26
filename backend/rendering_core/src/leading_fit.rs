// Port of services/rendering/layout/leading_fit.py.

use crate::config::BODY_LEADING_FACTOR;
use crate::item::Item;
use crate::typography::compactness::{occupied_ratio_x, source_compactness_score};
use crate::typography::content::formula_ratio;
use crate::typography::line_metrics::{local_line_pitch, median_line_pitch};
use crate::typography::scalars::clamp;
use crate::util::py_round;

pub const DEFAULT_LEADING_EM: f64 = 0.48;
pub const BODY_LEADING_MIN: f64 = 0.44;
pub const BODY_LEADING_MAX: f64 = 0.68;
pub const NON_BODY_LEADING_MIN: f64 = 0.26;
pub const NON_BODY_LEADING_MAX: f64 = 0.56;
pub const BODY_LEADING_SIZE_ADJUST: f64 = 0.62;
pub const NON_BODY_LEADING_SIZE_ADJUST: f64 = 0.78;
pub const BODY_LEADING_FLOOR_MIN: f64 = 0.44;
pub const NON_BODY_LEADING_FLOOR_MIN: f64 = 0.26;
pub const HIGH_DENSITY_LEADING_RATIO: f64 = 0.9;
pub const FORMULA_LEADING_RATIO: f64 = 0.92;
pub const BODY_ZH_TARGET_BASE: f64 = 0.56;
pub const BODY_ZH_TARGET_MIN: f64 = 0.50;
pub const BODY_NORMAL_LEADING_MIN: f64 = 0.52;
pub const BODY_COMPACT_LEADING_TIGHTEN_MAX: f64 = 0.015;
pub const WIDE_ASPECT_OCR_LEADING_WEIGHT: f64 = 0.5;
pub const WIDE_ASPECT_ZH_LEADING_WEIGHT: f64 = 0.5;
pub const WIDE_ASPECT_COMPACT_LEADING_TIGHTEN_MAX: f64 = 0.025;

pub fn normalize_leading_em_for_font_size(
    font_size_pt: f64,
    leading_em: f64,
    _reference_font_size_pt: f64,
    min_leading_em: f64,
    max_leading_em: f64,
    _strength: f64,
    floor_min_leading_em: Option<f64>,
) -> f64 {
    if font_size_pt <= 0.0 {
        return py_round(clamp(leading_em, min_leading_em, max_leading_em), 2);
    }
    let floor_min = floor_min_leading_em.unwrap_or(min_leading_em);
    py_round(clamp(leading_em, floor_min.max(min_leading_em), max_leading_em), 2)
}

pub fn estimate_leading_em(item: &Item, page_line_pitch: f64, font_size_pt: f64) -> f64 {
    let block_pitch = local_line_pitch(item).max(median_line_pitch(item));
    let density_ratio_x = occupied_ratio_x(item);
    let formula_weight = formula_ratio(item);
    let compactness = source_compactness_score(item);
    let wide_aspect_body_text = item.wide_aspect_body_text;
    if item.is_body_text_candidate {
        let pitch = if block_pitch > 0.0 { block_pitch } else { page_line_pitch };
        let zh_target = (BODY_ZH_TARGET_BASE - compactness * 0.07).max(BODY_ZH_TARGET_MIN);
        let base = if pitch > 0.0 && font_size_pt > 0.0 {
            let ocr_estimated = (pitch / font_size_pt) - 1.0;
            let mixed = if wide_aspect_body_text {
                (ocr_estimated * WIDE_ASPECT_OCR_LEADING_WEIGHT) + (zh_target * WIDE_ASPECT_ZH_LEADING_WEIGHT)
            } else {
                (ocr_estimated * 0.35) + (zh_target * 0.65)
            };
            mixed * BODY_LEADING_FACTOR
        } else {
            zh_target * BODY_LEADING_FACTOR
        };
        let base = if compactness > 0.0 {
            let tighten_max = if wide_aspect_body_text {
                WIDE_ASPECT_COMPACT_LEADING_TIGHTEN_MAX
            } else {
                BODY_COMPACT_LEADING_TIGHTEN_MAX
            };
            base * (1.0 - tighten_max.min(compactness * 0.07))
        } else {
            base
        };
        let base = if !wide_aspect_body_text {
            base.max(BODY_NORMAL_LEADING_MIN * BODY_LEADING_FACTOR)
        } else {
            base
        };
        let base = if density_ratio_x >= 0.86 {
            base.max(BODY_LEADING_MIN / HIGH_DENSITY_LEADING_RATIO)
        } else {
            base
        };
        let base = if formula_weight >= 0.08 {
            base.max(BODY_LEADING_MIN / FORMULA_LEADING_RATIO)
        } else {
            base
        };
        return normalize_leading_em_for_font_size(
            font_size_pt,
            base,
            0.0,
            BODY_LEADING_MIN,
            BODY_LEADING_MAX,
            0.0,
            Some(BODY_LEADING_FLOOR_MIN),
        );
    }
    let base = if block_pitch > 0.0 && font_size_pt > 0.0 {
        let ocr_estimated = (block_pitch / font_size_pt) - 1.0;
        let mixed = (ocr_estimated * 0.55) + (DEFAULT_LEADING_EM * 0.45);
        mixed * BODY_LEADING_FACTOR
    } else {
        DEFAULT_LEADING_EM * BODY_LEADING_FACTOR
    };
    let base = if density_ratio_x >= 0.9 {
        base.max(NON_BODY_LEADING_MIN / HIGH_DENSITY_LEADING_RATIO)
    } else {
        base
    };
    let base = if formula_weight >= 0.12 {
        base.max(NON_BODY_LEADING_MIN / FORMULA_LEADING_RATIO)
    } else {
        base
    };
    normalize_leading_em_for_font_size(
        font_size_pt,
        base,
        0.0,
        NON_BODY_LEADING_MIN,
        NON_BODY_LEADING_MAX,
        0.0,
        Some(NON_BODY_LEADING_FLOOR_MIN),
    )
}
