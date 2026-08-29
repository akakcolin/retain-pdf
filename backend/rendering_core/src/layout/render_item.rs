// Port of services/rendering/layout/payload/render_item.py (pure subset the seed
// boundary consumes) plus `fit_inner_bbox` from payload/fit_common.py. The
// group/seed-render-field helpers are run by Python's `seed_render_fields`
// before the native call and are not ported here.

use crate::item::Item;
use crate::typography::geometry::inner_bbox;

/// `get_render_first_line_indent_pt`:
/// `max(0.0, float(item.get("_render_first_line_indent_pt") or 0.0))`.
pub fn get_render_first_line_indent_pt(item: &Item) -> f64 {
    item.render_first_line_indent_pt.max(0.0)
}

/// `get_render_inner_bbox`: the `_render_inner_bbox` value when it is a
/// 4-float list, else None. The deserializer already rejects non-4-float shapes.
pub fn get_render_inner_bbox(item: &Item) -> Option<[f64; 4]> {
    item.render_inner_bbox
}

/// `set_render_inner_bbox`: write a 4-float `_render_inner_bbox` back onto a
/// cloned item (used for the title-fit and fit-item copies).
pub fn set_render_inner_bbox(item: &mut Item, bbox: [f64; 4]) {
    item.render_inner_bbox = Some(bbox);
}

/// `fit_inner_bbox`: `get_render_inner_bbox(item) or inner_bbox(item)`.
pub fn fit_inner_bbox(item: &Item) -> Vec<f64> {
    match get_render_inner_bbox(item) {
        Some(bbox) => vec![bbox[0], bbox[1], bbox[2], bbox[3]],
        None => inner_bbox(item),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_indent_clamps_negative() {
        let item = Item { render_first_line_indent_pt: -2.0, ..Default::default() };
        assert_eq!(get_render_first_line_indent_pt(&item), 0.0);
    }

    #[test]
    fn fit_inner_bbox_prefers_render_bbox() {
        let mut item = Item::default();
        set_render_inner_bbox(&mut item, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(get_render_inner_bbox(&item), Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(fit_inner_bbox(&item), vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn fit_inner_bbox_falls_back_to_geometry() {
        let item = Item { bbox: Some([10.0, 20.0, 110.0, 120.0]), ..Default::default() };
        assert_eq!(fit_inner_bbox(&item), vec![10.0, 20.0, 110.0, 120.0]);
    }
}
