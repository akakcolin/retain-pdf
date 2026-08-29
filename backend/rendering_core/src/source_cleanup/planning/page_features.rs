//! Port of `planning/page_features.py` — `PageCleanupFeatures.from_manifest`.

use serde_json::Value;

use crate::source_cleanup::planning::PageCleanupFeatures;

/// `PageCleanupFeatures.from_manifest` — rebuild features from a stored
/// manifest dict (missing / malformed → `None`).
pub fn page_features_from_manifest(value: &Value) -> Option<PageCleanupFeatures> {
    let payload = match value {
        Value::Object(map) => map,
        _ => return None,
    };
    if payload.is_empty() {
        return None;
    }
    let content_stream_size = payload
        .get("content_stream_size")
        .and_then(Value::as_i64)
        .map(|value| value.max(0) as u64)
        .unwrap_or(0);
    let has_form_xobjects = payload
        .get("has_form_xobjects")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(PageCleanupFeatures {
        content_stream_size,
        has_form_xobjects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_manifest() {
        let value = serde_json::json!({
            "content_stream_size": 1200,
            "has_form_xobjects": true,
        });
        let features = page_features_from_manifest(&value).expect("manifest");
        assert_eq!(features.content_stream_size, 1200);
        assert!(features.has_form_xobjects);
    }

    #[test]
    fn missing_payload_is_none() {
        assert!(page_features_from_manifest(&serde_json::Value::Null).is_none());
        assert!(page_features_from_manifest(&serde_json::json!({})).is_none());
    }
}
