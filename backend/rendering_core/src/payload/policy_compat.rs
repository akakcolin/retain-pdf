// Port of services/rendering/policy/compat.py — the overlay-fill policy the
// emit boundary needs. Operates on the raw JSON item dict (the typed `Item`
// does not carry `_render_policy` / `_render_overlay_fill`).

use serde_json::Value;

/// `item_overlay_fill`: `_render_policy.overlay_fill` wins, then
/// `_render_overlay_fill`, lowercased + trimmed.
pub fn item_overlay_fill(item: &Value) -> String {
    let from_policy = item
        .get("_render_policy")
        .and_then(|v| v.as_object())
        .and_then(|p| p.get("overlay_fill"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let from_direct = item
        .get("_render_overlay_fill")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let raw = if from_policy.is_empty() { from_direct } else { from_policy };
    raw.trim().to_lowercase()
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
