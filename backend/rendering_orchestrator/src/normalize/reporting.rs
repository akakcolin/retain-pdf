// Port of `services/document_schema/reporting.py::build_normalization_summary`.

use serde_json::{json, Value};

fn sum_default_hits(payload: Option<&Value>) -> i64 {
    match payload {
        Some(Value::Object(map)) => map
            .values()
            .filter_map(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
            .sum(),
        _ => 0,
    }
}

/// `build_normalization_summary(report)` — the flattening summary the worker
/// prints in its `normalized document report:` label.
pub fn build_normalization_summary(report: &Value) -> Value {
    let data = report.as_object();
    let get = |key: &str| data.and_then(|d| d.get(key));
    let defaults = get("defaults").and_then(Value::as_object);
    let defaults_get = |key: &str| defaults.and_then(|d| d.get(key));
    let validation = get("validation").and_then(Value::as_object);
    let detection = get("detection").and_then(Value::as_object);
    let document_defaults = defaults_get("document_defaults");
    let page_defaults = defaults_get("page_defaults");
    let block_defaults = defaults_get("block_defaults");
    let str_value = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let int_value = |value: Option<&Value>| value.and_then(Value::as_i64).unwrap_or(0);
    let bool_value = |value: Option<&Value>| value.and_then(Value::as_bool).unwrap_or(false);
    json!({
        "provider": str_value(get("provider")),
        "detected_provider": str_value(get("detected_provider")),
        "provider_was_explicit": bool_value(get("provider_was_explicit")),
        "pages_observed": int_value(defaults_get("pages_seen")),
        "blocks_observed": int_value(defaults_get("blocks_seen")),
        "defaulted_document_fields": sum_default_hits(document_defaults),
        "defaulted_page_fields": sum_default_hits(page_defaults),
        "defaulted_block_fields": sum_default_hits(block_defaults),
        "any_defaults_applied": document_defaults.map_or(false, Value::is_object)
            || page_defaults.map_or(false, Value::is_object)
            || block_defaults.map_or(false, Value::is_object),
        "valid": bool_value(validation.and_then(|v| v.get("valid"))),
        "page_count": int_value(validation.and_then(|v| v.get("page_count"))),
        "block_count": int_value(validation.and_then(|v| v.get("block_count"))),
        "detection_matched": bool_value(detection.and_then(|d| d.get("matched"))),
        "detection_attempts": detection
            .and_then(|d| d.get("attempts"))
            .and_then(Value::as_array)
            .map_or(0, |attempts| attempts.len()) as i64,
    })
}
