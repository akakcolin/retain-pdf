// Port of services/rendering/layout/title_fit_limits.py.

use crate::item::Item;
use crate::semantics::is_title_like_block;
use crate::typography::geometry::inner_bbox;
use crate::util::py_round;

pub const TITLE_FILL_MAX_FONT_SIZE_PT: f64 = 72.0;
pub const TITLE_FILL_HEIGHT_TO_FONT_RATIO: f64 = 0.92;
pub const TITLE_FILL_GROW_SCALE: f64 = 2.6;
pub const TITLE_FILL_MAX_FONT_SCALE: f64 = 3.0;

pub fn resolve_title_fill_max_font_size_pt(item: &Item, base_font_size_pt: f64) -> f64 {
    if !is_title_like_block(item) {
        return py_round(base_font_size_pt, 2);
    }
    let scaled_cap = base_font_size_pt.max(base_font_size_pt * TITLE_FILL_MAX_FONT_SCALE);
    let inner = inner_bbox(item);
    if inner.len() != 4 {
        return py_round(TITLE_FILL_MAX_FONT_SIZE_PT.min(scaled_cap).min(base_font_size_pt), 2);
    }
    let height_pt = (inner[3] - inner[1]).max(8.0);
    let height_cap = height_pt * TITLE_FILL_HEIGHT_TO_FONT_RATIO;
    let grow_floor = base_font_size_pt.max((base_font_size_pt * TITLE_FILL_GROW_SCALE).min(height_cap));
    let optimistic = TITLE_FILL_MAX_FONT_SIZE_PT.min(scaled_cap).min(grow_floor);
    py_round(base_font_size_pt.max(optimistic), 2)
}
