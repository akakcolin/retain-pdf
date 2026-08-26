// Port of services/rendering/layout/typography/line_metrics.py.

use crate::item::Item;
use crate::typography::constants::{
    LINE_HEIGHT_TO_FONT_SCALE, LINE_PITCH_TO_FONT_SCALE, SOURCE_HEIGHT_LIMIT_MIN_PT,
    SOURCE_HEIGHT_LIMIT_RATIO, TEXT_HEIGHT_PADDING_MAX_PT, TEXT_HEIGHT_PADDING_RATIO,
};
use crate::typography::line_count::source_visual_line_count;
use crate::util::median_f64;

fn line_bbox_height(bbox: &[f64; 4]) -> f64 {
    (bbox[3] - bbox[1]).max(0.0)
}

pub fn line_height(line: &crate::item::Line) -> f64 {
    match line.bbox {
        Some(b) => line_bbox_height(&b),
        None => 0.0,
    }
}

pub fn median_line_height(item: &Item) -> f64 {
    let heights: Vec<f64> = item
        .lines
        .iter()
        .map(line_height)
        .filter(|h| *h > 0.0)
        .collect();
    if heights.is_empty() {
        0.0
    } else {
        median_f64(&heights)
    }
}

pub fn line_centers(item: &Item) -> Vec<f64> {
    let mut centers: Vec<f64> = Vec::new();
    for line in &item.lines {
        if let Some(b) = line.bbox {
            centers.push((b[1] + b[3]) / 2.0);
        }
    }
    centers
}

pub fn median_line_pitch(item: &Item) -> f64 {
    let centers = line_centers(item);
    if centers.len() < 2 {
        return 0.0;
    }
    let mut diffs: Vec<f64> = Vec::with_capacity(centers.len() - 1);
    for i in 0..centers.len() - 1 {
        diffs.push(centers[i + 1] - centers[i]);
    }
    diffs.retain(|d| *d > 0.0);
    if diffs.is_empty() {
        0.0
    } else {
        median_f64(&diffs)
    }
}

pub fn local_glyph_height(item: &Item) -> f64 {
    median_line_height(item).max(0.0)
}

pub fn local_font_metric(item: &Item) -> f64 {
    let glyph_height = local_glyph_height(item);
    let pitch = median_line_pitch(item);
    if glyph_height > 0.0 {
        // Python branches on LOOSE_LINE_PITCH_RATIO but both return the same value.
        return glyph_height * LINE_HEIGHT_TO_FONT_SCALE;
    }
    if pitch > 0.0 {
        return pitch * LINE_PITCH_TO_FONT_SCALE;
    }
    0.0
}

pub fn bbox_width(item: &Item) -> f64 {
    match item.bbox {
        Some(b) => (b[2] - b[0]).max(0.0),
        None => 0.0,
    }
}

pub fn bbox_height(item: &Item) -> f64 {
    match item.bbox {
        Some(b) => (b[3] - b[1]).max(0.0),
        None => 0.0,
    }
}

pub fn effective_text_height(item: &Item) -> f64 {
    let line_boxes: Vec<[f64; 4]> = item.lines.iter().filter_map(|l| l.bbox).collect();
    if line_boxes.is_empty() {
        return bbox_height(item);
    }
    let top = line_boxes.iter().map(|b| b[1]).fold(f64::INFINITY, f64::min);
    let bottom = line_boxes.iter().map(|b| b[3]).fold(f64::NEG_INFINITY, f64::max);
    let raw_height = (bottom - top).max(0.0);
    let median_height = median_line_height(item);
    if raw_height <= 0.0 {
        return bbox_height(item);
    }
    let padding = if median_height > 0.0 {
        (median_height * TEXT_HEIGHT_PADDING_RATIO).min(TEXT_HEIGHT_PADDING_MAX_PT)
    } else {
        0.0
    };
    (raw_height + padding).min(bbox_height(item))
}

pub fn local_line_pitch(item: &Item) -> f64 {
    let block_height = effective_text_height(item);
    let lines = source_visual_line_count(item);
    if block_height <= 0.0 || lines <= 0 {
        return 0.0;
    }
    block_height / lines as f64
}

pub fn source_text_height_limit_pt(item: &Item) -> f64 {
    let mut text_height = effective_text_height(item);
    if text_height <= 0.0 {
        text_height = bbox_height(item);
    }
    if text_height <= 0.0 {
        return 0.0;
    }
    (text_height * SOURCE_HEIGHT_LIMIT_RATIO)
        .min(bbox_height(item))
        .max(SOURCE_HEIGHT_LIMIT_MIN_PT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line};

    fn item_with_lines(lines: Vec<Line>) -> Item {
        Item { lines, ..Default::default() }
    }

    #[test]
    fn median_line_height_avg_two() {
        let item = item_with_lines(vec![
            Line { bbox: Some([0.0, 0.0, 10.0, 10.0]), spans: vec![] },
            Line { bbox: Some([0.0, 10.0, 10.0, 20.0]), spans: vec![] },
            Line { bbox: Some([0.0, 20.0, 10.0, 22.0]), spans: vec![] },
        ]);
        assert_eq!(median_line_height(&item), 10.0);
    }

    #[test]
    fn median_line_pitch_pos_only() {
        let item = item_with_lines(vec![
            Line { bbox: Some([0.0, 0.0, 10.0, 10.0]), spans: vec![] },
            Line { bbox: Some([0.0, 10.0, 10.0, 20.0]), spans: vec![] },
            Line { bbox: Some([0.0, 8.0, 10.0, 18.0]), spans: vec![] }, // overlaps: -2 pitch
        ]);
        // centers: 5, 15, 13 → diffs 10, -2 → keep 10
        assert_eq!(median_line_pitch(&item), 10.0);
    }

    #[test]
    fn local_font_metric_uses_height() {
        let item = item_with_lines(vec![Line { bbox: Some([0.0, 0.0, 10.0, 20.0]), spans: vec![] }]);
        assert_eq!(local_font_metric(&item), 20.0 * 0.98);
    }

    #[test]
    fn effective_text_height_capped_by_bbox() {
        let item = Item {
            bbox: Some([0.0, 0.0, 100.0, 30.0]),
            lines: vec![
                Line { bbox: Some([0.0, 2.0, 100.0, 12.0]), spans: vec![] },
                Line { bbox: Some([0.0, 12.0, 100.0, 22.0]), spans: vec![] },
            ],
            ..Default::default()
        };
        // raw 20, median 10, padding 2.2 → 22.2, capped by bbox 30
        assert_eq!(effective_text_height(&item), 22.2);
    }
}
