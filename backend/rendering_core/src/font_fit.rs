// Port of services/rendering/layout/font_fit.py — the public facade of the
// font_fit package (its `__all__` is the public API consumed downstream).

pub use crate::font_roles::{
    is_body_text_candidate, is_caption_like_block, is_default_text_block, is_footnote_like_block,
    is_local_textual_item, is_title_like_block, item_layout_role_name, item_semantic_role_name,
    resolve_font_weight, BODY_FORMULA_RATIO_MAX,
};
pub use crate::font_size_fit::{
    estimate_font_size_pt, local_font_size_pt, BODY_COMPACT_FONT_SCALE_MAX, BODY_PAGE_BLEND_BASE,
    BODY_PAGE_BLEND_MIN, CAPTION_FONT_SCALE, CAPTION_MAX_FONT_SIZE_PT, FOOTNOTE_FONT_SCALE,
    FOOTNOTE_MIN_FONT_SIZE_PT, LOCAL_BLOCK_SCALE_MAX, LOCAL_BLOCK_SCALE_MIN, MAX_FONT_SIZE_PT,
    MAX_LOCAL_FONT_SIZE_PT, MIN_FONT_SIZE_PT, WIDE_ASPECT_COMPACT_FONT_SCALE_MAX,
    WIDE_ASPECT_PAGE_BLEND_REDUCTION,
};
pub use crate::leading_fit::{
    estimate_leading_em, normalize_leading_em_for_font_size, BODY_LEADING_FLOOR_MIN,
    BODY_LEADING_MAX, BODY_LEADING_MIN, BODY_LEADING_SIZE_ADJUST, DEFAULT_LEADING_EM,
    FORMULA_LEADING_RATIO, HIGH_DENSITY_LEADING_RATIO, NON_BODY_LEADING_FLOOR_MIN,
    NON_BODY_LEADING_MAX, NON_BODY_LEADING_MIN, NON_BODY_LEADING_SIZE_ADJUST,
};
pub use crate::title_fit_limits::{
    resolve_title_fill_max_font_size_pt, TITLE_FILL_GROW_SCALE, TITLE_FILL_HEIGHT_TO_FONT_RATIO,
    TITLE_FILL_MAX_FONT_SCALE, TITLE_FILL_MAX_FONT_SIZE_PT,
};

pub const ZH_FONT_SCALE: f64 = 0.91;
pub const PAGE_BASELINE_PERCENTILE: f64 = 0.42;
pub const BLOCK_SCALE_MIN: f64 = 0.985;
pub const BLOCK_SCALE_MAX: f64 = 1.015;
pub const LEADING_SIZE_DELTA_LIMIT: f64 = 0.18;
