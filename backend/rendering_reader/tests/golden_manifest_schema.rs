// Schema validation for the translation golden-replay manifest
// (scripts/devtools/tests/translation/golden_replay/manifest.json).

mod common;

use common::repo_root;
use serde_json::Value;

const SCHEMA: &str = "translation_replay_golden_manifest_v1";
const KNOWN_CATEGORIES: [&str; 4] = [
    "protocol_shell",
    "empty_output",
    "english_residue",
    "technical_block",
];

fn manifest() -> Value {
    let path = repo_root()
        .join("backend")
        .join("scripts")
        .join("devtools")
        .join("tests")
        .join("translation")
        .join("golden_replay")
        .join("manifest.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("manifest.json must be valid JSON")
}

#[test]
fn test_manifest_schema() {
    let m = manifest();
    assert_eq!(m["schema"].as_str(), Some(SCHEMA));
}

#[test]
fn test_every_case_has_required_fields() {
    let m = manifest();
    let cases = m["cases"].as_array().expect("cases must be an array");
    assert!(!cases.is_empty(), "manifest must have cases");
    for (i, case) in cases.iter().enumerate() {
        for field in ["id", "category", "description", "expected"] {
            assert!(
                case.get(field).is_some(),
                "case {i} missing required field '{field}': {case}"
            );
        }
    }
}

#[test]
fn test_case_categories_are_known() {
    let m = manifest();
    for case in m["cases"].as_array().unwrap() {
        let category = case["category"].as_str().expect("category must be a string");
        assert!(
            KNOWN_CATEGORIES.contains(&category),
            "unknown category '{category}' in case '{}'",
            case["id"]
        );
    }
}
