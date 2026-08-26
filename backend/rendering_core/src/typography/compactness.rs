// Port of services/rendering/layout/typography/compactness.py.

use crate::item::Item;
use crate::typography::constants::{
    SOURCE_COMPACTNESS_LINE_TRIGGER, SOURCE_COMPACTNESS_MAX, SOURCE_COMPACTNESS_TEXT_TRIGGER,
    SOURCE_COMPACTNESS_X_TRIGGER, SOURCE_COMPACTNESS_Y_TRIGGER,
};
use crate::typography::content::formula_ratio;
use crate::typography::line_count::source_visual_line_count;
use crate::typography::line_metrics::{bbox_height, bbox_width, line_height};
use crate::typography::scalars::clamp;
use crate::util::median_f64;

pub fn occupied_ratio(item: &Item) -> f64 {
    let block_height = bbox_height(item);
    if block_height <= 0.0 {
        return 0.0;
    }
    let total: f64 = item.lines.iter().map(line_height).sum();
    total / block_height
}

pub fn line_widths(item: &Item) -> Vec<f64> {
    let mut widths: Vec<f64> = Vec::new();
    for line in &item.lines {
        if let Some(b) = line.bbox {
            widths.push((b[2] - b[0]).max(0.0));
        }
    }
    widths
}

pub fn occupied_ratio_x(item: &Item) -> f64 {
    let block_width = bbox_width(item);
    if block_width <= 0.0 {
        return 0.0;
    }
    let mut widths = line_widths(item);
    if widths.len() > 1 {
        widths.truncate(widths.len() - 1);
    }
    widths.retain(|w| *w > 0.0);
    if widths.is_empty() {
        0.0
    } else {
        median_f64(&widths) / block_width
    }
}

pub fn source_compactness_score(item: &Item) -> f64 {
    let text_len = item
        .source_text
        .chars()
        .filter(|c| !c.is_whitespace())
        .count();
    if text_len < 36 {
        return 0.0;
    }
    let lines = source_visual_line_count(item);
    let density_x = occupied_ratio_x(item);
    let density_y = occupied_ratio(item);
    let mut score = 0.0;

    if text_len >= SOURCE_COMPACTNESS_TEXT_TRIGGER {
        score += (0.22f64).min((text_len - SOURCE_COMPACTNESS_TEXT_TRIGGER) as f64 / 220.0);
    }
    if lines >= SOURCE_COMPACTNESS_LINE_TRIGGER as i64 {
        score += (0.30f64).min(
            (lines - (SOURCE_COMPACTNESS_LINE_TRIGGER as i64 - 1)).max(0) as f64 * 0.08,
        );
    }
    if density_x >= SOURCE_COMPACTNESS_X_TRIGGER {
        score += (0.24f64).min(((density_x - SOURCE_COMPACTNESS_X_TRIGGER) / 0.16) * 0.24);
    }
    if density_y >= SOURCE_COMPACTNESS_Y_TRIGGER {
        score += (0.12f64).min(((density_y - SOURCE_COMPACTNESS_Y_TRIGGER) / 0.24) * 0.12);
    }
    if formula_ratio(item) >= 0.08 {
        score += 0.08;
    }

    clamp(score, 0.0, SOURCE_COMPACTNESS_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line};

    #[test]
    fn occupied_ratio_zero_when_no_height() {
        let item = Item::default();
        assert_eq!(occupied_ratio(&item), 0.0);
        assert_eq!(occupied_ratio_x(&item), 0.0);
        assert_eq!(source_compactness_score(&item), 0.0);
    }

    #[test]
    fn occupied_ratio_x_drops_last_line() {
        let item = Item {
            bbox: Some([0.0, 0.0, 100.0, 50.0]),
            lines: vec![
                Line { bbox: Some([0.0, 0.0, 80.0, 10.0]), spans: vec![] },
                Line { bbox: Some([0.0, 10.0, 60.0, 20.0]), spans: vec![] },
            ],
            ..Default::default()
        };
        // widths [80, 60], drop last → [80], median 80 / 100 = 0.8
        assert_eq!(occupied_ratio_x(&item), 0.8);
    }
}
