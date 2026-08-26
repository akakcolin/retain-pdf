// Port of services/rendering/layout/typography/line_count.py.

use crate::item::Item;
use crate::semantics::{is_plain_bodylike_block, semantic_role};
use crate::typography::constants::{
    APPROX_TEXT_CHAR_WIDTH_PT, FORMULA_CHARS_PER_LINE_PENALTY, LINE_COUNT_GROW_THRESHOLD,
    LINE_COUNT_PREDICT_TRIGGER_CHARS, MIN_TEXT_LINE_PITCH_PT, SINGLE_LINE_GLUE_HEIGHT_TRIGGER_LINES,
    SINGLE_LINE_GLUE_WIDTH_CHAR_RATIO, VISUAL_LINE_COUNT_MAX,
};
use crate::typography::content::{formula_ratio, plain_text_chars_per_line};
use crate::typography::line_metrics::{bbox_height, bbox_width, median_line_height};
use crate::typography::scalars::clamp;

fn nonspace_len(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

pub fn is_tall_single_line_glue(
    item: &Item,
    text_len: Option<usize>,
    observed_chars: Option<f64>,
    geometric_chars_per_line: Option<f64>,
) -> bool {
    let observed_line_count = item.lines.len();
    if observed_line_count > 1 {
        return false;
    }
    let block_height = bbox_height(item);
    if block_height <= 0.0 {
        return false;
    }
    let text_len = match text_len {
        Some(v) => v,
        None => nonspace_len(&item.source_text),
    };
    if text_len < LINE_COUNT_PREDICT_TRIGGER_CHARS {
        return false;
    }
    let observed_chars = match observed_chars {
        Some(v) => v,
        None => plain_text_chars_per_line(item),
    };
    let geometric_chars_per_line = match geometric_chars_per_line {
        Some(v) => v,
        None => clamp(bbox_width(item) / APPROX_TEXT_CHAR_WIDTH_PT, 10.0, 88.0),
    };
    let median_height = median_line_height(item);
    block_height
        >= (MIN_TEXT_LINE_PITCH_PT * SINGLE_LINE_GLUE_HEIGHT_TRIGGER_LINES)
            .max(median_height * 3.0)
        || (observed_chars > 0.0
            && observed_chars >= geometric_chars_per_line * SINGLE_LINE_GLUE_WIDTH_CHAR_RATIO)
}

fn predicted_wrapped_line_count(item: &Item, width: f64, text_len: usize) -> i64 {
    if width <= 0.0 || text_len < LINE_COUNT_PREDICT_TRIGGER_CHARS {
        return 0;
    }
    let observed_chars = plain_text_chars_per_line(item);
    let geometric_chars_per_line = clamp(width / APPROX_TEXT_CHAR_WIDTH_PT, 10.0, 88.0);
    let mut approx_chars_per_line = if observed_chars > 0.0 {
        observed_chars
    } else {
        geometric_chars_per_line
    };
    if is_tall_single_line_glue(
        item,
        Some(text_len),
        Some(observed_chars),
        Some(geometric_chars_per_line),
    ) {
        approx_chars_per_line = geometric_chars_per_line;
    }
    if formula_ratio(item) > 0.0 {
        approx_chars_per_line *= FORMULA_CHARS_PER_LINE_PENALTY;
    }
    let item_semantic_role = semantic_role(item);
    if item_semantic_role == "body" || item_semantic_role == "abstract" || is_plain_bodylike_block(item) {
        approx_chars_per_line *= 0.96;
    }
    let effective_chars_per_line = (approx_chars_per_line * 1.02).max(8.0);
    ((text_len as f64 / effective_chars_per_line).ceil() as i64).max(1)
}

pub fn visual_line_count(item: &Item) -> i64 {
    let observed = item.lines.len().max(1) as i64;
    let width = bbox_width(item);
    let block_height = bbox_height(item);
    let text_len = nonspace_len(&item.source_text);
    let predicted_by_text = predicted_wrapped_line_count(item, width, text_len);
    let max_lines_by_height = if block_height > 0.0 {
        ((block_height / MIN_TEXT_LINE_PITCH_PT).floor() as i64).max(1)
    } else {
        observed
    };
    let predicted_lower_bound = if predicted_by_text > 0 {
        max_lines_by_height.min(predicted_by_text)
    } else {
        observed
    };

    if predicted_lower_bound <= observed {
        return VISUAL_LINE_COUNT_MAX.min(observed);
    }

    let growth_ratio = predicted_lower_bound as f64 / (observed.max(1) as f64);
    if observed == 1 {
        return VISUAL_LINE_COUNT_MAX.min(observed.max(predicted_lower_bound));
    }

    if growth_ratio >= LINE_COUNT_GROW_THRESHOLD {
        return VISUAL_LINE_COUNT_MAX.min(predicted_lower_bound);
    }
    VISUAL_LINE_COUNT_MAX.min(observed)
}

pub fn source_visual_line_count(item: &Item) -> i64 {
    let line_count = item.lines.len();
    if line_count > 0 {
        return line_count as i64;
    }
    let explicit = item
        .source_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    (explicit.max(1)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line};

    #[test]
    fn tall_single_line_glue_by_height() {
        // One line but block_height is huge.
        let item = Item {
            bbox: Some([0.0, 0.0, 100.0, 50.0]),
            source_text: "a".repeat(60),
            lines: vec![Line { bbox: Some([0.0, 0.0, 100.0, 10.0]), spans: vec![] }],
            ..Default::default()
        };
        assert!(is_tall_single_line_glue(&item, None, None, None));
    }

    #[test]
    fn multiple_lines_not_glue() {
        let item = Item {
            bbox: Some([0.0, 0.0, 100.0, 50.0]),
            source_text: "a".repeat(60),
            lines: vec![
                Line { bbox: Some([0.0, 0.0, 100.0, 10.0]), spans: vec![] },
                Line { bbox: Some([0.0, 10.0, 100.0, 20.0]), spans: vec![] },
            ],
            ..Default::default()
        };
        assert!(!is_tall_single_line_glue(&item, None, None, None));
    }

    #[test]
    fn source_visual_count_falls_back_to_text() {
        let item = Item { source_text: "a\nb\n\nc\n".into(), ..Default::default() };
        assert_eq!(source_visual_line_count(&item), 3);
    }

    #[test]
    fn source_visual_count_uses_lines() {
        let item = Item {
            lines: vec![Line { bbox: None, spans: vec![] }, Line { bbox: None, spans: vec![] }],
            ..Default::default()
        };
        assert_eq!(source_visual_line_count(&item), 2);
    }
}
