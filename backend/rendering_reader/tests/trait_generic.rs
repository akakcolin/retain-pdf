//! Phase B1 acceptance proof: `PdfDocument` is usable through generic consumers
//! that never name a mupdf type.
//!
//! The three helpers below (`open_all`, `read_all`, `render_gray`) are written
//! only against the trait; the tests instantiate them with `mupdf::Document` at
//! the call site. This mirrors how Phase B2 call sites will consume the trait.
//!
//! Proof layers:
//!   1. Snapshot parity — generic open + read replay the golden corpus
//!      achievable facts (the same assertions `golden_pdf_replay.rs` makes).
//!   2. Render plumbing — full-page gray renders through the generic consumer
//!      are byte-identical to the free-function wrapper path.
//!   3. Pixel oracle — generic clipped renders reproduce fitz's recorded
//!      page pixels from `render_corpus.json` (byte-for-byte; same engine).

mod common;

use std::path::Path;

use base64::Engine;
use mupdf::Document;
use rendering_core::page::PageSnapshot;
use rendering_core::rect::Rect;
use rendering_reader::render::RenderedPixels;
use rendering_reader::{PdfDocument, PdfError};
use serde::Deserialize;

use common::{assert_snapshot_achievable, corpus, repo_root};

/// Generic open: `T` is any `PdfDocument` implementation.
fn open_all<T: PdfDocument>(path: &Path) -> Result<T, PdfError> {
    T::open(path)
}

/// Generic per-page snapshot read.
fn read_all<T: PdfDocument>(doc: &T, idx: i64) -> Result<PageSnapshot, PdfError> {
    doc.page_snapshot(idx)
}

/// Generic full-page grayscale render.
fn render_gray<T: PdfDocument>(doc: &T, page: i64, scale: f32) -> Result<RenderedPixels, PdfError> {
    doc.render_page_clip_gray(page, None, scale)
}

/// Generic clipped grayscale render.
fn render_gray_clip<T: PdfDocument>(
    doc: &T,
    page: i64,
    clip: &Rect,
    scale: f32,
) -> Result<RenderedPixels, PdfError> {
    doc.render_page_clip_gray(page, Some(clip), scale)
}

fn golden_dir() -> std::path::PathBuf {
    repo_root().join("resources").join("samples").join("golden-pdfs")
}

/// render_corpus.json DTO subset (mirrors render_diff.rs).
#[derive(Deserialize)]
struct RenderCorpus {
    render_scale: f64,
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    pdf: String,
    page: i64,
    bbox: [f64; 4],
    #[serde(default)]
    pixels: Option<Pixels>,
}

#[derive(Deserialize)]
struct Pixels {
    page: PixelEntry,
}

#[derive(Deserialize)]
struct PixelEntry {
    w: u32,
    h: u32,
    samples_b64: String,
}

#[test]
fn generic_open_read_matches_corpus() {
    let c = corpus();
    for (name, entry) in &c.pdfs {
        let doc = open_all::<Document>(&golden_dir().join(name))
            .unwrap_or_else(|e| panic!("open {name}: {e}"));
        for (idx_str, page_expected) in &entry.pages {
            let idx: i64 = idx_str.parse().unwrap();
            let snapshot = read_all(&doc, idx).unwrap_or_else(|e| panic!("read {name} p{idx}: {e}"));
            assert_snapshot_achievable(&snapshot, &page_expected.snapshot);
        }
    }
}

#[test]
fn generic_render_matches_wrapper_path() {
    let c = corpus();
    let scale = 2.0f32;
    for (name, entry) in &c.pdfs {
        if name == "3.pdf" {
            continue; // 18MB; full-page renders are covered for 1.pdf/2.pdf
        }
        let doc = open_all::<Document>(&golden_dir().join(name))
            .unwrap_or_else(|e| panic!("open {name}: {e}"));
        for idx_str in entry.pages.keys() {
            let idx: i64 = idx_str.parse().unwrap();
            let via_trait = render_gray(&doc, idx, scale)
                .unwrap_or_else(|e| panic!("trait render {name} p{idx}: {e}"));
            let via_wrapper = rendering_reader::render::render_page_clip_gray(&doc, idx as i32, None, scale)
                .unwrap_or_else(|e| panic!("wrapper render {name} p{idx}: {e}"));
            assert_eq!(via_trait.width, via_wrapper.width, "{name} p{idx} width");
            assert_eq!(via_trait.height, via_wrapper.height, "{name} p{idx} height");
            assert_eq!(
                via_trait.samples, via_wrapper.samples,
                "{name} p{idx} pixels"
            );
        }
    }
}

#[test]
fn generic_render_matches_fitz_pixels() {
    let corpus: RenderCorpus =
        serde_json::from_str(include_str!("render_corpus.json")).expect("parse render corpus");
    let scale = corpus.render_scale as f32;
    let mut checked = 0u32;
    for cand in corpus.candidates.iter().filter(|c| c.pixels.is_some()) {
        let px = cand.pixels.as_ref().unwrap();
        let doc = open_all::<Document>(&golden_dir().join(&cand.pdf))
            .unwrap_or_else(|e| panic!("open {}: {e}", cand.pdf));
        let clip = Rect::new(cand.bbox[0], cand.bbox[1], cand.bbox[2], cand.bbox[3]);
        let out = render_gray_clip(&doc, cand.page, &clip, scale)
            .unwrap_or_else(|e| panic!("render {} p{}: {e}", cand.pdf, cand.page));
        assert_eq!(
            (out.width, out.height),
            (px.page.w, px.page.h),
            "dims {} p{}",
            cand.pdf,
            cand.page
        );
        let expected = base64::engine::general_purpose::STANDARD
            .decode(px.page.samples_b64.as_bytes())
            .expect("decode oracle pixels");
        assert_eq!(
            out.samples, expected,
            "pixels {} p{}",
            cand.pdf,
            cand.page
        );
        checked += 1;
    }
    assert!(checked > 0, "no pixel candidates exercised");
}
