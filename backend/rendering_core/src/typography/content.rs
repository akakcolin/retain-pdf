// Port of services/rendering/layout/typography/content.py.

use crate::item::Item;
use crate::util::median_usize;

pub fn plain_text_chars_per_line(item: &Item) -> f64 {
    let mut counts: Vec<usize> = Vec::new();
    for line in &item.lines {
        let mut text_chunks: Vec<&str> = Vec::new();
        for span in &line.spans {
            if span.span_type != "text" {
                continue;
            }
            text_chunks.push(&span.content);
        }
        let plain: String = text_chunks
            .concat()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if !plain.is_empty() {
            counts.push(plain.chars().count());
        }
    }
    if counts.is_empty() {
        0.0
    } else {
        median_usize(&counts)
    }
}

pub fn formula_ratio(item: &Item) -> f64 {
    let mut text_spans = 0usize;
    let mut formula_spans = 0usize;
    for line in &item.lines {
        for span in &line.spans {
            if span.span_type == "inline_equation" {
                formula_spans += 1;
            } else if span.span_type == "text" {
                text_spans += 1;
            }
        }
    }
    let total = text_spans + formula_spans;
    if total == 0 {
        0.0
    } else {
        formula_spans as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line, Span};

    fn sample() -> Item {
        Item {
            lines: vec![
                Line {
                    bbox: Some([0.0, 0.0, 100.0, 12.0]),
                    spans: vec![Span { span_type: "text".into(), content: "ab 中文".into() }],
                },
                Line {
                    bbox: Some([0.0, 12.0, 100.0, 24.0]),
                    spans: vec![
                        Span { span_type: "text".into(), content: "xy".into() },
                        Span { span_type: "inline_equation".into(), content: "z".into() },
                    ],
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn chars_per_line_median() {
        // line1 plain "ab中文" (4), line2 plain "xy" (2) → median 3
        assert_eq!(plain_text_chars_per_line(&sample()), 3.0);
    }

    #[test]
    fn formula_ratio_counts_spans() {
        // 2 text spans, 1 equation span → 1/3
        let r = formula_ratio(&sample());
        assert!((r - 1.0 / 3.0).abs() < 1e-12);
    }
}
