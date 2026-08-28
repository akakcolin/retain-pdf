//! Error type for the `PdfDocument` IO abstraction.
//!
//! Wraps the underlying mupdf-rs `Error` so the trait API never leaks a mupdf
//! type to its consumers (Phase B: other crates depend only on the trait).

use std::fmt;

/// PDF IO error raised by `PdfDocument` methods.
#[derive(Debug)]
pub struct PdfError(mupdf::Error);

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for PdfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<mupdf::Error> for PdfError {
    fn from(e: mupdf::Error) -> Self {
        PdfError(e)
    }
}
