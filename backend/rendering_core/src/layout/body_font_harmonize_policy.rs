// Port of services/rendering/layout/payload/body_font_harmonize_policy.py — pulls
// long body blocks (tall, wide, not dense) toward their own font/leading medians
// (C3-N3). Operates on the raw JSON block-payload dicts, mutating in place.

use serde_json::Value;

use crate::layout::body_common::payload_density;
use crate::layout::payload_dict::{inner_bbox4, payload_f64};
use crate::util::{median_f64, py_round};

const LONG_BODY_MIN_HEIGHT_PT: f64 = 90.0;
const LONG_BODY_MAX_DENSITY: f64 = 0.98;
const LONG_BODY_FONT_SMOOTH_PT: f64 = 0.14;
const LONG_BODY_LEADING_SMOOTH_EM: f64 = 0.05;

/// `harmonize_long_body_payloads`: clamp long body fonts and leading toward
/// their shared medians when at least two qualify.
pub fn harmonize_long_body_payloads(body_payloads: &mut Vec<Value>, page_text_width_med: f64) {
    let long_indices: Vec<usize> = body_payloads
        .iter()
        .enumerate()
        .filter(|(_, payload)| {
            let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
            let inner_height = (bbox[3] - bbox[1]).max(8.0);
            let inner_width = (bbox[2] - bbox[0]).max(8.0);
            inner_height >= LONG_BODY_MIN_HEIGHT_PT
                && inner_width >= page_text_width_med * 0.72
                && payload_density(payload, None, None) <= LONG_BODY_MAX_DENSITY
        })
        .map(|(idx, _)| idx)
        .collect();

    if long_indices.len() < 2 {
        return;
    }

    let fonts: Vec<f64> = long_indices
        .iter()
        .map(|&idx| payload_f64(&body_payloads[idx], "font_size_pt", 0.0))
        .collect();
    let leadings: Vec<f64> = long_indices
        .iter()
        .map(|&idx| payload_f64(&body_payloads[idx], "leading_em", 0.0))
        .collect();
    let font_median = median_f64(&fonts);
    let leading_median = median_f64(&leadings);

    for idx in long_indices {
        let obj = body_payloads[idx].as_object_mut().expect("payload is an object");
        let font = obj.get("font_size_pt").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let leading = obj.get("leading_em").and_then(|v| v.as_f64()).unwrap_or(0.0);
        obj.insert("font_size_pt".to_string(), Value::from(py_round(font.max(font_median - LONG_BODY_FONT_SMOOTH_PT).min(font_median + LONG_BODY_FONT_SMOOTH_PT), 2)));
        obj.insert("leading_em".to_string(), Value::from(py_round(leading.max(leading_median - LONG_BODY_LEADING_SMOOTH_EM).min(leading_median + LONG_BODY_LEADING_SMOOTH_EM), 2)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(font: f64, leading: f64, height: f64) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 350.0, height],
            "translated_text": "这是一段足够长的中文段落文字内容，它需要能够在给定的宽度之内被排版成多行文字才能满足段落的自然阅读习惯。这里再补充一部分内容让文本继续变长。",
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": leading,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {},
        })
    }

    #[test]
    fn harmonizes_two_long_blocks() {
        let mut payloads = vec![payload(10.0, 0.4, 120.0), payload(12.0, 0.6, 130.0)];
        harmonize_long_body_payloads(&mut payloads, 200.0);
        let font0: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        let font1: f64 = payloads[1]["font_size_pt"].as_f64().unwrap();
        assert!(font0 >= 11.0 - 0.15 && font0 <= 11.0 + 0.15);
        assert!(font1 >= 11.0 - 0.15 && font1 <= 11.0 + 0.15);
    }

    #[test]
    fn single_long_block_left_alone() {
        let mut payloads = vec![payload(10.0, 0.4, 120.0)];
        harmonize_long_body_payloads(&mut payloads, 200.0);
        assert_eq!(payloads[0]["font_size_pt"], json!(10.0));
    }
}
