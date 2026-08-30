// Small JSON helpers shared by the normalize ports, mirroring Python coercion
// used across document_schema: `str(x or "")`-style string extraction, `int()`
// truncation, and bbox normalization.

use serde_json::Value;

/// `str(value or "")`-style extraction: None/null/false → empty string, strings
/// verbatim, numbers formatted as JSON does, bools "true"/"false".
pub fn value_as_str(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Coerce a value to a string like Python `str(x)`.
pub fn to_string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Python `int(x)` for JSON values: integers verbatim, floats truncated toward
/// zero, numeric strings parsed; everything else (including bool) → None.
pub fn py_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if let Some(f) = n.as_f64() {
                Some(f.trunc() as i64)
            } else {
                None
            }
        }
        Value::String(s) => {
            if let Ok(parsed) = s.trim().parse::<f64>() {
                Some(parsed.trunc() as i64)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `normalize_bbox`: a 4-element list passes through as floats; anything else →
/// `[0, 0, 0, 0]`.
pub fn normalize_bbox(value: Option<&Value>) -> Vec<f64> {
    match value {
        Some(Value::Array(items)) if items.len() == 4 => {
            items.iter().map(|item| item.as_f64().unwrap_or(0.0)).collect()
        }
        _ => vec![0.0, 0.0, 0.0, 0.0],
    }
}

/// `_normalize_tags`: lowercased, stripped, non-empty string tags as a set.
pub fn normalize_tags(value: Option<&Value>) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    match value {
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    let t = s.trim().to_lowercase();
                    if !t.is_empty() {
                        out.insert(t);
                    }
                }
            }
        }
        _ => {}
    }
    out
}
