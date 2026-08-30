// Port of `provider_adapters/generic_flat_ocr_adapter.py` — the skip-OCR /
// local text-layer payload → normalized document.v1 adapter (C5-N2d). A flat
// passthrough: blocks are re-indexed with derived layout/semantic/structure
// roles and a translation policy; no geometry rewrite.

use std::path::Path;

use serde_json::{json, Value};

use super::common::{normalize_bbox, to_string_value};
use super::defaults::{default_block_derived, normalize_block_continuation_hint};
use super::version::{DOCUMENT_SCHEMA_NAME, DOCUMENT_SCHEMA_VERSION};

pub const PROVIDER_GENERIC_FLAT_OCR: &str = "generic_flat_ocr";

/// `looks_like_generic_flat_ocr` — payload.provider == "generic_flat_ocr" + pages list.
pub fn looks_like_generic_flat_ocr(payload: &Value) -> bool {
    payload.is_object()
        && payload.get("provider").and_then(Value::as_str) == Some(PROVIDER_GENERIC_FLAT_OCR)
        && payload.get("pages").map_or(false, Value::is_array)
}

/// `str(block.get("type", "text") or "text")` — raw verbatim, no strip/lower.
fn raw_type(block: &Value) -> String {
    let raw = to_string_value(block.get("type"));
    if raw.is_empty() { "text".to_string() } else { raw }
}

fn raw_sub_type(block: &Value) -> String {
    let raw = to_string_value(block.get("sub_type"));
    if raw.is_empty() { "body".to_string() } else { raw }
}

/// `_block_kind` — raw type stripped + lowercased, "text" fallback.
fn block_kind(block: &Value) -> String {
    let kind = raw_type(block).trim().to_lowercase();
    if kind.is_empty() { "text".to_string() } else { kind }
}

fn block_sub_type(block: &Value) -> String {
    let sub = raw_sub_type(block).trim().to_lowercase();
    if sub.is_empty() { "body".to_string() } else { sub }
}

const TEXT_LAYOUT_ROLE_MAP: &[(&str, &str)] = &[
    ("title", "title"),
    ("heading", "heading"),
    ("abstract", "paragraph"),
    ("body", "paragraph"),
    ("header", "header"),
    ("footer", "footer"),
    ("page_number", "page_number"),
    ("footnote", "footnote"),
];

fn layout_role(block: &Value) -> &'static str {
    if block_kind(block) != "text" {
        return "unknown";
    }
    let sub = block_sub_type(block);
    TEXT_LAYOUT_ROLE_MAP
        .iter()
        .find(|(k, _)| *k == sub)
        .map(|(_, v)| *v)
        .unwrap_or("unknown")
}

fn semantic_role(block: &Value) -> &'static str {
    if block_kind(block) != "text" {
        return "unknown";
    }
    match block_sub_type(block).as_str() {
        "abstract" => "abstract",
        "header" | "footer" | "page_number" | "footnote" | "metadata" => "metadata",
        "reference_entry" => "reference",
        "body" => "body",
        _ => "unknown",
    }
}

fn structure_role(block: &Value) -> &'static str {
    if block_kind(block) != "text" {
        return "";
    }
    match block_sub_type(block).as_str() {
        "title" => "title",
        "heading" => "heading",
        "abstract" => "body",
        "body" => "body",
        "reference_entry" => "reference_entry",
        "footnote" => "footnote",
        _ => "",
    }
}

fn policy(block: &Value) -> Value {
    let kind = block_kind(block);
    let sub_type = block_sub_type(block);
    if kind != "text" {
        return json!({ "translate": false, "translate_reason": format!("provider_non_text:{kind}") });
    }
    match sub_type.as_str() {
        "abstract" => json!({ "translate": true, "translate_reason": "provider_body_whitelist:abstract" }),
        "body" => json!({ "translate": true, "translate_reason": "provider_body_whitelist:body" }),
        "heading" => json!({ "translate": true, "translate_reason": "provider_body_whitelist:heading" }),
        _ => json!({ "translate": false, "translate_reason": format!("provider_non_body:{}", if sub_type.is_empty() { "unknown" } else { &sub_type }) }),
    }
}

/// `_explicit_sub_type` — non-empty `derived.role` (stripped/lowered) else the sub_type.
fn explicit_sub_type(block: &Value) -> String {
    let role = match block.get("derived") {
        Some(Value::Object(map)) if !map.is_empty() => to_string_value(map.get("role")),
        _ => String::new(),
    };
    let role = role.trim().to_lowercase();
    if role.is_empty() { block_sub_type(block) } else { role }
}

/// `_is_front_matter_author_gap` — body block between a title and an
/// abstract/heading is front-matter author metadata, not translatable body.
fn is_front_matter_author_gap(blocks: &[Value], index: usize) -> bool {
    if block_kind(&blocks[index]) != "text" || block_sub_type(&blocks[index]) != "body" {
        return false;
    }
    let mut prev_explicit = String::new();
    for prev in blocks[..index].iter().rev() {
        let pe = explicit_sub_type(prev);
        if !pe.is_empty() {
            prev_explicit = pe;
            break;
        }
    }
    if prev_explicit != "title" {
        return false;
    }
    let mut next_explicit = String::new();
    for nxt in blocks[index + 1..].iter() {
        let ne = explicit_sub_type(nxt);
        if !ne.is_empty() {
            next_explicit = ne;
            break;
        }
    }
    matches!(next_explicit.as_str(), "abstract" | "heading")
}

fn dict_or_default(value: Option<&Value>, default: Value) -> Value {
    match value {
        Some(Value::Object(map)) if !map.is_empty() => Value::Object(map.clone()),
        _ => default,
    }
}

fn list_or_empty(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// `build_generic_flat_ocr_document` — raw skip-OCR payload → document.v1 dict.
pub fn build_generic_flat_ocr_document(
    payload: &Value,
    document_id: &str,
    source_json_path: &Path,
    provider_version: &str,
) -> Value {
    let mut pages: Vec<Value> = Vec::new();
    for (page_index, page) in payload
        .get("pages")
        .and_then(Value::as_array)
        .map(|p| p.clone())
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let raw_blocks = page
            .get("blocks")
            .and_then(Value::as_array)
            .map(|b| b.clone())
            .unwrap_or_default();
        let mut page_blocks: Vec<Value> = Vec::new();
        for (order, block) in raw_blocks.iter().enumerate() {
            let bbox = normalize_bbox(block.get("bbox"));
            let block_kind = block_kind(block);
            let raw_type = raw_type(block);
            let raw_sub_type = raw_sub_type(block);
            let text = {
                let t = to_string_value(block.get("text"));
                if t.is_empty() { String::new() } else { t }
            };
            let mut semantic = semantic_role(block);
            let mut policy = policy(block);
            if is_front_matter_author_gap(&raw_blocks, order) {
                semantic = "metadata";
                policy = json!({ "translate": false, "translate_reason": "provider_front_matter:title_abstract_gap" });
            }
            page_blocks.push(json!({
                "block_id": format!("p{:03}-b{:04}", page_index + 1, order),
                "page_index": page_index,
                "order": order,
                "type": raw_type,
                "sub_type": raw_sub_type,
                "bbox": bbox,
                "geometry": { "bbox": bbox },
                "content": { "kind": block_kind, "text": text },
                "text": text,
                "lines": list_or_empty(block.get("lines")),
                "segments": list_or_empty(block.get("segments")),
                "tags": list_or_empty(block.get("tags")),
                "derived": dict_or_default(block.get("derived"), default_block_derived()),
                "layout_role": layout_role(block),
                "semantic_role": semantic,
                "structure_role": structure_role(block),
                "policy": policy,
                "continuation_hint": normalize_block_continuation_hint(
                    block.get("continuation_hint").unwrap_or(&Value::Null),
                ),
                "metadata": dict_or_default(block.get("metadata"), json!({})),
                "source": {
                    "provider": PROVIDER_GENERIC_FLAT_OCR,
                    "raw_page_index": page_index,
                    "raw_type": raw_type,
                    "raw_sub_type": raw_sub_type,
                    "raw_bbox": bbox,
                    "raw_text_excerpt": text.chars().take(200).collect::<String>(),
                },
            }));
        }
        let unit = {
            let u = to_string_value(page.get("unit"));
            if u.is_empty() { "pt".to_string() } else { u }
        };
        pages.push(json!({
            "page_index": page_index,
            "width": page.get("width").and_then(Value::as_f64).unwrap_or(0.0),
            "height": page.get("height").and_then(Value::as_f64).unwrap_or(0.0),
            "unit": unit,
            "blocks": page_blocks,
        }));
    }

    json!({
        "schema": DOCUMENT_SCHEMA_NAME,
        "schema_version": DOCUMENT_SCHEMA_VERSION,
        "document_id": document_id,
        "doc_id": document_id,
        "source": {
            "provider": PROVIDER_GENERIC_FLAT_OCR,
            "provider_version": provider_version,
            "raw_files": { "source_json": source_json_path.display().to_string() },
        },
        "page_count": pages.len(),
        "pages": pages,
        "assets": {},
        "derived": { "notes": "Adapted from generic_flat_ocr sample payload." },
        "markers": {},
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block(type_name: &str, sub_type: &str, text: &str, bbox: Value) -> Value {
        json!({ "type": type_name, "sub_type": sub_type, "bbox": bbox, "text": text })
    }

    fn payload() -> Value {
        json!({
            "provider": "generic_flat_ocr",
            "pages": [
                {
                    "width": 595.0,
                    "height": 842.0,
                    "unit": "pt",
                    "blocks": [
                        block("text", "title", "Understanding Complex Systems", json!([60, 60, 535, 100])),
                        block("text", "body", "J. Author", json!([60, 110, 535, 130])),
                        block("text", "abstract", "We study complex systems.", json!([60, 140, 535, 180])),
                        block("text", "heading", "Introduction", json!([60, 190, 535, 220])),
                        block("text", "body", "Body paragraph text.", json!([60, 230, 535, 270])),
                        block("text", "footer", "Journal, 2026", json!([60, 810, 535, 830])),
                        block("image", "", "", json!([60, 280, 300, 420])),
                    ],
                },
            ],
        })
    }

    #[test]
    fn looks_like_generic_flat_ocr_requires_shape() {
        assert!(looks_like_generic_flat_ocr(&payload()));
        assert!(looks_like_generic_flat_ocr(&json!({"provider": "generic_flat_ocr", "pages": []})));
        assert!(!looks_like_generic_flat_ocr(&json!({"pages": []})));
        assert!(!looks_like_generic_flat_ocr(&json!({"provider": "generic_flat_ocr"})));
        assert!(!looks_like_generic_flat_ocr(&json!("nope")));
    }

    #[test]
    fn roles_and_policy_for_text_blocks() {
        let title = block("text", "title", "T", json!([0, 0, 1, 1]));
        assert_eq!(layout_role(&title), "title");
        assert_eq!(semantic_role(&title), "unknown");
        assert_eq!(structure_role(&title), "title");
        assert_eq!(policy(&title), json!({"translate": false, "translate_reason": "provider_non_body:title"}));

        let body = block("text", "body", "B", json!([0, 0, 1, 1]));
        assert_eq!(layout_role(&body), "paragraph");
        assert_eq!(semantic_role(&body), "body");
        assert_eq!(structure_role(&body), "body");
        assert_eq!(policy(&body), json!({"translate": true, "translate_reason": "provider_body_whitelist:body"}));

        let abstract_block = block("text", "abstract", "A", json!([0, 0, 1, 1]));
        assert_eq!(semantic_role(&abstract_block), "abstract");
        assert_eq!(policy(&abstract_block), json!({"translate": true, "translate_reason": "provider_body_whitelist:abstract"}));

        let footnote = block("text", "footnote", "F", json!([0, 0, 1, 1]));
        assert_eq!(semantic_role(&footnote), "metadata");
        assert_eq!(policy(&footnote), json!({"translate": false, "translate_reason": "provider_non_body:footnote"}));

        let reference = block("text", "reference_entry", "R", json!([0, 0, 1, 1]));
        assert_eq!(semantic_role(&reference), "reference");
        assert_eq!(structure_role(&reference), "reference_entry");

        let header = block("text", "header", "H", json!([0, 0, 1, 1]));
        assert_eq!(layout_role(&header), "header");
        assert_eq!(semantic_role(&header), "metadata");
    }

    #[test]
    fn non_text_block_roles_and_policy() {
        let img = block("image", "", "", json!([0, 0, 1, 1]));
        assert_eq!(block_kind(&img), "image");
        assert_eq!(layout_role(&img), "unknown");
        assert_eq!(semantic_role(&img), "unknown");
        assert_eq!(structure_role(&img), "");
        assert_eq!(policy(&img), json!({"translate": false, "translate_reason": "provider_non_text:image"}));
    }

    #[test]
    fn front_matter_author_gap_flips_body_to_metadata() {
        let doc = build_generic_flat_ocr_document(&payload(), "job-1", Path::new("/src/layout.json"), "1.0");
        let blocks = doc["pages"][0]["blocks"].as_array().unwrap();
        // "J. Author" (order 1) sits between title (order 0) and abstract (order 2).
        assert_eq!(blocks[1]["semantic_role"], "metadata");
        assert_eq!(
            blocks[1]["policy"],
            json!({"translate": false, "translate_reason": "provider_front_matter:title_abstract_gap"})
        );
        // The later body block (order 4) stays translatable body.
        assert_eq!(blocks[4]["semantic_role"], "body");
        assert_eq!(blocks[4]["policy"]["translate"], true);
    }

    #[test]
    fn build_document_full_shape() {
        let doc = build_generic_flat_ocr_document(&payload(), "job-1", Path::new("/src/layout.json"), "1.0");
        assert_eq!(doc["schema"], "normalized_document_v1");
        assert_eq!(doc["document_id"], "job-1");
        assert_eq!(doc["doc_id"], "job-1");
        assert_eq!(doc["page_count"], 1);
        assert_eq!(doc["assets"], json!({}));
        assert_eq!(doc["markers"], json!({}));
        assert_eq!(doc["source"]["provider"], "generic_flat_ocr");
        assert_eq!(doc["source"]["provider_version"], "1.0");
        assert_eq!(doc["source"]["raw_files"]["source_json"], "/src/layout.json");
        assert_eq!(doc["derived"]["notes"], "Adapted from generic_flat_ocr sample payload.");

        let page = &doc["pages"][0];
        assert_eq!(page["width"], 595.0);
        assert_eq!(page["height"], 842.0);
        assert_eq!(page["unit"], "pt");
        let blocks = page["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 7);
        assert_eq!(blocks[0]["block_id"], "p001-b0000");
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["sub_type"], "title");
        assert_eq!(blocks[0]["layout_role"], "title");
        assert_eq!(blocks[0]["bbox"], json!([60.0, 60.0, 535.0, 100.0]));
        assert_eq!(blocks[0]["geometry"]["bbox"], json!([60.0, 60.0, 535.0, 100.0]));
        assert_eq!(blocks[0]["content"]["kind"], "text");
        assert_eq!(blocks[0]["content"]["text"], "Understanding Complex Systems");
        assert_eq!(blocks[0]["source"]["provider"], "generic_flat_ocr");
        assert_eq!(blocks[0]["source"]["raw_page_index"], 0);
        assert_eq!(blocks[0]["source"]["raw_bbox"], json!([60.0, 60.0, 535.0, 100.0]));
        assert_eq!(blocks[6]["block_id"], "p001-b0006");
        assert_eq!(blocks[6]["type"], "image");
        assert_eq!(blocks[6]["content"]["kind"], "image");
    }

    #[test]
    fn empty_pages_and_block_defaults() {
        let payload = json!({"provider": "generic_flat_ocr", "pages": []});
        let doc = build_generic_flat_ocr_document(&payload, "job-2", Path::new("/src/x.json"), "v1");
        assert_eq!(doc["page_count"], 0);
        assert_eq!(doc["pages"], json!([]));

        let sparse = json!({
            "provider": "generic_flat_ocr",
            "pages": [{"blocks": [{"text": "no type"}]}],
        });
        let doc = build_generic_flat_ocr_document(&sparse, "job-3", Path::new("/src/y.json"), "v1");
        let block = &doc["pages"][0]["blocks"][0];
        assert_eq!(block["type"], "text");
        assert_eq!(block["sub_type"], "body");
        assert_eq!(block["bbox"], json!([0.0, 0.0, 0.0, 0.0]));
        assert_eq!(block["derived"], json!({"role": "", "by": "", "confidence": 0.0}));
        assert_eq!(block["continuation_hint"]["source"], "");
        assert_eq!(block["layout_role"], "paragraph");
        assert_eq!(block["semantic_role"], "body");
        assert_eq!(block["structure_role"], "body");
        assert_eq!(block["policy"], json!({"translate": true, "translate_reason": "provider_body_whitelist:body"}));
    }

    #[test]
    fn raw_text_excerpt_is_first_200_chars() {
        let long = "x".repeat(250);
        let single = json!({
            "provider": "generic_flat_ocr",
            "pages": [{"blocks": [{"type": "text", "text": long}]}],
        });
        let doc = build_generic_flat_ocr_document(&single, "job-4", Path::new("/src/z.json"), "v1");
        assert_eq!(doc["pages"][0]["blocks"][0]["source"]["raw_text_excerpt"].as_str().unwrap().len(), 200);
    }
}
