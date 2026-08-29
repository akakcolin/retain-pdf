// Port of services/rendering/policy/compat.py — the render-policy helpers the
// emit and body-pipeline boundaries need. Operates on the raw JSON item dict
// (the typed `Item` does not carry `_render_policy` / `_render_overlay_fill`).

use serde_json::{json, Value};

/// `_item_policy_dict`: `item["_render_policy"]` when it is a dict.
fn item_policy_dict(item: &Value) -> Option<&serde_json::Map<String, Value>> {
    item.get("_render_policy").and_then(|v| v.as_object())
}

/// `item_render_policy`: a copy of the `_render_policy` dict (or `{}`).
pub fn item_render_policy(item: &Value) -> Value {
    match item_policy_dict(item) {
        Some(policy) => json!(policy),
        None => json!({}),
    }
}

fn policy_or_item(item: &Value, policy_key: &str, item_key: &str) -> String {
    let from_policy = item_policy_dict(item)
        .and_then(|p| p.get(policy_key))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let from_direct = item
        .get(item_key)
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let raw = if from_policy.is_empty() { from_direct } else { from_policy };
    raw.trim().to_string()
}

/// `item_render_policy_reason`: `_render_policy.reason` wins, then
/// `_render_policy_reason`, trimmed.
pub fn item_render_policy_reason(item: &Value) -> String {
    policy_or_item(item, "reason", "_render_policy_reason")
}

/// `item_formula_protection_role`: `_render_policy.formula_protection_role`
/// wins, then `_render_formula_protection_role`, lowercased + trimmed.
pub fn item_formula_protection_role(item: &Value) -> String {
    policy_or_item(item, "formula_protection_role", "_render_formula_protection_role")
        .to_lowercase()
}

/// `item_cleanup_mode`: `_render_policy.cleanup_mode` wins, then
/// `_render_cleanup_mode`, lowercased + trimmed.
pub fn item_cleanup_mode(item: &Value) -> String {
    policy_or_item(item, "cleanup_mode", "_render_cleanup_mode").to_lowercase()
}

/// `item_overlay_fill`: `_render_policy.overlay_fill` wins, then
/// `_render_overlay_fill`, lowercased + trimmed.
pub fn item_overlay_fill(item: &Value) -> String {
    policy_or_item(item, "overlay_fill", "_render_overlay_fill").to_lowercase()
}

/// `item_requires_visual_cover_only`: visual_cover cleanup mode or an explicit
/// `_force_visual_cover_only` flag.
pub fn item_requires_visual_cover_only(item: &Value) -> bool {
    item_cleanup_mode(item) == "visual_cover"
        || item
            .get("_force_visual_cover_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

/// `item_uses_white_overlay_fill`: white/sampled overlay or an explicit
/// `_render_use_cover_fill` flag.
pub fn item_uses_white_overlay_fill(item: &Value) -> bool {
    let overlay = item_overlay_fill(item);
    overlay == "white"
        || overlay == "sampled"
        || item
            .get("_render_use_cover_fill")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

/// `item_uses_explicit_white_overlay_fill`: explicit "white" overlay fill.
pub fn item_uses_explicit_white_overlay_fill(item: &Value) -> bool {
    item_overlay_fill(item) == "white"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn white_from_policy() {
        let item = json!({"_render_policy": {"overlay_fill": "White"}});
        assert!(item_uses_white_overlay_fill(&item));
    }

    #[test]
    fn sampled_from_direct() {
        let item = json!({"_render_overlay_fill": "Sampled"});
        assert!(item_uses_white_overlay_fill(&item));
    }

    #[test]
    fn use_cover_fill_flag() {
        let item = json!({"_render_use_cover_fill": true});
        assert!(item_uses_white_overlay_fill(&item));
    }

    #[test]
    fn other_fills_reject() {
        let item = json!({"_render_policy": {"overlay_fill": "blue"}});
        assert!(!item_uses_white_overlay_fill(&item));
        assert_eq!(item_overlay_fill(&item), "blue");
    }

    #[test]
    fn missing_keys_default_false() {
        assert!(!item_uses_white_overlay_fill(&json!({})));
        assert_eq!(item_overlay_fill(&json!({})), "");
    }
}
