//! Render analysis / classification entry points: the whole-document manifest
//! (Inc 4) and single-page classify (B-A). Ports of
//! `analysis/document.py::build_render_document_analysis` and
//! `analysis/classify.py::classify_render_page`.

use std::collections::BTreeMap;

use mupdf::Document;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rendering_reader::PdfDocument as _;

use crate::helpers::temp_dir;

/// Build the whole-document render analysis manifest (Inc 4). Opens the PDF
/// ONCE, walks the pages named by `config_json`, and for each produces a
/// `RenderPageAnalysis` manifest entry via `page_snapshot` ->
/// `build_render_page_profile` -> `build_render_page_analysis`. `config_json`:
/// `{"<page_idx>": [[x0, y0, x1, y1], ...]}` (OCR bboxes per page). Returns
/// `{"algorithm": "render_document_profile_v1", "pages": [{...}]}` in page-index
/// order. A page whose snapshot cannot be read raises so the Python shim falls
/// back to the pure-Python reference.
#[pyfunction]
fn build_render_document_analysis(pdf_bytes: &[u8], config_json: &str) -> PyResult<String> {
    let config: BTreeMap<i64, Vec<[f64; 4]>> = serde_json::from_str(config_json)
        .map_err(|e| PyValueError::new_err(format!("config_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let mut pages: Vec<serde_json::Value> = Vec::new();
    for (idx, ocr_items) in config {
        let snapshot = doc
            .page_snapshot(idx)
            .map_err(|e| PyRuntimeError::new_err(format!("page_snapshot p{idx}: {e}")))?;
        let profile = rendering_core::profile_build::build_render_page_profile(
            &snapshot,
            &ocr_items,
            rendering_core::profile_build::DEFAULT_BACKGROUND_THRESHOLD,
        );
        let analysis = rendering_core::document_builder::build_render_page_analysis(&profile);
        pages.push(serde_json::json!({
            "page_index": analysis.page_index,
            "kind": analysis.kind.as_str(),
            "redaction": analysis.redaction,
            "background": analysis.background,
            "compose": analysis.compose,
            "layout": analysis.layout,
            "reason": analysis.reason,
            "has_large_background": analysis.has_large_background,
            "background_coverage_ratio":
                (analysis.background_coverage_ratio * 1_000_000.0).round() / 1_000_000.0,
            "visible_text": analysis.visible_text,
            "hidden_text": analysis.hidden_text,
            "editable_text": analysis.editable_text,
            "drawing_count": analysis.drawing_count,
            "vector_heavy": analysis.vector_heavy,
        }));
    }
    let out = serde_json::json!({
        "algorithm": "render_document_profile_v1",
        "pages": pages,
    });
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Classify a single page by index (B-A): `page_snapshot` ->
/// `rendering_core::classifier::classify_render_page`. Thin wrapper reusing the
/// whole-document analysis reader primitives; no new reader surface. The
/// `text_traces` fields follow the documented empty-trace divergence (mupdf has
/// no `get_texttrace`), so `visible/hidden_text_traces` may differ from fitz on
/// hidden-text pages while `kind`/route stay parity. Returns the manifest:
/// `{"kind", "large_background_image", "visible_text_traces",
/// "hidden_text_traces", "drawing_count", "background_coverage_ratio",
/// "route": {redaction, background, compose, layout, reason}}`. A page whose
/// snapshot cannot be read raises so the shim falls back to the reference.
#[pyfunction]
fn classify_render_page(pdf_bytes: &[u8], page_index: i64, background_threshold: f64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let snapshot = doc
        .page_snapshot(page_index)
        .map_err(|e| PyRuntimeError::new_err(format!("page_snapshot p{page_index}: {e}")))?;
    let c = rendering_core::classifier::classify_render_page(&snapshot, background_threshold);
    let out = serde_json::json!({
        "kind": c.kind.as_str(),
        "large_background_image": c.large_background_image,
        "visible_text_traces": c.visible_text_traces,
        "hidden_text_traces": c.hidden_text_traces,
        "drawing_count": c.drawing_count,
        "background_coverage_ratio": c.background_coverage_ratio,
        "route": {
            "redaction": c.route.redaction,
            "background": c.route.background,
            "compose": c.route.compose,
            "layout": c.route.layout,
            "reason": c.route.reason,
        },
    });
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build_render_document_analysis, m)?)?;
    m.add_function(wrap_pyfunction!(classify_render_page, m)?)?;
    Ok(())
}
