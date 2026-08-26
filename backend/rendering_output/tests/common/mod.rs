//! Shared support for the rendering_output differential tests.
//!
//! Measures the same semantic facts the corpus generators record: per-page word
//! count and ink ratio. Mirrors `rendering_writer/tests/common/mod.rs` (the
//! fitz-vs-mupdf-rs extraction paths differ up to ~5%, so 10% relative word
//! tolerance; both engines wrap the same MuPDF C renderer, so ±0.02 absolute
//! ink tolerance).

#![allow(dead_code)]

use base64::Engine;
use mupdf::{Document, TextExtractOptions, TextPageFlags};
use rendering_reader::render::render_page_clip_gray;
use serde::Deserialize;

/// Relative word-count tolerance vs fitz `get_text("words")`.
pub const WORD_COUNT_TOLERANCE: f64 = 0.10;
/// Absolute ink-ratio tolerance (both engines wrap MuPDF).
pub const INK_RATIO_TOLERANCE: f64 = 0.02;

#[derive(Deserialize, Clone, Debug)]
pub struct PageFact {
    pub words: usize,
    pub ink_ratio: f64,
}

pub fn decode(b64: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .unwrap_or_else(|e| panic!("base64 decode failed: {e}"))
}

/// Per-page word count via mupdf-rs `Page::words`.
pub fn page_words(doc: &Document, idx: i32) -> usize {
    let page = doc.load_page(idx).unwrap_or_else(|e| panic!("load_page {idx}: {e}"));
    let options = TextExtractOptions {
        flags: TextPageFlags::PRESERVE_LIGATURES,
    };
    page.words(options).unwrap_or_else(|e| panic!("words p{idx}: {e}")).len()
}

/// Ink ratio: fraction of gray-render samples below `ink_threshold`.
pub fn page_ink_ratio(doc: &Document, idx: i32, scale: f32, ink_threshold: u8) -> f64 {
    let px = render_page_clip_gray(doc, idx, None, scale)
        .unwrap_or_else(|e| panic!("render p{idx}: {e}"));
    let total = (px.width as usize).saturating_mul(px.height as usize);
    if total == 0 {
        return 0.0;
    }
    let dark = px.samples.iter().filter(|&&b| b < ink_threshold).count();
    dark as f64 / total as f64
}

pub fn measure_pages(
    doc: &Document,
    page_count: usize,
    scale: f32,
    ink_threshold: u8,
) -> Vec<PageFact> {
    (0..page_count as i32)
        .map(|idx| PageFact {
            words: page_words(doc, idx),
            ink_ratio: page_ink_ratio(doc, idx, scale, ink_threshold),
        })
        .collect()
}

pub fn assert_page_facts(actual: &[PageFact], expected: &[PageFact], label: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{label}: page fact count mismatch: actual={actual:?}, expected={expected:?}"
    );
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        let rel = (a.words as f64 - e.words as f64).abs() / (e.words.max(1) as f64);
        assert!(
            rel <= WORD_COUNT_TOLERANCE,
            "{label} p{i}: words divergence actual={}, expected={}, rel={rel:.3} > {WORD_COUNT_TOLERANCE}",
            a.words,
            e.words,
        );
        let ink_diff = (a.ink_ratio - e.ink_ratio).abs();
        assert!(
            ink_diff <= INK_RATIO_TOLERANCE,
            "{label} p{i}: ink_ratio divergence actual={:.6}, expected={:.6}, diff={ink_diff:.6} > {INK_RATIO_TOLERANCE}",
            a.ink_ratio,
            e.ink_ratio,
        );
    }
}
