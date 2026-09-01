// Port of `services/document_schema/validator.py` — the structural document.v1
// validation that gates `build_validation_report`. Python raises on the first
// violation (the worker then fails); the native port returns an error string
// identically. Only the valid path produces the `{valid: true, ...}` report.

use serde_json::Value;

use super::version::{
    DOCUMENT_SCHEMA_NAME, DOCUMENT_SCHEMA_VERSION, SUPPORTED_DOCUMENT_SCHEMA_VERSIONS,
};

type Result<T> = std::result::Result<T, String>;

fn fail(path: &str, message: &str) -> Result<()> {
    Err(format!("{path}: {message}"))
}

fn expect_type(path: &str, value: &Value, kind: &str) -> Result<()> {
    let ok = match kind {
        "dict" => value.is_object(),
        "list" => value.is_array(),
        "str" => value.is_string(),
        "int" => value.is_i64() || value.is_u64(),
        "bool" => value.is_boolean(),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        fail(path, &format!("expected {kind}, got {}", value_type_name(value)))
    }
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "float",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn expect_number(path: &str, value: &Value) -> Result<()> {
    if value.is_number() {
        Ok(())
    } else {
        fail(path, &format!("expected number, got {}", value_type_name(value)))
    }
}

fn validate_bbox(path: &str, bbox: &Value) -> Result<()> {
    expect_type(path, bbox, "list")?;
    let arr = bbox.as_array().unwrap();
    if arr.len() != 4 {
        return fail(path, &format!("expected 4 numbers, got {}", arr.len()));
    }
    for (index, value) in arr.iter().enumerate() {
        expect_number(&format!("{path}[{index}]"), value)?;
    }
    Ok(())
}

fn validate_geometry(path: &str, geometry: &Value) -> Result<()> {
    expect_type(path, geometry, "dict")?;
    let obj = geometry.as_object().unwrap();
    if !obj.contains_key("bbox") {
        return fail(path, "missing key 'bbox'");
    }
    validate_bbox(&format!("{path}.bbox"), &obj["bbox"])
}

fn validate_content(path: &str, content: &Value) -> Result<()> {
    expect_type(path, content, "dict")?;
    let obj = content.as_object().unwrap();
    if !obj.contains_key("kind") {
        return fail(path, "missing key 'kind'");
    }
    expect_type(&format!("{path}.kind"), &obj["kind"], "str")?;
    let kind = obj["kind"].as_str().unwrap();
    if !ALLOWED_CONTENT_KINDS.contains(&kind) {
        return fail(&format!("{path}.kind"), &format!("unexpected content kind '{kind}'"));
    }
    if let Some(text) = obj.get("text") {
        expect_type(&format!("{path}.text"), text, "str")?;
    }
    if let Some(line_texts) = obj.get("line_texts") {
        expect_type(&format!("{path}.line_texts"), line_texts, "list")?;
        for (index, line) in line_texts.as_array().unwrap().iter().enumerate() {
            expect_type(&format!("{path}.line_texts[{index}]"), line, "str")?;
        }
    }
    if let Some(text_flow) = obj.get("text_flow") {
        expect_type(&format!("{path}.text_flow"), text_flow, "str")?;
        let flow = text_flow.as_str().unwrap();
        if flow != "flow" && flow != "preserve_lines" {
            return fail(&format!("{path}.text_flow"), &format!("unexpected text flow '{flow}'"));
        }
    }
    if let Some(asset_id) = obj.get("asset_id") {
        expect_type(&format!("{path}.asset_id"), asset_id, "str")?;
        if asset_id.as_str().unwrap().is_empty() {
            return fail(&format!("{path}.asset_id"), "expected non-empty string");
        }
    }
    if let Some(toc_entries) = obj.get("toc_entries") {
        expect_type(&format!("{path}.toc_entries"), toc_entries, "list")?;
        for (index, entry) in toc_entries.as_array().unwrap().iter().enumerate() {
            expect_type(&format!("{path}.toc_entries[{index}]"), entry, "dict")?;
            let entry_obj = entry.as_object().unwrap();
            for key in ["title", "page_label"] {
                if !entry_obj.contains_key(key) {
                    return fail(&format!("{path}.toc_entries[{index}]"), &format!("missing key '{key}'"));
                }
                expect_type(&format!("{path}.toc_entries[{index}].{key}"), &entry_obj[key], "str")?;
            }
            if let Some(number) = entry_obj.get("number") {
                expect_type(&format!("{path}.toc_entries[{index}].number"), number, "str")?;
            }
            if let Some(level) = entry_obj.get("level") {
                expect_type(&format!("{path}.toc_entries[{index}].level"), level, "int")?;
            }
            if let Some(line_index) = entry_obj.get("line_index") {
                expect_type(&format!("{path}.toc_entries[{index}].line_index"), line_index, "int")?;
            }
            if let Some(bbox) = entry_obj.get("bbox") {
                validate_bbox(&format!("{path}.toc_entries[{index}].bbox"), bbox)?;
            }
        }
    }
    Ok(())
}

fn validate_policy(path: &str, policy: &Value) -> Result<()> {
    expect_type(path, policy, "dict")?;
    let obj = policy.as_object().unwrap();
    for key in ["translate", "translate_reason"] {
        if !obj.contains_key(key) {
            return fail(path, &format!("missing key '{key}'"));
        }
    }
    expect_type(&format!("{path}.translate"), &obj["translate"], "bool")?;
    expect_type(&format!("{path}.translate_reason"), &obj["translate_reason"], "str")
}

fn role_string(path: &str, value: &Value) -> Result<String> {
    expect_type(path, value, "str")?;
    Ok(value.as_str().unwrap().trim().to_lowercase())
}

fn validate_provenance(path: &str, provenance: &Value) -> Result<()> {
    expect_type(path, provenance, "dict")?;
    let obj = provenance.as_object().unwrap();
    for key in ["provider", "raw_label", "raw_sub_type", "raw_bbox", "raw_path"] {
        if !obj.contains_key(key) {
            return fail(path, &format!("missing key '{key}'"));
        }
    }
    expect_type(&format!("{path}.provider"), &obj["provider"], "str")?;
    if obj["provider"].as_str().unwrap().is_empty() {
        return fail(&format!("{path}.provider"), "expected non-empty string");
    }
    expect_type(&format!("{path}.raw_label"), &obj["raw_label"], "str")?;
    expect_type(&format!("{path}.raw_sub_type"), &obj["raw_sub_type"], "str")?;
    validate_bbox(&format!("{path}.raw_bbox"), &obj["raw_bbox"])?;
    expect_type(&format!("{path}.raw_path"), &obj["raw_path"], "str")
}

fn validate_assets(path: &str, assets: &Value) -> Result<()> {
    expect_type(path, assets, "dict")?;
    for (key, asset) in assets.as_object().unwrap() {
        expect_type(&format!("{path}.{key}"), asset, "dict")?;
        let obj = asset.as_object().unwrap();
        for required_key in ["kind", "uri", "source"] {
            if !obj.contains_key(required_key) {
                return fail(&format!("{path}.{key}"), &format!("missing key '{required_key}'"));
            }
        }
        expect_type(&format!("{path}.{key}.kind"), &obj["kind"], "str")?;
        expect_type(&format!("{path}.{key}.uri"), &obj["uri"], "str")?;
        if obj["uri"].as_str().unwrap().is_empty() {
            return fail(&format!("{path}.{key}.uri"), "expected non-empty string");
        }
        expect_type(&format!("{path}.{key}.source"), &obj["source"], "str")?;
    }
    Ok(())
}

fn validate_derived(path: &str, derived: &Value) -> Result<()> {
    expect_type(path, derived, "dict")?;
    let obj = derived.as_object().unwrap();
    for key in ["role", "by", "confidence"] {
        if !obj.contains_key(key) {
            return fail(path, &format!("missing key '{key}'"));
        }
    }
    expect_type(&format!("{path}.role"), &obj["role"], "str")?;
    expect_type(&format!("{path}.by"), &obj["by"], "str")?;
    let confidence = &obj["confidence"];
    expect_number(&format!("{path}.confidence"), confidence)?;
    let c = confidence.as_f64().unwrap();
    if c < 0.0 || c > 1.0 {
        return fail(&format!("{path}.confidence"), &format!("expected 0.0 <= confidence <= 1.0, got {c}"));
    }
    Ok(())
}

fn validate_continuation_hint(path: &str, hint: &Value) -> Result<()> {
    expect_type(path, hint, "dict")?;
    let obj = hint.as_object().unwrap();
    for key in ["source", "group_id", "role", "scope", "reading_order", "confidence"] {
        if !obj.contains_key(key) {
            return fail(path, &format!("missing key '{key}'"));
        }
    }
    expect_type(&format!("{path}.source"), &obj["source"], "str")?;
    let source = obj["source"].as_str().unwrap();
    if source != "" && source != "provider" {
        return fail(&format!("{path}.source"), &format!("unexpected continuation source '{source}'"));
    }
    expect_type(&format!("{path}.group_id"), &obj["group_id"], "str")?;
    expect_type(&format!("{path}.role"), &obj["role"], "str")?;
    let role = obj["role"].as_str().unwrap();
    if !["", "single", "head", "middle", "tail"].contains(&role) {
        return fail(&format!("{path}.role"), &format!("unexpected continuation role '{role}'"));
    }
    expect_type(&format!("{path}.scope"), &obj["scope"], "str")?;
    let scope = obj["scope"].as_str().unwrap();
    if !["", "intra_page", "cross_page"].contains(&scope) {
        return fail(&format!("{path}.scope"), &format!("unexpected continuation scope '{scope}'"));
    }
    let reading_order = &obj["reading_order"];
    if reading_order.is_boolean() || !(reading_order.is_i64() || reading_order.is_u64()) {
        return fail(&format!("{path}.reading_order"), "expected integer, got bool/non-int");
    }
    let ro = reading_order.as_i64().unwrap();
    if ro < -1 {
        return fail(&format!("{path}.reading_order"), &format!("expected >= -1, got {ro}"));
    }
    let confidence = &obj["confidence"];
    if confidence.is_boolean() || !confidence.is_number() {
        return fail(&format!("{path}.confidence"), "expected number");
    }
    let c = confidence.as_f64().unwrap();
    if c < 0.0 || c > 1.0 {
        return fail(&format!("{path}.confidence"), &format!("expected 0.0 <= confidence <= 1.0, got {c}"));
    }
    Ok(())
}

fn validate_segment(path: &str, segment: &Value) -> Result<()> {
    expect_type(path, segment, "dict")?;
    let obj = segment.as_object().unwrap();
    for key in ["type", "raw_type", "text", "bbox"] {
        if !obj.contains_key(key) {
            return fail(path, &format!("missing key '{key}'"));
        }
    }
    expect_type(&format!("{path}.type"), &obj["type"], "str")?;
    let seg_type = obj["type"].as_str().unwrap();
    if seg_type != "text" && seg_type != "formula" {
        return fail(&format!("{path}.type"), &format!("unexpected segment type '{seg_type}'"));
    }
    expect_type(&format!("{path}.raw_type"), &obj["raw_type"], "str")?;
    expect_type(&format!("{path}.text"), &obj["text"], "str")?;
    validate_bbox(&format!("{path}.bbox"), &obj["bbox"])?;
    if let Some(score) = obj.get("score") {
        if !score.is_null() {
            expect_number(&format!("{path}.score"), score)?;
        }
    }
    Ok(())
}

fn validate_line(path: &str, line: &Value) -> Result<()> {
    expect_type(path, line, "dict")?;
    let obj = line.as_object().unwrap();
    if !obj.contains_key("bbox") || !obj.contains_key("spans") {
        return fail(path, "missing key 'bbox' or 'spans'");
    }
    validate_bbox(&format!("{path}.bbox"), &obj["bbox"])?;
    expect_type(&format!("{path}.spans"), &obj["spans"], "list")?;
    for (index, span) in obj["spans"].as_array().unwrap().iter().enumerate() {
        validate_segment(&format!("{path}.spans[{index}]"), span)?;
    }
    Ok(())
}

pub(crate) const ALLOWED_CONTENT_KINDS: [&str; 6] =
    ["text", "image", "table", "formula", "code", "unknown"];
pub(crate) const ALLOWED_LAYOUT_ROLES: [&str; 11] = [
    "title", "heading", "paragraph", "list_item", "caption", "header", "footer",
    "footnote", "page_number", "toc", "unknown",
];
pub(crate) const ALLOWED_SEMANTIC_ROLES: [&str; 8] = [
    "body", "abstract", "reference", "metadata", "affiliation", "acknowledgement",
    "table_of_contents", "unknown",
];
pub(crate) const ALLOWED_BLOCK_TYPES: [&str; 6] = ["text", "formula", "image", "table", "code", "unknown"];
pub(crate) const DOCUMENT_REQUIRED_KEYS: [&str; 8] = [
    "schema", "schema_version", "document_id", "source", "page_count", "pages",
    "derived", "markers",
];
pub(crate) const PAGE_REQUIRED_KEYS: [&str; 5] = ["page_index", "width", "height", "unit", "blocks"];
pub(crate) const BLOCK_REQUIRED_KEYS: [&str; 13] = [
    "block_id", "page_index", "order", "geometry", "content", "layout_role",
    "semantic_role", "structure_role", "policy", "provenance", "continuation_hint",
    "metadata", "source",
];

fn validate_block(path: &str, block: &Value, page_index: i64) -> Result<()> {
    expect_type(path, block, "dict")?;
    let obj = block.as_object().unwrap();
    for key in BLOCK_REQUIRED_KEYS {
        if !obj.contains_key(key) {
            return fail(path, &format!("missing key '{key}'"));
        }
    }
    expect_type(&format!("{path}.block_id"), &obj["block_id"], "str")?;
    expect_type(&format!("{path}.page_index"), &obj["page_index"], "int")?;
    if obj["page_index"].as_i64().unwrap() != page_index {
        return fail(&format!("{path}.page_index"), &format!("expected {page_index}, got {}", obj["page_index"]));
    }
    expect_type(&format!("{path}.order"), &obj["order"], "int")?;
    validate_geometry(&format!("{path}.geometry"), &obj["geometry"])?;
    validate_content(&format!("{path}.content"), &obj["content"])?;
    let layout_role = role_string(&format!("{path}.layout_role"), &obj["layout_role"])?;
    if !ALLOWED_LAYOUT_ROLES.contains(&layout_role.as_str()) {
        return fail(&format!("{path}.layout_role"), &format!("unexpected layout role '{layout_role}'"));
    }
    let semantic_role = role_string(&format!("{path}.semantic_role"), &obj["semantic_role"])?;
    if !ALLOWED_SEMANTIC_ROLES.contains(&semantic_role.as_str()) {
        return fail(&format!("{path}.semantic_role"), &format!("unexpected semantic role '{semantic_role}'"));
    }
    role_string(&format!("{path}.structure_role"), &obj["structure_role"])?;
    validate_policy(&format!("{path}.policy"), &obj["policy"])?;
    validate_provenance(&format!("{path}.provenance"), &obj["provenance"])?;
    validate_continuation_hint(&format!("{path}.continuation_hint"), &obj["continuation_hint"])?;
    expect_type(&format!("{path}.metadata"), &obj["metadata"], "dict")?;
    expect_type(&format!("{path}.source"), &obj["source"], "dict")?;
    let source = obj["source"].as_object().unwrap();
    let provider = source.get("provider").and_then(Value::as_str);
    if provider.is_none() || provider.unwrap().is_empty() {
        return fail(&format!("{path}.source.provider"), "expected non-empty string");
    }
    if let Some(reading_order) = obj.get("reading_order") {
        expect_type(&format!("{path}.reading_order"), reading_order, "int")?;
        if reading_order.as_i64().unwrap() < 0 {
            return fail(&format!("{path}.reading_order"), "expected >= 0");
        }
    }
    if let Some(block_type) = obj.get("type") {
        expect_type(&format!("{path}.type"), block_type, "str")?;
        if !ALLOWED_BLOCK_TYPES.contains(&block_type.as_str().unwrap()) {
            return fail(&format!("{path}.type"), &format!("unexpected block type '{}'", block_type.as_str().unwrap()));
        }
    }
    if let Some(sub_type) = obj.get("sub_type") {
        expect_type(&format!("{path}.sub_type"), sub_type, "str")?;
    }
    if let Some(bbox) = obj.get("bbox") {
        validate_bbox(&format!("{path}.bbox"), bbox)?;
    }
    if let Some(text) = obj.get("text") {
        expect_type(&format!("{path}.text"), text, "str")?;
    }
    if let Some(lines) = obj.get("lines") {
        expect_type(&format!("{path}.lines"), lines, "list")?;
        for (index, line) in lines.as_array().unwrap().iter().enumerate() {
            validate_line(&format!("{path}.lines[{index}]"), line)?;
        }
    }
    if let Some(segments) = obj.get("segments") {
        expect_type(&format!("{path}.segments"), segments, "list")?;
        for (index, segment) in segments.as_array().unwrap().iter().enumerate() {
            validate_segment(&format!("{path}.segments[{index}]"), segment)?;
        }
    }
    if let Some(tags) = obj.get("tags") {
        expect_type(&format!("{path}.tags"), tags, "list")?;
        for (index, tag) in tags.as_array().unwrap().iter().enumerate() {
            expect_type(&format!("{path}.tags[{index}]"), tag, "str")?;
        }
    }
    if let Some(derived) = obj.get("derived") {
        validate_derived(&format!("{path}.derived"), derived)?;
    }
    Ok(())
}

fn validate_page(path: &str, page: &Value, page_index: i64) -> Result<()> {
    expect_type(path, page, "dict")?;
    let obj = page.as_object().unwrap();
    for key in PAGE_REQUIRED_KEYS {
        if !obj.contains_key(key) {
            return fail(path, &format!("missing key '{key}'"));
        }
    }
    expect_type(&format!("{path}.page_index"), &obj["page_index"], "int")?;
    if obj["page_index"].as_i64().unwrap() != page_index {
        return fail(&format!("{path}.page_index"), &format!("expected {page_index}, got {}", obj["page_index"]));
    }
    if let Some(page_num) = obj.get("page") {
        expect_type(&format!("{path}.page"), page_num, "int")?;
        if page_num.as_i64().unwrap() < 1 {
            return fail(&format!("{path}.page"), "expected >= 1");
        }
    }
    for key in ["width", "height"] {
        expect_number(&format!("{path}.{key}"), &obj[key])?;
        if obj[key].as_f64().unwrap() < 0.0 {
            return fail(&format!("{path}.{key}"), "expected >= 0");
        }
    }
    expect_type(&format!("{path}.unit"), &obj["unit"], "str")?;
    if obj["unit"].as_str().unwrap() != "pt" {
        return fail(&format!("{path}.unit"), &format!("expected 'pt', got '{}'", obj["unit"].as_str().unwrap()));
    }
    expect_type(&format!("{path}.blocks"), &obj["blocks"], "list")?;
    for (index, block) in obj["blocks"].as_array().unwrap().iter().enumerate() {
        validate_block(&format!("{path}.blocks[{index}]"), block, page_index)?;
    }
    Ok(())
}

/// `validate_document_payload` — raises on the first structural violation.
pub fn validate_document_payload(data: &Value) -> Result<()> {
    expect_type("$", data, "dict")?;
    let obj = data.as_object().unwrap();
    for key in DOCUMENT_REQUIRED_KEYS {
        if !obj.contains_key(key) {
            return fail("$", &format!("missing key '{key}'"));
        }
    }
    if obj["schema"].as_str() != Some(DOCUMENT_SCHEMA_NAME) {
        return fail("$.schema", &format!("expected '{DOCUMENT_SCHEMA_NAME}', got '{}'", obj["schema"]));
    }
    let schema_version = obj["schema_version"].as_str().unwrap_or_default();
    if !SUPPORTED_DOCUMENT_SCHEMA_VERSIONS.contains(&schema_version) {
        return fail(
            "$.schema_version",
            &format!(
                "expected one of {SUPPORTED_DOCUMENT_SCHEMA_VERSIONS:?}, got '{schema_version}'"
            ),
        );
    }
    expect_type("$.document_id", &obj["document_id"], "str")?;
    if let Some(doc_id) = obj.get("doc_id") {
        expect_type("$.doc_id", doc_id, "str")?;
        if doc_id.as_str().unwrap().is_empty() {
            return fail("$.doc_id", "expected non-empty string");
        }
    }
    expect_type("$.source", &obj["source"], "dict")?;
    expect_type("$.page_count", &obj["page_count"], "int")?;
    expect_type("$.pages", &obj["pages"], "list")?;
    if let Some(assets) = obj.get("assets") {
        validate_assets("$.assets", assets)?;
    }
    expect_type("$.derived", &obj["derived"], "dict")?;
    expect_type("$.markers", &obj["markers"], "dict")?;
    let pages = obj["pages"].as_array().unwrap();
    if obj["page_count"].as_i64().unwrap() != pages.len() as i64 {
        return fail("$.page_count", &format!("expected {}, got {}", pages.len(), obj["page_count"]));
    }
    for (index, page) in pages.iter().enumerate() {
        validate_page(&format!("$.pages[{index}]"), page, index as i64)?;
    }
    if let Some(reference_start) = obj["markers"].get("reference_start") {
        if !reference_start.is_null() {
            expect_type("$.markers.reference_start", reference_start, "dict")?;
            for key in ["page_index", "block_id", "order"] {
                if !reference_start.as_object().unwrap().contains_key(key) {
                    return fail("$.markers.reference_start", &format!("missing key '{key}'"));
                }
            }
        }
    }
    Ok(())
}

/// `build_validation_report` — validates, then reports the doc-level counts.
pub fn build_validation_report(data: &Value) -> Result<Value> {
    validate_document_payload(data)?;
    let page_count = data
        .get("pages")
        .and_then(Value::as_array)
        .map_or(0, |pages| pages.len());
    let block_count = data
        .get("pages")
        .and_then(Value::as_array)
        .map_or(0, |pages| {
            pages
                .iter()
                .map(|page| {
                    page.get("blocks")
                        .and_then(Value::as_array)
                        .map_or(0, |blocks| blocks.len())
                })
                .sum()
        });
    Ok(serde_json::json!({
        "valid": true,
        "schema": DOCUMENT_SCHEMA_NAME,
        "schema_version": DOCUMENT_SCHEMA_VERSION,
        "page_count": page_count,
        "block_count": block_count,
    }))
}
