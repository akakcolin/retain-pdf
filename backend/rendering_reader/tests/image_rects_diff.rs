//! Phase B2-8 image-rect differential: mupdf-rs
//! `PdfDocument::page_image_placement_rects` == fitz
//! `page.get_image_info(hashes=False)` placement bboxes (content /
//! rotation-stripped page space, `fill_image` only), recorded by
//! `rendering_writer/differential/gen_image_rects_corpus.py` on deterministic
//! synthetic PDFs (full-page large bg, small images, wide-strip tiled bg,
//! partial/fully off-page placements, Form XObject, masked SMask image,
//! multi-placements, rotated page) and the golden PDFs 1.pdf/2.pdf.
//!
//! This is the contract the bridge `read_page_image_rects` entry relies on for
//! the `page_has_large_background_image` boolean read (computed in Python from
//! these rects). The recorded `image_rects` are the RAW `get_image_info`
//! bboxes (unclipped — fitz reports off-page placements exactly like the
//! native display-list device); the consumer intersects them with the page
//! rect, which the smoke test checks via `has_large` parity.

use std::collections::HashMap;
use std::path::Path;

use base64::Engine;
use mupdf::Document;
use rendering_reader::{PdfDocument, PdfError};
use serde::Deserialize;

/// Absolute rect-coordinate tolerance (fitz records f32-based floats; mupdf-rs
/// widens the same f32 computation to f64).
const TOL: f64 = 0.01;

#[derive(Deserialize)]
struct ImageRectsCorpus {
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
    rect: [f64; 4],
    image_rects: Vec<[f64; 4]>,
    #[allow(dead_code)]
    has_large: bool,
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

#[test]
fn image_rects_replay_matches_fitz() {
    let corpus: ImageRectsCorpus =
        serde_json::from_str(include_str!("image_rects_corpus.json")).expect("parse image-rects corpus");
    assert_eq!(corpus.schema, "retainpdf_image_rects_corpus_v1");
    assert!(!corpus.cases.is_empty());

    let dir = std::env::temp_dir().join(format!("rps-img-{}", std::process::id()));
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

            let mut actual: Vec<[f64; 4]> = doc
                .page_image_placement_rects(idx)
                .into_iter()
                .map(|r| [r.x0, r.y0, r.x1, r.y1])
                .collect();
            let mut recorded: Vec<[f64; 4]> = expected.image_rects.clone();
            // Paint order can differ across the same display-list device; sort
            // both sides by (y0, x0) before comparing the SET.
            actual.sort_by(|a, b| (a[1], a[0]).partial_cmp(&(b[1], b[0])).unwrap());
            recorded.sort_by(|a, b| (a[1], a[0]).partial_cmp(&(b[1], b[0])).unwrap());

            assert_eq!(
                actual.len(),
                recorded.len(),
                "{label}: image placement count"
            );
            for (n, (a, e)) in actual.iter().zip(recorded.iter()).enumerate() {
                assert_close_rect(&format!("{label} placement {n}"), *a, *e);
            }
        }
    }
}
