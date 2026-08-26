//! Port of `output/typst/source_builder.py`: the three whole-document Typst
//! source builders.

use crate::block_renderer::build_typst_block;
use crate::dto::RenderBlock;
use crate::source_pages::{build_book_background_source_lines, build_book_overlay_source_lines};
use std::path::Path;

/// `build_typst_overlay_source`.
pub fn build_typst_overlay_source(
    page_width: f64,
    page_height: f64,
    translated_blocks: &[RenderBlock],
    font_family: &str,
    include_cover_rect: bool,
) -> String {
    build_typst_book_overlay_source(&[(page_width, page_height, translated_blocks.to_vec())], font_family, include_cover_rect)
}

/// `build_typst_book_overlay_source`.
pub fn build_typst_book_overlay_source(
    page_specs: &[(f64, f64, Vec<RenderBlock>)],
    font_family: &str,
    include_cover_rect: bool,
) -> String {
    let lines = build_book_overlay_source_lines(page_specs, font_family, |block_id, block| {
        build_typst_block(block_id, block, include_cover_rect)
    });
    format!("{}\n", lines.join("\n"))
}

/// `build_typst_book_background_source`.
pub fn build_typst_book_background_source(
    source_pdf_path: &Path,
    page_specs: &[(i64, f64, f64, Vec<RenderBlock>)],
    work_dir: &Path,
    font_family: &str,
) -> String {
    let lines = build_book_background_source_lines(source_pdf_path, page_specs, work_dir, font_family, |block_id, block| {
        build_typst_block(block_id, block, true)
    });
    format!("{}\n", lines.join("\n"))
}
