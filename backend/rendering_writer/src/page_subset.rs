//! Page extraction port of `document/pikepdf_pages.py::extract_pages_with_pikepdf`.
//!
//! Production copies the selected page objects with qpdf
//! (`output.pages.extend(source.pages[start:end+1])`); mupdf-rs
//! `PdfDocument::insert_pdf` with a `PageSelection::Range` grafts the page
//! objects structurally (`graft_page_raw`), preserving resources and the text
//! layer. The result is semantically equivalent, not byte-identical.

use mupdf::pdf::{InsertPdfOptions, InsertPosition, PageRange, PageSelection, PdfDocument};
use mupdf::Error;

/// Extract pages `start_page..=end_page` into a new document, clamped like
/// production (`end_page < 0` selects through the last page; `start_page >
/// end_page` yields an empty document).
pub fn extract_pages(
    source: &mut PdfDocument,
    start_page: i64,
    end_page: i64,
) -> Result<PdfDocument, Error> {
    let page_count = source.page_count()? as usize;
    let last = page_count.saturating_sub(1);
    let start = start_page.max(0) as usize;
    let end = if end_page < 0 {
        last
    } else {
        (end_page as usize).min(last)
    };
    let mut out = PdfDocument::new();
    if start <= end {
        out.insert_pdf(
            source,
            InsertPdfOptions {
                source_pages: PageSelection::Range(PageRange::new(start, end + 1)),
                target: InsertPosition::Append,
                ..Default::default()
            },
        )?;
    }
    Ok(out)
}
