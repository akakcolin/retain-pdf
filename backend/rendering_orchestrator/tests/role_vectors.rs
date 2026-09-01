//! Role-golden parity gate (Rust side).
//!
//! Reads the SHARED `fixtures/role_vectors.json` (schema
//! `retainpdf_role_vector_v1`) — the same file the Python side
//! (`test_role_golden_vectors.py`) asserts against. Every case feeds one block
//! through the production role chain
//! `build_layout_role → build_semantic_role → build_structure_role` and asserts
//! the recorded `expected`. Any drift in the Rust implementation turns this test
//! red; the Python twin turns red on the Python side — a cross-language parity
//! lock on top of the schema parity gates.

use rendering_orchestrator::normalize::contract::{
    build_layout_role, build_semantic_role, build_structure_role,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct RoleVectorCorpus {
    schema: String,
    cases: Vec<RoleVectorCase>,
}

#[derive(Deserialize)]
struct RoleVectorCase {
    id: String,
    block: serde_json::Value,
    expected: ExpectedRoles,
}

#[derive(Deserialize)]
struct ExpectedRoles {
    layout_role: String,
    semantic_role: String,
    structure_role: String,
}

#[test]
fn role_vectors_agree_with_shared_golden() {
    let raw = include_str!("fixtures/role_vectors.json");
    let corpus: RoleVectorCorpus = serde_json::from_str(raw).expect("parse role_vectors.json");
    assert_eq!(corpus.schema, "retainpdf_role_vector_v1");
    assert_eq!(corpus.cases.len(), 13, "one case per role-decision branch");

    for case in &corpus.cases {
        let block = case.block.as_object().unwrap_or_else(|| {
            panic!("{}: block must be a JSON object", case.id)
        });
        let layout = build_layout_role(block);
        let semantic = build_semantic_role(block, &layout);
        let structure = build_structure_role(block, &layout, &semantic);

        assert_eq!(layout, case.expected.layout_role, "{}: layout_role", case.id);
        assert_eq!(semantic, case.expected.semantic_role, "{}: semantic_role", case.id);
        assert_eq!(
            structure, case.expected.structure_role,
            "{}: structure_role",
            case.id
        );
    }
}
