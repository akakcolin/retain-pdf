// Port of services/rendering/layout/typography/cover_geometry.py.

use crate::item::Item;
use crate::semantics::{block_kind, is_bodylike_block, is_caption_like_block, is_footnote_like_block, is_title_like_block};
use crate::util::py_round;

pub const COVER_EXPAND_BODY_RATIO: f64 = 0.01;
pub const COVER_EXPAND_OTHER_RATIO: f64 = 0.006;
pub const COVER_EXPAND_TITLE_RATIO: f64 = 0.004;
pub const COVER_EXPAND_MIN_PT: f64 = 1.0;
pub const COVER_EXPAND_BODY_MAX_PT: f64 = 3.0;
pub const COVER_EXPAND_OTHER_MAX_PT: f64 = 2.0;
pub const COVER_EXPAND_TITLE_MAX_PT: f64 = 1.5;
pub const COVER_FORMULA_NEARBY_Y_SCALE: f64 = 0.35;
pub const COVER_FORMULA_NEARBY_X_SCALE: f64 = 0.75;

fn expand_policy(item: &Item) -> (f64, f64) {
    if is_title_like_block(item) {
        return (COVER_EXPAND_TITLE_RATIO, COVER_EXPAND_TITLE_MAX_PT);
    }
    if is_caption_like_block(item) || is_footnote_like_block(item) {
        return (COVER_EXPAND_OTHER_RATIO, COVER_EXPAND_OTHER_MAX_PT);
    }
    let layout_role = item.layout_role.as_deref().unwrap_or("").trim().to_lowercase();
    if block_kind(item) == "text"
        && (item.is_body_text_candidate || is_bodylike_block(item) || layout_role == "paragraph")
    {
        return (COVER_EXPAND_BODY_RATIO, COVER_EXPAND_BODY_MAX_PT);
    }
    (COVER_EXPAND_OTHER_RATIO, COVER_EXPAND_OTHER_MAX_PT)
}

fn has_formula_pressure(item: &Item) -> bool {
    if block_kind(item) == "formula" {
        return true;
    }
    if !item.formula_map.is_empty() {
        return true;
    }
    let text = &item.source_text;
    text.contains('$') || text.contains("\\frac") || text.contains("\\sqrt")
}

pub fn expanded_cover_bbox(item: &Item, bbox: &[f64; 4]) -> [f64; 4] {
    let [x0, y0, x1, y1] = *bbox;
    let width = (x1 - x0).max(0.0);
    let height = (y1 - y0).max(0.0);
    if width <= 0.0 || height <= 0.0 {
        return *bbox;
    }

    let (ratio, max_expand) = expand_policy(item);
    let expand_x = (width * ratio).max(COVER_EXPAND_MIN_PT).min(max_expand);
    let expand_y = (height * ratio).max(COVER_EXPAND_MIN_PT).min(max_expand);
    let (expand_x, expand_y) = if has_formula_pressure(item) {
        (expand_x * COVER_FORMULA_NEARBY_X_SCALE, expand_y * COVER_FORMULA_NEARBY_Y_SCALE)
    } else {
        (expand_x, expand_y)
    };
    [
        py_round(x0 - expand_x, 3),
        py_round(y0 - expand_y, 3),
        py_round(x1 + expand_x, 3),
        py_round(y1 + expand_y, 3),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_around_bbox() {
        let item = Item {
            bbox: Some([0.0, 0.0, 100.0, 50.0]),
            layout_role: Some("paragraph".into()),
            ..Default::default()
        };
        // body policy: ratio 0.01 → expand_x = 1.0 (min), expand_y = 1.0
        let out = expanded_cover_bbox(&item, &[0.0, 0.0, 100.0, 50.0]);
        assert_eq!(out, [-1.0, -1.0, 101.0, 51.0]);
    }

    #[test]
    fn formula_pressure_scales_expand() {
        let item = Item {
            bbox: Some([0.0, 0.0, 100.0, 50.0]),
            layout_role: Some("paragraph".into()),
            source_text: "contains \\frac{1}{2}".into(),
            ..Default::default()
        };
        let out = expanded_cover_bbox(&item, &[0.0, 0.0, 100.0, 50.0]);
        // expand_x = 1.0*0.75 = 0.75, expand_y = 1.0*0.35 = 0.35
        assert_eq!(out, [-0.75, -0.35, 100.75, 50.35]);
    }
}
