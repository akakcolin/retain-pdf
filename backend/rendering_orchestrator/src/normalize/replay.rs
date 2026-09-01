//! Full-contract replay: adapt the committed `generic_flat_ocr` fixture through
//! the native pipeline and assert byte-identity against the committed
//! document.v1 golden. Regenerate the golden with:
//!
//! ```text
//! CARGO_INCREMENTAL=0 cargo test --manifest-path backend/rendering_orchestrator/Cargo.toml \
//!   -- --ignored regen_generic_flat_ocr_golden --nocapture
//! ```
//!
//! Byte-identity is deterministic because the only path-derived field,
//! `source.raw_files.source_json`, is pinned to `/src/layout.json`.

use std::path::Path;

use serde_json::Value;

use super::validator::build_validation_report;
use super::{adapt_document_with_report, PROVIDER_GENERIC_FLAT_OCR};

const FIXTURE_JSON: &str = include_str!("../../tests/fixtures/generic_flat_ocr.minimal.json");
const GOLDEN_JSON: &str = include_str!("../../tests/fixtures/generic_flat_ocr.document.v1.golden.json");

fn replay() -> Value {
    let fixture: Value = serde_json::from_str(FIXTURE_JSON).expect("fixture is valid JSON");
    let (document, report) = adapt_document_with_report(
        Path::new("/src/layout.json"),
        "golden-generic-flat-ocr",
        PROVIDER_GENERIC_FLAT_OCR,
        "1.0",
        &fixture,
    )
    .expect("generic_flat_ocr fixture adapts natively");
    assert_eq!(report["detected_provider"], PROVIDER_GENERIC_FLAT_OCR);
    build_validation_report(&document).expect("replayed document is schema-valid");
    document
}

#[test]
fn generic_flat_ocr_replay_matches_golden() {
    let document = replay();
    let pretty = serde_json::to_string_pretty(&document).expect("pretty serialize");
    let golden = GOLDEN_JSON.trim_end_matches('\n');
    assert_eq!(
        pretty, golden,
        "replay diverged from committed golden; run the regen test and commit the update"
    );
}

#[test]
#[ignore = "regenerate tests/fixtures/generic_flat_ocr.document.v1.golden.json"]
fn regen_generic_flat_ocr_golden() {
    let document = replay();
    let pretty = serde_json::to_string_pretty(&document).expect("pretty serialize");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/generic_flat_ocr.document.v1.golden.json");
    std::fs::write(&path, format!("{pretty}\n")).expect("write golden");
    eprintln!("wrote {}", path.display());
}
