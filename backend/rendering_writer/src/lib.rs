//! PDF write path for `rendering_core`, backed by mupdf-rs (the same MuPDF
//! engine fitz/pikepdf wrap). Replaces the production pikepdf/QPDF writing
//! steps: bbox text strip (`cleanup_writer`), page extraction, overlay, image
//! recompression, and preparation (Phase 5C).

pub mod cleanup_writer;
pub mod contents;
pub mod hidden_text;
pub mod image_compress;
pub mod overlay;
pub mod page_subset;
pub mod sanitize;
pub mod save;
