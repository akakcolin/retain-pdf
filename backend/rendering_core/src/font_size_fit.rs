// Port of services/rendering/layout/font_size_fit.py.

use crate::config::{BODY_FONT_SIZE_FACTOR, DEFAULT_FONT_SIZE};
use crate::font_roles::{is_caption_like_block, is_footnote_like_block, is_local_textual_item};
use crate::item::Item;
use crate::typography::compactness::source_compactness_score;
use crate::typography::line_metrics::{local_font_metric, local_line_pitch, median_line_height, median_line_pitch};
use crate::typography::scalars::clamp;
use crate::util::py_round;

pub const MIN_FONT_SIZE_PT: f64 = 8.4;
pub const MAX_FONT_SIZE_PT: f64 = 11.6;
pub const MAX_LOCAL_FONT_SIZE_PT: f64 = 14.2;
pub const LOCAL_BLOCK_SCALE_MIN: f64 = 0.97;
pub const LOCAL_BLOCK_SCALE_MAX: f64 = 1.03;
pub const CAPTION_FONT_SCALE: f64 = 0.86;
pub const CAPTION_MAX_FONT_SIZE_PT: f64 = 10.0;
pub const FOOTNOTE_FONT_SCALE: f64 = 0.78;
pub const FOOTNOTE_MIN_FONT_SIZE_PT: f64 = 6.6;
pub const BODY_PAGE_BLEND_BASE: f64 = 0.86;
pub const BODY_PAGE_BLEND_MIN: f64 = 0.74;
pub const BODY_COMPACT_FONT_SCALE_MAX: f64 = 0.04;
pub const WIDE_ASPECT_PAGE_BLEND_REDUCTION: f64 = 0.14;
pub const WIDE_ASPECT_COMPACT_FONT_SCALE_MAX: f64 = 0.018;

pub fn local_font_size_pt(item: &Item) -> f64 {
    if !is_local_textual_item(item) {
        return DEFAULT_FONT_SIZE;
    }
    let metric = local_font_metric(item);
    if metric <= 0.0 {
        return DEFAULT_FONT_SIZE;
    }
    let base_size = metric * BODY_FONT_SIZE_FACTOR;
    if is_footnote_like_block(item) {
        return py_round(
            clamp(base_size * FOOTNOTE_FONT_SCALE, FOOTNOTE_MIN_FONT_SIZE_PT, MAX_LOCAL_FONT_SIZE_PT),
            2,
        );
    }
    if is_caption_like_block(item) {
        return py_round(clamp(base_size * CAPTION_FONT_SCALE, MIN_FONT_SIZE_PT, CAPTION_MAX_FONT_SIZE_PT), 2);
    }
    py_round(clamp(base_size, MIN_FONT_SIZE_PT, MAX_LOCAL_FONT_SIZE_PT), 2)
}

pub fn estimate_font_size_pt(
    item: &Item,
    page_font_size: f64,
    page_line_pitch: f64,
    page_line_height: f64,
    _density_baseline: f64,
) -> f64 {
    if !is_local_textual_item(item) {
        return DEFAULT_FONT_SIZE;
    }
    let local_font = local_font_size_pt(item);
    if !item.is_body_text_candidate {
        return local_font;
    }

    let mut block_scale = 1.0;
    // Python: `local_line_pitch(item) or median_line_pitch(item)` (local if nonzero).
    let block_line_pitch = {
        let lp = local_line_pitch(item);
        if lp > 0.0 { lp } else { median_line_pitch(item) }
    };
    let block_line_height = median_line_height(item);
    if page_line_pitch > 0.0 && block_line_pitch > 0.0 {
        block_scale = clamp(block_line_pitch / page_line_pitch, LOCAL_BLOCK_SCALE_MIN, LOCAL_BLOCK_SCALE_MAX);
    } else if page_line_height > 0.0 && block_line_height > 0.0 {
        block_scale = clamp(block_line_height / page_line_height, LOCAL_BLOCK_SCALE_MIN, LOCAL_BLOCK_SCALE_MAX);
    }

    let compactness = source_compactness_score(item);
    let wide_aspect_body_text = item.wide_aspect_body_text;
    let page_estimate = if page_font_size > 0.0 {
        page_font_size * block_scale
    } else {
        local_font
    };
    let mut page_weight = (BODY_PAGE_BLEND_BASE - compactness * 0.18).max(BODY_PAGE_BLEND_MIN);
    if wide_aspect_body_text {
        page_weight = (page_weight - WIDE_ASPECT_PAGE_BLEND_REDUCTION).max(BODY_PAGE_BLEND_MIN - 0.1);
    }
    let local_weight = 1.0 - page_weight;
    let mut blended = page_estimate * page_weight + local_font * local_weight;
    if compactness > 0.0 {
        let compact_scale_max = if wide_aspect_body_text {
            WIDE_ASPECT_COMPACT_FONT_SCALE_MAX
        } else {
            BODY_COMPACT_FONT_SCALE_MAX
        };
        blended *= 1.0 - compact_scale_max.min(compactness * 0.055);
    }
    py_round(clamp(blended, MIN_FONT_SIZE_PT, MAX_LOCAL_FONT_SIZE_PT), 2)
}
