//! Differential replay: the whole-book overlay/background source builders must
//! emit byte-identical Typst source to production Python.
//!
//! Corpus regenerated with:
//!   .venv/bin/python3 backend/rendering_output/differential/gen_typst_book_corpus.py

use rendering_output::block_config::TYPST_DEFAULT_FONT_FAMILY;
use rendering_output::dto::RenderBlock;
use rendering_output::source_builder::{
    build_typst_book_background_source, build_typst_book_overlay_source,
};
use serde::Deserialize;
use std::path::Path;
use std::sync::OnceLock;

static CORPUS: &str = include_str!("book_corpus.json");

#[derive(Deserialize)]
struct Case {
    name: String,
    kind: String,
    font_family: String,
    #[serde(default)]
    include_cover_rect: bool,
    #[serde(default)]
    work_dir: String,
    #[serde(default)]
    source_pdf_path: String,
    page_specs: Vec<Vec<RenderBlock>>,
    dims: Vec<Vec<f64>>,
    expected: String,
}

fn cases() -> &'static Vec<Case> {
    static CASES: OnceLock<Vec<Case>> = OnceLock::new();
    CASES.get_or_init(|| serde_json::from_str(CORPUS).expect("book corpus JSON"))
}

#[test]
fn book_builders_replay_production_byte_exact() {
    let failures: Vec<String> = cases()
        .iter()
        .filter_map(|case| {
            let actual = match case.kind.as_str() {
                "overlay" => {
                    let specs: Vec<(f64, f64, Vec<RenderBlock>)> = case
                        .page_specs
                        .iter()
                        .zip(case.dims.iter())
                        .map(|(blocks, dims)| (dims[0], dims[1], blocks.clone()))
                        .collect();
                    build_typst_book_overlay_source(&specs, &case.font_family, case.include_cover_rect)
                }
                "background" => {
                    let specs: Vec<(i64, f64, f64, Vec<RenderBlock>)> = case
                        .page_specs
                        .iter()
                        .zip(case.dims.iter())
                        .map(|(blocks, dims)| (dims[0] as i64, dims[1], dims[2], blocks.clone()))
                        .collect();
                    build_typst_book_background_source(
                        Path::new(&case.source_pdf_path),
                        &specs,
                        Path::new(&case.work_dir),
                        &case.font_family,
                    )
                }
                other => panic!("unknown kind {other}"),
            };
            if actual == case.expected {
                None
            } else {
                Some(case.name.clone())
            }
        })
        .collect();
    assert!(
        failures.is_empty(),
        "book_diff mismatches: {}",
        failures.join(", ")
    );
}

#[test]
fn font_family_constant_matches_production_default() {
    assert_eq!(TYPST_DEFAULT_FONT_FAMILY, "Source Han Serif SC");
}
