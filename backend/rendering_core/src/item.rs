// Rust replacement for the Python `item: dict` (layout block) consumed by the
// typography / font_fit / payload modules. Tests construct these directly, so
// every field mirrors a Python dict key from the fixtures.
//
// The struct is also the deserialization target for the translated-item dicts
// that arrive at the C3-N2 `build_block_payloads` seed boundary. `from_json_value`
// maps every Python key the seed path reads onto a typed field, including the
// leading-underscore render keys (`_render_inner_bbox`, `_render_text_color`, ...).

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// "text" or "inline_equation".
    pub span_type: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub bbox: Option<[f64; 4]>,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormulaEntry {
    pub placeholder: String,
    /// From `formula_text`, falling back to `latex`.
    pub formula_text: String,
}

/// A protected-token entry (`ProtectedToken.to_dict()`): the placeholder tag in
/// the protected text plus the text it restores to. `token_type` "formula"
/// triggers inline-math wrapping on restore.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtectedEntry {
    pub token_tag: String,
    pub token_type: String,
    pub restore_text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Item {
    pub bbox: Option<[f64; 4]>,
    pub lines: Vec<Line>,
    pub source_text: String,
    pub layout_role: Option<String>,
    pub semantic_role: Option<String>,
    pub structure_role: Option<String>,
    pub block_kind: Option<String>,
    pub block_type: Option<String>,
    pub sub_type: Option<String>,
    pub normalized_sub_type: Option<String>,
    pub raw_block_type: Option<String>,
    pub tags: Vec<String>,
    /// Mirrors `payload["derived"]["role"]`.
    pub derived_role: Option<String>,
    /// Mirrors `payload["policy"]["translate"]`.
    pub policy_translate: Option<bool>,
    pub formula_map: Vec<FormulaEntry>,
    /// Mirrors `payload["body_repair_applied"]`.
    pub body_repair_applied: Option<bool>,
    /// Mirrors `payload["provider_body_repair_applied"]`.
    pub provider_body_repair_applied: Option<bool>,
    /// Mirrors `payload["body_repair_role"]`.
    pub body_repair_role: Option<String>,
    /// Mirrors `payload["provider_body_repair_role"]`.
    pub provider_body_repair_role: Option<String>,
    /// Mirrors `payload["body_repair_peer_block_id"]`.
    pub body_repair_peer_block_id: Option<String>,
    /// Mirrors `payload["provider_suspected_peer_block_id"]`.
    pub provider_suspected_peer_block_id: Option<String>,

    // ---- Translation / render-text fields (mirror the translated item dict) ----
    pub translated_text: String,
    /// `render_protected_text`.
    pub render_protected_text: String,
    /// Whether the `render_protected_text` key was present — Python
    /// `"render_protected_text" in item` gates on presence, not truthiness.
    pub has_render_protected_text: bool,
    /// `render_source_text`.
    pub render_source_text: String,
    /// `_protected_source_text`.
    pub protected_source_text: String,
    /// `_protected_translated_text`.
    pub protected_translated_text: String,
    pub translation_unit_kind: Option<String>,
    pub continuation_group: Option<bool>,
    pub continuation_group_id: Option<String>,
    /// `None` reads as true (Python `item.get("should_translate", True)`).
    pub should_translate: Option<bool>,
    pub skip_reason: Option<String>,
    pub classification_label: Option<String>,
    pub translation_unit_protected_source_text: String,
    pub translation_unit_source_text: String,
    pub group_protected_source_text: String,
    pub group_source_text: String,
    pub translation_unit_protected_translated_text: String,
    pub translation_unit_translated_text: String,
    pub group_protected_translated_text: String,
    pub group_translated_text: String,
    pub render_formula_map: Vec<FormulaEntry>,
    pub translation_unit_formula_map: Vec<FormulaEntry>,
    pub group_formula_map: Vec<FormulaEntry>,
    /// Whether any `render_formula_map` entry carries a `token_tag` key.
    pub render_formula_map_has_token_tag: bool,
    /// Whether any `formula_map` entry carries a `token_tag` key.
    pub formula_map_has_token_tag: bool,
    /// Whether any `translation_unit_formula_map` entry carries a `token_tag` key.
    pub translation_unit_formula_map_has_token_tag: bool,
    /// Whether any `group_formula_map` entry carries a `token_tag` key.
    pub group_formula_map_has_token_tag: bool,
    /// Protected (token-tag) map carried by the unit when a group translation.
    pub translation_unit_protected_map: Vec<ProtectedEntry>,
    /// The item's own protected map (protected-token entries, never formula).
    pub protected_map: Vec<ProtectedEntry>,
    /// Whether `protected_map` entries carry a `token_tag` (Python
    /// `_render_protected_map`'s `any("token_tag" in entry)` gate).
    pub protected_map_has_token_tag: bool,
    /// `_render_first_line_indent_pt`.
    pub render_first_line_indent_pt: f64,
    /// `_render_inner_bbox`.
    pub render_inner_bbox: Option<[f64; 4]>,
    /// `_use_raw_text_bbox`.
    pub use_raw_text_bbox: bool,
    /// `_force_plain_line`.
    pub force_plain_line: bool,
    /// `_render_preserve_line_breaks`.
    pub render_preserve_line_breaks: bool,
    /// `_render_text_color`, default (0, 0, 0).
    pub render_text_color: (f64, f64, f64),
    /// `_render_cover_fill`, default (1, 1, 1).
    pub render_cover_fill: (f64, f64, f64),

    // Internal layout flags.
    pub is_body_text_candidate: bool,
    pub wide_aspect_body_text: bool,
    pub cover_with_inner_bbox: bool,
    /// `_dense_small_box` (fit_item flag set by the seed factory).
    pub dense_small_box: bool,
    /// `_heavy_dense_small_box` (fit_item flag set by the seed factory).
    pub heavy_dense_small_box: bool,
    /// `_short_body_inherited_font_floor_pt` (font floor inherited from a wider
    /// same-column body block; 0 when not set).
    pub short_body_inherited_font_floor_pt: f64,
}

impl Item {
    /// Deserialize a translated-item dict (the JSON value that Python hands the
    /// seed boundary) into the typed `Item`. Missing keys fall back to the same
    /// defaults the Python `dict.get` / `or` chains use, so the struct is a
    /// faithful mirror of the raw dict.
    pub fn from_json_value(value: &serde_json::Value) -> Item {
        let lines = value
            .get("lines")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(line_from_json).collect())
            .unwrap_or_default();
        Item {
            bbox: f64x4(value.get("bbox")),
            lines,
            source_text: string_key(value, "source_text"),
            layout_role: option_string_key(value, "layout_role"),
            semantic_role: option_string_key(value, "semantic_role"),
            structure_role: option_string_key(value, "structure_role"),
            block_kind: option_string_key(value, "block_kind"),
            block_type: option_string_key(value, "block_type"),
            sub_type: option_string_key(value, "sub_type"),
            normalized_sub_type: option_string_key(value, "normalized_sub_type"),
            raw_block_type: option_string_key(value, "raw_block_type"),
            tags: string_array(value, "tags"),
            derived_role: option_string_key(value, "derived_role"),
            policy_translate: value.get("policy_translate").and_then(|v| v.as_bool()),
            formula_map: formula_map(value.get("formula_map")),
            body_repair_applied: bool_key(value, "body_repair_applied"),
            provider_body_repair_applied: bool_key(value, "provider_body_repair_applied"),
            body_repair_role: option_string_key(value, "body_repair_role"),
            provider_body_repair_role: option_string_key(value, "provider_body_repair_role"),
            body_repair_peer_block_id: option_string_key(value, "body_repair_peer_block_id"),
            provider_suspected_peer_block_id: option_string_key(
                value,
                "provider_suspected_peer_block_id",
            ),

            translated_text: string_key(value, "translated_text"),
            render_protected_text: string_key(value, "render_protected_text"),
            has_render_protected_text: value.get("render_protected_text").is_some(),
            render_source_text: string_key(value, "render_source_text"),
            protected_source_text: string_key(value, "protected_source_text"),
            protected_translated_text: string_key(value, "protected_translated_text"),
            translation_unit_kind: option_string_key(value, "translation_unit_kind"),
            continuation_group: bool_key(value, "continuation_group"),
            continuation_group_id: option_string_key(value, "continuation_group_id"),
            should_translate: bool_key(value, "should_translate"),
            skip_reason: option_string_key(value, "skip_reason"),
            classification_label: option_string_key(value, "classification_label"),
            translation_unit_protected_source_text: string_key(
                value,
                "translation_unit_protected_source_text",
            ),
            translation_unit_source_text: string_key(value, "translation_unit_source_text"),
            group_protected_source_text: string_key(value, "group_protected_source_text"),
            group_source_text: string_key(value, "group_source_text"),
            translation_unit_protected_translated_text: string_key(
                value,
                "translation_unit_protected_translated_text",
            ),
            translation_unit_translated_text: string_key(value, "translation_unit_translated_text"),
            group_protected_translated_text: string_key(value, "group_protected_translated_text"),
            group_translated_text: string_key(value, "group_translated_text"),
            render_formula_map: formula_map(value.get("render_formula_map")),
            translation_unit_formula_map: formula_map(value.get("translation_unit_formula_map")),
            group_formula_map: formula_map(value.get("group_formula_map")),
            render_formula_map_has_token_tag: array_has_key(value, "render_formula_map", "token_tag"),
            formula_map_has_token_tag: array_has_key(value, "formula_map", "token_tag"),
            translation_unit_formula_map_has_token_tag: array_has_key(
                value,
                "translation_unit_formula_map",
                "token_tag",
            ),
            group_formula_map_has_token_tag: array_has_key(value, "group_formula_map", "token_tag"),
            translation_unit_protected_map: protected_map(value.get("translation_unit_protected_map")),
            protected_map: protected_map(value.get("protected_map")),
            protected_map_has_token_tag: value
                .get("protected_map")
                .and_then(|v| v.as_array())
                .map_or(false, |arr| {
                    arr.iter().any(|e| e.get("token_tag").is_some())
                }),
            render_first_line_indent_pt: f64_key(value, "_render_first_line_indent_pt"),
            render_inner_bbox: f64x4(value.get("_render_inner_bbox")),
            use_raw_text_bbox: bool_key(value, "_use_raw_text_bbox").unwrap_or(false),
            force_plain_line: bool_key(value, "_force_plain_line").unwrap_or(false),
            render_preserve_line_breaks: bool_key(value, "_render_preserve_line_breaks").unwrap_or(false),
            render_text_color: color_tuple(value.get("_render_text_color"), (0.0, 0.0, 0.0)),
            render_cover_fill: color_tuple(value.get("_render_cover_fill"), (1.0, 1.0, 1.0)),

            is_body_text_candidate: bool_key(value, "_is_body_text_candidate").unwrap_or(false),
            wide_aspect_body_text: bool_key(value, "_wide_aspect_body_text").unwrap_or(false),
            cover_with_inner_bbox: bool_key(value, "_cover_with_inner_bbox").unwrap_or(false),
            dense_small_box: bool_key(value, "_dense_small_box").unwrap_or(false),
            heavy_dense_small_box: bool_key(value, "_heavy_dense_small_box").unwrap_or(false),
            short_body_inherited_font_floor_pt: f64_key(value, "_short_body_inherited_font_floor_pt"),
        }
    }
}

fn line_from_json(value: &serde_json::Value) -> Line {
    let bbox = f64x4(value.get("bbox"));
    let spans = value
        .get("spans")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|span| Span {
                    span_type: span
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("text")
                        .to_string(),
                    content: first_string_key(span, &["content", "text"]),
                })
                .collect()
        })
        .unwrap_or_else(|| {
            let text = string_key(value, "text");
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Span { span_type: "text".into(), content: text }]
            }
        });
    Line { bbox, spans }
}

pub fn formula_map(value: Option<&serde_json::Value>) -> Vec<FormulaEntry> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|entry| FormulaEntry {
                    placeholder: first_string_key(entry, &["placeholder", "token_tag"]),
                    formula_text: first_string_key(entry, &["formula_text", "latex"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn protected_map(value: Option<&serde_json::Value>) -> Vec<ProtectedEntry> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|entry| ProtectedEntry {
                    token_tag: first_string_key(entry, &["token_tag", "placeholder"]),
                    token_type: string_key(entry, "token_type"),
                    restore_text: first_string_key(
                        entry,
                        &["restore_text", "original_text", "formula_text"],
                    ),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn string_key(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// First non-empty string among `keys`, in order — mirrors the Python
/// `item.get(a) or item.get(b) or ... or ""` fallback chains.
fn first_string_key(value: &serde_json::Value, keys: &[&str]) -> String {
    for key in keys {
        let text = string_key(value, key);
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
}

fn option_string_key(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| {
        if v.is_null() {
            None
        } else {
            v.as_str().map(|s| s.to_string())
        }
    })
}

fn bool_key(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|v| {
        if v.is_boolean() {
            v.as_bool()
        } else if v.is_number() {
            Some(v.as_f64().map_or(false, |f| f != 0.0))
        } else {
            None
        }
    })
}

fn f64_key(value: &serde_json::Value, key: &str) -> f64 {
    value.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

/// True if any object in the `key` array has a `member` key — mirrors Python's
/// `any(isinstance(entry, dict) and "token_tag" in entry for entry in map)`.
fn array_has_key(value: &serde_json::Value, key: &str, member: &str) -> bool {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map_or(false, |arr| arr.iter().any(|e| e.get(member).is_some()))
}

fn string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn f64x4(value: Option<&serde_json::Value>) -> Option<[f64; 4]> {
    value.and_then(|v| v.as_array()).and_then(|arr| {
        if arr.len() != 4 {
            return None;
        }
        let mut out = [0.0; 4];
        for (index, v) in arr.iter().enumerate() {
            out[index] = v.as_f64()?;
        }
        Some(out)
    })
}

fn color_tuple(value: Option<&serde_json::Value>, default: (f64, f64, f64)) -> (f64, f64, f64) {
    value.and_then(|v| v.as_array()).and_then(|arr| {
        if arr.len() < 3 {
            return None;
        }
        Some((arr[0].as_f64()?, arr[1].as_f64()?, arr[2].as_f64()?))
    })
    .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_json() -> serde_json::Value {
        serde_json::json!({
            "bbox": [10.0, 20.0, 210.0, 80.0],
            "source_text": "hello world",
            "translated_text": "你好 世界",
            "render_protected_text": "protected hello <f1-abc/>",
            "_render_inner_bbox": [11.0, 21.0, 209.0, 79.0],
            "_render_text_color": [0.1, 0.2, 0.3],
            "_render_cover_fill": [0.9, 0.8, 0.7],
            "_render_first_line_indent_pt": 2.5,
            "_use_raw_text_bbox": true,
            "_force_plain_line": true,
            "_render_preserve_line_breaks": false,
            "_cover_with_inner_bbox": true,
            "_dense_small_box": true,
            "_heavy_dense_small_box": false,
            "should_translate": false,
            "skip_reason": "skip_display_formula",
            "translation_unit_kind": "group",
            "continuation_group": true,
            "continuation_group_id": "cg-1",
            "lines": [
                {"bbox": [10.0, 20.0, 200.0, 30.0], "spans": [
                    {"type": "text", "content": "hello"},
                    {"type": "inline_equation", "content": "x+1"}
                ]},
                {"bbox": [10.0, 30.0, 200.0, 40.0], "text": "fallback text"}
            ],
            "formula_map": [
                {"placeholder": "[[FORMULA_1]]", "formula_text": "x^2"}
            ],
            "render_formula_map": [
                {"placeholder": "__F2__", "formula_text": "y^3", "latex": "y^{3}"}
            ],
            "protected_map": [
                {"token_tag": "<g1-abc/>", "token_type": "term", "restore_text": "TermName"}
            ]
        })
    }

    #[test]
    fn from_json_parses_full_item() {
        let item = Item::from_json_value(&item_json());
        assert_eq!(item.bbox, Some([10.0, 20.0, 210.0, 80.0]));
        assert_eq!(item.source_text, "hello world");
        assert_eq!(item.translated_text, "你好 世界");
        assert_eq!(item.render_protected_text, "protected hello <f1-abc/>");
        assert_eq!(item.render_inner_bbox, Some([11.0, 21.0, 209.0, 79.0]));
        assert_eq!(item.render_text_color, (0.1, 0.2, 0.3));
        assert_eq!(item.render_cover_fill, (0.9, 0.8, 0.7));
        assert_eq!(item.render_first_line_indent_pt, 2.5);
        assert!(item.use_raw_text_bbox);
        assert!(item.force_plain_line);
        assert!(!item.render_preserve_line_breaks);
        assert!(item.cover_with_inner_bbox);
        assert!(item.dense_small_box);
        assert!(!item.heavy_dense_small_box);
        assert_eq!(item.should_translate, Some(false));
        assert_eq!(item.skip_reason.as_deref(), Some("skip_display_formula"));
        assert_eq!(item.translation_unit_kind.as_deref(), Some("group"));
        assert_eq!(item.continuation_group, Some(true));
        assert_eq!(item.continuation_group_id.as_deref(), Some("cg-1"));
    }

    #[test]
    fn from_json_parses_lines_both_shapes() {
        let item = Item::from_json_value(&item_json());
        assert_eq!(item.lines.len(), 2);
        assert_eq!(item.lines[0].bbox, Some([10.0, 20.0, 200.0, 30.0]));
        assert_eq!(item.lines[0].spans.len(), 2);
        assert_eq!(item.lines[0].spans[0].span_type, "text");
        assert_eq!(item.lines[0].spans[0].content, "hello");
        assert_eq!(item.lines[0].spans[1].span_type, "inline_equation");
        assert_eq!(item.lines[1].spans.len(), 1);
        assert_eq!(item.lines[1].spans[0].span_type, "text");
        assert_eq!(item.lines[1].spans[0].content, "fallback text");
    }

    #[test]
    fn from_json_parses_maps() {
        let item = Item::from_json_value(&item_json());
        assert_eq!(item.formula_map.len(), 1);
        assert_eq!(item.formula_map[0].placeholder, "[[FORMULA_1]]");
        assert_eq!(item.formula_map[0].formula_text, "x^2");
        assert_eq!(item.render_formula_map.len(), 1);
        assert_eq!(item.render_formula_map[0].placeholder, "__F2__");
        assert_eq!(item.render_formula_map[0].formula_text, "y^3");
        assert_eq!(item.protected_map.len(), 1);
        assert_eq!(item.protected_map[0].token_tag, "<g1-abc/>");
        assert_eq!(item.protected_map[0].token_type, "term");
        assert_eq!(item.protected_map[0].restore_text, "TermName");
        assert!(item.protected_map_has_token_tag);
    }

    #[test]
    fn from_json_defaults_for_missing_keys() {
        let item = Item::from_json_value(&serde_json::json!({"source_text": "x"}));
        assert_eq!(item.bbox, None);
        assert_eq!(item.lines.len(), 0);
        assert_eq!(item.should_translate, None);
        assert!(!item.use_raw_text_bbox);
        assert_eq!(item.render_text_color, (0.0, 0.0, 0.0));
        assert_eq!(item.render_cover_fill, (1.0, 1.0, 1.0));
        assert!(!item.protected_map_has_token_tag);
    }

    #[test]
    fn from_json_ignores_bad_bbox_shapes() {
        let item = Item::from_json_value(&serde_json::json!({
            "bbox": [1.0, 2.0],
            "_render_inner_bbox": [1.0, 2.0, 3.0, 4.0, 5.0]
        }));
        assert_eq!(item.bbox, None);
        assert_eq!(item.render_inner_bbox, None);
    }
}
