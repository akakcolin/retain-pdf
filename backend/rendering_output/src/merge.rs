//! Port of `document/pikepdf_overlay.py::overlay_pdf_pages_with_pikepdf`
//! (Phase 5D-5): overlay compiled Typst overlay pages onto a source base PDF.
//!
//! Production merges overlay-page `j` onto source-page `source_page_indices[j]`
//! via pikepdf `source_page.add_overlay(overlay_page, rect=cropbox,
//! push_stack=True, shrink=False, expand=False)`, then saves with
//! `object_stream_mode=generate, compress_streams=True`. The per-page overlay
//! is the qpdf/pikepdf placement math already ported byte-compatibly in
//! `rendering_writer::overlay::overlay_page`; the save is mirrored by
//! `rendering_writer::save` (trailer `/ID` dropped for determinism).

use std::path::Path;

use mupdf::Error;

use rendering_reader::open_pdf_document;
use rendering_writer::overlay::overlay_page;
use rendering_writer::save::{delete_trailer_id, save_atomic};

/// Merge `overlay_pdf` onto `source_pdf` and write `output_pdf`, mirroring
/// `overlay_pdf_pages_with_pikepdf`. Overlay page `j` lands on source page
/// `source_page_indices[j]`. `None` (or an empty slice) falls back to the
/// production default `range(min(source_pages, overlay_pages))`. Returns the
/// number of pages merged.
pub fn overlay_pdf_pages(
    source_pdf_path: &Path,
    overlay_pdf_path: &Path,
    output_pdf_path: &Path,
    source_page_indices: Option<&[i32]>,
) -> Result<usize, Error> {
    let mut source = open_pdf_document(source_pdf_path)?;
    let overlay = open_pdf_document(overlay_pdf_path)?;
    let source_pages = source.page_count()?;
    let overlay_pages = overlay.page_count()?;

    let page_indices: Vec<i32> = match source_page_indices {
        Some(indices) if !indices.is_empty() => indices.to_vec(),
        _ => (0..source_pages.min(overlay_pages)).collect(),
    };

    let mut pages_merged = 0usize;
    for (overlay_page_idx, source_page_idx) in page_indices.iter().enumerate() {
        let overlay_page_idx = overlay_page_idx as i32;
        if *source_page_idx < 0 || *source_page_idx >= source_pages {
            continue;
        }
        if overlay_page_idx < 0 || overlay_page_idx >= overlay_pages {
            continue;
        }
        overlay_page(&mut source, *source_page_idx, &overlay, overlay_page_idx)?;
        pages_merged += 1;
    }

    delete_trailer_id(&source)?;
    save_atomic(&source, output_pdf_path)?;
    Ok(pages_merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build a minimal single-page PDF that mupdf can open, using a stream with
    /// a tiny content line (no external deps).
    fn minimal_pdf(title: &str) -> Vec<u8> {
        let content = format!("BT /F1 12 Tf 50 720 Td ({title}) Tj ET");
        let objects = format!(
            "%PDF-1.4\n\
             1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
             2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
             3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj\n\
             4 0 obj << /Length {len} >>\nstream\n{content}\nendstream endobj\n\
             5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n\
             trailer << /Root 1 0 R >>\n%%EOF\n",
            len = content.len()
        );
        objects.into_bytes()
    }

    fn run_case(indices: Option<&[i32]>) -> Result<usize, Error> {
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rp_merge_unit_{}_{}",
            std::process::id(),
            unique
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let base_path = dir.join("base.pdf");
        let overlay_path = dir.join("overlay.pdf");
        let out_path = dir.join("out.pdf");
        fs::write(&base_path, minimal_pdf("base")).unwrap();
        fs::write(&overlay_path, minimal_pdf("overlay")).unwrap();
        let merged = overlay_pdf_pages(&base_path, &overlay_path, &out_path, indices);
        let _ = fs::remove_dir_all(&dir);
        merged
    }

    #[test]
    fn default_indices_merge_all_pages() {
        assert_eq!(run_case(None).unwrap(), 1);
    }

    #[test]
    fn empty_indices_fall_back_to_default() {
        assert_eq!(run_case(Some(&[])).unwrap(), 1);
    }

    #[test]
    fn out_of_bounds_source_index_skipped() {
        assert_eq!(run_case(Some(&[0, 5])).unwrap(), 1);
    }
}
