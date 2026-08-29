//! Save stage: copy the source outline into the compiled PDF (identity page
//! remap — the production `save_background_pdf_to_output` TOC copy), drop the
//! trailer id for deterministic bytes, and write the optimised output.

use std::path::{Path, PathBuf};

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use rendering_writer::background::toc::copy_toc;
use rendering_writer::save::{delete_trailer_id, save_optimized};

use crate::bundle::RenderBundle;

pub fn run_save(bundle: &RenderBundle, compiled_pdf_path: &Path) -> anyhow::Result<PathBuf> {
    let output_pdf = bundle.output_pdf.as_path();
    let source_doc = Document::open(bundle.source_pdf.as_path()).map_err(|e| anyhow::anyhow!("open render: {e}"))?;
    let mut compiled_pdf =
        PdfDocument::open(compiled_pdf_path).map_err(|e| anyhow::anyhow!("open compiled: {e}"))?;
    copy_toc(&source_doc, &mut compiled_pdf, 0, -1)
        .map_err(|e| anyhow::anyhow!("copy_toc: {e}"))?;
    delete_trailer_id(&compiled_pdf).map_err(|e| anyhow::anyhow!("delete_trailer_id: {e}"))?;
    if let Some(parent) = output_pdf.parent() {
        std::fs::create_dir_all(parent)?;
    }
    save_optimized(&compiled_pdf, output_pdf).map_err(|e| anyhow::anyhow!("save_optimized: {e}"))?;
    Ok(output_pdf.to_path_buf())
}
