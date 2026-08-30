// Port of `services/document_schema/contract_v1.py` — `enrich_document_contract_v1`,
// the post-defaults contract enrichment (layout/semantic/structure roles, policy,
// provenance, content, assets). Reuses the already-ported `text_flow` and
// `toc_document` helpers from `rendering_core`.

use rendering_core::payload::toc_document::build_toc_entries;
use rendering_core::text_flow::classify_text_flow_for_role;
use rendering_core::text_flow::line_texts_from_lines;
use serde_json::{json, Map, Value};

use super::common::{normalize_bbox, normalize_tags, py_int, to_string_value, value_as_str};

const TEXT_LAYOUT_SUBTYPE_MAP: [(&str, &str); 7] = [
    ("title", "title"),
    ("heading", "heading"),
    ("body", "paragraph"),
    ("header", "header"),
    ("footer", "footer"),
    ("page_number", "page_number"),
    ("footnote", "footnote"),
];

const TEXT_ANCILLARY_LAYOUT_ROLES: [&str; 5] = ["header", "footer", "page_number", "footnote", "caption"];
const BODYLIKE_LAYOUT_ROLES: [&str; 4] = ["title", "heading", "paragraph", "list_item"];
const BODYLIKE_SEMANTIC_ROLES: [&str; 2] = ["body", "abstract"];
const STRUCTURE_ROLE_FROM_LAYOUT_ROLE: [(&str, &str); 9] = [
    ("title", "title"),
    ("heading", "heading"),
    ("paragraph", "body"),
    ("list_item", "body"),
    ("caption", "caption"),
    ("footnote", "footnote"),
    ("header", "metadata"),
    ("footer", "metadata"),
    ("page_number", "metadata"),
];

const CAPTION_TAGS: [&str; 5] = ["caption", "image_caption", "table_caption", "table_footnote", "image_footnote"];
const REFERENCE_TAGS: [&str; 2] = ["reference_entry", "reference_zone"];
const ANCILLARY_SUB_TYPES: [&str; 5] = ["caption", "figure_caption", "image_caption", "table_caption", "code_caption"];
const ACKNOWLEDGEMENT_ROLES: [&str; 3] = ["acknowledgments", "acknowledgements", "acknowledgement"];

fn role_str(value: Option<&Value>) -> String {
    value_as_str(value)
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default()
}

fn derived_role(block: &Map<String, Value>) -> String {
    let derived = block.get("derived").and_then(Value::as_object);
    role_str(derived.and_then(|d| d.get("role")))
}

fn is_caption_semantic(block: &Map<String, Value>) -> bool {
    derived_role(block) == "caption"
        || {
            let tags = normalize_tags(block.get("tags"));
            CAPTION_TAGS.iter().any(|t| tags.contains(*t))
        }
}

fn is_reference_entry_semantic(block: &Map<String, Value>) -> bool {
    derived_role(block) == "reference_entry"
        || {
            let tags = normalize_tags(block.get("tags"));
            REFERENCE_TAGS.iter().any(|t| tags.contains(*t))
        }
}

fn is_metadata_semantic(block: &Map<String, Value>) -> bool {
    role_str(block.get("sub_type")) == "metadata"
}

fn build_layout_role(block: &Map<String, Value>) -> String {
    let explicit = role_str(block.get("layout_role"));
    if !explicit.is_empty() && explicit != "unknown" {
        return explicit;
    }
    let block_type = role_str(block.get("type"));
    let sub_type = role_str(block.get("sub_type"));
    if block_type == "text" {
        if let Some((_, mapped)) = TEXT_LAYOUT_SUBTYPE_MAP.iter().find(|(s, _)| *s == sub_type) {
            return mapped.to_string();
        }
        if is_caption_semantic(block) || ANCILLARY_SUB_TYPES.contains(&sub_type.as_str()) {
            return "caption".to_string();
        }
        return "paragraph".to_string();
    }
    "unknown".to_string()
}

fn build_semantic_role(block: &Map<String, Value>, layout_role: &str) -> String {
    let explicit = role_str(block.get("semantic_role"));
    if !explicit.is_empty() && explicit != "unknown" {
        return explicit;
    }
    let role = derived_role(block);
    let tags = normalize_tags(block.get("tags"));
    if role == "abstract" || tags.contains("abstract") {
        return "abstract".to_string();
    }
    if role == "formula_number" || role_str(block.get("sub_type")) == "formula_number" {
        return "metadata".to_string();
    }
    if is_reference_entry_semantic(block) || role_str(block.get("sub_type")) == "reference_entry" {
        return "reference".to_string();
    }
    if is_metadata_semantic(block) || role == "metadata" {
        return "metadata".to_string();
    }
    if ACKNOWLEDGEMENT_ROLES.contains(&role.as_str()) {
        return "acknowledgement".to_string();
    }
    if role == "affiliation" {
        return "affiliation".to_string();
    }
    if TEXT_ANCILLARY_LAYOUT_ROLES.contains(&layout_role) {
        return "metadata".to_string();
    }
    if role_str(block.get("type")) == "text" && (layout_role == "paragraph" || layout_role == "list_item") {
        return "body".to_string();
    }
    "unknown".to_string()
}

fn build_structure_role(block: &Map<String, Value>, layout_role: &str, semantic_role: &str) -> String {
    let explicit = role_str(block.get("structure_role"));
    if !explicit.is_empty() && explicit != "unknown" {
        return explicit;
    }
    let metadata_role = block
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|m| m.get("structure_role"));
    let metadata_role = role_str(metadata_role);
    if !metadata_role.is_empty() && metadata_role != "unknown" {
        return metadata_role;
    }
    let sub_type = role_str(block.get("sub_type"));
    if sub_type == "reference_entry" || semantic_role == "reference" {
        return "reference_entry".to_string();
    }
    if semantic_role == "abstract" {
        return "body".to_string();
    }
    if semantic_role == "metadata" {
        return "metadata".to_string();
    }
    STRUCTURE_ROLE_FROM_LAYOUT_ROLE
        .iter()
        .find(|(r, _)| *r == layout_role)
        .map(|(_, s)| s.to_string())
        .unwrap_or_default()
}

fn translate_policy_reason(kind: &str, layout_role: &str, semantic_role: &str) -> (bool, String) {
    if kind != "text" {
        return (false, "non_text".to_string());
    }
    if TEXT_ANCILLARY_LAYOUT_ROLES.contains(&layout_role) {
        return (false, format!("layout_role={layout_role}"));
    }
    if matches!(semantic_role, "reference" | "metadata" | "affiliation" | "acknowledgement" | "unknown") {
        return (false, format!("semantic_role={semantic_role}"));
    }
    if BODYLIKE_LAYOUT_ROLES.contains(&layout_role) && BODYLIKE_SEMANTIC_ROLES.contains(&semantic_role) {
        if semantic_role == "abstract" {
            return (true, "semantic_role=abstract".to_string());
        }
        return (true, "main_text".to_string());
    }
    (false, "conservative_skip".to_string())
}

/// `text.splitlines()`-style explicit line split (non-empty stripped lines).
fn explicit_line_texts(text: &str) -> Vec<String> {
    rendering_core::text_flow::py_splitlines(text)
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn build_content(
    block: &Map<String, Value>,
    page_index: usize,
    order: usize,
    semantic_role: &str,
    structure_role: &str,
) -> Value {
    let existing = block
        .get("content")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let kind = existing
        .get("kind")
        .or_else(|| block.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .trim()
        .to_lowercase();
    let mut content = Map::new();
    content.insert("kind".to_string(), Value::String(if kind.is_empty() { "unknown".to_string() } else { kind.clone() }));
    let text = to_string_value(block.get("text"));
    if !text.is_empty() {
        content.insert("text".to_string(), Value::String(text.clone()));
    }
    let explicit = explicit_line_texts(&text);
    let line_texts = if explicit.len() >= 2 {
        explicit
    } else {
        let lines = block.get("lines").cloned().unwrap_or_else(|| Value::Array(vec![]));
        line_texts_from_lines(&lines)
    };
    if !line_texts.is_empty() {
        content.insert("line_texts".to_string(), Value::Array(line_texts.iter().cloned().map(Value::String).collect()));
        let lines_value = block.get("lines").cloned().unwrap_or_else(|| Value::Array(vec![]));
        content.insert(
            "text_flow".to_string(),
            Value::String(
                classify_text_flow_for_role(&text, &lines_value, semantic_role, structure_role).to_string(),
            ),
        );
    }
    if structure_role.trim().to_lowercase() == "table_of_contents" {
        let lines_value = block.get("lines").cloned().unwrap_or_else(|| Value::Array(vec![]));
        let line_texts_value = Value::Array(line_texts.iter().cloned().map(Value::String).collect());
        let toc_entries = build_toc_entries(&lines_value, &line_texts_value);
        if !toc_entries.is_empty() {
            content.insert("toc_entries".to_string(), Value::Array(toc_entries));
            content.insert("text_flow".to_string(), Value::String("preserve_lines".to_string()));
        }
    }
    let metadata = block.get("metadata").and_then(Value::as_object);
    let asset_key = metadata
        .and_then(|m| m.get("asset_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    let asset_url = metadata
        .and_then(|m| m.get("asset_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    if !asset_key.is_empty() || !asset_url.is_empty() {
        let asset_id = if !asset_key.is_empty() {
            asset_key
        } else {
            format!("page_{:03}_asset_{:04}", page_index + 1, order)
        };
        content.insert("asset_id".to_string(), Value::String(asset_id));
    }
    for (k, v) in existing.iter() {
        if !content.contains_key(k) {
            content.insert(k.clone(), v.clone());
        }
    }
    Value::Object(content)
}

fn build_policy(block: &Map<String, Value>, kind: &str, layout_role: &str, semantic_role: &str) -> Value {
    let existing = block.get("policy");
    let mut translate: Option<bool> = None;
    let mut translate_reason = String::new();
    if let Some(Value::Object(existing)) = existing {
        if let Some(b) = existing.get("translate").and_then(Value::as_bool) {
            translate = Some(b);
        }
        translate_reason = existing
            .get("translate_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if translate_reason == "missing_contract_fields" {
            translate = None;
            translate_reason = String::new();
        }
    }
    if translate.is_none() {
        let (t, r) = translate_policy_reason(kind, layout_role, semantic_role);
        translate = Some(t);
        translate_reason = r;
    } else if translate_reason.is_empty() {
        translate_reason = "explicit_policy".to_string();
    }
    json!({ "translate": translate.unwrap_or(false), "translate_reason": translate_reason })
}

fn build_provenance(block: &Map<String, Value>) -> Value {
    let explicit = block.get("provenance").and_then(Value::as_object);
    if let Some(explicit) = explicit {
        if !explicit.is_empty() {
            let raw_bbox = normalize_bbox(explicit.get("raw_bbox"));
            let mut out = Map::new();
            out.insert("provider".to_string(), Value::String(to_string_value(explicit.get("provider"))));
            out.insert("raw_label".to_string(), Value::String(to_string_value(explicit.get("raw_label"))));
            out.insert("raw_sub_type".to_string(), Value::String(to_string_value(explicit.get("raw_sub_type"))));
            out.insert("raw_bbox".to_string(), Value::Array(raw_bbox.into_iter().map(Value::from).collect()));
            out.insert("raw_path".to_string(), Value::String(to_string_value(explicit.get("raw_path"))));
            return Value::Object(out);
        }
    }
    let source = block.get("source").and_then(Value::as_object).cloned().unwrap_or_default();
    let provider = to_string_value(source.get("provider"));
    let raw_label = to_string_value(source.get("raw_type").or_else(|| source.get("raw_label")));
    let raw_sub_type = to_string_value(source.get("raw_sub_type"));
    let raw_bbox = normalize_bbox(source.get("raw_bbox").or_else(|| block.get("bbox")));
    let raw_path = to_string_value(source.get("raw_path"));
    json!({
        "provider": provider,
        "raw_label": raw_label,
        "raw_sub_type": raw_sub_type,
        "raw_bbox": raw_bbox,
        "raw_path": raw_path,
    })
}

fn collect_assets(pages: &[Value]) -> Value {
    let mut assets = Map::new();
    for page in pages {
        let Some(blocks) = page.get("blocks").and_then(Value::as_array) else {
            continue;
        };
        for block in blocks {
            let Some(content) = block.get("content").and_then(Value::as_object) else {
                continue;
            };
            let asset_id = content
                .get("asset_id")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let Some(asset_id) = asset_id else {
                continue;
            };
            let Some(metadata) = block.get("metadata").and_then(Value::as_object) else {
                continue;
            };
            let uri = metadata
                .get("asset_url")
                .or_else(|| metadata.get("asset_path"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let Some(uri) = uri else {
                continue;
            };
            if assets.contains_key(&asset_id) {
                continue;
            }
            let source = metadata
                .get("asset_kind")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            assets.insert(asset_id, json!({ "kind": "image", "uri": uri, "source": source }));
        }
    }
    Value::Object(assets)
}

/// `enrich_document_contract_v1` — mutate the normalized document in place:
/// doc_id, per-page page number, per-block reading_order/geometry/content/roles/
/// policy/provenance, then the collected assets.
pub fn enrich_document_contract_v1(document: &mut Value) {
    let doc_obj = document.as_object_mut().expect("document object");
    let doc_id = doc_obj
        .get("doc_id")
        .or_else(|| doc_obj.get("document_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    doc_obj.insert("doc_id".to_string(), Value::String(doc_id));

    let pages = document
        .get("pages")
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    let pages_list = pages.as_array().cloned().unwrap_or_default();
    for (page_index, page_value) in pages_list.iter().enumerate() {
        let Some(page) = page_value.as_object() else {
            continue;
        };
        let mut page = page.clone();
        // `int(page.get("page", page_index + 1) or (page_index + 1))` — a truthy
        // `page` wins, else `page_index + 1` (the adapter always sets page_index).
        let page_number = match page.get("page") {
            Some(Value::Number(n)) if !n.as_f64().map_or(true, |f| f == 0.0) => {
                py_int(&Value::Number(n.clone())).unwrap_or((page_index + 1) as i64)
            }
            _ => page
                .get("page_index")
                .and_then(py_int)
                .map(|n| n + 1)
                .unwrap_or((page_index + 1) as i64),
        };
        page.insert("page".to_string(), Value::from(page_number));

        let blocks = page.get("blocks").cloned().unwrap_or_else(|| Value::Array(vec![]));
        let blocks_list = blocks.as_array().cloned().unwrap_or_default();
        let mut enriched_blocks: Vec<Value> = Vec::with_capacity(blocks_list.len());
        for (order, block_value) in blocks_list.iter().enumerate() {
            let Some(block) = block_value.as_object() else {
                enriched_blocks.push(block_value.clone());
                continue;
            };
            let mut block = block.clone();

            let reading_order = block
                .get("reading_order")
                .or_else(|| block.get("order"))
                .and_then(py_int)
                .unwrap_or(order as i64);
            block.insert("reading_order".to_string(), Value::from(reading_order));

            let geometry_bbox = block
                .get("geometry")
                .and_then(Value::as_object)
                .and_then(|g| g.get("bbox"))
                .or_else(|| block.get("bbox"));
            block.insert(
                "geometry".to_string(),
                json!({ "bbox": normalize_bbox(geometry_bbox) }),
            );

            let semantic_role_in = role_str(block.get("semantic_role"));
            let structure_role_in = role_str(block.get("structure_role"));
            let content = build_content(&block, page_index, order, &semantic_role_in, &structure_role_in);
            block.insert("content".to_string(), content);

            let layout_role = build_layout_role(&block);
            let semantic_role = build_semantic_role(&block, &layout_role);
            let structure_role = build_structure_role(&block, &layout_role, &semantic_role);
            block.insert("layout_role".to_string(), Value::String(layout_role.clone()));
            block.insert("semantic_role".to_string(), Value::String(semantic_role.clone()));
            block.insert("structure_role".to_string(), Value::String(structure_role.clone()));

            let kind = block
                .get("content")
                .and_then(Value::as_object)
                .and_then(|c| c.get("kind"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let policy = build_policy(&block, kind, &layout_role, &semantic_role);
            block.insert("policy".to_string(), policy);
            block.insert("provenance".to_string(), build_provenance(&block));
            enriched_blocks.push(Value::Object(block));
        }
        page.insert("blocks".to_string(), Value::Array(enriched_blocks));
        if let Some(page_obj) = document
            .get_mut("pages")
            .and_then(Value::as_array_mut)
            .and_then(|pages| pages.get_mut(page_index))
            .and_then(Value::as_object_mut)
        {
            *page_obj = page;
        }
    }

    let pages_ref = document
        .get("pages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    document
        .as_object_mut()
        .expect("document object")
        .insert("assets".to_string(), collect_assets(&pages_ref));
}
