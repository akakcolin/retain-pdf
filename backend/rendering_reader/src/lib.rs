//! Real PDF reader for `rendering_core`'s `PageSnapshot`, backed by mupdf-rs
//! (the MuPDF C engine — the same engine PyMuPDF/fitz wraps). Phase 4 wires the
//! Phase 3 data-shape abstraction to a real reader; golden-replay tests compare
//! the achievable snapshot fields against Python-computed values from fitz.

pub mod reader;
