//! Phase B2-1 page-sizes differential: mupdf-rs `PdfDocument` page geometry
//! (`page_rect` / `page_cropbox` / `page_rotation`) vs fitz `page.rect` /
//! `page.cropbox` / `page.rotation`, recorded by
//! `differential/gen_page_sizes_corpus.py` on deterministic synthetic PDFs
//! (varied sizes, rotations, cropboxes). This is the contract the bridge
//! `read_page_sizes` entry relies on for the layout `build_render_page_specs`
//! size lookup.

use std::collections::HashMap;
use std::path::Path;

use base64::Engine;
use mupdf::Document;
use rendering_reader::{PdfDocument, PdfError};
use serde::Deserialize;

/// Absolute rect-coordinate tolerance. fitz records f32-based floats; mupdf-rs
/// widens the same f32 computation to f64, so values agree well within 0.01.
const TOL: f64 = 0.01;

#[derive(Deserialize)]
struct PageSizesCorpus {
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
    rect: [f64; 4],
    cropbox: [f64; 4],
    rotation: i64,
}

/// Generic open through the `PdfDocument` trait (never names a mupdf type in
/// the signature, mirroring Phase B2 call sites).
fn open_all<T: PdfDocument>(path: &Path) -> Result<T, PdfError> {
    T::open(path)
}

fn assert_close(label: &str, actual: [f64; 4], expected: [f64; 4]) {
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!(
            (a - e).abs() <= TOL,
            "{label}: coord {i} actual={a}, expected={e}, tol={TOL}"
        );
    }
}

#[test]
fn page_sizes_replay_matches_fitz() {
    let corpus: PageSizesCorpus =
        serde_json::from_str(include_str!("page_sizes_corpus.json")).expect("parse page-sizes corpus");
    assert_eq!(corpus.schema, "retainpdf_page_sizes_corpus_v1");
    assert!(!corpus.cases.is_empty());

    let dir = std::env::temp_dir().join(format!("rps-psz-{}", std::process::id()));
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
            let rect = doc
                .page_rect(idx)
                .unwrap_or_else(|e| panic!("{label} page_rect: {e}"));
            assert_close(&label, [rect.x0, rect.y0, rect.x1, rect.y1], expected.rect);
            // mupdf-rs `PdfPage::crop_box()` returns the rotation-applied bounds
            // re-positioned from the crop-box origin (dimensions equal to
            // `page_rect`), so it matches fitz `page.cropbox` only on unrotated
            // pages; assert parity there and rely on `page_rect` + rotation for
            // the rotated cases.
            if expected.rotation == 0 {
                let cropbox = doc
                    .page_cropbox(idx)
                    .unwrap_or_else(|e| panic!("{label} page_cropbox: {e}"));
                assert_close(
                    &label,
                    [cropbox.x0, cropbox.y0, cropbox.x1, cropbox.y1],
                    expected.cropbox,
                );
            }
            assert_eq!(
                doc.page_rotation(idx)
                    .unwrap_or_else(|e| panic!("{label} page_rotation: {e}")),
                expected.rotation,
                "{label}: rotation"
            );
        }
    }
}
