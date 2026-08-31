//! Generic PDF write-path primitives (bytes in -> bytes out): text strip,
//! sanitize, image compress, page extract/overlay, dual-doc assembly, subset +
//! optimized save, and TOC copy. These are the Phase 5 write-path entries
//! consumed by the Python shims as the native half of each operation.

use std::collections::HashMap;

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use rendering_writer::save::subset_and_clean;

use crate::helpers::{meta_json, parse_rect_map, save_to_bytes, temp_dir, with_pdf_edit};

/// Strip text show ops inside the given per-page rects (port of
/// `strip_bbox_text_rects_from_pdf_copy`). `page_rects_json` /
/// `page_protected_rects_json` are `{"<page_idx>": [[x0, y0, x1, y1], ...]}`
/// maps (PDF user-space rects, string keys). Returns the saved PDF bytes plus
/// the `StripPdfResult` metadata (the Python shim rebuilds the production
/// result contract: pages_changed / text_show_ops_removed / forms_changed /
/// changed_page_indices).
#[pyfunction]
fn strip_bbox_text_rects(
    pdf_bytes: &[u8],
    page_rects_json: &str,
    page_protected_rects_json: Option<&str>,
    recurse_forms: Option<bool>,
) -> PyResult<(Vec<u8>, String)> {
    let page_rects = parse_rect_map(page_rects_json, "page_rects")?;
    let protected = match page_protected_rects_json {
        Some(s) => parse_rect_map(s, "page_protected_rects")?,
        None => HashMap::new(),
    };
    let recurse = recurse_forms.unwrap_or(true);
    let (bytes, result) = with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::cleanup_writer::strip_bbox_text_rects_from_pdf(
            pdf, &page_rects, &protected, recurse,
        )
    })?;
    Ok((bytes, meta_json(&result)?))
}

/// Remove hidden text (all-zero / zero-size / all-white color) show ops (port
/// of `hidden_text.py::strip_hidden_text`). Returns bytes plus the
/// `HiddenTextStripResult` metadata.
#[pyfunction]
fn strip_hidden_text(pdf_bytes: &[u8]) -> PyResult<(Vec<u8>, String)> {
    let (bytes, result) =
        with_pdf_edit(pdf_bytes, |pdf| rendering_writer::hidden_text::strip_hidden_text(pdf))?;
    Ok((bytes, meta_json(&result)?))
}

/// Remove hidden text from only the given page indices (production's
/// candidate-page set from the fitz pre-scan). `page_indices_json` is a JSON
/// array of page indices; out-of-range / duplicate entries are ignored. Returns
/// bytes plus the `HiddenTextStripResult` metadata.
#[pyfunction]
fn strip_hidden_text_pages(pdf_bytes: &[u8], page_indices_json: &str) -> PyResult<(Vec<u8>, String)> {
    let indices: Vec<i32> = serde_json::from_str(page_indices_json)
        .map_err(|e| PyRuntimeError::new_err(format!("page_indices json: {e}")))?;
    let (bytes, result) = with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::hidden_text::strip_hidden_text_pages(pdf, &indices)
    })?;
    Ok((bytes, meta_json(&result)?))
}

/// Replace invalid image XObjects with the recorded fill (port of
/// `sanitize.py::sanitize_invalid_xobjects`). Returns bytes plus the
/// `SanitizeResult` metadata.
#[pyfunction]
fn sanitize_invalid_xobjects(pdf_bytes: &[u8]) -> PyResult<(Vec<u8>, String)> {
    let (bytes, result) = with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::sanitize::sanitize_invalid_xobjects(pdf)
    })?;
    Ok((bytes, meta_json(&result)?))
}

/// Recompress displayed images toward `dpi` (port of
/// `image_compress.py::compress_images`). Returns bytes plus the
/// `ImageCompressResult` metadata.
#[pyfunction]
fn compress_images(pdf_bytes: &[u8], dpi: i32) -> PyResult<(Vec<u8>, String)> {
    let (bytes, result) = with_pdf_edit(pdf_bytes, |pdf| {
        rendering_writer::image_compress::compress_images(pdf, dpi)
    })?;
    Ok((bytes, meta_json(&result)?))
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

/// Place page `source_page` of `source_pdf_bytes` onto page `target_page` of
/// `target_pdf_bytes` at `rect` (fitz display coords), returning the new target
/// bytes (port of PyMuPDF `Page.show_pdf_page(rect, src, pno, overlay=True)`).
#[pyfunction]
fn show_pdf_page(
    target_pdf_bytes: &[u8],
    source_pdf_bytes: &[u8],
    target_page: i32,
    source_page: i32,
    rect: (f64, f64, f64, f64),
) -> PyResult<Vec<u8>> {
    let dir = temp_dir()?;
    let target_path = dir.join("target.pdf");
    let source_path = dir.join("source.pdf");
    std::fs::write(&target_path, target_pdf_bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("write target: {e}")))?;
    std::fs::write(&source_path, source_pdf_bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("write source: {e}")))?;
    let mut target = PdfDocument::open(target_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open target: {e}")))?;
    let source = PdfDocument::open(source_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open source: {e}")))?;
    rendering_writer::overlay::show_pdf_page(
        &mut target,
        target_page,
        &source,
        source_page,
        [rect.0, rect.1, rect.2, rect.3],
    )
    .map_err(|e| PyRuntimeError::new_err(format!("show_pdf_page: {e}")))?;
    save_to_bytes(&target, &dir)
}

/// Build the dual-book doc: each page = source page (left) + translated page
/// (right), mirroring `book_support.build_dual_doc_pages`. Returns the new doc
/// bytes. The page composition lives in
/// `rendering_writer::overlay::build_dual_doc_pages`, shared with the render
/// orchestrator.
#[pyfunction]
fn build_dual_doc_pages(
    source_pdf_bytes: &[u8],
    translated_pdf_bytes: &[u8],
    start_page: i32,
    end_page: i32,
) -> PyResult<Vec<u8>> {
    let dir = temp_dir()?;
    let src_path = dir.join("dual-src.pdf");
    let trl_path = dir.join("dual-trl.pdf");
    std::fs::write(&src_path, source_pdf_bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("write src: {e}")))?;
    std::fs::write(&trl_path, translated_pdf_bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("write trl: {e}")))?;
    let source = PdfDocument::open(src_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open src: {e}")))?;
    let translated = PdfDocument::open(trl_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open trl: {e}")))?;
    let dual = rendering_writer::overlay::build_dual_doc_pages(&source, &translated, start_page, end_page)
        .map_err(|e| PyRuntimeError::new_err(format!("build dual doc: {e}")))?;
    save_to_bytes(&dual, &dir)
}

/// `save_optimized_pdf` entry point — PDF bytes in, optimised bytes out
/// (garbage=4 + image/font stream compression). Full port of
/// `document/pdf_ops.save_optimized_pdf` including the font subsetting half:
/// the C shim in `rendering_writer/c/save_clean.c` runs mupdf
/// `pdf_subset_fonts` + clean write on an isolated context, so production no
/// longer needs fitz `subset_fonts()` first (see the
/// `source/_native.py::save_optimized` shim).
#[pyfunction]
fn subset_and_save_optimized_pdf(pdf_bytes: &[u8]) -> PyResult<Vec<u8>> {
    subset_and_clean(pdf_bytes).map_err(PyRuntimeError::new_err)
}

/// `document/pdf_ops.save_fast_pdf` entry point — PDF bytes in, raw bytes out
/// (no subset / no compaction, mirroring fitz `doc.save`'s default write). The
/// native half of the default production save so the default render path has
/// zero fitz write calls (see the `source/_native.py::save_fast` shim).
#[pyfunction]
fn save_fast_pdf(pdf_bytes: &[u8]) -> PyResult<Vec<u8>> {
    let dir = temp_dir()?;
    let in_path = dir.join("in.pdf");
    std::fs::write(&in_path, pdf_bytes).map_err(|e| PyRuntimeError::new_err(format!("write: {e}")))?;
    let pdf = PdfDocument::open(in_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open: {e}")))?;
    save_to_bytes(&pdf, &dir)
}

/// `metadata.py::copy_toc` — copy the source outline (remapped to the target
/// page range) into the target document. Returns the target bytes plus the
/// number of outline entries written (0 = unchanged, so the shim can skip a
/// doc swap). `end_page < 0` is unbounded (Python `end_page=None`).
#[pyfunction]
fn copy_toc(
    source_bytes: &[u8],
    target_bytes: &[u8],
    start_page: i32,
    end_page: i32,
) -> PyResult<(Vec<u8>, usize)> {
    let dir = temp_dir()?;
    let src_path = dir.join("toc-src.pdf");
    let tgt_path = dir.join("toc-tgt.pdf");
    std::fs::write(&src_path, source_bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("write source: {e}")))?;
    std::fs::write(&tgt_path, target_bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("write target: {e}")))?;
    let source = Document::open(src_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open source: {e}")))?;
    let mut target = PdfDocument::open(tgt_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open target: {e}")))?;
    let count = rendering_writer::background::toc::copy_toc(
        &source,
        &mut target,
        start_page,
        end_page,
    )
    .map_err(|e| PyRuntimeError::new_err(format!("copy_toc: {e}")))?;
    let bytes = save_to_bytes(&target, &dir)?;
    Ok((bytes, count))
}

/// `metadata.py::copy_toc_for_page_map` — copy the source outline with pages
/// remapped through `source_page_indices` (target page = output slot + 1).
#[pyfunction]
fn copy_toc_for_page_map(
    source_bytes: &[u8],
    target_bytes: &[u8],
    source_page_indices: Vec<u32>,
) -> PyResult<(Vec<u8>, usize)> {
    let dir = temp_dir()?;
    let src_path = dir.join("tocpm-src.pdf");
    let tgt_path = dir.join("tocpm-tgt.pdf");
    std::fs::write(&src_path, source_bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("write source: {e}")))?;
    std::fs::write(&tgt_path, target_bytes)
        .map_err(|e| PyRuntimeError::new_err(format!("write target: {e}")))?;
    let source = Document::open(src_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open source: {e}")))?;
    let mut target = PdfDocument::open(tgt_path.as_path())
        .map_err(|e| PyRuntimeError::new_err(format!("open target: {e}")))?;
    let count = rendering_writer::background::toc::copy_toc_for_page_map(
        &source,
        &mut target,
        &source_page_indices,
    )
    .map_err(|e| PyRuntimeError::new_err(format!("copy_toc_for_page_map: {e}")))?;
    let bytes = save_to_bytes(&target, &dir)?;
    Ok((bytes, count))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(strip_bbox_text_rects, m)?)?;
    m.add_function(wrap_pyfunction!(strip_hidden_text, m)?)?;
    m.add_function(wrap_pyfunction!(strip_hidden_text_pages, m)?)?;
    m.add_function(wrap_pyfunction!(sanitize_invalid_xobjects, m)?)?;
    m.add_function(wrap_pyfunction!(compress_images, m)?)?;
    m.add_function(wrap_pyfunction!(extract_pages, m)?)?;
    m.add_function(wrap_pyfunction!(overlay_page, m)?)?;
    m.add_function(wrap_pyfunction!(show_pdf_page, m)?)?;
    m.add_function(wrap_pyfunction!(build_dual_doc_pages, m)?)?;
    m.add_function(wrap_pyfunction!(subset_and_save_optimized_pdf, m)?)?;
    m.add_function(wrap_pyfunction!(save_fast_pdf, m)?)?;
    m.add_function(wrap_pyfunction!(copy_toc, m)?)?;
    m.add_function(wrap_pyfunction!(copy_toc_for_page_map, m)?)?;
    Ok(())
}
