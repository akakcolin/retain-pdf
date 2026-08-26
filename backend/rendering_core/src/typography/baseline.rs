// Port of services/rendering/layout/typography/baseline.py.

use crate::config::{BODY_FONT_SIZE_FACTOR, DEFAULT_FONT_SIZE};
use crate::item::Item;
use crate::semantics::{block_kind, is_caption_like_block, is_footnote_like_block};
use crate::typography::constants::{
    BODY_FORMULA_RATIO_MAX, LINE_HEIGHT_TO_FONT_SCALE, LINE_PITCH_TO_FONT_SCALE,
    MAX_LOCAL_FONT_SIZE_PT, MIN_FONT_SIZE_PT, PAGE_BASELINE_PERCENTILE,
};
use crate::typography::content::{formula_ratio, plain_text_chars_per_line};
use crate::typography::line_count::source_visual_line_count;
use crate::typography::line_metrics::{bbox_width, local_font_metric, local_line_pitch, median_line_height, median_line_pitch};
use crate::typography::scalars::percentile_value;
use crate::util::{median_f64, median_usize};

fn nonspace_len(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

pub fn candidate_text_items<'a>(items: &[&'a Item]) -> Vec<&'a Item> {
    let mut widths: Vec<f64> = Vec::new();
    for item in items {
        if block_kind(item) == "text" && !is_caption_like_block(item) && !is_footnote_like_block(item) {
            widths.push(bbox_width(item));
        }
    }
    let page_text_width_med = if widths.is_empty() { 0.0 } else { median_f64(&widths) };

    let mut candidates: Vec<&Item> = Vec::new();
    for item in items {
        if block_kind(item) != "text" {
            continue;
        }
        if is_caption_like_block(item) || is_footnote_like_block(item) {
            continue;
        }
        if source_visual_line_count(item) < 3 {
            continue;
        }
        if nonspace_len(&item.source_text) < 40 {
            continue;
        }
        if formula_ratio(item) > BODY_FORMULA_RATIO_MAX {
            continue;
        }
        if page_text_width_med > 0.0 && bbox_width(item) < page_text_width_med * 0.6 {
            continue;
        }
        candidates.push(item);
    }
    candidates
}

pub fn page_baseline_font_size(items: &[&Item]) -> (f64, f64, f64, f64) {
    let candidates = candidate_text_items(items);

    let mut line_pitches: Vec<f64> = Vec::new();
    for item in &candidates {
        let pitch = local_line_pitch(item);
        if pitch > 0.0 {
            line_pitches.push(pitch);
        } else {
            let m = median_line_pitch(item);
            if m > 0.0 {
                line_pitches.push(m);
            }
        }
    }
    let mut line_heights: Vec<f64> = Vec::new();
    for item in &candidates {
        let h = median_line_height(item);
        if h > 0.0 {
            line_heights.push(h);
        }
    }
    let mut font_metrics: Vec<f64> = Vec::new();
    for item in &candidates {
        let m = local_font_metric(item);
        if m > 0.0 {
            font_metrics.push(m);
        }
    }

    let baseline_line_pitch = if line_pitches.is_empty() {
        0.0
    } else {
        percentile_value(&line_pitches, PAGE_BASELINE_PERCENTILE)
    };
    let baseline_line_height = if line_heights.is_empty() {
        0.0
    } else {
        percentile_value(&line_heights, PAGE_BASELINE_PERCENTILE)
    };
    let mut metric = if font_metrics.is_empty() {
        0.0
    } else {
        percentile_value(&font_metrics, PAGE_BASELINE_PERCENTILE)
    };
    if metric <= 0.0 {
        metric = if baseline_line_height > 0.0 {
            baseline_line_height * LINE_HEIGHT_TO_FONT_SCALE
        } else {
            baseline_line_pitch * LINE_PITCH_TO_FONT_SCALE
        };
    }
    if metric <= 0.0 {
        return (DEFAULT_FONT_SIZE, 0.0, 0.0, 0.0);
    }
    let page_font_size = (metric * BODY_FONT_SIZE_FACTOR)
        .min(MAX_LOCAL_FONT_SIZE_PT)
        .max(MIN_FONT_SIZE_PT);
    let mut chars_per_line: Vec<usize> = Vec::new();
    for item in &candidates {
        let v = plain_text_chars_per_line(item);
        if v > 0.0 {
            chars_per_line.push(v as usize);
        }
    }
    let density_baseline = if chars_per_line.is_empty() { 0.0 } else { median_usize(&chars_per_line) };
    (page_font_size, baseline_line_pitch, baseline_line_height, density_baseline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line, Span};

    #[test]
    fn no_candidates_returns_defaults() {
        let item = Item::default();
        let (size, pitch, height, density) = page_baseline_font_size(&[&item]);
        assert_eq!(size, DEFAULT_FONT_SIZE);
        assert_eq!(pitch, 0.0);
        assert_eq!(height, 0.0);
        assert_eq!(density, 0.0);
    }

    #[test]
    fn single_candidate_baseline() {
        // One body-like text block with 3+ source lines and 40+ chars.
        let mut item = Item::default();
        item.block_type = Some("text".into());
        item.source_text = "这是一段足够长的正文文字，用于满足候选条件。".repeat(3);
        item.lines = vec![
            Line { bbox: Some([0.0, 0.0, 100.0, 12.0]), spans: vec![Span { span_type: "text".into(), content: "abc".into() }] },
            Line { bbox: Some([0.0, 12.0, 100.0, 24.0]), spans: vec![Span { span_type: "text".into(), content: "def".into() }] },
            Line { bbox: Some([0.0, 24.0, 100.0, 36.0]), spans: vec![Span { span_type: "text".into(), content: "ghi".into() }] },
        ];
        let (size, pitch, height, _density) = page_baseline_font_size(&[&item]);
        assert!(size >= MIN_FONT_SIZE_PT);
        assert!(size <= MAX_LOCAL_FONT_SIZE_PT);
        assert!(pitch > 0.0);
        assert!(height > 0.0);
    }
}
