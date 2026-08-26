//! Port of `output/typst/emitter.py`: turn `RenderPageSpec` DTOs into a full
//! Typst source document.

use crate::block_config::{typst_package_imports, DEFAULT_FONT_SIZE_PT};
use crate::block_renderer::build_typst_block;
use crate::dto::{RenderBlock, RenderPageSpec};
use crate::fit_helpers::page_spec_fit_helpers;
use crate::util::fmt_f;
use std::path::Path;

/// `build_typst_source_from_page_specs`. `work_dir` is used to compute the
/// background image path relative to the emitted source (Python `relpath`).
pub fn build_typst_source_from_page_specs(
    background_pdf_path: &Path,
    page_specs: &[RenderPageSpec],
    work_dir: &Path,
    font_family: &str,
) -> String {
    let source_rel = relpath(background_pdf_path, work_dir);
    let mut lines: Vec<String> = vec![format!(
        "#set text(font: \"{font_family}\", size: {}pt)",
        fmt_f(DEFAULT_FONT_SIZE_PT)
    )];
    lines.extend(typst_package_imports());
    lines.extend(page_spec_fit_helpers().iter().map(|s| s.to_string()));

    let total_pages = page_specs.len();
    for (page_offset, spec) in page_specs.iter().enumerate() {
        lines.push(format!(
            "#set page(width: {}pt, height: {}pt, margin: 0pt, fill: none)",
            fmt_f(spec.page_width_pt),
            fmt_f(spec.page_height_pt),
        ));
        lines.push(format!(
            "#place(top + left, dx: 0pt, dy: 0pt, image(\"{source_rel}\", page: {}, width: {}pt))",
            spec.page_index + 1,
            fmt_f(spec.page_width_pt),
        ));
        for (block_index, block) in spec.blocks.iter().enumerate() {
            let block_id = format!("rp{page_offset}_{}_{block_index}", block.block_id);
            lines.push(build_typst_block(&block_id, &RenderBlock::from_layout(block), true));
        }
        if page_offset + 1 < total_pages {
            lines.push("#pagebreak()".to_string());
        }
    }
    format!("{}\n", lines.join("\n"))
}

/// `os.path.relpath(path, start)`.
pub fn relpath(path: &Path, start: &Path) -> String {
    pathdiff::diff_paths(path, start)
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relpath_within_work_dir() {
        let root = Path::new("/tmp/rendering");
        let pdf = root.join("background.pdf");
        assert_eq!(relpath(&pdf, root), "background.pdf");
    }

    #[test]
    fn relpath_sibling_directory() {
        let root = Path::new("/tmp/rendering/work");
        let pdf = Path::new("/tmp/rendering/background.pdf");
        assert_eq!(relpath(&pdf, root), "../background.pdf");
    }
}
