// Golden replay for the real golden sample PDFs.
//
// Opens resources/samples/golden-pdfs/{1,2}.pdf with the mupdf-rs reader and
// asserts the reader extracts the same achievable page facts as fitz/PyMuPDF
// (recorded in golden_pdf_corpus.json) and that those facts flow through
// rendering_core to matching geometry / vector profile fields. text-layer /
// image-background / classification fields depend on texttrace and image-bbox
// data that mupdf-rs does not expose; those are Phase 5.

mod common;

use common::*;
use rendering_core::profile_build::build_render_page_profile;
use rendering_reader::reader;

fn golden_dir() -> std::path::PathBuf {
    repo_root()
        .join("resources")
        .join("samples")
        .join("golden-pdfs")
}

fn open_golden(name: &str) -> mupdf::Document {
    reader::open(&golden_dir().join(name)).unwrap_or_else(|e| panic!("open {name}: {e}"))
}

#[test]
fn test_page_count_and_smoke() {
    let c = corpus();
    for (name, entry) in &c.pdfs {
        let doc = open_golden(name);
        assert_eq!(
            reader::page_count(&doc).unwrap(),
            entry.page_count,
            "page count for {name}"
        );
    }
    // 3.pdf is large (18MB); smoke only its page count, no per-page processing.
    let doc3 = open_golden("3.pdf");
    assert!(reader::page_count(&doc3).unwrap() >= 1, "3.pdf opens");
}

#[test]
fn test_reader_achievable_snapshot_parity() {
    let c = corpus();
    for (name, entry) in &c.pdfs {
        let doc = open_golden(name);
        for (idx_str, page_expected) in &entry.pages {
            let idx: i64 = idx_str.parse().unwrap();
            let snapshot = reader::read_page_snapshot(&doc, idx)
                .unwrap_or_else(|e| panic!("read {name} page {idx}: {e}"));
            assert_snapshot_achievable(&snapshot, &page_expected.snapshot);
        }
    }
}

#[test]
fn test_geometry_and_vector_profile_parity() {
    let c = corpus();
    let threshold = c.background_threshold;
    for (name, entry) in &c.pdfs {
        let doc = open_golden(name);
        for (idx_str, page_expected) in &entry.pages {
            let idx: i64 = idx_str.parse().unwrap();
            let snapshot = reader::read_page_snapshot(&doc, idx)
                .unwrap_or_else(|e| panic!("read {name} page {idx}: {e}"));
            let profile = build_render_page_profile(&snapshot, &[], threshold);
            assert_close_geometry(&profile.geometry, &page_expected.expected_profile.geometry);
            assert_close_vector_layer(
                &profile.vector_layer,
                &page_expected.expected_profile.vector_layer,
            );
        }
    }
}

/// Image-background + classification parity (Inc 3). Once `page_snapshot` fills
/// `image_rects` with the aggregate placement set, `build_render_page_profile`
/// reproduces the reference `image_background` (has_large_background /
/// coverage_ratio) and the derived `kind`. Classification parity is the key gate
/// for the Inc 4 routing divergence ledger. `kind` from text-layer traces is a
/// documented divergence surface on "hidden text and <20 words" pages only; the
/// golden pages (editable-paper / pseudo-editable) carry enough visible text
/// that the empty-text_traces fallback keeps the classification identical.
#[test]
fn test_image_background_and_kind_parity() {
    let c = corpus();
    let threshold = c.background_threshold;
    for (name, entry) in &c.pdfs {
        let doc = open_golden(name);
        for (idx_str, page_expected) in &entry.pages {
            let idx: i64 = idx_str.parse().unwrap();
            let snapshot = reader::read_page_snapshot(&doc, idx)
                .unwrap_or_else(|e| panic!("read {name} page {idx}: {e}"));
            let profile = build_render_page_profile(&snapshot, &[], threshold);
            assert_close_image_background(
                &profile.image_background,
                &page_expected.expected_profile.image_background,
            );
            assert_eq!(
                profile.kind.as_str(),
                page_expected.expected_profile.kind,
                "{name} p{idx}: kind"
            );
        }
    }
}
