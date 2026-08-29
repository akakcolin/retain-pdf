// Port of services/rendering/layout/payload/body_context.py — only the
// `page_box_area_ratio` leaf the seed boundary consumes. The adjacent-body
// smoothing / density-capping helpers feed C3-N3 (body_pipeline) and are not
// ported here.

/// `page_box_area_ratio`: fraction of the page area covered by `bbox`, or 0.0
/// when `bbox` is not a 4-float list or either page dimension is missing or
/// non-positive.
pub fn page_box_area_ratio(bbox: &[f64], page_width: Option<f64>, page_height: Option<f64>) -> f64 {
    let page_width = match page_width {
        Some(v) if v > 0.0 => v,
        _ => return 0.0,
    };
    let page_height = match page_height {
        Some(v) if v > 0.0 => v,
        _ => return 0.0,
    };
    if bbox.len() != 4 {
        return 0.0;
    }
    let width = (bbox[2] - bbox[0]).max(0.0);
    let height = (bbox[3] - bbox[1]).max(0.0);
    if width <= 0.0 || height <= 0.0 {
        return 0.0;
    }
    (width * height) / (page_width * page_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_of_full_box() {
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 100.0, 100.0], Some(200.0), Some(200.0)), 0.25);
    }

    #[test]
    fn missing_dimension_returns_zero() {
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 10.0, 10.0], None, Some(100.0)), 0.0);
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 10.0, 10.0], Some(100.0), None), 0.0);
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 10.0, 10.0], Some(0.0), Some(100.0)), 0.0);
    }

    #[test]
    fn degenerate_box_returns_zero() {
        assert_eq!(page_box_area_ratio(&[5.0, 5.0, 5.0, 10.0], Some(100.0), Some(100.0)), 0.0);
        assert_eq!(page_box_area_ratio(&[0.0, 0.0, 1.0], Some(100.0), Some(100.0)), 0.0);
    }
}
