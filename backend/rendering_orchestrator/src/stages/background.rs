//! Background stage: `build_clean_background_pdf` (mirror of the bridge
//! `build_clean_background_pdf` pyfunction, `rendering_bridge/src/lib.rs:350`).
//! Opens the render doc (reader) and the edit pdf (writer) on the same source
//! path, applies the page-spec replacement + visual-profile fills natively,
//! runs the redaction stage with a render-clip closure, then saves the cleaned
//! background PDF optimised.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use mupdf::pdf::PdfDocument;
use mupdf::Document;
use rendering_core::rect::Rect as CoreRect;
use rendering_core::source_cleanup::hit_test::RectTuple;
use rendering_reader::PdfDocument as _;
use rendering_writer::background::fill::RgbPixmap;
use rendering_writer::background::redaction::page_specs;
use rendering_writer::background::stage::build_clean_background_pdf;
use rendering_writer::save::{delete_trailer_id, save_optimized};

use crate::bundle::RenderBundle;

/// `work_dir/book-background-cleaned.pdf` — the same name production uses, so
/// the typst `--root` (common root of the compile paths) resolves the emitted
/// relative background image path exactly like Python.
pub fn cleaned_background_path(bundle: &RenderBundle) -> PathBuf {
    bundle.work_dir.join("book-background-cleaned.pdf")
}

pub fn run_background(bundle: &RenderBundle) -> anyhow::Result<PathBuf> {
    let source_pdf = bundle.source_pdf.as_path();
    let cleaned_bg_path = cleaned_background_path(bundle);

    let doc = Document::open(source_pdf).map_err(|e| anyhow::anyhow!("open render: {e}"))?;
    let page = doc
        .load_page(0)
        .map_err(|e| anyhow::anyhow!("load_page 0: {e}"))?;
    let bounds = page
        .bounds()
        .map_err(|e| anyhow::anyhow!("bounds: {e}"))?;
    let page_rect: RectTuple = [bounds.x0 as f64, bounds.y0 as f64, bounds.x1 as f64, bounds.y1 as f64];
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

    let redaction_specs = bundle.redaction_page_specs()?;
    let fill_map: HashMap<String, [f64; 3]> = bundle.visual_profile_fill_map.clone();
    let precleaned: HashSet<i32> = bundle.precleaned_page_indices.iter().copied().collect();
    let (translated_pages, formula_source_pages) =
        page_specs::apply_page_specs_and_fills(&bundle.translated_pages, &redaction_specs, &fill_map);

    let mut pdf = PdfDocument::open(source_pdf).map_err(|e| anyhow::anyhow!("open edit: {e}"))?;
    build_clean_background_pdf(
        &doc,
        &mut pdf,
        &page_rect,
        &translated_pages,
        &formula_source_pages,
        bundle.redaction_strategy.as_deref(),
        &precleaned,
        &render_clip,
    )
    .map_err(|e| anyhow::anyhow!("build_clean_background_pdf: {e}"))?;

    delete_trailer_id(&pdf).map_err(|e| anyhow::anyhow!("delete_trailer_id: {e}"))?;
    std::fs::create_dir_all(&bundle.work_dir)?;
    save_optimized(&pdf, &cleaned_bg_path).map_err(|e| anyhow::anyhow!("save_optimized: {e}"))?;
    Ok(cleaned_bg_path)
}
