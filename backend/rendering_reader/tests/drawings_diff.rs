//! Phase B2-7 drawings differential: mupdf-rs `PdfDocument::page_drawings`
//! per-drawing type/rect/width vs fitz `page.get_cdrawings()`, recorded by
//! `rendering_writer/differential/gen_drawings_corpus.py` on deterministic
//! synthetic PDFs (RGB/gray/cmyk fills, cm-scaled strokes, fill+stroke merges,
//! bezier, multi-drawing order, thin strokes) and the golden PDFs 1.pdf/2.pdf.
//! This is the contract the bridge `read_page_drawing_count` +
//! `collect_vector_text_rects` entries rely on for the vector_profile /
//! vector_text drawing reads.
//!
//! `width` (`line_width * path_factor`) is `None` for fills on both sides;
//! `count` mirrors `page_drawing_count` (same `drawings()` source).

use std::collections::HashMap;
use std::path::Path;

use base64::Engine;
use mupdf::Document;
use rendering_reader::{PdfDocument, PdfError};
use serde::Deserialize;

/// Absolute rect-coordinate / stroke-width tolerance (fitz records f32-based
/// floats; mupdf-rs widens the same f32 computation to f64).
const TOL: f64 = 0.01;

#[derive(Deserialize)]
struct DrawingsCorpus {
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
    drawings: Vec<RecordedDrawing>,
    count: usize,
}

#[derive(Deserialize)]
struct RecordedDrawing {
    #[serde(rename = "type")]
    drawing_type: String,
    rect: [f64; 4],
    width: Option<f32>,
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
fn drawings_replay_matches_fitz() {
    let corpus: DrawingsCorpus =
        serde_json::from_str(include_str!("drawings_corpus.json")).expect("parse drawings corpus");
    assert_eq!(corpus.schema, "retainpdf_drawings_corpus_v1");
    assert!(!corpus.cases.is_empty());

    let dir = std::env::temp_dir().join(format!("rps-drw-{}", std::process::id()));
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

            let count = doc
                .page_drawing_count(idx)
                .unwrap_or_else(|e| panic!("{label} page_drawing_count: {e}")) as usize;
            assert_eq!(count, expected.count, "{label}: drawing count");

            let drawings = doc
                .page_drawings(idx)
                .unwrap_or_else(|e| panic!("{label} page_drawings: {e}"));
            assert_eq!(
                drawings.len(),
                expected.count,
                "{label}: page_drawings length"
            );

            for (n, (actual, recorded)) in drawings.iter().zip(expected.drawings.iter()).enumerate() {
                let dlabel = format!("{label} drawing {n}");
                assert_eq!(
                    actual.drawing_type.as_fitz_str(),
                    recorded.drawing_type,
                    "{dlabel}: type"
                );
                assert_close_rect(
                    &dlabel,
                    [actual.rect.x0, actual.rect.y0, actual.rect.x1, actual.rect.y1],
                    recorded.rect,
                );
                match (actual.width, recorded.width) {
                    (Some(a), Some(e)) => assert!(
                        (a - e).abs() <= TOL as f32,
                        "{dlabel}: width actual={a}, expected={e}"
                    ),
                    (None, None) => {}
                    (a, e) => panic!("{dlabel}: width Some/None mismatch {a:?} vs {e:?}"),
                }
            }
        }
    }
}
