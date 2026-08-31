//! Source-cleanup planning entry points: cleanup-context reads and the ported
//! `plan_source_cleanup` / unsafe-vector-overlap candidates assembly (ports of
//! `page_probe.py` / `planning/accumulator.py`).

use std::collections::BTreeMap;

use mupdf::Document;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rendering_core::source_cleanup::planning::planner::{
    allows_pikepdf_from_json, item_ids_with_uncovered_unsafe_vector_overlap, pages_from_json, plan_source_cleanup,
};
use rendering_reader::cleanup_context::build_planning_contexts;
use rendering_reader::PdfDocument as _;

use crate::helpers::temp_dir;

/// Read the source-cleanup planning contexts for `page_indices_json`:
/// `{"<idx>": {"rect": [x0,y0,x1,y1], "ctm": [a,b,c,d,e,f],
/// "bboxlog": [[kind, x0, y0, x1, y1], ...], "content_stream_size": N,
/// "has_form_xobjects": bool}}`.
///
/// `rect`/`ctm`/`bboxlog` are in rotation-stripped fitz page space (fitz
/// `page.rect` / `page.transformation_matrix` / `page.get_bboxlog()`); the
/// bboxlog keeps op order. A page whose rect or ctm cannot be read is skipped
/// (production filters `0 <= idx < page_count` first). `content_stream_threshold`
/// is the early-return cap mirrored from
/// `page_probe.page_content_stream_size`.
#[pyfunction]
fn read_page_cleanup_contexts(
    pdf_bytes: &[u8],
    page_indices_json: &str,
    content_stream_threshold: u64,
) -> PyResult<String> {
    let indices: Vec<i64> = serde_json::from_str(page_indices_json)
        .map_err(|e| PyValueError::new_err(format!("page_indices_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let mut out = BTreeMap::new();
    for idx in indices {
        let Ok(rect) = doc.page_rect(idx) else {
            continue;
        };
        let Some(ctm) = doc.page_ctm(idx) else {
            continue;
        };
        let entries: Vec<serde_json::Value> = doc
            .page_bboxlog(idx)
            .iter()
            .map(|e| {
                serde_json::json!([
                    e.kind,
                    [e.rect.x0, e.rect.y0, e.rect.x1, e.rect.y1],
                ])
            })
            .collect();
        let stream_size = doc.page_content_stream_size(idx, content_stream_threshold);
        let has_form_xobjects = doc.page_has_form_xobjects(idx);
        out.insert(
            idx.to_string(),
            serde_json::json!({
                "rect": [rect.x0, rect.y0, rect.x1, rect.y1],
                "ctm": ctm,
                "bboxlog": entries,
                "content_stream_size": stream_size,
                "has_form_xobjects": has_form_xobjects,
            }),
        );
    }
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Run the ported `plan_source_cleanup` candidates assembly entirely in Rust.
/// `translated_pages_json` / `protected_pages_json` are
/// `{"<page_idx>": [item, ...]}`; `options_json` is
/// `{"skip_formula_pages": bool, "skip_form_xobject_pages": bool,
/// "allows_pikepdf_strip": {"<page_idx>": bool}}`. Returns the serialized
/// `BBoxTextStripCandidates` (mirrors `planning/accumulator.py::build`).
#[pyfunction]
fn plan_source_cleanup_native(
    pdf_bytes: &[u8],
    translated_pages_json: &str,
    protected_pages_json: &str,
    options_json: &str,
) -> PyResult<String> {
    let translated_value: serde_json::Value = serde_json::from_str(translated_pages_json)
        .map_err(|e| PyValueError::new_err(format!("translated_pages_json: {e}")))?;
    let protected_value: serde_json::Value = serde_json::from_str(protected_pages_json)
        .map_err(|e| PyValueError::new_err(format!("protected_pages_json: {e}")))?;
    let options: serde_json::Value = serde_json::from_str(options_json)
        .map_err(|e| PyValueError::new_err(format!("options_json: {e}")))?;
    let translated_pages = pages_from_json(&translated_value);
    let protected_pages = pages_from_json(&protected_value);
    let skip_formula_pages = options
        .get("skip_formula_pages")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let skip_form_xobject_pages = options
        .get("skip_form_xobject_pages")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let allows_pikepdf_strip = options
        .get("allows_pikepdf_strip")
        .and_then(allows_pikepdf_from_json);

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let indices: Vec<i64> = translated_pages.keys().copied().collect();
    let contexts = build_planning_contexts(&doc, &indices);
    let candidates = plan_source_cleanup(
        &contexts,
        &translated_pages,
        &protected_pages,
        skip_formula_pages,
        skip_form_xobject_pages,
        allows_pikepdf_strip.as_ref(),
    );
    serde_json::to_string(&candidates).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Run the ported `item_ids_with_uncovered_unsafe_vector_overlap` entirely in
/// Rust. `translated_pages_json` is `{"<page_idx>": [item, ...]}`; returns a
/// JSON array of uncovered item ids.
#[pyfunction]
fn uncovered_unsafe_vector_item_ids_native(
    pdf_bytes: &[u8],
    translated_pages_json: &str,
) -> PyResult<String> {
    let translated_value: serde_json::Value = serde_json::from_str(translated_pages_json)
        .map_err(|e| PyValueError::new_err(format!("translated_pages_json: {e}")))?;
    let translated_pages = pages_from_json(&translated_value);

    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let indices: Vec<i64> = translated_pages.keys().copied().collect();
    let contexts = build_planning_contexts(&doc, &indices);
    let mut item_ids = item_ids_with_uncovered_unsafe_vector_overlap(&contexts, &translated_pages);
    let mut sorted: Vec<String> = item_ids.drain().collect();
    sorted.sort_unstable();
    serde_json::to_string(&sorted).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read_page_cleanup_contexts, m)?)?;
    m.add_function(wrap_pyfunction!(plan_source_cleanup_native, m)?)?;
    m.add_function(wrap_pyfunction!(uncovered_unsafe_vector_item_ids_native, m)?)?;
    Ok(())
}
