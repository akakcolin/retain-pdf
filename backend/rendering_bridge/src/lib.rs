//! Native (pyo3/maturin) bridge exposing the ported Rust rendering pipeline to
//! production Python.
//!
//! Functions:
//!   * `emit_typst_source(page_specs_json, background_pdf_path, work_dir,
//!     font_family) -> str` — the pure Typst emitter (port of
//!     `output/typst/emitter.py`).
//!   * `compile_typst_source(...) -> str` — orchestrate the `typst` CLI
//!     (port of `output/typst/compiler.py`).
//!   * Phase 5 write-path entries operating on PDF bytes in -> bytes out:
//!     `strip_bbox_text_rects`, `strip_hidden_text`, `sanitize_invalid_xobjects`,
//!     `compress_images`, `extract_pages`, `overlay_page`.
//!   * `clean_background(source_pdf_bytes, config_json) -> bytes` — sample a
//!     fill per rect (ported `background::fill`) and draw opaque covers.
//!     This is the fill-cover capability validated by the background
//!     differential; the full production `stage.build_clean_background_pdf`
//!     (redaction-based) is not ported.
//!
//! The Python shim `output/typst/_native.py` imports this module and falls
//! back to the pure-Python implementations on `ImportError`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_reader::render::render_page_clip_rgb;
use rendering_writer::background::fill::{draw_white_covers, RgbPixmap};
use rendering_writer::save::{delete_trailer_id, save_atomic};
use serde::Deserialize;

/// `emit_typst_source` entry point (pure; no PDF editing).
#[pyfunction]
fn emit_typst_source(
    page_specs_json: &str,
    background_pdf_path: &str,
    work_dir: &str,
    font_family: &str,
) -> PyResult<String> {
    let specs: Vec<rendering_output::dto::RenderPageSpec> = serde_json::from_str(page_specs_json)
        .map_err(|e| PyValueError::new_err(format!("page_specs_json: {e}")))?;
    Ok(rendering_output::emitter::build_typst_source_from_page_specs(
        Path::new(background_pdf_path),
        &specs,
        Path::new(work_dir),
        font_family,
    ))
}

/// `compile_typst_source` entry point (orchestrates the `typst` CLI). Returns
/// the compiled PDF path. `root`, `font_paths`, `typ_bin`, and
/// `timeout_seconds` override the compile context defaults.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn compile_typst_source(
    source: &str,
    stem: &str,
    phase: &str,
    work_dir: &str,
    root: Option<&str>,
    font_paths: Vec<String>,
    typ_bin: Option<&str>,
    timeout_seconds: Option<f64>,
) -> PyResult<String> {
    let ctx = rendering_output::compile::CompileContext {
        typ_bin: typ_bin
            .unwrap_or(rendering_output::compile::DEFAULT_TYPST_BIN)
            .to_string(),
        timeout_seconds: timeout_seconds
            .unwrap_or(rendering_output::compile::DEFAULT_COMPILE_TIMEOUT_SECONDS),
        backends_fonts_dir: None,
        env_font_dirs: None,
    };
    let fonts: Vec<PathBuf> = font_paths.into_iter().map(PathBuf::from).collect();
    let out = rendering_output::compile::compile_typst_source(
        source,
        stem,
        phase,
        Path::new(work_dir),
        root.map(Path::new),
        &fonts,
        &ctx,
        serde_json::Map::new(),
    )
    .map_err(|e| PyRuntimeError::new_err(e.message()))?;
    Ok(out.to_string_lossy().into_owned())
}

// --- write-path helpers ------------------------------------------------------

/// Deserialize `{"<page>": [[x0, y0, x1, y1], ...]}` into strip-API rects.
fn parse_rect_map(json: &str, label: &str) -> PyResult<HashMap<i32, Vec<RectTuple>>> {
    let raw: HashMap<String, Vec<Vec<f64>>> = serde_json::from_str(json)
        .map_err(|e| PyValueError::new_err(format!("{label}: {e}")))?;
    let mut out = HashMap::new();
    for (k, rects) in raw {
        let idx: i32 = k
            .parse()
            .map_err(|e| PyValueError::new_err(format!("{label}: bad page key {k}: {e}")))?;
        out.insert(idx, rects.into_iter().map(|r| [r[0], r[1], r[2], r[3]]).collect());
    }
    Ok(out)
}

/// Fresh temp working dir for in-memory PDF round-trips.
fn temp_dir() -> PyResult<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rpbridge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| PyRuntimeError::new_err(format!("mkdir {dir:?}: {e}")))?;
    Ok(dir)
}

fn save_to_bytes(pdf: &PdfDocument, dir: &Path) -> PyResult<Vec<u8>> {
    delete_trailer_id(pdf).map_err(|e| PyRuntimeError::new_err(format!("delete_trailer_id: {e}")))?;
    let out_path = dir.join("out.pdf");
    save_atomic(pdf, &out_path).map_err(|e| PyRuntimeError::new_err(format!("save: {e}")))?;
    std::fs::read(&out_path).map_err(|e| PyRuntimeError::new_err(format!("read: {e}")))
}

/// Open `pdf_bytes`, run `edit` (which may replace/append content), and return
/// the deterministically saved PDF bytes.
fn with_pdf_edit<F>(pdf_bytes: &[u8], edit: F) -> PyResult<Vec<u8>>
where
    F: FnOnce(&mut PdfDocument) -> Result<(), mupdf::Error>,
{
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let mut pdf = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    edit(&mut pdf).map_err(|e| PyRuntimeError::new_err(format!("edit: {e}")))?;
    save_to_bytes(&pdf, &dir)
}

/// Strip text show ops inside the given per-page rects (port of
/// `strip_bbox_text_rects_from_pdf_copy`).
#[pyfunction]
fn strip_bbox_text_rects(
    pdf_bytes: &[u8],
    page_rects_json: &str,
    page_protected_rects_json: Option<&str>,
    recurse_forms: Option<bool>,
) -> PyResult<Vec<u8>> {
    let page_rects = parse_rect_map(page_rects_json, "page_rects")?;
    let protected = match page_protected_rects_json {
        Some(s) => parse_rect_map(s, "page_protected_rects")?,
        None => HashMap::new(),
    };
    let recurse = recurse_forms.unwrap_or(true);
    with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::cleanup_writer::strip_bbox_text_rects_from_pdf(
            pdf, &page_rects, &protected, recurse,
        )
        .map(|_| ())
    })
}

/// Remove hidden text (all-zero / zero-size / all-white color) show ops (port
/// of `hidden_text.py::strip_hidden_text`).
#[pyfunction]
fn strip_hidden_text(pdf_bytes: &[u8]) -> PyResult<Vec<u8>> {
    with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::hidden_text::strip_hidden_text(pdf).map(|_| ())
    })
}

/// Replace invalid image XObjects with the recorded fill (port of
/// `sanitize.py::sanitize_invalid_xobjects`).
#[pyfunction]
fn sanitize_invalid_xobjects(pdf_bytes: &[u8]) -> PyResult<Vec<u8>> {
    with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::sanitize::sanitize_invalid_xobjects(pdf).map(|_| ())
    })
}

/// Recompress displayed images toward `dpi` (port of
/// `image_compress.py::compress_images`).
#[pyfunction]
fn compress_images(pdf_bytes: &[u8], dpi: i32) -> PyResult<Vec<u8>> {
    with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::image_compress::compress_images(pdf, dpi).map(|_| ())
    })
}

/// Extract `start..=end` pages into a new document (port of
/// `pikepdf_pages.py::extract_pages_with_pikepdf`).
#[pyfunction]
fn extract_pages(pdf_bytes: &[u8], start: i64, end: i64) -> PyResult<Vec<u8>> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let mut source = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let out = rendering_writer::page_subset::extract_pages(&mut source, start, end)
        .map_err(|e| PyRuntimeError::new_err(format!("extract_pages: {e}")))?;
    save_to_bytes(&out, &dir)
}

/// Overlay one page of `overlay_bytes` onto one page of `pdf_bytes` (port of
/// `pikepdf_overlay.py::overlay_page_pdfs_with_pikepdf`).
#[pyfunction]
fn overlay_page(
    pdf_bytes: &[u8],
    overlay_bytes: &[u8],
    source_page: i32,
    overlay_page_idx: i32,
) -> PyResult<Vec<u8>> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    let ovl_path = dir.join("ovl.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    std::fs::write(&ovl_path, overlay_bytes).map_err(|e| PyRuntimeError::new_err(format!("write ovl: {e}")))?;
    let mut source = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open source: {e}")))?;
    let overlay = PdfDocument::open(ovl_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open overlay: {e}")))?;
    rendering_writer::overlay::overlay_page(&mut source, source_page, &overlay, overlay_page_idx)
        .map_err(|e| PyRuntimeError::new_err(format!("overlay: {e}")))?;
    save_to_bytes(&source, &dir)
}

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
        let r = mupdf::Rect::new(clip[0] as f32, clip[1] as f32, clip[2] as f32, clip[3] as f32);
        render_page_clip_rgb(&doc, config.page_index, Some(&r), scale)
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

#[pymodule]
fn rendering_bridge(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(emit_typst_source, m)?)?;
    m.add_function(wrap_pyfunction!(compile_typst_source, m)?)?;
    m.add_function(wrap_pyfunction!(strip_bbox_text_rects, m)?)?;
    m.add_function(wrap_pyfunction!(strip_hidden_text, m)?)?;
    m.add_function(wrap_pyfunction!(sanitize_invalid_xobjects, m)?)?;
    m.add_function(wrap_pyfunction!(compress_images, m)?)?;
    m.add_function(wrap_pyfunction!(extract_pages, m)?)?;
    m.add_function(wrap_pyfunction!(overlay_page, m)?)?;
    m.add_function(wrap_pyfunction!(clean_background, m)?)?;
    Ok(())
}
