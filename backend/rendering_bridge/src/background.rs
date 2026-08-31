//! Background fill-cover and stage entry points: `clean_background` (per-rect
//! fill sampling + opaque covers, port of `background/fill.py`) and
//! `build_clean_background_pdf` (the full production stage, port of
//! `background/stage.py`).

use std::collections::{BTreeMap, HashMap, HashSet};

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rendering_core::rect::Rect as CoreRect;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_reader::PdfDocument as _;
use rendering_writer::background::fill::{draw_white_covers, RgbPixmap};
use rendering_writer::background::redaction::page_specs::{self, RenderPageSpec};
use rendering_writer::background::redaction::RedactionItem;
use rendering_writer::save::{delete_trailer_id, save_optimized};
use serde::Deserialize;

use crate::helpers::{save_to_bytes, temp_dir};

/// Sample a background fill per rect from the pristine page and draw opaque
/// covers (port of `background/fill.py::draw_white_covers`). `config_json`:
/// `{"page_index": 0, "rects": [[x0, y0, x1, y1], ...], "scale": 2.0}`.
#[pyfunction]
fn clean_background(source_pdf_bytes: &[u8], config_json: &str) -> PyResult<Vec<u8>> {
    #[derive(Deserialize)]
    struct CleanConfig {
        #[serde(default)]
        page_index: i32,
        rects: Vec<Vec<f64>>,
        #[serde(default = "default_scale")]
        scale: f64,
    }
    fn default_scale() -> f64 {
        2.0
    }

    let config: CleanConfig = serde_json::from_str(config_json)
        .map_err(|e| PyValueError::new_err(format!("config_json: {e}")))?;
    let rects: Vec<RectTuple> = config.rects.iter().map(|r| [r[0], r[1], r[2], r[3]]).collect();

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, source_pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;

    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open render: {e}")))?;
    let page = doc
        .load_page(config.page_index)
        .map_err(|e| PyRuntimeError::new_err(format!("load_page: {e}")))?;
    let b = page.bounds().map_err(|e| PyRuntimeError::new_err(format!("bounds: {e}")))?;
    let page_rect: RectTuple = [b.x0 as f64, b.y0 as f64, b.x1 as f64, b.y1 as f64];
    let scale = config.scale as f32;

    let render_clip = |clip: &RectTuple| -> Option<RgbPixmap> {
        let r = CoreRect::new(clip[0], clip[1], clip[2], clip[3]);
        doc.render_page_clip_rgb(config.page_index as i64, Some(&r), scale)
            .ok()
            .map(|px| RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            })
    };

    let mut pdf = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open edit: {e}")))?;
    let mut edit_page = pdf
        .load_pdf_page(config.page_index)
        .map_err(|e| PyRuntimeError::new_err(format!("load_pdf_page: {e}")))?;
    draw_white_covers(&mut edit_page, &mut pdf, &page_rect, &rects, &render_clip)
        .map_err(|e| PyRuntimeError::new_err(format!("draw_white_covers: {e}")))?;
    save_to_bytes(&pdf, &dir)
}

/// `build_clean_background_pdf` entry point — the full production stage
/// (copy_toc + per-page redaction orchestration; port of
/// `background/stage.py::build_clean_background_pdf`). `translated_pages_json`:
/// `{"<page>": [{bbox, translated_text, ...}, ...]}` — the ORIGINAL translated
/// items (page-spec replacement happens here in Rust); item dicts are the
/// stable serde DTO (unknown keys ignored). `page_specs_json`: the raw
/// `RenderPageSpec` list (only `page_index` + `blocks` consumed).
/// `visual_profile_fill_map_json`: flat first-wins `{item_id: [r,g,b]}` fill
/// table extracted by the shim from the visual profile. The page-spec
/// replacement and per-item `_visual_profile_fill` annotation are computed by
/// `page_specs::apply_page_specs_and_fills`, whose formula-source map (the
/// un-replaced originals) is what the stage's formula detection reads.
/// `precleaned_page_indices_json`: `[0, 2, ...]`. Returns the optimised PDF
/// bytes.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn build_clean_background_pdf(
    source_pdf_bytes: &[u8],
    translated_pages_json: &str,
    redaction_strategy: Option<&str>,
    precleaned_page_indices_json: &str,
    page_specs_json: &str,
    visual_profile_fill_map_json: &str,
) -> PyResult<Vec<u8>> {
    let translated_pages: BTreeMap<i32, Vec<RedactionItem>> =
        serde_json::from_str(translated_pages_json)
            .map_err(|e| PyValueError::new_err(format!("translated_pages_json: {e}")))?;
    let page_specs: Vec<RenderPageSpec> = serde_json::from_str(page_specs_json)
        .map_err(|e| PyValueError::new_err(format!("page_specs_json: {e}")))?;
    let fill_map: HashMap<String, [f64; 3]> =
        serde_json::from_str(visual_profile_fill_map_json)
            .map_err(|e| PyValueError::new_err(format!("visual_profile_fill_map_json: {e}")))?;
    let precleaned: HashSet<i32> = serde_json::from_str(precleaned_page_indices_json)
        .map_err(|e| PyValueError::new_err(format!("precleaned_page_indices_json: {e}")))?;
    let (translated_pages, formula_source_pages) =
        page_specs::apply_page_specs_and_fills(&translated_pages, &page_specs, &fill_map);

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, source_pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;

    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open render: {e}")))?;
    let page = doc
        .load_page(0)
        .map_err(|e| PyRuntimeError::new_err(format!("load_page 0: {e}")))?;
    let b = page.bounds().map_err(|e| PyRuntimeError::new_err(format!("bounds: {e}")))?;
    let page_rect: RectTuple = [b.x0 as f64, b.y0 as f64, b.x1 as f64, b.y1 as f64];
    let scale: f32 = 2.0;

    let render_clip = |page_index: i32, clip: &RectTuple| -> Option<RgbPixmap> {
        let rect = CoreRect::new(clip[0], clip[1], clip[2], clip[3]);
        doc.render_page_clip_rgb(page_index as i64, Some(&rect), scale)
            .ok()
            .map(|px| RgbPixmap {
                width: px.width as usize,
                height: px.height as usize,
                samples: px.samples,
            })
    };

    let mut pdf = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open edit: {e}")))?;
    rendering_writer::background::stage::build_clean_background_pdf(
        &doc,
        &mut pdf,
        &page_rect,
        &translated_pages,
        &formula_source_pages,
        redaction_strategy,
        &precleaned,
        &render_clip,
    )
    .map_err(|e| PyRuntimeError::new_err(format!("build_clean_background_pdf: {e}")))?;

    delete_trailer_id(&pdf).map_err(|e| PyRuntimeError::new_err(format!("delete_trailer_id: {e}")))?;
    let out_path = dir.join("out.pdf");
    save_optimized(&pdf, &out_path).map_err(|e| PyRuntimeError::new_err(format!("save_optimized: {e}")))?;
    std::fs::read(&out_path).map_err(|e| PyRuntimeError::new_err(format!("read: {e}")))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(clean_background, m)?)?;
    m.add_function(wrap_pyfunction!(build_clean_background_pdf, m)?)?;
    Ok(())
}
