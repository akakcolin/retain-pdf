//! Dual stage: mirror of `book_renderer._build_dual_book_pdf_native`. Builds
//! the translated side (render-source base + compiled overlay pages), composes
//! each page as original | translated via `build_dual_doc_pages`, copies the
//! source TOC over `[start_page, end_page]`, then saves.

use std::path::PathBuf;

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use rendering_writer::overlay::build_dual_doc_pages;

use crate::bundle::RenderBundle;
use crate::stages::overlay::{merge_overlay_onto_base, run_overlay_compile};
use crate::stages::save::run_save_pages;

pub const DUAL_TRANSLATED_STEM: &str = "book-overlay-dual";
pub const DUAL_TRANSLATED_PHASE: &str = "overlay_book";

pub fn run_dual(bundle: &RenderBundle) -> anyhow::Result<PathBuf> {
    let compiled = run_overlay_compile(bundle, DUAL_TRANSLATED_STEM, DUAL_TRANSLATED_PHASE)?;
    let translated_side = merge_overlay_onto_base(bundle, &compiled)?;
    let source_doc =
        PdfDocument::open(bundle.source_pdf.as_path()).map_err(|e| anyhow::anyhow!("open render: {e}"))?;
    let mut dual = build_dual_doc_pages(
        &source_doc,
        &translated_side,
        bundle.start_page,
        bundle.end_page,
    )
    .map_err(|e| anyhow::anyhow!("build_dual_doc_pages: {e}"))?;
    let source_read =
        Document::open(bundle.source_pdf.as_path()).map_err(|e| anyhow::anyhow!("open render: {e}"))?;
    run_save_pages(bundle, &source_read, &mut dual, bundle.start_page, bundle.end_page)?;
    Ok(bundle.output_pdf.to_path_buf())
}
