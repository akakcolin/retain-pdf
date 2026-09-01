//! Parity lock: `schemas/document.v1.schema.json` is the single source of
//! truth for the document.v1 contract. The `validator.rs` constants must mirror
//! the schema exactly (sets compared order-independently), and the schema may
//! only evolve additively (`additionalProperties: true` everywhere, required
//! sets and enums must stay supersets of the locked-in baseline).

use std::collections::BTreeSet;

use serde_json::Value;

use super::validator::{
    ALLOWED_BLOCK_TYPES, ALLOWED_CONTENT_KINDS, ALLOWED_LAYOUT_ROLES, ALLOWED_SEMANTIC_ROLES,
    BLOCK_REQUIRED_KEYS, DOCUMENT_REQUIRED_KEYS, PAGE_REQUIRED_KEYS, validate_document_payload,
};
use super::version::{DOCUMENT_SCHEMA_NAME, DOCUMENT_SCHEMA_VERSION, SUPPORTED_DOCUMENT_SCHEMA_VERSIONS};

const SCHEMA_JSON: &str = include_str!("../../schemas/document.v1.schema.json");

fn schema() -> Value {
    serde_json::from_str(SCHEMA_JSON).expect("schema is valid JSON")
}

fn str_set<'a>(values: &[&'a str]) -> BTreeSet<&'a str> {
    values.iter().copied().collect()
}

fn enum_str_set(value: &Value) -> BTreeSet<&str> {
    value
        .as_array()
        .expect("enum is an array")
        .iter()
        .map(|v| v.as_str().expect("enum value is a string"))
        .collect()
}

fn required_set(value: &Value) -> BTreeSet<&str> {
    value
        .as_array()
        .expect("required is an array")
        .iter()
        .map(|v| v.as_str().expect("required key is a string"))
        .collect()
}

#[test]
fn schema_metadata_matches_version_consts() {
    let doc = schema();
    let props = &doc["properties"];
    assert_eq!(props["schema"]["const"].as_str(), Some(DOCUMENT_SCHEMA_NAME));
    assert_eq!(
        props["schema_version"]["enum"],
        serde_json::json!([DOCUMENT_SCHEMA_VERSION])
    );
    assert_eq!(
        doc["$defs"]["page"]["properties"]["unit"]["const"].as_str(),
        Some("pt")
    );
}

#[test]
fn required_keys_parity() {
    let doc = schema();
    assert_eq!(required_set(&doc["required"]), str_set(&DOCUMENT_REQUIRED_KEYS));
    assert_eq!(
        required_set(&doc["$defs"]["block"]["required"]),
        str_set(&BLOCK_REQUIRED_KEYS)
    );
    assert_eq!(
        required_set(&doc["$defs"]["page"]["required"]),
        str_set(&PAGE_REQUIRED_KEYS)
    );
}

#[test]
fn enum_parity() {
    let doc = schema();
    let block_props = &doc["$defs"]["block"]["properties"];
    assert_eq!(
        enum_str_set(&block_props["layout_role"]["enum"]),
        str_set(&ALLOWED_LAYOUT_ROLES)
    );
    assert_eq!(
        enum_str_set(&block_props["semantic_role"]["enum"]),
        str_set(&ALLOWED_SEMANTIC_ROLES)
    );
    assert_eq!(
        enum_str_set(&block_props["type"]["enum"]),
        str_set(&ALLOWED_BLOCK_TYPES)
    );
    assert_eq!(
        enum_str_set(&doc["$defs"]["content"]["properties"]["kind"]["enum"]),
        str_set(&ALLOWED_CONTENT_KINDS)
    );
}

fn assert_permissive(node: &Value, path: &str) {
    if node.get("type").and_then(Value::as_str) == Some("object") {
        // The contract may only evolve additively: an object may declare
        // `additionalProperties: true` or a permissive subschema (map pattern,
        // e.g. `assets`), but must never explicitly close with `false`.
        // A missing key defaults to `true` in JSON Schema.
        let closed = node
            .get("additionalProperties")
            .map(|value| value == &Value::Bool(false))
            .unwrap_or(false);
        assert!(!closed, "{path}: object must not close additionalProperties=false");
    }
    if let Some(properties) = node.get("properties").and_then(Value::as_object) {
        for (key, value) in properties {
            assert_permissive(value, &format!("{path}.properties.{key}"));
        }
    }
    if let Some(defs) = node.get("$defs").and_then(Value::as_object) {
        for (key, value) in defs {
            assert_permissive(value, &format!("{path}.$defs.{key}"));
        }
    }
    if let Some(items) = node.get("items").filter(|value| value.is_object()) {
        assert_permissive(items, &format!("{path}.items"));
    }
    if let Some(additional) = node
        .get("additionalProperties")
        .filter(|value| value.is_object())
    {
        assert_permissive(additional, &format!("{path}.additionalProperties"));
    }
}

#[test]
fn additional_properties_permissive() {
    assert_permissive(&schema(), "$");
}

#[test]
fn required_keys_additive_only() {
    // Adding a required key is breaking (existing documents stop validating),
    // so the schema may only ever drop required keys, never add them.
    const BASELINE_DOCUMENT_REQUIRED_KEYS: [&str; 8] = [
        "schema", "schema_version", "document_id", "source", "page_count", "pages",
        "derived", "markers",
    ];
    const BASELINE_PAGE_REQUIRED_KEYS: [&str; 5] = ["page_index", "width", "height", "unit", "blocks"];
    const BASELINE_BLOCK_REQUIRED_KEYS: [&str; 13] = [
        "block_id", "page_index", "order", "geometry", "content", "layout_role",
        "semantic_role", "structure_role", "policy", "provenance", "continuation_hint",
        "metadata", "source",
    ];

    let doc = schema();
    assert!(
        required_set(&doc["required"]).is_subset(&str_set(&BASELINE_DOCUMENT_REQUIRED_KEYS)),
        "top-level required gained keys"
    );
    assert!(
        required_set(&doc["$defs"]["block"]["required"]).is_subset(&str_set(&BASELINE_BLOCK_REQUIRED_KEYS)),
        "block required gained keys"
    );
    assert!(
        required_set(&doc["$defs"]["page"]["required"]).is_subset(&str_set(&BASELINE_PAGE_REQUIRED_KEYS)),
        "page required gained keys"
    );
}

#[test]
fn enums_additive_only() {
    // Locked-in baseline: values valid when the parity lock was introduced.
    // Shrinking any enum below this baseline is a breaking contract change and
    // must be a deliberate, reviewable decision (update baseline alongside).
    const BASELINE_LAYOUT_ROLES: [&str; 11] = [
        "title", "heading", "paragraph", "list_item", "caption", "header", "footer",
        "footnote", "page_number", "toc", "unknown",
    ];
    const BASELINE_SEMANTIC_ROLES: [&str; 8] = [
        "body", "abstract", "reference", "metadata", "affiliation", "acknowledgement",
        "table_of_contents", "unknown",
    ];
    const BASELINE_BLOCK_TYPES: [&str; 6] = ["text", "formula", "image", "table", "code", "unknown"];
    const BASELINE_CONTENT_KINDS: [&str; 6] = ["text", "image", "table", "formula", "code", "unknown"];

    let doc = schema();
    let block_props = &doc["$defs"]["block"]["properties"];
    assert!(
        str_set(&BASELINE_LAYOUT_ROLES).is_subset(&enum_str_set(&block_props["layout_role"]["enum"])),
        "layout_role enum shrank below baseline"
    );
    assert!(
        str_set(&BASELINE_SEMANTIC_ROLES).is_subset(&enum_str_set(&block_props["semantic_role"]["enum"])),
        "semantic_role enum shrank below baseline"
    );
    assert!(
        str_set(&BASELINE_BLOCK_TYPES).is_subset(&enum_str_set(&block_props["type"]["enum"])),
        "block type enum shrank below baseline"
    );
    assert!(
        str_set(&BASELINE_CONTENT_KINDS).is_subset(&enum_str_set(
            &doc["$defs"]["content"]["properties"]["kind"]["enum"]
        )),
        "content kind enum shrank below baseline"
    );
}

#[test]
fn schema_version_membership_check() {
    assert_eq!(SUPPORTED_DOCUMENT_SCHEMA_VERSIONS, &["1.1"]);
    assert!(SUPPORTED_DOCUMENT_SCHEMA_VERSIONS.contains(&DOCUMENT_SCHEMA_VERSION));

    let data = serde_json::json!({
        "schema": "normalized_document_v1",
        "schema_version": "9.9",
        "document_id": null,
        "source": null,
        "page_count": null,
        "pages": null,
        "derived": null,
        "markers": null,
    });
    let err = validate_document_payload(&data).expect_err("unsupported schema_version");
    assert!(err.contains("expected one of"), "got: {err}");
    assert!(err.contains("9.9"), "got: {err}");
}
