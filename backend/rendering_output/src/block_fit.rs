//! Port of `output/typst/block_fit.py`.

use crate::block_config as typst_config;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitDimensions {
    pub fit_min_font: f64,
    pub fit_min_leading: f64,
    pub fit_height: f64,
    pub fit_target_height: f64,
    pub width: f64,
}

fn or_default(x: f64, default: f64) -> f64 {
    if x != 0.0 {
        x
    } else {
        default
    }
}

pub fn fit_dimensions(
    width: f64,
    height: f64,
    font_size: f64,
    leading: f64,
    fit_min_font_size_pt: f64,
    fit_min_leading_em: f64,
    fit_max_height_pt: f64,
) -> FitDimensions {
    let fit_min_font = typst_config::MIN_FIT_FONT_SIZE_PT
        .max(or_default(fit_min_font_size_pt, font_size).min(font_size));
    let fit_min_leading = typst_config::MIN_FIT_LEADING_EM
        .max(or_default(fit_min_leading_em, leading).min(leading));
    let fit_height = typst_config::MIN_BLOCK_SIZE_PT.max(height);
    let fit_target_height = typst_config::MIN_BLOCK_SIZE_PT
        .max(height.min(or_default(fit_max_height_pt, height)));
    FitDimensions {
        fit_min_font,
        fit_min_leading,
        fit_height,
        fit_target_height,
        width: typst_config::MIN_BLOCK_SIZE_PT.max(width),
    }
}
