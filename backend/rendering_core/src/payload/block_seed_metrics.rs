// Port of services/rendering/layout/payload/block_seed_metrics.py — the
// per-page seed baseline consumed by `build_seed_payload_for_item`.

use crate::font_roles::is_body_text_candidate;
use crate::font_size_fit::estimate_font_size_pt;
use crate::item::Item;
use crate::leading_fit::estimate_leading_em;
use crate::payload::geometry_adjustments::build_effective_inner_bboxes;
use crate::semantics::{block_kind, is_caption_like_block, is_footnote_like_block};
use crate::typography::baseline::page_baseline_font_size;
use crate::typography::line_metrics::bbox_width;
use crate::typography::scalars::percentile_value;
use crate::util::{median_f64, py_round};
use std::collections::HashMap;

const BODY_PAGE_FONT_ANCHOR_PERCENTILE: f64 = 0.46;
const BODY_PAGE_FONT_FLOOR_DELTA_PT: f64 = 0.38;

#[derive(Debug, Clone)]
pub struct PageSeedMetrics {
    pub page_font_size: f64,
    pub page_line_pitch: f64,
    pub page_line_height: f64,
    pub density_baseline: f64,
    pub page_text_width_med: f64,
    pub body_flags: HashMap<usize, bool>,
    pub base_metrics: HashMap<usize, (f64, f64)>,
    pub effective_inner_bboxes: HashMap<usize, Vec<f64>>,
    pub page_body_font_size_pt: Option<f64>,
    pub page_body_width_pt: Option<f64>,
}

fn is_annotation_like(item: &Item) -> bool {
    is_caption_like_block(item) || is_footnote_like_block(item)
}

/// `collect_page_seed_metrics`: page font baseline, per-item base metrics with
/// the body-candidate flag cloned in, effective inner bboxes after geometry
/// adjustments, and the body font anchors.
pub fn collect_page_seed_metrics(translated_items: &[Item], page_width: Option<f64>) -> PageSeedMetrics {
    let item_refs: Vec<&Item> = translated_items.iter().collect();
    let (page_font_size, page_line_pitch, page_line_height, density_baseline) =
        page_baseline_font_size(&item_refs);
    let text_widths: Vec<f64> = translated_items
        .iter()
        .filter(|item| block_kind(item) == "text" && !is_annotation_like(item))
        .map(bbox_width)
        .collect();
    let page_text_width_med = if text_widths.is_empty() { 0.0 } else { median_f64(&text_widths) };

    let mut body_base_sizes: Vec<f64> = Vec::new();
    let mut body_flags: HashMap<usize, bool> = HashMap::new();
    let mut base_metrics: HashMap<usize, (f64, f64)> = HashMap::new();

    for (index, item) in translated_items.iter().enumerate() {
        let is_body = is_body_text_candidate(item, page_text_width_med);
        let mut item_with_flag = item.clone();
        item_with_flag.is_body_text_candidate = is_body;
        body_flags.insert(index, is_body);
        let font_size_pt =
            estimate_font_size_pt(&item_with_flag, page_font_size, page_line_pitch, page_line_height, density_baseline);
        let leading_em = estimate_leading_em(&item_with_flag, page_line_pitch, font_size_pt);
        base_metrics.insert(index, (font_size_pt, leading_em));
        if is_body {
            body_base_sizes.push(font_size_pt);
        }
    }

    let page_body_font_size_pt = if body_base_sizes.is_empty() {
        None
    } else {
        let mut anchor = py_round(percentile_value(&body_base_sizes, BODY_PAGE_FONT_ANCHOR_PERCENTILE), 2);
        if page_font_size > 0.0 {
            anchor = py_round(anchor.max(page_font_size - BODY_PAGE_FONT_FLOOR_DELTA_PT), 2);
        }
        Some(anchor)
    };

    let body_widths: Vec<f64> = translated_items
        .iter()
        .enumerate()
        .filter(|(index, _)| body_flags.get(index).copied().unwrap_or(false))
        .map(|(_, item)| bbox_width(item))
        .collect();
    let page_body_width_pt = if body_widths.is_empty() { None } else { Some(median_f64(&body_widths)) };

    let effective_inner_bboxes = build_effective_inner_bboxes(translated_items, &body_flags, page_width);

    PageSeedMetrics {
        page_font_size,
        page_line_pitch,
        page_line_height,
        density_baseline,
        page_text_width_med,
        body_flags,
        base_metrics,
        effective_inner_bboxes,
        page_body_font_size_pt,
        page_body_width_pt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{Item, Line, Span};

    fn text_item(x0: f64, width: f64) -> Item {
        Item {
            block_type: Some("text".into()),
            layout_role: Some("paragraph".into()),
            semantic_role: Some("body".into()),
            source_text: "这是一段足够长的正文文字，用于满足候选条件，保证长度超过阈值。".repeat(2),
            bbox: Some([x0, 40.0, x0 + width, 160.0]),
            lines: vec![
                Line { bbox: Some([x0, 40.0, x0 + width, 80.0]), spans: vec![Span { span_type: "text".into(), content: "a".into() }] },
                Line { bbox: Some([x0, 80.0, x0 + width, 120.0]), spans: vec![Span { span_type: "text".into(), content: "b".into() }] },
                Line { bbox: Some([x0, 120.0, x0 + width, 160.0]), spans: vec![Span { span_type: "text".into(), content: "c".into() }] },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn empty_items_zero_metrics() {
        let metrics = collect_page_seed_metrics(&[], Some(595.0));
        assert_eq!(metrics.page_text_width_med, 0.0);
        assert!(metrics.page_body_font_size_pt.is_none());
        assert!(metrics.page_body_width_pt.is_none());
        assert!(metrics.body_flags.is_empty());
    }

    #[test]
    fn page_text_width_med_medians_text_widths() {
        let items = vec![text_item(20.0, 300.0), text_item(340.0, 240.0)];
        let metrics = collect_page_seed_metrics(&items, Some(595.0));
        assert_eq!(metrics.page_text_width_med, 270.0);
        assert!(metrics.body_flags.len() == 2);
        assert!(metrics.base_metrics.len() == 2);
    }

    #[test]
    fn caption_like_excluded_from_text_widths() {
        let mut caption = text_item(20.0, 300.0);
        caption.layout_role = Some("caption".into());
        let items = vec![caption];
        let metrics = collect_page_seed_metrics(&items, Some(595.0));
        assert_eq!(metrics.page_text_width_med, 0.0);
    }
}
