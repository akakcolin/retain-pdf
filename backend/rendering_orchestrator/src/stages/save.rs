//! Save stage: copy the source outline into the compiled PDF (identity page
//! remap — the production `save_background_pdf_to_output` TOC copy), drop the
//! trailer id for deterministic bytes, and write the optimised output.

use std::path::{Path, PathBuf};
use std::time::Instant;

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use rendering_reader::{open_document, open_pdf_document};
use rendering_writer::background::toc::copy_toc;
use rendering_writer::save::{delete_trailer_id, save_optimized};

use crate::bundle::RenderBundle;

/// `(output_pdf, save_elapsed_seconds)` — the save elapsed mirrors
/// `book_renderer.py`'s `background_save_elapsed_seconds` timing.
pub fn run_save(bundle: &RenderBundle, compiled_pdf_path: &Path) -> anyhow::Result<(PathBuf, f64)> {
    let source_doc = open_document(bundle.source_pdf.as_path()).map_err(|e| anyhow::anyhow!("open render: {e}"))?;
    let mut compiled_pdf =
        open_pdf_document(compiled_pdf_path).map_err(|e| anyhow::anyhow!("open compiled: {e}"))?;
    let save_elapsed = run_save_range(bundle, &source_doc, &mut compiled_pdf)?;
    Ok((bundle.output_pdf.to_path_buf(), save_elapsed))
}

/// Copy the source TOC (identity range 0..-1), drop the trailer id, and save
/// the merged/compiled in-memory doc to `bundle.output_pdf`.
pub fn run_save_range(
    bundle: &RenderBundle,
    source_doc: &Document,
    pdf: &mut PdfDocument,
) -> anyhow::Result<f64> {
    run_save_pages(bundle, source_doc, pdf, 0, -1)
}

/// Copy the source TOC over `[start_page, end_page]` (dual remaps each source
/// page to `source_page - start + 1`, mirroring `_build_dual_book_pdf_native`),
/// drop the trailer id, and save the in-memory doc to `bundle.output_pdf`.
/// Returns the save elapsed in seconds.
pub fn run_save_pages(
    bundle: &RenderBundle,
    source_doc: &Document,
    pdf: &mut PdfDocument,
    start_page: i32,
    end_page: i32,
) -> anyhow::Result<f64> {
    let started = Instant::now();
    copy_toc(source_doc, pdf, start_page, end_page)
        .map_err(|e| anyhow::anyhow!("copy_toc: {e}"))?;
    delete_trailer_id(pdf).map_err(|e| anyhow::anyhow!("delete_trailer_id: {e}"))?;
    if let Some(parent) = bundle.output_pdf.parent() {
        std::fs::create_dir_all(parent)?;
    }
    save_optimized(pdf, &bundle.output_pdf)
        .map_err(|e| anyhow::anyhow!("save_optimized: {e}"))?;
    Ok(started.elapsed().as_secs_f64())
}
