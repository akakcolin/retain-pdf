//! Visual-profile fill-map loader (native mirror of
//! `source.background._native.visual_profile_fill_map` over the retired Python
//! `build_bundle`'s `load_visual_profile_runtime`). Reads the prewarmed
//! `<translations_dir>/../artifacts/render_prewarm/visual_profile.v1.json` and
//! emits the flat first-wins `{item_id: [r, g, b]}` table the background stage
//! applies per item. Missing file / unparseable payload -> empty map (a render
//! must never block on the visual profile).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

pub const RENDER_PREWARM_DIR_NAME: &str = "render_prewarm";
pub const VISUAL_PROFILE_MANIFEST_NAME: &str = "visual_profile.v1.json";

/// `<translations_dir>/../artifacts/render_prewarm/visual_profile.v1.json`.
pub fn visual_profile_path(translations_dir: &Path) -> PathBuf {
    match translations_dir.parent() {
        Some(parent) => parent
            .join("artifacts")
            .join(RENDER_PREWARM_DIR_NAME)
            .join(VISUAL_PROFILE_MANIFEST_NAME),
        None => translations_dir.to_path_buf(),
    }
}

/// First-wins `{item_id: [r, g, b]}` fill table; empty when the profile is
/// absent or not loadable. Pages and items are iterated in ascending key order
/// to match the manifest's `sorted()` serialization and the Python reference.
pub fn visual_profile_fill_map(translations_dir: &Path) -> Result<HashMap<String, [f64; 3]>> {
    let path = visual_profile_path(translations_dir);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text = std::fs::read_to_string(&path)?;
    let payload: serde_json::Value = serde_json::from_str(&text)?;
    Ok(fill_map_from_payload(&payload))
}

fn fill_map_from_payload(payload: &serde_json::Value) -> HashMap<String, [f64; 3]> {
    let mut fills: HashMap<String, [f64; 3]> = HashMap::new();
    let Some(pages) = payload.get("pages").and_then(|v| v.as_object()) else {
        return fills;
    };
    let mut page_keys: Vec<&String> = pages.keys().collect();
    page_keys.sort();
    for page_key in page_keys {
        let Some(items) = pages[page_key].get("items").and_then(|v| v.as_object()) else {
            continue;
        };
        let mut item_keys: Vec<&String> = items.keys().collect();
        item_keys.sort();
        for item_id in item_keys {
            if fills.contains_key(item_id) {
                continue;
            }
            let Some(rgb) = items[item_id].get("background_rgb").and_then(|v| v.as_array()) else {
                continue;
            };
            if rgb.len() != 3 {
                continue;
            }
            let mut color = [0.0f64; 3];
            for (i, c) in rgb.iter().take(3).enumerate() {
                color[i] = c.as_f64().unwrap_or(0.0);
            }
            fills.insert(item_id.clone(), color);
        }
    }
    fills
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_wins_sorted_across_pages() {
        let payload = serde_json::json!({
            "algorithm": "v16",
            "pages": {
                "1": {"items": {
                    "b": {"background_rgb": [0.1, 0.2, 0.3], "text_rgb": [0, 0, 0]},
                    "a": {"background_rgb": [0.9, 0.8, 0.7], "text_rgb": [0, 0, 0]}
                }},
                "0": {"items": {
                    "a": {"background_rgb": [0.1, 0.0, 0.0], "text_rgb": [0, 0, 0]},
                    "c": {"background_rgb": [0.2, 0.2, 0.2], "text_rgb": [0, 0, 0]}
                }}
            }
        });
        let fills = fill_map_from_payload(&payload);
        assert_eq!(fills.len(), 3);
        // "a" first-wins from page 0; "b"/"c" from their pages.
        assert_eq!(fills["a"], [0.1, 0.0, 0.0]);
        assert_eq!(fills["b"], [0.1, 0.2, 0.3]);
        assert_eq!(fills["c"], [0.2, 0.2, 0.2]);
    }

    #[test]
    fn missing_file_is_empty() {
        let fills = visual_profile_fill_map(Path::new("/nonexistent/job/translations")).unwrap();
        assert!(fills.is_empty());
    }

    #[test]
    fn path_derivation() {
        assert_eq!(
            visual_profile_path(Path::new("/job/translations")),
            PathBuf::from("/job/artifacts/render_prewarm/visual_profile.v1.json")
        );
    }

    #[test]
    fn bad_payload_is_empty() {
        let payload = serde_json::json!({ "algorithm": "v16" });
        assert!(fill_map_from_payload(&payload).is_empty());
    }
}
