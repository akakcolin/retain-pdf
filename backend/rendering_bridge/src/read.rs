//! Page-read primitives (B2-7 / B2-8 / B2-9 / B2-Inc2): rects, drawing counts,
//! image placements, text spans/blocks, math rects, form XObjects, span heights,
//! and the vector-text classifier. Ports of the fitz-read family consumed by the
//! Python shims as the native half of each read.

use std::collections::BTreeMap;

use mupdf::Document;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_reader::PdfDocument as _;
use rendering_writer::background::vector_text;

use crate::helpers::temp_dir;

/// Read per-page rects (rotation-applied bounds, == fitz `page.rect`) for the
/// requested page indices. Returns `{"<idx>": [x0, y0, x1, y1]}`; unreadable
/// indices are skipped (production filters `0 <= idx < page_count` before
/// asking, so an out-of-range request is not fatal).
#[pyfunction]
fn read_page_sizes(pdf_bytes: &[u8], page_indices_json: &str) -> PyResult<String> {
    let indices: Vec<i64> = serde_json::from_str(page_indices_json)
        .map_err(|e| PyValueError::new_err(format!("page_indices_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let mut sizes = BTreeMap::new();
    for idx in indices {
        if let Ok(rect) = doc.page_rect(idx) {
            sizes.insert(idx.to_string(), [rect.x0, rect.y0, rect.x1, rect.y1]);
        }
    }
    serde_json::to_string(&sizes).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read total page count and per-page rects (rotation-applied bounds, == fitz
/// `page.rect`) in a single open. Returns
/// `{"page_count": N, "rects": {"<idx>": [x0, y0, x1, y1]}}`; `rects` uses a
/// BTreeMap so indices stay ordered. An index whose rect cannot be read is
/// skipped (fitz-equivalent for an unreadable page); `page_count` is still
/// authoritative so `len(doc)` parity is preserved.
#[pyfunction]
fn read_page_geometry(pdf_bytes: &[u8]) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    // UFCS: mupdf's inherent `Document::page_count` (i32) shadows the trait
    // method; the trait's i64 keeps the loop index aligned with `page_rect`.
    let page_count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    let mut rects = BTreeMap::new();
    for idx in 0..page_count {
        if let Ok(rect) = doc.page_rect(idx) {
            rects.insert(idx.to_string(), [rect.x0, rect.y0, rect.x1, rect.y1]);
        }
    }
    let out = serde_json::json!({
        "page_count": page_count,
        "rects": rects,
    });
    serde_json::to_string(&out).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's vector-drawing count (B2-7): `page_drawing_count` (== fitz
/// `len(page.get_cdrawings())`; count parity holds exactly — see
/// `rendering_reader::PdfDocument::page_drawing_count`). This powers the
/// count-based vector-heavy / cover-only heuristics. A page that cannot be
/// loaded raises so the shim falls back to the pure-Python reference.
///
/// Note: per-drawing rect/type/width parity is NOT exposed here — PyMuPDF's
/// `get_cdrawings` drops the last path item's first point on some stroked
/// zigzag paths, so the native `page_drawings` rect can exceed fitz's (see
/// `rendering_reader::pdf_document`). The rects consumer
/// (`collect_page_drawing_rects`) therefore stays on the Python reference.
#[pyfunction]
fn read_page_drawing_count(pdf_bytes: &[u8], page_index: i64) -> PyResult<i64> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_drawing_count(&doc, page_index)
        .map_err(|e| PyRuntimeError::new_err(format!("page_drawing_count: {e}")))?;
    Ok(count)
}

/// Read one page's image placement rects (B2-8): `page_image_placement_rects`
/// (== fitz `page.get_image_info(hashes=False)` bboxes in content /
/// rotation-stripped page space, paint order — see
/// `rendering_reader::PdfDocument::page_image_placement_rects`). Returns
/// `[[x0,y0,x1,y1], ...]`. A PDF that cannot be opened, or a page index outside
/// `[0, page_count)`, raises so the shim falls back to the pure-Python
/// reference (mirrors the `read_page_drawing_count` contract). A page that
/// loads but has no `fill_image` placements yields `[]`.
#[pyfunction]
fn read_page_image_rects(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let rects: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_image_placement_rects(&doc, page_index)
        .iter()
        .map(|r| serde_json::json!([r.x0, r.y0, r.x1, r.y1]))
        .collect();
    serde_json::to_string(&rects).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's fitz `get_text("dict")` spans (B2-9):
/// `text_extract.extract_page_text_spans` — `[[x0,y0,x1,y1,text], ...]` in
/// content / rotation-stripped page space (see
/// `rendering_reader::PdfDocument::page_text_spans`). A PDF that cannot be
/// opened, or a page index outside `[0, page_count)`, raises so the shim falls
/// back to the pure-Python reference (mirrors the `read_page_image_rects`
/// contract).
#[pyfunction]
fn read_page_text_spans(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let spans: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_text_spans(&doc, page_index)
        .iter()
        .map(|(r, text)| serde_json::json!([r.x0, r.y0, r.x1, r.y1, text]))
        .collect();
    serde_json::to_string(&spans).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's fitz `get_text("blocks")` text blocks (B2-9):
/// `text_extract.extract_page_text_blocks` — `[[x0,y0,x1,y1,text], ...]`.
#[pyfunction]
fn read_page_text_blocks(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let blocks: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_text_blocks(&doc, page_index)
        .iter()
        .map(|(r, text)| serde_json::json!([r.x0, r.y0, r.x1, r.y1, text]))
        .collect();
    serde_json::to_string(&blocks).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's math-font span rects (B2-9):
/// `math_spans.collect_page_math_protection_rects` — `[[x0,y0,x1,y1], ...]`,
/// deduped by `rect_key`.
#[pyfunction]
fn read_page_math_rects(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let rects: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_math_rects(&doc, page_index)
        .iter()
        .map(|r| serde_json::json!([r.x0, r.y0, r.x1, r.y1]))
        .collect();
    serde_json::to_string(&rects).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read a page's form XObjects (B2-Inc2):
/// `pdf_structure_profile.sampler._form_xobject_objects` — the `/Resources/XObject`
/// entries that carry a `/BBox` as `[{"name","xref","bbox":[x0,y0,x1,y1]}, ...]`
/// in resource-dict order. A PDF that cannot be opened, or a page index outside
/// `[0, page_count)`, raises so the shim falls back to the pure-Python reference
/// (mirrors the `read_page_text_spans` contract).
#[pyfunction]
fn read_page_form_xobjects(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let entries: Vec<serde_json::Value> = rendering_reader::PdfDocument::page_form_xobjects(&doc, page_index)
        .iter()
        .map(|info| {
            serde_json::json!({
                "name": info.name,
                "xref": info.xref,
                "bbox": [info.bbox.x0, info.bbox.y0, info.bbox.x1, info.bbox.y1],
            })
        })
        .collect();
    serde_json::to_string(&entries).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Read one page's non-math span heights (B2-9):
/// `math_spans.collect_page_non_math_span_heights` — `[h0, h1, ...]`.
#[pyfunction]
fn read_page_span_heights(pdf_bytes: &[u8], page_index: i64) -> PyResult<String> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let count = rendering_reader::PdfDocument::page_count(&doc)
        .map_err(|e| PyRuntimeError::new_err(format!("page_count: {e}")))?;
    if page_index < 0 || page_index >= count {
        return Err(PyRuntimeError::new_err(format!("page_index out of range: {page_index}")));
    }
    let heights: Vec<f64> = rendering_reader::PdfDocument::page_span_heights(&doc, page_index);
    serde_json::to_string(&heights).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

/// Run the ported vector-text classifier on one page (B2-7). `target_rects_json`
/// is `[[x0,y0,x1,y1], ...]`; returns the matched rects `[[x0,y0,x1,y1], ...]`
/// (mirrors `source/vector_text.py::collect_vector_text_rects`). Any failure
/// yields an empty list (Python `except Exception: return []`).
#[pyfunction]
fn collect_vector_text_rects(pdf_bytes: &[u8], page_index: i64, target_rects_json: &str) -> PyResult<String> {
    let target_rects: Vec<RectTuple> = serde_json::from_str(target_rects_json)
        .map_err(|e| PyValueError::new_err(format!("target_rects_json: {e}")))?;
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let doc = Document::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    let rects = vector_text::collect_vector_text_rects(&doc, page_index as i32, &target_rects);
    serde_json::to_string(&rects).map_err(|e| PyRuntimeError::new_err(format!("serialize: {e}")))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read_page_sizes, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_geometry, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_drawing_count, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_image_rects, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_text_spans, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_text_blocks, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_math_rects, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_form_xobjects, m)?)?;
    m.add_function(wrap_pyfunction!(read_page_span_heights, m)?)?;
    m.add_function(wrap_pyfunction!(collect_vector_text_rects, m)?)?;
    Ok(())
}
