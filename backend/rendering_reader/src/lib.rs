//! Real PDF reader for `rendering_core`'s `PageSnapshot`, backed by mupdf-rs
//! (the MuPDF C engine — the same engine PyMuPDF/fitz wraps). Phase 4 wires the
//! Phase 3 data-shape abstraction to a real reader; golden-replay tests compare
//! the achievable snapshot fields against Python-computed values from fitz.

pub mod bboxlog;
pub mod error;
pub mod image_placements;
pub mod indent;
pub mod pdf_document;
pub mod reader;
pub mod render;
pub mod text_spans;

pub use error::PdfError;
pub use pdf_document::PdfDocument;
