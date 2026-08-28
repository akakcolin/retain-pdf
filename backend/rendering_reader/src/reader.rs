//! Convenience entry points over `PdfDocument` (Phase B1).
//!
//! The implementation lives in `PdfDocument` (see `pdf_document.rs`); these
//! generic wrappers keep the pre-trait free-function API working for existing
//! callers and tests without naming a mupdf type. Phase B2 deletes the
//! wrappers once production fitz call sites migrate onto the trait.

use std::path::Path;

use rendering_core::page::PageSnapshot;

use crate::error::PdfError;
use crate::pdf_document::PdfDocument;

/// Open a PDF file via the `PdfDocument` implementation for `T`.
pub fn open<T: PdfDocument>(path: &Path) -> Result<T, PdfError> {
    T::open(path)
}

/// Total page count via `PdfDocument`.
pub fn page_count<T: PdfDocument>(doc: &T) -> Result<i64, PdfError> {
    doc.page_count()
}

/// Per-page achievable snapshot via `PdfDocument`.
pub fn read_page_snapshot<T: PdfDocument>(doc: &T, idx: i64) -> Result<PageSnapshot, PdfError> {
    doc.page_snapshot(idx)
}
