//! Differential replay: `build_typst_source_from_page_specs` must emit
//! byte-identical Typst source to the real production Python for the same
//! `RenderPageSpec` DTOs.
//!
//! Corpus regenerated with:
//!   .venv/bin/python3 backend/rendering_output/differential/gen_typst_emit_corpus.py

use rendering_output::dto::RenderPageSpec;
use rendering_output::emitter::build_typst_source_from_page_specs;
use serde::Deserialize;
use std::path::Path;
use std::sync::OnceLock;

static CORPUS: &str = include_str!("emit_corpus.json");

#[derive(Deserialize)]
struct Case {
    name: String,
    work_dir: String,
    background_pdf_path: String,
    page_specs: Vec<RenderPageSpec>,
    expected: String,
}

fn cases() -> &'static Vec<Case> {
    static CASES: OnceLock<Vec<Case>> = OnceLock::new();
    CASES.get_or_init(|| serde_json::from_str(CORPUS).expect("emit corpus JSON"))
}

#[test]
fn emitter_replays_production_byte_exact() {
    let failures: Vec<String> = cases()
        .iter()
        .filter_map(|case| {
            let actual = build_typst_source_from_page_specs(
                Path::new(&case.background_pdf_path),
                &case.page_specs,
                Path::new(&case.work_dir),
                "Source Han Serif SC",
            );
            if actual == case.expected {
                None
            } else {
                Some(case.name.clone())
            }
        })
        .collect();
    assert!(
        failures.is_empty(),
        "emit_diff mismatches: {}",
        failures.join(", ")
    );
}
