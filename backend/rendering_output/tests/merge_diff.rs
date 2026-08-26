//! Differential replay: `merge::overlay_pdf_pages` must reproduce the merged
//! page facts of the real production `overlay_pdf_pages_with_pikepdf` for the
//! same base + overlay PDFs.
//!
//! Corpus regenerated with:
//!   .venv/bin/python3 backend/rendering_output/differential/gen_typst_merge_corpus.py
//!
//! For each case: decode the embedded base + overlay PDFs, run the ported
//! merge (overlay each overlay page onto its mapped source page, save
//! deterministically), reopen the result and compare per-page words/ink_ratio
//! to the fitz-computed corpus facts (word count within 10% rel, ink within
//! ±0.02 abs).

use std::fs;
use std::sync::OnceLock;

use mupdf::Document;
use rendering_output::merge::overlay_pdf_pages;
use serde::Deserialize;

mod common;
use common::{assert_page_facts, decode, measure_pages, PageFact};

static CORPUS: &str = include_str!("merge_corpus.json");

#[derive(Deserialize)]
struct MergeCorpus {
    schema: String,
    render_scale: f64,
    ink_threshold: u8,
    cases: Vec<MergeCase>,
}

#[derive(Deserialize)]
struct MergeCase {
    name: String,
    base_pdf_b64: String,
    overlay_pdf_b64: String,
    source_page_indices: Vec<i32>,
    expected: MergeExpected,
}

#[derive(Deserialize)]
struct MergeExpected {
    pages: Vec<PageFact>,
}

fn corpus() -> &'static MergeCorpus {
    static CASES: OnceLock<MergeCorpus> = OnceLock::new();
    CASES.get_or_init(|| serde_json::from_str(CORPUS).expect("merge corpus JSON"))
}

fn fresh_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rp_merge_diff_{}_{}_{}", std::process::id(), unique, label));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn merge_replay_matches_production() {
    let data = corpus();
    assert_eq!(data.schema, "retainpdf_merge_corpus_v1");
    assert!(!data.cases.is_empty());
    let scale = data.render_scale as f32;
    let threshold = data.ink_threshold;

    for case in &data.cases {
        let dir = fresh_dir(&case.name);
        let base_path = dir.join("base.pdf");
        let overlay_path = dir.join("overlay.pdf");
        let out_path = dir.join("out.pdf");
        fs::write(&base_path, decode(&case.base_pdf_b64))
            .unwrap_or_else(|e| panic!("{} write base: {e}", case.name));
        fs::write(&overlay_path, decode(&case.overlay_pdf_b64))
            .unwrap_or_else(|e| panic!("{} write overlay: {e}", case.name));

        let merged = overlay_pdf_pages(
            &base_path,
            &overlay_path,
            &out_path,
            Some(&case.source_page_indices),
        )
        .unwrap_or_else(|e| panic!("{} merge: {e}", case.name));
        assert_eq!(
            merged,
            case.source_page_indices.len(),
            "{} pages_merged",
            case.name
        );

        let out_doc = Document::open(out_path.as_path())
            .unwrap_or_else(|e| panic!("{} reopen output: {e}", case.name));
        assert_eq!(
            out_doc.page_count().unwrap_or(0) as usize,
            case.expected.pages.len(),
            "{} output page_count",
            case.name
        );
        let facts = measure_pages(&out_doc, case.expected.pages.len(), scale, threshold);
        assert_page_facts(&facts, &case.expected.pages, &case.name);
    }
}
