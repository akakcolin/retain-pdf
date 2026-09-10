//! Path-based mupdf document opening.
//!
//! mupdf gates `impl AsRef<FilePath> for Path` behind
//! `#[cfg(any(unix, target_os = "wasi"))]`; only `str` and `String` implement it
//! unconditionally. Passing a `&Path` straight to `mupdf::*::open` therefore
//! fails to compile on Windows (`E0277`). Every crate that opens a PDF by path
//! must go through the helpers here instead of calling mupdf directly.

use std::path::Path;

use mupdf::{pdf::PdfDocument, Document};

/// Opens a path as a general mupdf document (any supported format).
pub fn open_document(path: &Path) -> Result<Document, mupdf::Error> {
    Document::open(&path.to_string_lossy().into_owned())
}

/// Opens a path as an editable PDF document.
pub fn open_pdf_document(path: &Path) -> Result<PdfDocument, mupdf::Error> {
    PdfDocument::open(&path.to_string_lossy().into_owned())
}
