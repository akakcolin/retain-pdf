//! Phase B2-9 text-read differential: mupdf-rs
//! `PdfDocument::{page_text_spans, page_text_blocks, page_math_rects,
//! page_span_heights}` == fitz `get_text("dict"/"blocks")` spans/blocks,
//! math-font span rects, and non-math span heights, recorded by
//! `rendering_writer/differential/gen_text_read_corpus.py` on deterministic
//! synthetic PDFs (simple, multi-font/color/size, styled bold/italic, math font,
//! rotated page) and every page of the golden PDFs 1.pdf/2.pdf.
//!
//! Both sides build the SAME mupdf stext page with the SAME flags
//! (PRESERVE_LIGATURES | PRESERVE_IMAGES | PRESERVE_WHITESPACE) in the
//! rotation-stripped content space, so ordering is positional (blocks -> lines
//! -> spans): rects within 0.01 pt, span/block text exact, math rects within
//! 0.01 pt, heights within 0.05 pt (derived from f32-quad-union rects widened
//! to f64 — the rect tolerance carries over).

use std::collections::HashMap;
use std::path::Path;

use base64::Engine;
use mupdf::Document;
use rendering_reader::{PdfDocument, PdfError};
use serde::Deserialize;
use serde_json::Value;

/// Absolute rect-coordinate tolerance (fitz records f32-based floats; mupdf-rs
/// widens the same f32 computation to f64).
const TOL: f64 = 0.01;
/// Height tolerance: heights are `y1 - y0` of the same spans whose rects carry
/// `TOL`, so the derived value can differ by up to 2 * TOL.
const TOL_HEIGHT: f64 = 0.05;

#[derive(Deserialize)]
struct TextReadCorpus {
    schema: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    pdf_b64: String,
    pages: HashMap<String, PageFacts>,
}

#[derive(Deserialize)]
struct PageFacts {
    #[allow(dead_code)]
    page_rect: [f64; 4],
    text_spans: Vec<TextEntry>,
    text_blocks: Vec<TextEntry>,
    math_rects: Vec<[f64; 4]>,
    span_heights: Vec<f64>,
}

/// A recorded `[x0, y0, x1, y1, text]` entry — deserialized from the flat
/// JSON array (the last element is the text string).
struct TextEntry {
    rect: [f64; 4],
    text: String,
}

impl<'de> Deserialize<'de> for TextEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw: Vec<Value> = Vec::deserialize(deserializer)?;
        let mut it = raw.into_iter();
        let mut rect = [0.0f64; 4];
        for slot in rect.iter_mut() {
            *slot = it
                .next()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
        }
        let text = it
            .next()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        Ok(TextEntry { rect, text })
    }
}

fn open_all<T: PdfDocument>(path: &Path) -> Result<T, PdfError> {
    T::open(path)
}

fn assert_close_rect(label: &str, actual: [f64; 4], expected: [f64; 4]) {
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(
            (a - e).abs() <= TOL,
            "{label}: coord {i} actual={a}, expected={e}, tol={TOL}"
        );
    }
}

fn assert_close_rects(label: &str, actual: &[[f64; 4]], expected: &[[f64; 4]]) {
    assert_eq!(actual.len(), expected.len(), "{label}: count");
    for (n, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_close_rect(&format!("{label} entry {n}"), *a, *e);
    }
}

fn assert_texts<A>(label: &str, actual: &[(A, String)], expected: &[TextEntry]) {
    assert_eq!(actual.len(), expected.len(), "{label}: count");
    for (n, ((_, a_text), e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(a_text, &e.text, "{label} text {n}");
    }
}

#[test]
fn text_read_replay_matches_fitz() {
    let corpus: TextReadCorpus =
        serde_json::from_str(include_str!("text_read_corpus.json")).expect("parse text-read corpus");
    assert_eq!(corpus.schema, "retainpdf_text_read_corpus_v1");
    assert!(!corpus.cases.is_empty());

    let dir = std::env::temp_dir().join(format!("rps-txt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");

    for case in &corpus.cases {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(case.pdf_b64.as_bytes())
            .unwrap_or_else(|e| panic!("{} decode: {e}", case.name));
        let path = dir.join(format!("{}.pdf", case.name));
        std::fs::write(&path, bytes).expect("write case pdf");
        let doc = open_all::<Document>(&path).unwrap_or_else(|e| panic!("{} open: {e}", case.name));

        assert_eq!(
            doc.page_count().unwrap_or_else(|e| panic!("{} page_count: {e}", case.name)) as usize,
            case.pages.len(),
            "{}: page count",
            case.name
        );

        for (idx_str, expected) in &case.pages {
            let idx: i64 = idx_str.parse().unwrap();
            let label = format!("{} p{idx}", case.name);

            let spans = doc.page_text_spans(idx);
            let span_rects: Vec<[f64; 4]> = spans.iter().map(|(r, _)| [r.x0, r.y0, r.x1, r.y1]).collect();
            let expected_span_rects: Vec<[f64; 4]> = expected.text_spans.iter().map(|e| e.rect).collect();
            assert_close_rects(&format!("{label} spans"), &span_rects, &expected_span_rects);
            assert_texts(&format!("{label} spans"), &spans, &expected.text_spans);

            let blocks = doc.page_text_blocks(idx);
            let block_rects: Vec<[f64; 4]> = blocks.iter().map(|(r, _)| [r.x0, r.y0, r.x1, r.y1]).collect();
            let expected_block_rects: Vec<[f64; 4]> = expected.text_blocks.iter().map(|e| e.rect).collect();
            assert_close_rects(&format!("{label} blocks"), &block_rects, &expected_block_rects);
            assert_texts(&format!("{label} blocks"), &blocks, &expected.text_blocks);

            let math_actual: Vec<[f64; 4]> = doc
                .page_math_rects(idx)
                .iter()
                .map(|r| [r.x0, r.y0, r.x1, r.y1])
                .collect();
            assert_close_rects(&format!("{label} math"), &math_actual, &expected.math_rects);

            let heights_actual = doc.page_span_heights(idx);
            assert_eq!(
                heights_actual.len(),
                expected.span_heights.len(),
                "{label}: height count"
            );
            for (n, (a, e)) in heights_actual.iter().zip(expected.span_heights.iter()).enumerate() {
                assert!(
                    (a - e).abs() <= TOL_HEIGHT,
                    "{label}: height {n} actual={a}, expected={e}, tol={TOL_HEIGHT}"
                );
            }
        }
    }
}
