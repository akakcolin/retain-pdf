//! Overlay stage: emit the whole-book overlay Typst source from the
//! bundle's `overlay_page_specs` (RenderBlock DTOs, geometry from the original
//! source), compile it with the `typst` CLI (mirror of
//! `compiler.py::compile_typst_book_overlay_pdf`), merge every overlay page
//! onto the render-source base via `show_pdf_page`, then save. The dual stage
//! reuses the compile + merge helpers to build its translated side.

use std::path::PathBuf;

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use rendering_output::compile::compile_typst_source;
use rendering_output::dto::RenderBlock;
use rendering_output::source_builder::build_typst_book_overlay_source;
use rendering_writer::overlay::show_pdf_page;

use crate::bundle::RenderBundle;
use crate::stages::save::run_save_range;
use crate::stages::typst::compile_context;

pub const OVERLAY_COMPILE_STEM: &str = "book-overlay";
pub const OVERLAY_COMPILE_PHASE: &str = "overlay_book";

/// Emit + compile the overlay pages to `work_dir/{stem}.pdf`. The compiled
/// pages are in `overlay_page_specs` order (sorted source page indices).
pub fn run_overlay_compile(
    bundle: &RenderBundle,
    stem: &str,
    phase: &str,
) -> anyhow::Result<PathBuf> {
    let specs = bundle
        .overlay_page_specs
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bundle missing overlay_page_specs"))?;
    if specs.is_empty() {
        anyhow::bail!("bundle overlay_page_specs empty");
    }
    let page_blocks: Vec<(f64, f64, Vec<RenderBlock>)> = specs
        .iter()
        .map(|spec| (spec.page_width_pt, spec.page_height_pt, spec.blocks.clone()))
        .collect();
    // Production `overlay_translated_pages_on_doc` compiles with cover rects.
    let source = build_typst_book_overlay_source(&page_blocks, &bundle.font_family, true);

    let ctx = compile_context();
    compile_typst_source(
        &source,
        stem,
        phase,
        bundle.work_dir.as_path(),
        None,
        &[],
        &ctx,
        serde_json::Map::new(),
    )
    .map_err(|e| anyhow::anyhow!("typst compile: {}", e.message()))
}

/// Merge each compiled overlay page onto the render-source base page
/// (`overlay_page_specs[i].page_index`) via `show_pdf_page`, returning the
/// in-memory merged doc.
pub fn merge_overlay_onto_base(bundle: &RenderBundle, overlay_pdf_path: &std::path::Path) -> anyhow::Result<PdfDocument> {
    let specs = bundle
        .overlay_page_specs
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("bundle missing overlay_page_specs"))?;
    let overlay_doc =
        PdfDocument::open(overlay_pdf_path).map_err(|e| anyhow::anyhow!("open overlay: {e}"))?;
    let mut base_doc =
        PdfDocument::open(bundle.source_pdf.as_path()).map_err(|e| anyhow::anyhow!("open base: {e}"))?;
    for (overlay_page_idx, spec) in specs.iter().enumerate() {
        show_pdf_page(
            &mut base_doc,
            spec.page_index,
            &overlay_doc,
            overlay_page_idx as i32,
            [0.0, 0.0, spec.page_width_pt, spec.page_height_pt],
        )
        .map_err(|e| anyhow::anyhow!("show_pdf_page page {}: {e}", spec.page_index))?;
    }
    Ok(base_doc)
}

pub fn run_overlay(bundle: &RenderBundle) -> anyhow::Result<PathBuf> {
    let compiled = run_overlay_compile(bundle, OVERLAY_COMPILE_STEM, OVERLAY_COMPILE_PHASE)?;
    let mut base_doc = merge_overlay_onto_base(bundle, &compiled)?;
    let source_doc =
        Document::open(bundle.source_pdf.as_path()).map_err(|e| anyhow::anyhow!("open render: {e}"))?;
    run_save_range(bundle, &source_doc, &mut base_doc)?;
    Ok(bundle.output_pdf.to_path_buf())
}
