//! Differential replay: `build_typst_block` must emit byte-identical Typst
//! source to the real production Python for the same `RenderBlock` DTO.
//!
//! Corpus regenerated with:
//!   .venv/bin/python3 backend/rendering_output/differential/gen_block_corpus.py

use rendering_output::block_renderer::build_typst_block;
use rendering_output::dto::RenderBlock;
use serde::Deserialize;
use std::sync::OnceLock;

static CORPUS: &str = include_str!("block_corpus.json");

#[derive(Deserialize)]
struct Case {
    name: String,
    block: RenderBlock,
    expected: Expected,
}

#[derive(Deserialize)]
struct Expected {
    include_fill_false: String,
    include_fill_true: String,
}

fn cases() -> &'static Vec<Case> {
    static CASES: OnceLock<Vec<Case>> = OnceLock::new();
    CASES.get_or_init(|| serde_json::from_str(CORPUS).expect("block corpus JSON"))
}

#[test]
fn build_typst_block_replays_production() {
    let failures: Vec<String> = cases()
        .iter()
        .filter_map(|case| {
            let mut problems = Vec::new();
            let actual_false = build_typst_block(&case.block.block_id, &case.block, false);
            if actual_false != case.expected.include_fill_false {
                problems.push("include_fill=False".to_string());
            }
            let actual_true = build_typst_block(&case.block.block_id, &case.block, true);
            if actual_true != case.expected.include_fill_true {
                problems.push("include_fill=True".to_string());
            }
            if problems.is_empty() {
                None
            } else {
                Some(format!("case '{}': {}", case.name, problems.join(", ")))
            }
        })
        .collect();
    assert!(
        failures.is_empty(),
        "block_diff mismatches:\n{}",
        failures.join("\n")
    );
}
