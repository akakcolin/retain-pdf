// Port of services/rendering/layout/payload/block_seed_body_policy.py.

use crate::item::FormulaEntry;
use crate::payload::capacity::estimated_render_height_pt;
use crate::util::py_round;

pub const SMALL_PAGE_BOX_RATIO: f64 = 0.06;
pub const ULTRA_SMALL_PAGE_BOX_RATIO: f64 = 0.04;
pub const GEOMETRY_DENSE_TRIGGER: f64 = 0.86;
pub const GEOMETRY_HEAVY_DENSE_TRIGGER: f64 = 0.98;
pub const LENGTH_DENSITY_AUX_TRIGGER: f64 = 1.18;
pub const WIDE_ASPECT_BODY_RATIO: f64 = 3.6;
pub const WIDE_ASPECT_BODY_FONT_BOOST_PT: f64 = 0.28;
pub const WIDE_ASPECT_BODY_LEADING_TARGET: f64 = 0.46;
pub const WIDE_ASPECT_BODY_LEADING_STEP: f64 = 0.02;
pub const WIDE_ASPECT_BODY_MIN_SLACK_PT: f64 = 2.8;
pub const DENSE_BODY_FONT_MAX_PT: f64 = 10.35;
pub const HEAVY_DENSE_BODY_FONT_MAX_PT: f64 = 10.2;

/// `relax_wide_aspect_body_leading`: grow leading in 0.02-em steps while the
/// estimated render height still leaves `WIDE_ASPECT_BODY_MIN_SLACK_PT` headroom.
pub fn relax_wide_aspect_body_leading(
    inner: &[f64],
    translated_text: &str,
    formula_map: &[FormulaEntry],
    font_size_pt: f64,
    leading_em: f64,
) -> f64 {
    if inner.len() != 4 {
        return leading_em;
    }
    let available_height_pt = (inner[3] - inner[1]).max(8.0);
    let mut candidate = leading_em;
    while candidate + WIDE_ASPECT_BODY_LEADING_STEP <= WIDE_ASPECT_BODY_LEADING_TARGET {
        let next_leading = py_round(candidate + WIDE_ASPECT_BODY_LEADING_STEP, 2);
        let next_height =
            estimated_render_height_pt(inner, translated_text, formula_map, font_size_pt, next_leading);
        if next_height > available_height_pt - WIDE_ASPECT_BODY_MIN_SLACK_PT {
            break;
        }
        candidate = next_leading;
    }
    candidate
}

pub fn is_dense_small_box(density_ratio: f64, layout_density: f64, page_box_area_ratio: f64) -> bool {
    if !(0.0 < page_box_area_ratio && page_box_area_ratio <= SMALL_PAGE_BOX_RATIO) {
        return false;
    }
    if layout_density >= GEOMETRY_DENSE_TRIGGER {
        return true;
    }
    density_ratio >= LENGTH_DENSITY_AUX_TRIGGER && layout_density >= GEOMETRY_DENSE_TRIGGER - 0.08
}

pub fn is_heavy_dense_small_box(
    density_ratio: f64,
    layout_density: f64,
    page_box_area_ratio: f64,
    heavy_compact_ratio: f64,
) -> bool {
    if !(0.0 < page_box_area_ratio && page_box_area_ratio <= ULTRA_SMALL_PAGE_BOX_RATIO) {
        return false;
    }
    if layout_density >= GEOMETRY_HEAVY_DENSE_TRIGGER {
        return true;
    }
    density_ratio >= heavy_compact_ratio.max(LENGTH_DENSITY_AUX_TRIGGER)
        && layout_density >= GEOMETRY_DENSE_TRIGGER
}

pub fn is_wide_aspect_body_text(is_body: bool, block_width: f64, block_height: f64) -> bool {
    is_body && block_height > 0.0 && (block_width / block_height) >= WIDE_ASPECT_BODY_RATIO
}

pub fn adjust_body_seed_font_size(
    font_size_pt: f64,
    page_body_font_size_pt: Option<f64>,
    is_body: bool,
    dense_small_box: bool,
    heavy_dense_small_box: bool,
    wide_aspect_body_text: bool,
) -> f64 {
    let page_body = match page_body_font_size_pt {
        Some(v) => v,
        None => return font_size_pt,
    };
    if !is_body {
        return font_size_pt;
    }
    let down_band = if heavy_dense_small_box {
        0.34
    } else if dense_small_box {
        0.2
    } else {
        0.06
    };
    let up_band = if dense_small_box { 0.18 } else { 0.24 };
    let mut adjusted = py_round(font_size_pt.max(page_body - down_band).min(page_body + up_band), 2);
    if dense_small_box {
        let dense_cap = if heavy_dense_small_box {
            HEAVY_DENSE_BODY_FONT_MAX_PT
        } else {
            DENSE_BODY_FONT_MAX_PT
        };
        adjusted = py_round(adjusted.min(dense_cap), 2);
    }
    if wide_aspect_body_text {
        adjusted = py_round((page_body + up_band).min(adjusted + WIDE_ASPECT_BODY_FONT_BOOST_PT), 2);
    }
    adjusted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_small_box_gates_on_page_ratio() {
        assert!(!is_dense_small_box(1.2, 0.9, 0.07));
        assert!(is_dense_small_box(1.2, 0.9, 0.05));
        assert!(!is_dense_small_box(1.2, 0.7, 0.05));
        assert!(is_dense_small_box(1.2, 0.8, 0.05));
    }

    #[test]
    fn heavy_dense_requires_ultra_small_page() {
        assert!(!is_heavy_dense_small_box(1.2, 0.99, 0.05, 1.0));
        assert!(is_heavy_dense_small_box(1.2, 0.99, 0.03, 1.0));
        assert!(!is_heavy_dense_small_box(1.2, 0.8, 0.03, 1.2));
    }

    #[test]
    fn wide_aspect_body_ratio() {
        assert!(is_wide_aspect_body_text(true, 400.0, 100.0));
        assert!(!is_wide_aspect_body_text(true, 100.0, 400.0));
        assert!(!is_wide_aspect_body_text(false, 400.0, 100.0));
    }

    #[test]
    fn adjust_font_respects_page_band() {
        assert_eq!(adjust_body_seed_font_size(9.0, Some(10.0), true, false, false, false), 9.94);
        assert_eq!(adjust_body_seed_font_size(12.0, Some(10.0), true, false, false, false), 10.24);
        assert_eq!(adjust_body_seed_font_size(9.0, None, true, false, false, false), 9.0);
        assert_eq!(adjust_body_seed_font_size(9.0, Some(10.0), false, false, false, false), 9.0);
    }
}
