// Shared raw-JSON accessors for the layout payload-dict leaf modules (ports of
// services/rendering/layout/payload/*.py). These operate on the raw block-payload
// dicts; the typed `Item` DTO does not carry the pipeline keys they read.

use serde_json::Value;

/// `f64_list`: a JSON array of numbers as `Vec<f64>`, else `None`.
pub fn f64_list(value: &Value) -> Option<Vec<f64>> {
    let arr = value.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        match v.as_f64() {
            Some(n) => out.push(n),
            None => return None,
        }
    }
    Some(out)
}

pub fn payload_f64(payload: &Value, key: &str, default: f64) -> f64 {
    payload.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
}

pub fn payload_bool(payload: &Value, key: &str) -> bool {
    payload.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

pub fn payload_string(payload: &Value, key: &str, default: &str) -> String {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

/// `payload["inner_bbox"]` as `[x0, y0, x1, y1]` when it holds exactly 4 numbers.
pub fn inner_bbox4(payload: &Value) -> Option<[f64; 4]> {
    let list = f64_list(payload.get("inner_bbox")?)?;
    if list.len() != 4 {
        return None;
    }
    Some([list[0], list[1], list[2], list[3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inner_bbox_reads_four_numbers() {
        let payload = json!({"inner_bbox": [1.0, 2.0, 3, 4.0]});
        assert_eq!(inner_bbox4(&payload), Some([1.0, 2.0, 3.0, 4.0]));
    }

    #[test]
    fn inner_bbox_rejects_short_lists() {
        assert_eq!(inner_bbox4(&json!({"inner_bbox": [1.0, 2.0]})), None);
        assert_eq!(inner_bbox4(&json!({})), None);
    }

    #[test]
    fn scalar_helpers_default() {
        assert_eq!(payload_f64(&json!({}), "x", 1.5), 1.5);
        assert!(!payload_bool(&json!({}), "y"));
        assert_eq!(payload_string(&json!({}), "z", "d"), "d");
    }
}
