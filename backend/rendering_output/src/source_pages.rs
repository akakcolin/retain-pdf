//! Port of `output/typst/source_pages.py`: the whole-book overlay/background
//! Typst source builders.
//!
//! The production code calls `build_render_blocks` (the layout pipeline) to
//! turn `translated_items` into render blocks; that step is out of scope for
//! the Rust port, so these functions accept already-built `RenderBlock`s.

use crate::block_config::{typst_package_imports, DEFAULT_FONT_SIZE_PT};
use crate::dto::RenderBlock;
use crate::emitter::relpath;
use crate::fit_helpers::render_block_fit_helpers;
use crate::util::fmt_f;
use std::path::Path;

/// `typst_book_prelude`.
pub fn typst_book_prelude(font_family: &str) -> Vec<String> {
    let mut lines = vec![format!(
        "#set text(font: \"{font_family}\", size: {}pt)",
        fmt_f(DEFAULT_FONT_SIZE_PT)
    )];
    lines.extend(typst_package_imports());
    lines.extend(render_block_fit_helpers().iter().map(|s| s.to_string()));
    lines
}

/// `append_overlay_page_source`.
pub fn append_overlay_page_source<F>(
    lines: &mut Vec<String>,
    page_index: i64,
    page_count: i64,
    page_width: f64,
    page_height: f64,
    render_blocks: &[RenderBlock],
    block_builder: F,
) where
    F: Fn(&str, &RenderBlock) -> String,
{
    lines.push(format!(
        "#set page(width: {}pt, height: {}pt, margin: 0pt, fill: none)",
        fmt_f(page_width),
        fmt_f(page_height),
    ));
    for (index, block) in render_blocks.iter().enumerate() {
        let block_id = format!("p{page_index}_{}_{index}", block.block_id);
        lines.push(block_builder(&block_id, block));
    }
    if page_index + 1 < page_count {
        lines.push("#pagebreak()".to_string());
    }
}

/// `append_background_page_source`.
pub fn append_background_page_source<F>(
    lines: &mut Vec<String>,
    page_index: i64,
    page_count: i64,
    source_page_idx: i64,
    source_rel: &str,
    page_width: f64,
    page_height: f64,
    render_blocks: &[RenderBlock],
    block_builder: F,
) where
    F: Fn(&str, &RenderBlock) -> String,
{
    lines.push(format!(
        "#set page(width: {}pt, height: {}pt, margin: 0pt, fill: none)",
        fmt_f(page_width),
        fmt_f(page_height),
    ));
    lines.push(format!(
        "#place(top + left, dx: 0pt, dy: 0pt, image(\"{source_rel}\", page: {}, width: {}pt))",
        source_page_idx + 1,
        fmt_f(page_width),
    ));
    for (index, block) in render_blocks.iter().enumerate() {
        let block_id = format!("bgp{page_index}_{}_{index}", block.block_id);
        lines.push(block_builder(&block_id, block));
    }
    if page_index + 1 < page_count {
        lines.push("#pagebreak()".to_string());
    }
}

/// `build_book_overlay_source_lines`.
pub fn build_book_overlay_source_lines<F>(
    page_specs: &[(f64, f64, Vec<RenderBlock>)],
    font_family: &str,
    block_builder: F,
) -> Vec<String>
where
    F: Fn(&str, &RenderBlock) -> String,
{
    let mut lines = typst_book_prelude(font_family);
    let page_count = page_specs.len() as i64;
    for (page_index, (page_width, page_height, render_blocks)) in page_specs.iter().enumerate() {
        append_overlay_page_source(
            &mut lines,
            page_index as i64,
            page_count,
            *page_width,
            *page_height,
            render_blocks,
            &block_builder,
        );
    }
    lines
}

/// `build_book_background_source_lines`.
pub fn build_book_background_source_lines<F>(
    source_pdf_path: &Path,
    page_specs: &[(i64, f64, f64, Vec<RenderBlock>)],
    work_dir: &Path,
    font_family: &str,
    block_builder: F,
) -> Vec<String>
where
    F: Fn(&str, &RenderBlock) -> String,
{
    let source_rel = relpath(source_pdf_path, work_dir);
    let mut lines = typst_book_prelude(font_family);
    let page_count = page_specs.len() as i64;
    for (page_index, (source_page_idx, page_width, page_height, render_blocks)) in
        page_specs.iter().enumerate()
    {
        append_background_page_source(
            &mut lines,
            page_index as i64,
            page_count,
            *source_page_idx,
            &source_rel,
            *page_width,
            *page_height,
            render_blocks,
            &block_builder,
        );
    }
    lines
}
