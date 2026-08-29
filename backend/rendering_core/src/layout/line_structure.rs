// Port of services/rendering/layout/payload/line_structure.py — the subset the
// emit boundary consumes: `fit_preserved_line_block_metrics` (seed path) and
// `preserved_line_boxes_for_item` (emit path). The structured-line detection /
// splitting helpers feed `seed_render_fields`, which runs in Python before the
// native call, and are not ported here.

use serde_json::{json, Value};

use crate::util::py_round;

pub const PRESERVED_LINE_LEADING_CANDIDATES: [f64; 6] = [0.12, 0.16, 0.2, 0.24, 0.28, 0.32];
pub const PRESERVED_LINE_HEIGHT_FILL: f64 = 0.96;
pub const PRESERVED_LINE_MIN_FONT_PT: f64 = 7.2;
pub const PRESERVED_LINE_IDEAL_LEADING: f64 = 0.22;
pub const PRESERVED_LINE_IDEAL_FONT_PT: f64 = 10.6;

/// `fit_preserved_line_block_metrics`: pick the leading candidate that best
/// packs `line_count` preserved lines into `inner`, scoring pitch fit, leading
/// preference and font-size deviation.
pub fn fit_preserved_line_block_metrics(
    inner: &[f64],
    protected_text: &str,
    font_size_pt: f64,
    leading_em: f64,
) -> (f64, f64) {
    if inner.len() != 4 {
        return (font_size_pt, leading_em);
    }
    let line_count = protected_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        .max(1);
    if line_count <= 1 {
        return (font_size_pt, leading_em);
    }
    let height = (inner[3] - inner[1]).max(1.0);
    if height <= 0.0 {
        return (font_size_pt, leading_em);
    }

    let mut best: Option<(f64, f64, f64)> = None; // (score, font, leading)
    let source_font_hint = font_size_pt.max(PRESERVED_LINE_IDEAL_FONT_PT);
    for &candidate_leading in &PRESERVED_LINE_LEADING_CANDIDATES {
        let candidate_font = height * PRESERVED_LINE_HEIGHT_FILL
            / (line_count as f64 * (1.0 + candidate_leading)).max(1.0);
        if candidate_font < PRESERVED_LINE_MIN_FONT_PT {
            continue;
        }
        let line_pitch = candidate_font * (1.0 + candidate_leading);
        let target_pitch = height / (line_count as f64).max(1.0);
        let pitch_error = (line_pitch - target_pitch).abs() / target_pitch.max(1.0);
        let leading_error = (candidate_leading - PRESERVED_LINE_IDEAL_LEADING).abs() * 0.9;
        let font_error = (candidate_font - source_font_hint.min(PRESERVED_LINE_IDEAL_FONT_PT + 1.0)).abs() / 18.0;
        let score = pitch_error + leading_error + font_error;
        if best.is_none() || score < best.unwrap().0 {
            best = Some((score, candidate_font, candidate_leading));
        }
    }

    match best {
        Some((_score, font, leading)) => (py_round(font, 2), py_round(leading, 2)),
        None => {
            let fallback_leading = PRESERVED_LINE_LEADING_CANDIDATES[0];
            let fallback_font = height * PRESERVED_LINE_HEIGHT_FILL
                / (line_count as f64 * (1.0 + fallback_leading)).max(1.0);
            (py_round(fallback_font.max(PRESERVED_LINE_MIN_FONT_PT), 2), fallback_leading)
        }
    }
}

/// `preserved_line_boxes_for_item`: zip translated text lines with the source
/// `lines` bboxes into `RenderLineBox` DTOs for preserve-line-break blocks.
/// Any shape mismatch aborts the whole list (matches the Python early-returns).
pub fn preserved_line_boxes_for_item(item: &Value, translated_text: &str) -> Vec<Value> {
    let preserve = item
        .get("_render_preserve_line_breaks")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !preserve {
        return vec![];
    }
    let text_lines: Vec<&str> = translated_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let raw_lines = item.get("lines").and_then(|v| v.as_array());
    let Some(raw_lines) = raw_lines else {
        return vec![];
    };
    if text_lines.is_empty() || raw_lines.len() < text_lines.len() {
        return vec![];
    }
    let mut boxes: Vec<Value> = Vec::new();
    for (text, raw_line) in text_lines.iter().zip(raw_lines.iter()) {
        let bbox_value = raw_line.get("bbox");
        let Some(bbox) = bbox_value.and_then(|v| v.as_array()) else {
            return vec![];
        };
        if bbox.len() != 4 {
            return vec![];
        }
        let mut line_bbox: Vec<f64> = Vec::with_capacity(4);
        for value in bbox {
            match value.as_f64() {
                Some(v) => line_bbox.push(v),
                None => return vec![],
            }
        }
        if line_bbox[2] <= line_bbox[0] || line_bbox[3] <= line_bbox[1] {
            return vec![];
        }
        boxes.push(json!({"text": text, "bbox": line_bbox}));
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn single_line_returns_unchanged() {
        assert_eq!(fit_preserved_line_block_metrics(&[0.0, 0.0, 100.0, 20.0], "single", 10.0, 0.4), (10.0, 0.4));
    }

    #[test]
    fn short_box_forces_fallback() {
        // Three lines in a 24pt-tall box: every candidate font lands below 7.2.
        let (font, leading) = fit_preserved_line_block_metrics(&[0.0, 0.0, 100.0, 24.0], "a\nb\nc", 10.6, 0.4);
        assert_eq!(leading, 0.12);
        assert_eq!(font, 7.2);
    }

    #[test]
    fn preserved_boxes_zip_text_and_raw_lines() {
        let item = json!({
            "_render_preserve_line_breaks": true,
            "lines": [
                {"bbox": [0.0, 0.0, 100.0, 20.0]},
                {"bbox": [0.0, 20.0, 100.0, 40.0]},
            ],
        });
        let boxes = preserved_line_boxes_for_item(&item, "one\ntwo");
        assert_eq!(
            boxes,
            vec![
                json!({"text": "one", "bbox": [0.0, 0.0, 100.0, 20.0]}),
                json!({"text": "two", "bbox": [0.0, 20.0, 100.0, 40.0]}),
            ]
        );
    }

    #[test]
    fn preserved_boxes_require_flag_and_shape() {
        assert_eq!(preserved_line_boxes_for_item(&json!({}), "x"), Vec::<Value>::new());
        let item = json!({"_render_preserve_line_breaks": true, "lines": []});
        assert_eq!(preserved_line_boxes_for_item(&item, "x"), Vec::<Value>::new());
        let item = json!({
            "_render_preserve_line_breaks": true,
            "lines": [{"bbox": [0.0, 0.0, 100.0, 20.0]}],
        });
        // Two translated lines but only one source line -> abort whole list.
        assert_eq!(preserved_line_boxes_for_item(&item, "a\nb"), Vec::<Value>::new());
        // Degenerate bbox aborts the list.
        let item = json!({
            "_render_preserve_line_breaks": true,
            "lines": [{"bbox": [0.0, 0.0, 0.0, 20.0]}],
        });
        assert_eq!(preserved_line_boxes_for_item(&item, "a"), Vec::<Value>::new());
    }
}
