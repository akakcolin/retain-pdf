//! Per-page sampling against rendered output: first-line indent detection and
//! the color-adaptation / visual-profile fill + foreground samples (ports of
//! `rendering_reader::indent` and `background/color_adapt.py` /
//! `visual_profile/foreground.py`).

use std::collections::{BTreeMap, HashMap};

use mupdf::Document;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rendering_core::rect::Rect as CoreRect;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_reader::PdfDocument as _;
use rendering_writer::background::color_adapt::{
    build_text_page_for_extraction, extract_span_dicts_from_text_page, foreground_color_from_pixmap,
    title_foreground_color_from_pixmap,
};
use rendering_writer::background::fill::{
    batch_sampler_clip_rect, build_local_background_sampler, sample_local_background_fill, RgbPixmap,
};
use rendering_writer::background::patch::{rect_intersection, rect_is_empty, rect_is_finite};
use serde::Deserialize;

use crate::helpers::temp_dir;

/// Detect per-candidate first-line indent from each page's display list.
/// `page_indices_json`: `[page_idx, ...]`; `candidates_json`:
/// `{"<page_idx>": [[x0, y0, x1, y1, font_size_pt], ...]}`. Returns
/// `{"<page_idx>": {"<cand_idx>": indent_pt}}`; unreadable pages / failed
/// renders are skipped (absent keys; the shim maps them to 0.0, matching the
/// Python reference where an empty/out-of-page clip yields empty samples and a
/// 0.0 result). See `rendering_reader::indent`.
#[pyfunction]
fn detect_first_line_indents(pdf_bytes: &[u8], page_indices_json: &str, candidates_json: &str) -> PyResult<String> {
    let page_indices: Vec<i64> = serde_json::from_str(page_indices_json)
        .map_err(|e| PyValueError::new_err(format!("page_indices_json: {e}")))?;
    let candidates: HashMap<String, Vec<[f64; 5]>> = serde_json::from_str(candidates_json)
        .map_err(|e| PyValueError::new_err(format!("candidates_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let parsed: BTreeMap<i64, Vec<([f64; 4], f64)>> = candidates
        .into_iter()
        .map(|(page, cands)| {
            (
                page.parse::<i64>().unwrap_or(-1),
                cands.into_iter()
                    .map(|[x0, y0, x1, y1, font]| ([x0, y0, x1, y1], font))
                    .collect(),
            )
        })
        .collect();
    let detected = rendering_reader::indent::detect_first_line_indents(&doc, &page_indices, &parsed);
    let mut out: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for (idx, entries) in detected {
        let mut page_out = BTreeMap::new();
        for (ci, entry) in entries.iter().enumerate() {
            if let Some(indent) = entry {
                page_out.insert(ci.to_string(), *indent);
            }
        }
        if !page_out.is_empty() {
            out.insert(idx.to_string(), page_out);
        }
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Color-adaptation: sample a local background fill per target rect from each
/// page's pristine content. `config_json`:
/// `{"<page>": {"batch_rects": [[x0,y0,x1,y1], ...], "target_rects": [[...], ...]}}`.
/// `batch_rects` are the cover rects that form the `LocalBackgroundSampler`
/// clip (`BACKGROUND_CLIP_SAMPLER_MIN_RECTS` gate); `target_rects` are every
/// rect whose fill the decision tree may read (an over-approximation — extra
/// results are ignored). Returns
/// `{"<page>": {"batch": {"count": N, "clip": [x0,y0,x1,y1] | null},
/// "targets": {"<i>": [r,g,b]}}}`. Mirrors
/// `color_adapt._sample_item_cover_fill` with
/// `sample_local_background_fill(page, rect, sampler=...)` where the sampler is
/// built from `batch_rects` (scale `BACKGROUND_COVER_SAMPLE_SCALE`, RGB,
/// alpha=false).
#[pyfunction]
fn sample_page_color_fills(pdf_bytes: &[u8], config_json: &str) -> PyResult<String> {
    #[derive(Deserialize)]
    struct PageColorConfig {
        batch_rects: Vec<Vec<f64>>,
        target_rects: Vec<Vec<f64>>,
    }
    let config: HashMap<String, PageColorConfig> = serde_json::from_str(config_json)
        .map_err(|e| PyValueError::new_err(format!("config_json: {e}")))?;

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;

    let scale: f32 = 2.0;
    let mut out: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for (page_key, cfg) in config {
        let idx: i64 = page_key
            .parse()
            .map_err(|e| PyValueError::new_err(format!("bad page key {page_key}: {e}")))?;
        let Ok(rect) = doc.page_rect(idx) else {
            continue;
        };
        let page_rect: RectTuple = [rect.x0, rect.y0, rect.x1, rect.y1];
        let batch_rects: Vec<RectTuple> =
            cfg.batch_rects.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect();
        let target_rects: Vec<RectTuple> =
            cfg.target_rects.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect();

        let render_clip = |clip: &RectTuple| -> Option<RgbPixmap> {
            let r = CoreRect::new(clip[0], clip[1], clip[2], clip[3]);
            doc.render_page_clip_rgb(idx, Some(&r), scale)
                .ok()
                .map(|px| RgbPixmap {
                    width: px.width as usize,
                    height: px.height as usize,
                    samples: px.samples,
                })
        };
        let sampler = build_local_background_sampler(&page_rect, &batch_rects, &render_clip);
        let batch_clip = batch_sampler_clip_rect(&page_rect, &batch_rects, true);
        let valid_count = batch_rects
            .iter()
            .filter(|r| !rect_is_empty(r) && rect_is_finite(r))
            .count();

        let mut targets = BTreeMap::new();
        for (i, rect) in target_rects.iter().enumerate() {
            let fill = sample_local_background_fill(&page_rect, &render_clip, rect, sampler.as_ref());
            targets.insert(i.to_string(), fill);
        }
        out.insert(
            page_key,
            serde_json::json!({
                "batch": {"count": valid_count, "clip": batch_clip},
                "targets": targets,
            }),
        );
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Color-adaptation: structured-text span dicts per page and per clip.
/// `clips_json`: `{"<page>": [[x0,y0,x1,y1], null, ...]}` (null = full page).
/// Returns `{"<page>": {"<i>": [[x0,y0,x1,y1, color_int, "text"], ...]}}`;
/// unreadable pages are skipped. Mirrors fitz `get_text("dict", clip=...)`
/// (PRESERVE_LIGATURES; characters filtered by quad∩clip; span bbox = union of
/// surviving glyph quads).
#[pyfunction]
fn extract_page_span_dicts(pdf_bytes: &[u8], clips_json: &str) -> PyResult<String> {
    let clips: HashMap<String, Vec<Option<Vec<f64>>>> = serde_json::from_str(clips_json)
        .map_err(|e| PyValueError::new_err(format!("clips_json: {e}")))?;

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;

    let mut out: BTreeMap<String, BTreeMap<String, Vec<serde_json::Value>>> = BTreeMap::new();
    for (page_key, page_clips) in clips {
        let idx: i64 = page_key
            .parse()
            .map_err(|e| PyValueError::new_err(format!("bad page key {page_key}: {e}")))?;
        let Ok(page) = doc.load_page(idx as i32) else {
            continue;
        };
        let Ok(text_page) = build_text_page_for_extraction(&page) else {
            continue;
        };
        let mut clip_out = BTreeMap::new();
        for (i, clip) in page_clips.iter().enumerate() {
            let clip_tuple: Option<RectTuple> = clip.as_ref().map(|c| [c[0], c[1], c[2], c[3]]);
            let spans = extract_span_dicts_from_text_page(&text_page, clip_tuple.as_ref())
                .map_err(|e| PyRuntimeError::new_err(format!("extract spans: {e}")))?;
            let encoded: Vec<serde_json::Value> = spans
                .iter()
                .map(|s| serde_json::json!([s.rect[0], s.rect[1], s.rect[2], s.rect[3], s.color, s.text]))
                .collect();
            clip_out.insert(i.to_string(), encoded);
        }
        out.insert(page_key, clip_out);
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Color-adaptation: title foreground color sampled from a 3x RGB render of each
/// probe rect against its background fill. `visuals_json`:
/// `{"<page>": [{"rect": [x0,y0,x1,y1], "fill": [r,g,b]}, ...]}`. Returns
/// `{"<page>": {"<i>": [r,g,b]}}`; a probe that fails (empty clip / no text-like
/// foreground) is an absent key. Mirrors
/// `color_adapt.title_text_color_from_visual_components`
/// (`TITLE_COLOR_SAMPLE_SCALE`, device-RGB, alpha=false).
#[pyfunction]
fn sample_title_visual_colors(pdf_bytes: &[u8], visuals_json: &str) -> PyResult<String> {
    #[derive(Deserialize)]
    struct VisualProbe {
        rect: Vec<f64>,
        fill: Vec<f64>,
    }
    let visuals: HashMap<String, Vec<VisualProbe>> = serde_json::from_str(visuals_json)
        .map_err(|e| PyValueError::new_err(format!("visuals_json: {e}")))?;

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;

    let scale: f32 = 3.0;
    let mut out: BTreeMap<String, BTreeMap<String, [f64; 3]>> = BTreeMap::new();
    for (page_key, probes) in visuals {
        let idx: i64 = page_key
            .parse()
            .map_err(|e| PyValueError::new_err(format!("bad page key {page_key}: {e}")))?;
        let mut probe_out = BTreeMap::new();
        for (i, probe) in probes.iter().enumerate() {
            let rect = CoreRect::new(probe.rect[0], probe.rect[1], probe.rect[2], probe.rect[3]);
            let background = [probe.fill[0], probe.fill[1], probe.fill[2]];
            let Ok(px) = doc.render_page_clip_rgb(idx, Some(&rect), scale) else {
                continue;
            };
            let pix = RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            };
            if let Some(color) = title_foreground_color_from_pixmap(&pix, background) {
                probe_out.insert(i.to_string(), color);
            }
        }
        if !probe_out.is_empty() {
            out.insert(page_key, probe_out);
        }
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Visual-profile: foreground color + confidence sampled from a 3x RGB render of
/// each probe rect (clipped to the page bounds) against its background fill.
/// `probes_json`: `{"<page>": [{"rect": [x0,y0,x1,y1], "fill": [r,g,b]}, ...]}`.
/// Returns `{"<page>": {"<i>": [r,g,b, confidence]}}`; a probe that fails (rect
/// does not overlap the page / render failure / no text-like foreground) is an
/// absent key. Mirrors
/// `visual_profile/foreground.py::sample_foreground_color_from_pixels`
/// (`FOREGROUND_SAMPLE_SCALE`, device-RGB, alpha=false, rect clipped to
/// `page.rect`).
#[pyfunction]
fn sample_foreground_colors(pdf_bytes: &[u8], probes_json: &str) -> PyResult<String> {
    #[derive(Deserialize)]
    struct VisualProbe {
        rect: Vec<f64>,
        fill: Vec<f64>,
    }
    let probes: HashMap<String, Vec<VisualProbe>> = serde_json::from_str(probes_json)
        .map_err(|e| PyValueError::new_err(format!("probes_json: {e}")))?;

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;

    let scale: f32 = 3.0;
    let mut out: BTreeMap<String, BTreeMap<String, [f64; 4]>> = BTreeMap::new();
    for (page_key, page_probes) in probes {
        let idx: i64 = page_key
            .parse()
            .map_err(|e| PyValueError::new_err(format!("bad page key {page_key}: {e}")))?;
        let Ok(pr) = doc.page_rect(idx) else {
            continue;
        };
        let page_rect: RectTuple = [pr.x0, pr.y0, pr.x1, pr.y1];
        let mut probe_out = BTreeMap::new();
        for (i, probe) in page_probes.iter().enumerate() {
            let rect: RectTuple = [probe.rect[0], probe.rect[1], probe.rect[2], probe.rect[3]];
            let clipped = rect_intersection(&rect, &page_rect);
            if rect_is_empty(&clipped) {
                continue;
            }
            let clip_rect = CoreRect::new(clipped[0], clipped[1], clipped[2], clipped[3]);
            let Ok(px) = doc.render_page_clip_rgb(idx, Some(&clip_rect), scale) else {
                continue;
            };
            let pix = RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            };
            let background = [probe.fill[0], probe.fill[1], probe.fill[2]];
            let (color, confidence) = foreground_color_from_pixmap(&pix, background);
            if let Some(color) = color {
                probe_out.insert(i.to_string(), [color[0], color[1], color[2], confidence]);
            }
        }
        if !probe_out.is_empty() {
            out.insert(page_key, probe_out);
        }
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(detect_first_line_indents, m)?)?;
    m.add_function(wrap_pyfunction!(sample_page_color_fills, m)?)?;
    m.add_function(wrap_pyfunction!(extract_page_span_dicts, m)?)?;
    m.add_function(wrap_pyfunction!(sample_title_visual_colors, m)?)?;
    m.add_function(wrap_pyfunction!(sample_foreground_colors, m)?)?;
    Ok(())
}
