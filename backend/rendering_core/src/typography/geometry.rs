// Port of services/rendering/layout/typography/geometry.py.

use crate::config::{INNER_BBOX_DENSE_SHRINK_X, INNER_BBOX_DENSE_SHRINK_Y, INNER_BBOX_SHRINK_X, INNER_BBOX_SHRINK_Y};
use crate::item::Item;
use crate::typography::compactness::{occupied_ratio, occupied_ratio_x};
use crate::typography::cover_geometry::expanded_cover_bbox;

pub fn inner_bbox(item: &Item) -> Vec<f64> {
    let bbox = match item.bbox {
        Some(b) => b,
        None => return Vec::new(),
    };
    let [x0, y0, x1, y1] = bbox;
    let width = x1 - x0;
    let height = y1 - y0;

    let mut shrink_x = width * INNER_BBOX_SHRINK_X;
    let mut shrink_y = height * INNER_BBOX_SHRINK_Y;

    let rho_x = occupied_ratio_x(item);
    let rho_y = occupied_ratio(item);
    if rho_x > 0.82 {
        shrink_x = width * INNER_BBOX_DENSE_SHRINK_X;
    }
    if rho_y > 0.82 {
        shrink_y = height * INNER_BBOX_DENSE_SHRINK_Y;
    }

    let mut nx0 = x0 + shrink_x;
    let mut nx1 = x1 - shrink_x;
    let mut ny0 = y0 + shrink_y;
    let mut ny1 = y1 - shrink_y;
    if nx1 - nx0 < width * 0.7 {
        nx0 = x0 + width * 0.015;
        nx1 = x1 - width * 0.015;
    }
    if ny1 - ny0 < height * 0.7 {
        ny0 = y0 + height * 0.015;
        ny1 = y1 - height * 0.015;
    }
    vec![nx0, ny0, nx1, ny1]
}

pub fn cover_bbox(item: &Item) -> Vec<f64> {
    let bbox = match item.bbox {
        Some(b) => b,
        None => return Vec::new(),
    };
    if item.cover_with_inner_bbox {
        let inner = inner_bbox(item);
        if inner.len() == 4 {
            return expanded_cover_bbox(item, &[inner[0], inner[1], inner[2], inner[3]]).to_vec();
        }
    }
    expanded_cover_bbox(item, &bbox).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inner_bbox_no_shrink_by_default() {
        // Default layout shrink is 0.0, so inner == bbox unless dense triggers.
        let item = Item { bbox: Some([10.0, 20.0, 110.0, 120.0]), ..Default::default() };
        assert_eq!(inner_bbox(&item), vec![10.0, 20.0, 110.0, 120.0]);
    }

    #[test]
    fn cover_bbox_uses_inner_when_flagged() {
        let item = Item {
            bbox: Some([10.0, 20.0, 110.0, 120.0]),
            cover_with_inner_bbox: true,
            layout_role: Some("paragraph".into()),
            ..Default::default()
        };
        let out = cover_bbox(&item);
        assert_eq!(out.len(), 4);
    }
}
