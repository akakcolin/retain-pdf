// Port of `provider_adapters/paddle/relations.py` — block-kind resolution with
// previous-anchor context, front-matter/metadata cues, and vision-footnote
// target resolution.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::super::common::classify_with_previous_anchor;
use super::block_labels::{map_block_kind, BlockKind};

fn metadata_text_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:^keywords?\s*:|^doi:|^cite this article as:|submit your manuscript here|open access|copyright|authors declare|competing interests?|competing financial interest|funded by|received:|accepted:|published:|supporting information is available free of charge|e-mail:|orcid)",
        ).expect("paddle metadata text regex")
    })
}

fn metadata_bullet_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^[•▪◦]\s*(?:keywords?\s*:|doi:|cite this article as:|submit your manuscript here|open access|copyright|authors declare|competing interests?|competing financial interest|funded by|received:|accepted:|published:|supporting information is available free of charge|e-mail:|orcid)",
        ).expect("paddle metadata bullet regex")
    })
}

fn ascii_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[A-Za-z0-9]+(?:[-'][A-Za-z0-9]+)?").expect("ascii word regex"))
}

fn looks_like_ancillary_tail_heading(text: &str) -> bool {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_lowercase();
    matches!(
        compact.as_str(),
        "competing interests" | "authors' contributions" | "acknowledgments" | "references"
    )
}

fn looks_like_metadata_text(text: &str) -> bool {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_string();
    if compact.is_empty() {
        return false;
    }
    if is_short_metadata_bullet(&compact) {
        return true;
    }
    metadata_text_re().is_match(&compact)
}

fn ascii_word_count(text: &str) -> usize {
    ascii_word_re().find_iter(text).count()
}

fn is_short_metadata_bullet(text: &str) -> bool {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_string();
    if compact.is_empty() || !metadata_bullet_re().is_match(&compact) {
        return false;
    }
    ascii_word_count(&compact) <= 24
}

/// `resolve_figure_title`.
fn resolve_figure_title() -> BlockKind {
    BlockKind {
        block_type: "text".into(),
        sub_type: "figure_caption".into(),
        tags: vec!["caption".into(), "figure_caption".into()],
        metadata: map_one("caption_target", "figure"),
    }
}

/// `resolve_vision_footnote`.
fn resolve_vision_footnote(text: &str, previous_anchor: Option<&(String, i64)>) -> BlockKind {
    let lowered = text.to_lowercase();
    if lowered.starts_with("表注") || lowered.contains("table") {
        return footnotes_kind("table_footnote", "table");
    }
    if lowered.starts_with("图注") || lowered.contains("figure") {
        return footnotes_kind("image_footnote", "image");
    }
    if let Some((target, _)) = previous_anchor {
        if target == "table_html" || target == "table" {
            return footnotes_kind("table_footnote", "table");
        }
        if target == "image_body" || target == "image" {
            return footnotes_kind("image_footnote", "image");
        }
    }
    footnotes_kind("footnote", "unknown")
}

fn footnotes_kind(sub_type: &str, target: &str) -> BlockKind {
    let tags = match sub_type {
        "table_footnote" => vec!["footnote".to_string(), "table_footnote".to_string()],
        "image_footnote" => vec!["footnote".to_string(), "image_footnote".to_string()],
        _ => vec!["footnote".to_string()],
    };
    BlockKind {
        block_type: "text".into(),
        sub_type: sub_type.into(),
        tags,
        metadata: map_one("footnote_target", target),
    }
}

fn map_one(key: &str, value: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(key.to_string(), Value::String(value.to_string()));
    map
}

/// `_body_flow_start_order`.
fn body_flow_start_order(parsing_res_list: &[Value]) -> i64 {
    let mut has_front_matter = false;
    let mut front_matter_end_order: i64 = -1;
    for (order, block) in parsing_res_list.iter().enumerate() {
        let label = block_label_lower(block);
        let text = block_text_lower(block);
        if label == "doc_title" || label == "abstract" {
            has_front_matter = true;
            front_matter_end_order = order as i64;
            continue;
        }
        if label == "paragraph_title" && text == "abstract" {
            has_front_matter = true;
            front_matter_end_order = order as i64;
            continue;
        }
    }
    if !has_front_matter {
        return -1;
    }
    for (order, block) in parsing_res_list.iter().enumerate() {
        if (order as i64) <= front_matter_end_order {
            continue;
        }
        let label = block_label_lower(block);
        let text = block_text_lower(block);
        if label == "paragraph_title" && !text.is_empty() && text != "abstract"
            && !looks_like_ancillary_tail_heading(&text)
        {
            return order as i64;
        }
        if (label == "text" || label == "abstract") && !text.is_empty()
            && !looks_like_metadata_text(&text)
        {
            return order as i64;
        }
    }
    -1
}

fn block_label_lower(block: &Value) -> String {
    block
        .get("block_label")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

fn block_text_lower(block: &Value) -> String {
    block
        .get("block_content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

/// `_resolve_block_kind`.
fn resolve_block_kind(
    block: &Value,
    previous_anchor: Option<&(String, i64)>,
    order: i64,
    body_flow_start: i64,
) -> BlockKind {
    let raw_label = block.get("block_label").and_then(Value::as_str).unwrap_or("");
    let text = block
        .get("block_content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let label = raw_label.trim().to_lowercase();
    if label == "text" && body_flow_start >= 0 && 0 <= order && order < body_flow_start {
        return metadata_kind("front_matter_text");
    }
    if label == "text" && looks_like_metadata_text(&text) {
        return metadata_kind("metadata_text_cue");
    }
    if label == "paragraph_title" && looks_like_ancillary_tail_heading(&text) {
        return metadata_kind("ancillary_tail_heading");
    }
    if label == "figure_title" {
        return resolve_figure_title();
    }
    if label == "vision_footnote" {
        return resolve_vision_footnote(&text, previous_anchor);
    }
    map_block_kind(raw_label, &text)
}

fn metadata_kind(key: &str) -> BlockKind {
    let mut metadata = Map::new();
    metadata.insert(key.to_string(), Value::Bool(true));
    metadata.insert("skip_translation".to_string(), Value::Bool(true));
    BlockKind {
        block_type: "text".into(),
        sub_type: "metadata".into(),
        tags: vec!["metadata".into(), "skip_translation".into()],
        metadata,
    }
}

/// `classify_page_blocks`.
pub fn classify_page_blocks(parsing_res_list: &[Value]) -> Vec<BlockKind> {
    let body_flow_start = body_flow_start_order(parsing_res_list);
    let enriched: Vec<Value> = parsing_res_list
        .iter()
        .enumerate()
        .map(|(order, block)| {
            let mut b = block.clone();
            if let Some(obj) = b.as_object_mut() {
                obj.insert("__rp_order__".to_string(), Value::from(order as i64));
            }
            b
        })
        .collect();
    classify_with_previous_anchor(
        &enriched,
        &|block, previous_anchor| {
            let order = block
                .get("__rp_order__")
                .and_then(Value::as_i64)
                .unwrap_or(-1);
            let kind = resolve_block_kind(block, previous_anchor.as_ref(), order, body_flow_start);
            Value::Object(kind_to_map(&kind))
        },
        &|value| {
            let block_type = value.get("block_type").and_then(Value::as_str).unwrap_or("");
            let sub_type = value.get("sub_type").and_then(Value::as_str).unwrap_or("");
            Some((block_type.to_string(), sub_type.to_string()))
        },
    )
    .into_iter()
    .map(|value| map_to_kind(&value))
    .collect()
}

fn kind_to_map(kind: &BlockKind) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("block_type".to_string(), Value::String(kind.block_type.clone()));
    map.insert("sub_type".to_string(), Value::String(kind.sub_type.clone()));
    map.insert(
        "tags".to_string(),
        Value::Array(kind.tags.iter().map(|t| Value::String(t.clone())).collect()),
    );
    map.insert("metadata".to_string(), Value::Object(kind.metadata.clone()));
    map
}

fn map_to_kind(value: &Value) -> BlockKind {
    BlockKind {
        block_type: value.get("block_type").and_then(Value::as_str).unwrap_or("").to_string(),
        sub_type: value.get("sub_type").and_then(Value::as_str).unwrap_or("").to_string(),
        tags: value
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default(),
        metadata: value
            .get("metadata")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
    }
}
