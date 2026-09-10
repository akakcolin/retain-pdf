//! N11e: native `apply_adaptive_overlay_colors_batch` —
//! `output/typst/color_adapt.py`'s production color-adaptation entry point. All
//! four render modes funnel here with `precomputed_colors_by_item_id={}`:
//! overlay/dual via `overlay_color.apply_overlay_page_colors` and
//! typst/typst_visual via `book_renderer._apply_background_page_color_adapt`
//! (the visual profile is not loaded in the bundle path), so this single native
//! implementation serves every mode. `indent_pdf_path` is the RAW source PDF
//! (`spec.inputs.source_pdf`), so color samples come from the original source,
//! never the render-source intermediate.
//!
//! Mirrors `output/typst/_native.py::apply_adaptive_overlay_colors_batch`:
//! per page it builds the batch sampler + per-target fills over the needs-
//! sampling / title-like cover rects, extracts per-title (or whole-page) span
//! dicts, probes title-visual foreground colors, then runs the shared
//! `_apply_adaptive_overlay_colors_with_data` decision tree that writes
//! `_render_cover_fill` / `_render_text_color` onto every item. Out-of-range or
//! unreadable pages pass through as shallow copies.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use mupdf::Document;
use rendering_core::item::Item;
use rendering_core::payload::policy_compat::{item_overlay_fill, item_uses_explicit_white_overlay_fill};
use rendering_core::rect::Rect;
use rendering_core::semantics::is_title_like_block;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_core::typography::geometry::cover_bbox;
use rendering_reader::{open_document, PdfDocument as _};
use rendering_writer::background::color_adapt::{
    build_text_page_for_extraction, extract_span_dicts_from_text_page, title_foreground_color_from_pixmap,
    SpanEntry,
};
use rendering_writer::background::fill::{
    build_local_background_sampler, sample_local_background_fill, RgbPixmap,
};
use rendering_writer::background::patch::{rect_is_empty, rect_is_finite, rect_intersection};
use serde_json::{json, Value};

use super::prepare::{py_item_id, value_falsy};

const DARK_BACKGROUND_BRIGHTNESS_MAX: f64 = 0.42;
const LIGHT_BACKGROUND_VISUAL_TITLE_BRIGHTNESS_MIN: f64 = 0.94;
const PAGE_TEXT_COLOR_SAMPLER_MIN_TITLES: usize = 2;
const TITLE_COLOR_SAMPLE_SCALE: f32 = 3.0;
const TITLE_COLOR_QUANTUM: f64 = 16.0;
const DEFAULT_COVER_FILL: [f64; 3] = [1.0, 1.0, 1.0];

/// `color_adapt.relative_brightness`.
fn relative_brightness(color: &[f64; 3]) -> f64 {
    0.299 * color[0] + 0.587 * color[1] + 0.114 * color[2]
}

/// `color_adapt.text_color_for_fill`.
fn text_color_for_fill(fill: &[f64; 3]) -> [f64; 3] {
    if relative_brightness(fill) <= DARK_BACKGROUND_BRIGHTNESS_MAX {
        [1.0, 1.0, 1.0]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// `color_adapt.should_probe_title_visual_color`.
fn should_probe_title_visual_color(fill: &[f64; 3]) -> bool {
    relative_brightness(fill) < LIGHT_BACKGROUND_VISUAL_TITLE_BRIGHTNESS_MIN
}

/// `color_adapt._item_needs_local_color_sampling`.
fn item_needs_local_color_sampling(item: &Value) -> bool {
    item_overlay_fill(item) == "sampled" || py_bool(item, "_render_use_cover_fill")
}

/// `color_adapt._item_uses_explicit_white_fill`.
fn item_uses_explicit_white_fill(item: &Value) -> bool {
    item_uses_explicit_white_overlay_fill(item) && !item_needs_local_color_sampling(item)
}

/// Python `bool(item.get(key))` over scalar dict values.
fn py_bool(item: &Value, key: &str) -> bool {
    match item.get(key) {
        Some(v) => !value_falsy(v),
        None => false,
    }
}

/// `color_adapt._local_sampling_rects` / `cover_rect`: the item's `cover_bbox`
/// as a finite non-empty `RectTuple`, else `None`.
fn cover_rect(item: &Value) -> Option<RectTuple> {
    let bbox = cover_bbox(&Item::from_json_value(item));
    if bbox.len() != 4 {
        return None;
    }
    let rect: RectTuple = [bbox[0], bbox[1], bbox[2], bbox[3]];
    if rect_is_empty(&rect) || !rect_is_finite(&rect) {
        return None;
    }
    Some(rect)
}

/// A span-dict sample as the decision tree consumes it (Python
/// `SpanColorSample`): page-space rect, raw text, 0xRRGGBB-split `rgb`.
struct SpanSample {
    rect: RectTuple,
    text: String,
    rgb: [u8; 3],
}

/// Decode `SpanEntry` (0xRRGGBB `color`) into `SpanSample` (per-channel `rgb`),
/// mirroring the bridge shim's `decode_spans`.
fn decode_spans(entries: &[SpanEntry]) -> Vec<SpanSample> {
    entries
        .iter()
        .map(|entry| SpanSample {
            rect: entry.rect,
            text: entry.text.clone(),
            rgb: [
                ((entry.color >> 16) & 255) as u8,
                ((entry.color >> 8) & 255) as u8,
                (entry.color & 255) as u8,
            ],
        })
        .collect()
}

/// Non-empty-overlap test between two page-space rects (fitz `Rect.intersects`).
fn rect_intersects(rect: &RectTuple, other: &RectTuple) -> bool {
    rect[0] < other[2] && rect[2] > other[0] && rect[1] < other[3] && rect[3] > other[1]
}

/// Python `round(x)` (round-half-even), matching `color_adapt._float_color_from_rgb`.
fn round_ties_even(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// `color_adapt._title_text_color_from_span_samples` with `background=None` (the
/// bundle path never filters against a background): bucket intersecting spans by
/// `color // 16`, weight by `max(1, len(text.strip()))`, pick the max-count
/// bucket (first on ties), average back to a 0..1 color.
fn title_text_color_from_span_samples(samples: &[SpanSample], rect: &RectTuple) -> Option<[f64; 3]> {
    if rect_is_empty(rect) || !rect_is_finite(rect) {
        return None;
    }
    // Insertion-ordered buckets so the max-count pick is first-wins on ties,
    // matching Python dict ordering.
    let mut buckets: Vec<((u8, u8, u8), [i64; 4])> = Vec::new();
    for sample in samples {
        if !rect_intersects(&sample.rect, rect) {
            continue;
        }
        let key = (
            (sample.rgb[0] as i64 / TITLE_COLOR_QUANTUM as i64) as u8,
            (sample.rgb[1] as i64 / TITLE_COLOR_QUANTUM as i64) as u8,
            (sample.rgb[2] as i64 / TITLE_COLOR_QUANTUM as i64) as u8,
        );
        let weight = sample.text.trim().chars().count().max(1) as i64;
        match buckets.iter_mut().find(|(k, _)| *k == key) {
            Some((_, sums)) => {
                sums[0] += sample.rgb[0] as i64 * weight;
                sums[1] += sample.rgb[1] as i64 * weight;
                sums[2] += sample.rgb[2] as i64 * weight;
                sums[3] += weight;
            }
            None => buckets.push((
                key,
                [
                    sample.rgb[0] as i64 * weight,
                    sample.rgb[1] as i64 * weight,
                    sample.rgb[2] as i64 * weight,
                    weight,
                ],
            )),
        }
    }
    if buckets.is_empty() {
        return None;
    }
    let mut best = 0usize;
    for (i, (_, sums)) in buckets.iter().enumerate().skip(1) {
        if sums[3] > buckets[best].1[3] {
            best = i;
        }
    }
    let bucket = &buckets[best].1;
    let count = bucket[3];
    if count <= 0 {
        return None;
    }
    Some([
        round_ties_even(bucket[0] as f64 / count as f64) as f64 / 255.0,
        round_ties_even(bucket[1] as f64 / count as f64) as f64 / 255.0,
        round_ties_even(bucket[2] as f64 / count as f64) as f64 / 255.0,
    ])
}

/// `color_adapt._apply_adaptive_overlay_colors_with_data` — the decision tree
/// over the per-page lookups. `precomputed_colors_by_item_id` is always empty in
/// the bundle path, so every item goes through the sampling branches.
fn adapt_item(
    item: &Value,
    fill_by_item_id: &BTreeMap<String, [f64; 3]>,
    span_sampler_samples: Option<&[SpanSample]>,
    span_clip_by_item_id: &BTreeMap<String, Vec<SpanSample>>,
    visual_by_item_id: &BTreeMap<String, Option<[f64; 3]>>,
) -> Value {
    let mut next = item.clone();
    let item_id = py_item_id(item);
    let title_like = is_title_like_block(&Item::from_json_value(item));
    let needs_sampling = item_needs_local_color_sampling(item);

    let mut rect: Option<RectTuple> = None;
    let mut fill: [f64; 3];
    if needs_sampling {
        fill = fill_by_item_id.get(&item_id).copied().unwrap_or(DEFAULT_COVER_FILL);
        if let Some(r) = cover_rect(item) {
            rect = Some(r);
        }
    } else {
        fill = DEFAULT_COVER_FILL;
        if title_like {
            rect = cover_rect(item);
        }
    }
    next["_render_cover_fill"] = json!([fill[0], fill[1], fill[2]]);
    let mut text_color = text_color_for_fill(&fill);
    if let Some(r) = rect {
        if title_like {
            let mut title_color = if let Some(samples) = span_sampler_samples {
                title_text_color_from_span_samples(samples, &r)
            } else {
                title_text_color_from_span_samples(
                    span_clip_by_item_id.get(&item_id).map(Vec::as_slice).unwrap_or(&[]),
                    &r,
                )
            };
            if title_color.is_none() {
                if !needs_sampling && !item_uses_explicit_white_fill(item) {
                    fill = fill_by_item_id.get(&item_id).copied().unwrap_or(DEFAULT_COVER_FILL);
                    next["_render_cover_fill"] = json!([fill[0], fill[1], fill[2]]);
                    text_color = text_color_for_fill(&fill);
                }
                title_color = if should_probe_title_visual_color(&fill) {
                    visual_by_item_id.get(&item_id).copied().flatten()
                } else {
                    None
                };
            }
            if let Some(color) = title_color {
                text_color = color;
            }
        }
    }
    next["_render_text_color"] = json!([text_color[0], text_color[1], text_color[2]]);
    next
}

/// Color-adapt one page in place of the fitz page reference: build the fills,
/// span samples, and visual probes from one open document, then run the decision
/// tree. Mirrors `output/typst/_native.py`'s per-page config + primitives.
fn adapt_page(
    doc: &Document,
    page_idx: i64,
    page_rect: RectTuple,
    items: &[Value],
) -> Result<Vec<Value>> {
    // Batch sampler + per-target fills. Targets are the needs-sampling items
    // plus the title-like non-sampling / non-explicit-white re-sample candidates.
    let batch_rects: Vec<RectTuple> = items
        .iter()
        .filter(|item| item_needs_local_color_sampling(item))
        .filter_map(cover_rect)
        .collect();
    let mut target_ids: Vec<String> = Vec::new();
    let mut target_rects: Vec<RectTuple> = Vec::new();
    for item in items {
        if !item_needs_local_color_sampling(item) {
            continue;
        }
        let Some(rect) = cover_rect(item) else { continue };
        target_ids.push(py_item_id(item));
        target_rects.push(rect);
    }
    for item in items {
        if item_needs_local_color_sampling(item) || item_uses_explicit_white_fill(item) {
            continue;
        }
        if !is_title_like_block(&Item::from_json_value(item)) {
            continue;
        }
        let Some(rect) = cover_rect(item) else { continue };
        target_ids.push(py_item_id(item));
        target_rects.push(rect);
    }

    let render_clip = |clip: &RectTuple| -> Option<RgbPixmap> {
        let rect = Rect::new(clip[0], clip[1], clip[2], clip[3]);
        doc.render_page_clip_rgb(page_idx, Some(&rect), 2.0)
            .ok()
            .map(|px| RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            })
    };
    let sampler = build_local_background_sampler(&page_rect, &batch_rects, &render_clip);
    let mut fill_by_item_id: BTreeMap<String, [f64; 3]> = BTreeMap::new();
    for (i, rect) in target_rects.iter().enumerate() {
        let fill = sample_local_background_fill(&page_rect, &render_clip, rect, sampler.as_ref());
        fill_by_item_id.insert(target_ids[i].clone(), fill);
    }

    // Span dicts: whole-page when `PAGE_TEXT_COLOR_SAMPLER_MIN_TITLES` titles are
    // present, otherwise per-title clips (mirrors `extract_page_span_dicts`). A
    // page that cannot produce a text page yields no samples (the fitz reference
    // swallows the exception into `None`).
    let title_count = items
        .iter()
        .filter(|item| is_title_like_block(&Item::from_json_value(item)))
        .count();
    let whole_page = title_count >= PAGE_TEXT_COLOR_SAMPLER_MIN_TITLES;
    let page = doc
        .load_page(page_idx as i32)
        .map_err(|e| anyhow!("load page {page_idx} for color adaptation: {e}"))?;
    let text_page = build_text_page_for_extraction(&page).ok();
    let mut span_sampler_samples: Option<Vec<SpanSample>> = None;
    let mut span_clip_by_item_id: BTreeMap<String, Vec<SpanSample>> = BTreeMap::new();
    if whole_page {
        if let Some(text_page) = text_page.as_ref() {
            match extract_span_dicts_from_text_page(text_page, None) {
                Ok(spans) => span_sampler_samples = Some(decode_spans(&spans)),
                Err(_) => span_sampler_samples = Some(Vec::new()),
            }
        } else {
            span_sampler_samples = Some(Vec::new());
        }
    } else {
        for item in items {
            if !is_title_like_block(&Item::from_json_value(item)) {
                continue;
            }
            let Some(rect) = cover_rect(item) else { continue };
            let clip = rect_intersection(&rect, &page_rect);
            let mut samples = Vec::new();
            if let Some(text_page) = text_page.as_ref() {
                if let Ok(spans) = extract_span_dicts_from_text_page(text_page, Some(&clip)) {
                    samples = decode_spans(&spans);
                }
            }
            span_clip_by_item_id.insert(py_item_id(item), samples);
        }
    }

    // Title-visual foreground probes for title-like items whose fill is dark
    // enough to probe (`sample_title_visual_colors`).
    let mut visual_by_item_id: BTreeMap<String, Option<[f64; 3]>> = BTreeMap::new();
    for item in items {
        if !is_title_like_block(&Item::from_json_value(item)) {
            continue;
        }
        let Some(rect) = cover_rect(item) else { continue };
        let item_id = py_item_id(item);
        let fill = fill_by_item_id.get(&item_id).copied().unwrap_or(DEFAULT_COVER_FILL);
        if !should_probe_title_visual_color(&fill) {
            continue;
        }
        let clip = Rect::new(rect[0], rect[1], rect[2], rect[3]);
        let color = doc
            .render_page_clip_rgb(page_idx, Some(&clip), TITLE_COLOR_SAMPLE_SCALE)
            .ok()
            .and_then(|px| {
                title_foreground_color_from_pixmap(
                    &RgbPixmap {
                        width: px.width as usize,
                        height: px.height as usize,
                        samples: px.samples,
                    },
                    fill,
                )
            });
        visual_by_item_id.insert(item_id, color);
    }

    Ok(items
        .iter()
        .map(|item| adapt_item(item, &fill_by_item_id, span_sampler_samples.as_deref(), &span_clip_by_item_id, &visual_by_item_id))
        .collect())
}

/// Production color-adaptation entry point for the bundle. Out-of-range or
/// unreadable pages pass through as shallow copies (no keys added).
pub fn apply_adaptive_overlay_colors_batch(
    source_pdf_path: &Path,
    pages: &BTreeMap<i64, Vec<Value>>,
) -> Result<BTreeMap<i64, Vec<Value>>> {
    let doc = open_document(source_pdf_path).map_err(|e| {
        anyhow!(
            "open source pdf for color adaptation {}: {e}",
            source_pdf_path.display()
        )
    })?;
    let mut results = BTreeMap::new();
    for (&page_idx, items) in pages {
        let Ok(page_rect) = doc.page_rect(page_idx) else {
            results.insert(page_idx, items.clone());
            continue;
        };
        let rect: RectTuple = [page_rect.x0, page_rect.y0, page_rect.x1, page_rect.y1];
        results.insert(page_idx, adapt_page(&doc, page_idx, rect, items)?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use serde_json::json;

    #[test]
    fn text_color_black_on_light_fill() {
        assert_eq!(text_color_for_fill(&[1.0, 1.0, 1.0]), [0.0, 0.0, 0.0]);
        assert_eq!(text_color_for_fill(&[0.0, 0.0, 0.0]), [1.0, 1.0, 1.0]);
        // brightness == 0.42 boundary: 0.42 <= 0.42 -> white text.
        assert_eq!(text_color_for_fill(&[0.42, 0.42, 0.42]), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn probe_title_only_for_darkish_fill() {
        assert!(!should_probe_title_visual_color(&[1.0, 1.0, 1.0]));
        assert!(!should_probe_title_visual_color(&[0.94, 0.94, 0.94]));
        assert!(should_probe_title_visual_color(&[0.93, 0.93, 0.93]));
        assert!(should_probe_title_visual_color(&[0.0, 0.0, 0.0]));
    }

    #[test]
    fn needs_sampling_from_policy_or_flag() {
        let sampled = json!({"_render_policy": {"overlay_fill": "sampled"}});
        assert!(item_needs_local_color_sampling(&sampled));
        let flag = json!({"_render_use_cover_fill": true});
        assert!(item_needs_local_color_sampling(&flag));
        let plain = json!({"_render_policy": {"overlay_fill": "white"}});
        assert!(!item_needs_local_color_sampling(&plain));
    }

    #[test]
    fn explicit_white_requires_no_sampling() {
        let explicit = json!({"_render_policy": {"overlay_fill": "white"}});
        assert!(item_uses_explicit_white_fill(&explicit));
        // "sampled" is white-cover-ish but needs local sampling.
        let sampled = json!({"_render_policy": {"overlay_fill": "sampled"}});
        assert!(!item_uses_explicit_white_fill(&sampled));
    }

    #[test]
    fn span_samples_bucket_and_average() {
        // Two spans with the same quantized color -> merged bucket -> average.
        let rect: RectTuple = [10.0, 10.0, 100.0, 60.0];
        let samples = vec![
            SpanSample { rect, text: "ab".to_string(), rgb: [100, 100, 100] },
            SpanSample { rect: [10.0, 10.0, 100.0, 60.0], text: "cd".to_string(), rgb: [104, 104, 104] },
        ];
        let color = title_text_color_from_span_samples(&samples, &rect).unwrap();
        // (100*2 + 104*2) / 4 = 102 -> round(102)/255
        assert!((color[0] - 102.0 / 255.0).abs() < 1e-9);
    }

    #[test]
    fn span_samples_respect_intersection_and_empty() {
        let rect: RectTuple = [10.0, 10.0, 100.0, 60.0];
        let outside = vec![SpanSample {
            rect: [200.0, 200.0, 300.0, 300.0],
            text: "x".to_string(),
            rgb: [0, 0, 0],
        }];
        assert_eq!(title_text_color_from_span_samples(&outside, &rect), None);
        assert_eq!(title_text_color_from_span_samples(&[], &rect), None);
        // Empty rect never yields a color.
        assert_eq!(
            title_text_color_from_span_samples(&outside, &[0.0, 0.0, 0.0, 0.0]),
            None
        );
    }

    #[test]
    fn non_title_body_items_get_default_cover_and_black_text() {
        // A body paragraph with overlay_fill=sampled and a white page: cover is
        // white, text is black — no span/visual probes consulted (not title-like).
        let item = json!({
            "item_id": "p001-b001",
            "page_idx": 0,
            "block_type": "text",
            "layout_role": "paragraph",
            "semantic_role": "body",
            "bbox": [40.0, 40.0, 360.0, 80.0],
            "cover_with_inner_bbox": false,
            "translated_text": "译文",
            "_render_policy": {"overlay_fill": "sampled", "cleanup_mode": "delete_text"},
        });
        let adapted = adapt_item(
            &item,
            &BTreeMap::new(),
            Some(&[]),
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert_eq!(adapted["_render_cover_fill"], json!([1.0, 1.0, 1.0]));
        assert_eq!(adapted["_render_text_color"], json!([0.0, 0.0, 0.0]));
    }
}
