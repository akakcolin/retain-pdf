//! Native ports of the last three fitz-backed derived-artifact paths: the
//! page/cover JPEG renderer (`preview.rs`), the upload-time PDF repair
//! (`upload.rs`), and the side-by-side merge (`side_by_side_pdf.py`).
//!
//! Each is a `render_rs` argv subcommand (see `main.rs`); `rust_api` keeps
//! spawning the binary so the layering gate stays satisfied.

use std::path::Path;

use mupdf::pdf::PdfDocument as PdfWriter;
use mupdf::Size;

use rendering_reader::render::render_page_clip_rgb;
use rendering_reader::{open_document, open_pdf_document, PdfDocument as PdfReader};
use rendering_writer::image_compress::encode_jpeg_rgb;
use rendering_writer::overlay::show_pdf_page;
use rendering_writer::save::save_optimized;

/// fitz `pix.save(output)` default JPEG quality (cover/thumbnail).
pub const JPEG_QUALITY_DEFAULT: u8 = 95;
/// fitz `pix.save(output, jpg_quality=82)` (page preview).
pub const JPEG_QUALITY_PREVIEW: u8 = 82;

/// `preview.rs::render_book_image` / `render_pdf_page_preview` — render one
/// page at `scale = dpi/72` (or `width_px/page_width` when `dpi == 0`) and save
/// it as JPEG.
pub fn render_page_jpeg(
    input: &Path,
    output: &Path,
    page_index: i64,
    width_px: u32,
    dpi: u32,
    quality: u8,
) -> anyhow::Result<()> {
    let doc = open_document(input).map_err(|e| anyhow::anyhow!("open {}: {e}", input.display()))?;
    let count = PdfReader::page_count(&doc)?;
    if page_index < 0 || page_index >= count {
        anyhow::bail!("page out of range: {}/{}", page_index + 1, count);
    }
    let bounds = PdfReader::page_rect(&doc, page_index)?;
    let scale = if dpi > 0 {
        dpi as f32 / 72.0
    } else {
        width_px as f32 / (bounds.width().max(1.0) as f32)
    };
    let pix = render_page_clip_rgb(&doc, page_index as i32, None, scale)?;
    if pix.stride != 3 {
        anyhow::bail!("unexpected pixmap stride {} (expected rgb)", pix.stride);
    }
    let jpeg = encode_jpeg_rgb(&pix.samples, pix.width, pix.height, quality)?;
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, jpeg)?;
    Ok(())
}

/// `upload.rs::repair_pdf_with_pymupdf` — tolerant open + `garbage=4,
/// deflate=True` re-save. `max_output_bytes` / `max_pages` of 0 mean unlimited;
/// exceeding either is an error and leaves no partial output behind.
pub fn repair_pdf(
    input: &Path,
    output: &Path,
    max_output_bytes: u64,
    max_pages: u32,
) -> anyhow::Result<()> {
    let doc =
        open_pdf_document(input).map_err(|e| anyhow::anyhow!("open {}: {e}", input.display()))?;
    let pages = doc.page_count()?;
    if max_pages > 0 && pages as u64 > max_pages as u64 {
        anyhow::bail!("pdf page count {pages} exceeds limit {max_pages}");
    }
    save_optimized(&doc, output)?;
    if max_output_bytes > 0 {
        let size = std::fs::metadata(output)?.len();
        if size > max_output_bytes {
            let _ = std::fs::remove_file(output);
            anyhow::bail!("repaired pdf size {size} exceeds limit {max_output_bytes}");
        }
    }
    Ok(())
}

/// `side_by_side_pdf.py::build_side_by_side_pdf` — original page on the left,
/// translated page on the right, one output page per max page count.
pub fn side_by_side(source: &Path, translated: &Path, output: &Path) -> anyhow::Result<()> {
    let source_doc = open_pdf_document(source)
        .map_err(|e| anyhow::anyhow!("open {}: {e}", source.display()))?;
    let translated_doc = open_pdf_document(translated)
        .map_err(|e| anyhow::anyhow!("open {}: {e}", translated.display()))?;
    let src_count = source_doc.page_count()?;
    let trl_count = translated_doc.page_count()?;
    if src_count < 1 {
        anyhow::bail!("source pdf has no pages: {}", source.display());
    }
    if trl_count < 1 {
        anyhow::bail!("translated pdf has no pages: {}", translated.display());
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = PdfWriter::new();
    for page_index in 0..src_count.max(trl_count) {
        let src_rect = page_rect(&source_doc, page_index, src_count)?;
        let trl_rect = page_rect(&translated_doc, page_index, trl_count)?;
        let (left_w, right_w, page_h) = side_by_side_geometry(src_rect, trl_rect);
        out.new_page(Size::new((left_w + right_w) as f32, page_h as f32))?;
        let page_no = out.page_count()? - 1;
        if src_rect.is_some() {
            show_pdf_page(&mut out, page_no, &source_doc, page_index, [0.0, 0.0, left_w, page_h])?;
        }
        if trl_rect.is_some() {
            show_pdf_page(
                &mut out,
                page_no,
                &translated_doc,
                page_index,
                [left_w, 0.0, left_w + right_w, page_h],
            )?;
        }
    }
    save_optimized(&out, output)?;
    Ok(())
}

/// `_page_rect` — the page's fitz rect, or `None` past the end.
fn page_rect(doc: &PdfWriter, page_index: i32, page_count: i32) -> anyhow::Result<Option<[f64; 4]>> {
    if page_index >= page_count {
        return Ok(None);
    }
    let bounds = doc.load_pdf_page(page_index)?.bounds()?;
    Ok(Some([
        bounds.x0 as f64,
        bounds.y0 as f64,
        bounds.x1 as f64,
        bounds.y1 as f64,
    ]))
}

/// Missing-side fallbacks: a present side's width fills the absent column and
/// the page height is the max of the present sides.
fn side_by_side_geometry(src: Option<[f64; 4]>, trl: Option<[f64; 4]>) -> (f64, f64, f64) {
    let src_w = src.map(|r| r[2] - r[0]);
    let trl_w = trl.map(|r| r[2] - r[0]);
    let left_w = src_w.or(trl_w).unwrap_or(0.0);
    let right_w = trl_w.or(src_w).unwrap_or(0.0);
    let src_h = src.map(|r| r[3] - r[1]).unwrap_or(0.0);
    let trl_h = trl.map(|r| r[3] - r[1]).unwrap_or(0.0);
    (left_w, right_w, src_h.max(trl_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_uses_missing_side_width_for_both_columns() {
        let page = [0.0, 0.0, 200.0, 300.0];
        assert_eq!(side_by_side_geometry(Some(page), None), (200.0, 200.0, 300.0));
        assert_eq!(side_by_side_geometry(None, Some(page)), (200.0, 200.0, 300.0));
    }

    #[test]
    fn geometry_sums_widths_and_takes_max_height() {
        let (l, r, h) = side_by_side_geometry(Some([0.0, 0.0, 200.0, 300.0]), Some([0.0, 0.0, 150.0, 400.0]));
        assert_eq!((l, r, h), (200.0, 150.0, 400.0));
    }
}
