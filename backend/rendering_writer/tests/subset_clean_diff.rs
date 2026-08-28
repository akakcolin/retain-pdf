//! Native font-subset + clean-write differential.
//!
//! Runs `save::subset_and_clean` — mupdf `pdf_subset_fonts` + clean write via
//! the C shim in `c/save_clean.c` (fitz's `subset_fonts`+`tobytes` no longer
//! involved) — over the corpus `save_cases` and asserts:
//!   1. the output is a parseable PDF with the expected page count and page
//!      facts (same WORD/INK tolerances as the rest of the corpus diff), and
//!   2. subsetting actually happens: for every case where the fitz reference
//!      save shrank the input below 95% (i.e. the source really carried
//!      embedded subsettable fonts), the mupdf output must also shrink it.

use std::fs;

use mupdf::Document;
use rendering_writer::save::subset_and_clean;

mod common;
use common::{assert_page_facts, corpus, decode, measure_pages};

/// Same relative-size tolerance as `write_diff.rs`: mupdf's object graph can
/// differ slightly from fitz's; 1.15 catches a compaction regression.
const SAVE_SIZE_K: f64 = 1.15;
/// The generator's own observed shrink from fitz subset+save, below which we
/// consider the case to have had real subsettable fonts.
const REF_SHRINK_RATIO: f64 = 0.95;

fn tempfile_dir(name: &str) -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("rpsubset-{}-{}", name, std::process::id()));
    fs::create_dir_all(&base).expect("create temp dir");
    base
}

#[test]
fn subset_clean_matches_reference_and_subsets() {
    let corpus = corpus();
    assert!(!corpus.save_cases.is_empty());
    let scale = corpus.render_scale as f32;
    let threshold = corpus.ink_threshold;

    for case in &corpus.save_cases {
        let input = decode(&case.input_pdf_b64);
        let out = subset_and_clean(&input)
            .unwrap_or_else(|e| panic!("{} subset_and_clean: {e}", case.name));
        assert!(
            out.starts_with(b"%PDF-"),
            "{} output is not a valid PDF (bad header)",
            case.name
        );

        let dir = tempfile_dir(&format!("subset-{}", case.name));
        let out_path = dir.join("out.pdf");
        fs::write(&out_path, &out).expect("write output");
        let out_doc = Document::open(out_path.as_path())
            .unwrap_or_else(|e| panic!("{} reopen output: {e}", case.name));
        let out_pages = out_doc.page_count().unwrap_or(0) as usize;
        assert_eq!(
            out_pages,
            case.expected.output_page_count,
            "{} output_page_count",
            case.name
        );
        let facts = measure_pages(&out_doc, out_pages, scale, threshold);
        assert_page_facts(
            &facts,
            &case.expected.output.pages,
            &format!("{} output", case.name),
        );

        let size_ratio = out.len() as f64 / case.expected.output_bytes as f64;
        assert!(
            size_ratio <= SAVE_SIZE_K,
            "{} native subset output {}/{} = {:.3} > k={SAVE_SIZE_K}",
            case.name,
            out.len(),
            case.expected.output_bytes,
            size_ratio,
        );

        let ref_shrank = (case.expected.output_bytes as f64) < REF_SHRINK_RATIO * input.len() as f64;
        if ref_shrank {
            assert!(
                out.len() < input.len(),
                "{} subset did not shrink input: out={} input={}",
                case.name,
                out.len(),
                input.len(),
            );
        }
    }
}

#[test]
fn subset_clean_errors_on_corrupt_input() {
    let err = subset_and_clean(b"this is not a pdf at all").unwrap_err();
    assert!(!err.is_empty(), "expected a mupdf error string");
}
