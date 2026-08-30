// Port of `provider_adapters/paddle/block_reader.py` — per-block spec building
// with text-role rules, provenance, layout/rich-content trace, and toc entries.

use rendering_core::payload::toc_document::build_toc_entries;
use rendering_core::text_flow::{
    classify_text_flow_for_role, line_texts_from_lines, py_splitlines, TEXT_FLOW_PRESERVE_LINES,
};
use serde_json::{json, Map, Value};

use super::super::common::{normalize_bbox, to_string_value};
use super::content_extract::{build_lines, build_segments, tighten_text_bbox};
use super::context::PaddlePageContext;
use super::page_trace::attach_layout_trace;
use super::rich_content::enrich_rich_content_trace;
use super::trace::{build_derived, build_metadata, build_source};

#[derive(Clone)]
struct PaddleTextRoleRule {
    layout_role: String,
    semantic_role: String,
    structure_role: String,
    translate: bool,
    translate_reason: String,
}

impl PaddleTextRoleRule {
    fn new(
        layout_role: &str,
        semantic_role: &str,
        structure_role: &str,
        translate: bool,
        translate_reason: &str,
    ) -> Self {
        PaddleTextRoleRule {
            layout_role: layout_role.into(),
            semantic_role: semantic_role.into(),
            structure_role: structure_role.into(),
            translate,
            translate_reason: translate_reason.into(),
        }
    }
}

impl Default for PaddleTextRoleRule {
    fn default() -> Self {
        PaddleTextRoleRule {
            layout_role: "unknown".into(),
            semantic_role: "unknown".into(),
            structure_role: String::new(),
            translate: false,
            translate_reason: String::new(),
        }
    }
}

/// `_TEXT_ROLE_BY_SUBTYPE`.
fn text_role_by_subtype(sub_type: &str) -> PaddleTextRoleRule {
    match sub_type {
        "title" => PaddleTextRoleRule::new("title", "unknown", "title", true, "provider_title_candidate"),
        "heading" => PaddleTextRoleRule::new("heading", "unknown", "heading", true, "provider_heading_candidate"),
        "body" => PaddleTextRoleRule::new("paragraph", "body", "body", true, "provider_body_whitelist:body"),
        "table_of_contents" => PaddleTextRoleRule::new(
            "toc",
            "table_of_contents",
            "table_of_contents",
            true,
            "provider_toc_whitelist:content",
        ),
        "header" => PaddleTextRoleRule::new("header", "metadata", "", false, ""),
        "footer" => PaddleTextRoleRule::new("footer", "metadata", "", false, ""),
        "page_number" => PaddleTextRoleRule::new("page_number", "metadata", "", false, ""),
        "metadata" => PaddleTextRoleRule::new("unknown", "metadata", "", false, ""),
        "formula_number" => PaddleTextRoleRule::new("unknown", "metadata", "", false, ""),
        "reference_entry" => PaddleTextRoleRule::new("unknown", "reference", "reference_entry", false, ""),
        "figure_caption" => PaddleTextRoleRule::new(
            "caption",
            "unknown",
            "figure_caption",
            true,
            "provider_caption_whitelist:figure_caption",
        ),
        "caption" => PaddleTextRoleRule::new("caption", "unknown", "caption", false, ""),
        "image_caption" => PaddleTextRoleRule::new("caption", "unknown", "caption", false, ""),
        "table_caption" => PaddleTextRoleRule::new("caption", "unknown", "caption", false, ""),
        "code_caption" => PaddleTextRoleRule::new("unknown", "unknown", "caption", false, ""),
        "footnote" => PaddleTextRoleRule::new("footnote", "unknown", "footnote", false, ""),
        "image_footnote" => PaddleTextRoleRule::new(
            "footnote",
            "unknown",
            "footnote",
            true,
            "provider_footnote_whitelist:image_footnote",
        ),
        "table_footnote" => PaddleTextRoleRule::new(
            "footnote",
            "unknown",
            "footnote",
            true,
            "provider_footnote_whitelist:table_footnote",
        ),
        _ => PaddleTextRoleRule::default(),
    }
}

/// `_TEXT_ROLE_BY_RAW_LABEL`.
fn text_role_by_raw_label(label: &str) -> Option<PaddleTextRoleRule> {
    match label {
        "abstract" => Some(PaddleTextRoleRule::new(
            "paragraph",
            "abstract",
            "body",
            true,
            "provider_body_whitelist:abstract",
        )),
        "footnote" => Some(PaddleTextRoleRule::new(
            "footnote",
            "unknown",
            "footnote",
            false,
            "provider_non_body:footnote",
        )),
        _ => None,
    }
}

/// `_VISION_FOOTNOTE_FALLBACK_RULE`.
fn vision_footnote_fallback_rule() -> PaddleTextRoleRule {
    PaddleTextRoleRule::new(
        "footnote",
        "unknown",
        "footnote",
        true,
        "provider_footnote_whitelist:vision_footnote",
    )
}

/// `_merge_role_rule`.
fn merge_role_rule(base: &PaddleTextRoleRule, override_: Option<&PaddleTextRoleRule>) -> PaddleTextRoleRule {
    match override_ {
        None => base.clone(),
        Some(o) => PaddleTextRoleRule {
            layout_role: if o.layout_role != "unknown" {
                o.layout_role.clone()
            } else {
                base.layout_role.clone()
            },
            semantic_role: if o.semantic_role != "unknown" {
                o.semantic_role.clone()
            } else {
                base.semantic_role.clone()
            },
            structure_role: if o.structure_role.is_empty() {
                base.structure_role.clone()
            } else {
                o.structure_role.clone()
            },
            translate: o.translate,
            translate_reason: if o.translate_reason.is_empty() {
                base.translate_reason.clone()
            } else {
                o.translate_reason.clone()
            },
        },
    }
}

/// `_paddle_text_role_rule`.
fn paddle_text_role_rule(raw_label: &str, block_type: &str, sub_type: &str) -> PaddleTextRoleRule {
    if block_type != "text" {
        let bt = if block_type.is_empty() { "unknown".to_string() } else { block_type.to_string() };
        return PaddleTextRoleRule {
            translate: false,
            translate_reason: format!("provider_non_text:{bt}"),
            ..PaddleTextRoleRule::default()
        };
    }
    let label = raw_label.trim().to_lowercase();
    let base = text_role_by_subtype(sub_type);
    let override_ = if label == "vision_footnote" && sub_type == "footnote" {
        Some(vision_footnote_fallback_rule())
    } else {
        text_role_by_raw_label(&label)
    };
    let mut rule = merge_role_rule(&base, override_.as_ref());
    if rule.translate_reason.is_empty() {
        let subject = if sub_type.is_empty() {
            if label.is_empty() { "unknown".to_string() } else { label.clone() }
        } else {
            sub_type.to_string()
        };
        rule.translate = false;
        rule.translate_reason = format!("provider_non_body:{subject}");
    }
    rule
}

/// `_build_provenance`.
fn build_provenance(source: &Value, raw_label: &str) -> Value {
    let raw_bbox = match source.get("raw_bbox") {
        Some(Value::Array(arr)) if !arr.is_empty() => Value::Array(arr.clone()),
        _ => json!([0, 0, 0, 0]),
    };
    json!({
        "provider": to_string_value(source.get("provider")),
        "raw_label": raw_label,
        "raw_sub_type": to_string_value(source.get("raw_sub_type")),
        "raw_bbox": raw_bbox,
        "raw_path": to_string_value(source.get("raw_path")),
    })
}

/// `_apply_normalized_paddle_signals`.
fn apply_normalized_paddle_signals(metadata: &mut Map<String, Value>) {
    let b = |m: &Map<String, Value>, key: &str| m.get(key).and_then(Value::as_bool).unwrap_or(false);
    let s = |m: &Map<String, Value>, key: &str| to_string_value(m.get(key));
    let cross = b(metadata, "provider_cross_column_merge_suspected");
    let reading = b(metadata, "provider_reading_order_unreliable");
    let structure = b(metadata, "provider_structure_unreliable");
    let text_missing = b(metadata, "provider_text_missing_but_bbox_present");
    let peer_absorbed = b(metadata, "provider_peer_block_absorbed_text");
    let repair_attempted = b(metadata, "provider_body_repair_attempted");
    let repair_applied = b(metadata, "provider_body_repair_applied");
    let repair_role = s(metadata, "provider_body_repair_role");
    let repair_strategy = s(metadata, "provider_body_repair_strategy");
    let repair_peer_block = s(metadata, "provider_suspected_peer_block_id");
    let cont_suppressed = b(metadata, "provider_continuation_suppressed");
    let cont_reason = s(metadata, "provider_continuation_suppressed_reason");
    let column_mode = s(metadata, "provider_column_layout_mode");
    let column_index = s(metadata, "provider_column_index_guess");
    metadata.insert("cross_column_merge_suspected".to_string(), Value::Bool(cross));
    metadata.insert("reading_order_unreliable".to_string(), Value::Bool(reading));
    metadata.insert("structure_unreliable".to_string(), Value::Bool(structure));
    metadata.insert("text_missing_but_bbox_present".to_string(), Value::Bool(text_missing));
    metadata.insert("peer_block_absorbed_text".to_string(), Value::Bool(peer_absorbed));
    metadata.insert("body_repair_attempted".to_string(), Value::Bool(repair_attempted));
    metadata.insert("body_repair_applied".to_string(), Value::Bool(repair_applied));
    metadata.insert("body_repair_role".to_string(), Value::String(repair_role));
    metadata.insert("body_repair_strategy".to_string(), Value::String(repair_strategy));
    metadata.insert("body_repair_peer_block_id".to_string(), Value::String(repair_peer_block));
    metadata.insert("continuation_suppressed".to_string(), Value::Bool(cont_suppressed));
    metadata.insert("continuation_suppressed_reason".to_string(), Value::String(cont_reason));
    metadata.insert("column_layout_mode".to_string(), Value::String(column_mode));
    metadata.insert("column_index_guess".to_string(), Value::String(column_index));
}

/// `build_block_metadata` — trace/metadata for a single block spec.
fn build_block_metadata(
    page_context: &PaddlePageContext,
    order: usize,
    kind_metadata: &Map<String, Value>,
) -> Value {
    let block = &page_context.parsing_res_list[order];
    let raw_label = to_string_value(block.get("block_label"));
    let text = to_string_value(block.get("block_content")).trim().to_string();
    let bbox = normalize_bbox(block.get("block_bbox"));
    let mut metadata = match build_metadata(block, kind_metadata) {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let block_signals = page_context
        .column_signals
        .get("block_signals")
        .and_then(Value::as_object);
    if let Some(signals) = block_signals
        .and_then(|m| m.get(&order.to_string()))
        .and_then(Value::as_object)
    {
        for (key, value) in signals {
            metadata.insert(key.clone(), value.clone());
        }
    }
    if let Some(repair) = page_context.repair_metadata.get(&(order as i64)) {
        for (key, value) in repair {
            metadata.insert(key.clone(), value.clone());
        }
    }
    attach_layout_trace(&mut metadata, &bbox, &page_context.layout_box_lookup);
    enrich_rich_content_trace(
        &mut metadata,
        &raw_label,
        &text,
        &page_context.markdown_images,
        &page_context.markdown_text,
    );
    let peer_order = metadata.get("provider_suspected_peer_order").and_then(Value::as_i64);
    let peer_block_id = match peer_order {
        Some(p) if p >= 0 => format!("p{:03}-b{:04}", page_context.page_index + 1, p),
        _ => String::new(),
    };
    metadata.insert("provider_suspected_peer_block_id".to_string(), Value::String(peer_block_id));
    apply_normalized_paddle_signals(&mut metadata);
    Value::Object(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::normalize::paddle::block_labels::map_block_kind;

    fn rule(raw_label: &str, block_type: &str, sub_type: &str) -> PaddleTextRoleRule {
        paddle_text_role_rule(raw_label, block_type, sub_type)
    }

    #[test]
    fn role_rule_by_subtype() {
        let title = rule("doc_title", "text", "title");
        assert_eq!(title.layout_role, "title");
        assert_eq!(title.structure_role, "title");
        assert!(title.translate);
        assert_eq!(title.translate_reason, "provider_title_candidate");

        let body = rule("text", "text", "body");
        assert_eq!(body.semantic_role, "body");
        assert_eq!(body.layout_role, "paragraph");
        assert_eq!(body.translate_reason, "provider_body_whitelist:body");

        let caption = rule("figure_title", "text", "figure_caption");
        assert_eq!(caption.layout_role, "caption");
        assert_eq!(caption.translate_reason, "provider_caption_whitelist:figure_caption");

        let footer = rule("footer", "text", "footer");
        assert_eq!(footer.semantic_role, "metadata");
        assert!(!footer.translate);
    }

    #[test]
    fn role_rule_override_by_raw_label() {
        let abstract_ = rule("abstract", "text", "body");
        assert_eq!(abstract_.semantic_role, "abstract");
        assert_eq!(abstract_.structure_role, "body");
        assert_eq!(abstract_.translate_reason, "provider_body_whitelist:abstract");

        let footnote = rule("footnote", "text", "footnote");
        assert_eq!(footnote.structure_role, "footnote");
        assert_eq!(footnote.translate_reason, "provider_non_body:footnote");

        let vision = rule("vision_footnote", "text", "footnote");
        assert!(vision.translate);
        assert_eq!(vision.translate_reason, "provider_footnote_whitelist:vision_footnote");
    }

    #[test]
    fn role_rule_non_text_and_unknown() {
        let image = rule("image", "image", "image_body");
        assert!(!image.translate);
        assert_eq!(image.translate_reason, "provider_non_text:image");

        let unknown_sub = rule("text", "text", "");
        assert!(!unknown_sub.translate);
        assert_eq!(unknown_sub.translate_reason, "provider_non_body:text");
    }

    fn context_with(blocks: Vec<Value>) -> PaddlePageContext {
        let kinds = blocks.iter().enumerate().map(|(order, block)| {
            let label = block.get("block_label").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
            let text = block.get("block_content").and_then(Value::as_str).unwrap_or("");
            map_block_kind(&label, text)
        }).collect();
        PaddlePageContext {
            page_index: 0,
            page_payload: json!({}),
            page_meta: json!({"width": 595.0, "height": 842.0}),
            preprocessed_image: String::new(),
            pruned: json!({}),
            parsing_res_list: blocks,
            layout_box_lookup: Vec::new(),
            markdown_text: String::new(),
            markdown_images: Map::new(),
            classified_kinds: kinds,
            column_signals: json!({}),
            repair_metadata: BTreeMap::new(),
            repair_summary: json!({"body_repair_pair_count": 0, "body_repair_pairs": [], "body_repair_block_count": 0}),
        }
    }

    #[test]
    fn build_block_spec_title_full_shape() {
        let ctx = context_with(vec![json!({
            "block_label": "doc_title",
            "block_content": "Understanding Systems",
            "block_bbox": [40.0, 40.0, 560.0, 90.0],
            "block_id": "b0",
            "group_id": "g0",
            "block_order": 3,
        })]);
        let spec = build_block_spec(&ctx, 0);
        assert_eq!(spec["block_id"], "p001-b0000");
        assert_eq!(spec["page_index"], 0);
        assert_eq!(spec["order"], 0);
        assert_eq!(spec["block_type"], "text");
        assert_eq!(spec["sub_type"], "title");
        assert_eq!(spec["text"], "Understanding Systems");
        assert_eq!(spec["layout_role"], "title");
        assert_eq!(spec["semantic_role"], "unknown");
        assert_eq!(spec["structure_role"], "title");
        assert_eq!(spec["policy"]["translate"], true);
        assert_eq!(spec["derived"]["role"], "title");
        assert_eq!(spec["provenance"]["raw_label"], "doc_title");
        assert_eq!(spec["provenance"]["raw_bbox"], json!([40.0, 40.0, 560.0, 90.0]));
        assert_eq!(spec["metadata"]["raw_group_id"], "g0");
        assert_eq!(spec["metadata"]["raw_block_order"], 3);
        assert_eq!(spec["metadata"]["layout_role"], "title");
        assert_eq!(spec["source"]["raw_path"], "/layoutParsingResults/0/prunedResult/parsing_res_list/0");
        assert_eq!(spec["content"]["kind"], "text");
        // single line -> flow
        assert_eq!(spec["content"]["text_flow"], "flow");
    }

    #[test]
    fn build_block_spec_toc_builds_entries_and_preserves_lines() {
        let ctx = context_with(vec![json!({
            "block_label": "content",
            "block_content": "1 Introduction 1\n2 Methods 2\n3 Results 3",
            "block_bbox": [40.0, 100.0, 560.0, 140.0],
        })]);
        let spec = build_block_spec(&ctx, 0);
        assert_eq!(spec["sub_type"], "table_of_contents");
        assert_eq!(spec["semantic_role"], "table_of_contents");
        assert_eq!(spec["content"]["text_flow"], "preserve_lines");
        let toc = spec["content"]["toc_entries"].as_array().unwrap();
        assert!(!toc.is_empty());
        assert_eq!(toc[0]["number"], "1");
        assert_eq!(toc[0]["title"], "Introduction");
        assert_eq!(spec["metadata"]["policy_translate"], true);
    }

    #[test]
    fn build_block_spec_image_non_text_policy() {
        let ctx = context_with(vec![json!({
            "block_label": "image",
            "block_content": "<img src=\"fig.png\" />",
            "block_bbox": [40.0, 200.0, 300.0, 320.0],
        })]);
        let spec = build_block_spec(&ctx, 0);
        assert_eq!(spec["block_type"], "image");
        assert_eq!(spec["sub_type"], "image_body");
        assert_eq!(spec["policy"]["translate"], false);
        assert_eq!(spec["policy"]["translate_reason"], "provider_non_text:image");
        assert_eq!(spec["lines"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn build_block_metadata_applies_normalized_signals() {
        let mut ctx = context_with(vec![json!({
            "block_label": "text",
            "block_content": "",
            "block_bbox": [40.0, 100.0, 250.0, 150.0],
        })]);
        ctx.column_signals = json!({
            "block_signals": {
                "0": {
                    "provider_cross_column_merge_suspected": true,
                    "provider_reading_order_unreliable": true,
                    "provider_structure_unreliable": true,
                    "provider_column_layout_mode": "double",
                    "provider_column_index_guess": "left",
                    "provider_suspected_peer_order": 1,
                }
            }
        });
        let mut repair_meta = BTreeMap::new();
        let mut m = Map::new();
        m.insert("provider_body_repair_applied".into(), Value::Bool(true));
        repair_meta.insert(0, m);
        ctx.repair_metadata = repair_meta;
        ctx.repair_summary = json!({"body_repair_pair_count": 1, "body_repair_pairs": [{"absorber_order": 1}], "body_repair_block_count": 1});

        let metadata = build_block_metadata(&ctx, 0, &Map::new());
        assert_eq!(metadata["cross_column_merge_suspected"], true);
        assert_eq!(metadata["reading_order_unreliable"], true);
        assert_eq!(metadata["column_layout_mode"], "double");
        assert_eq!(metadata["column_index_guess"], "left");
        assert_eq!(metadata["body_repair_applied"], true);
        assert_eq!(metadata["provider_suspected_peer_block_id"], "p001-b0001");
    }
}

/// `build_block_spec` — the `NormalizedBlockSpec` consumed by `build_block_record`.
pub fn build_block_spec(page_context: &PaddlePageContext, order: usize) -> Value {
    let block = &page_context.parsing_res_list[order];
    let raw_label = to_string_value(block.get("block_label"));
    let text = to_string_value(block.get("block_content")).trim().to_string();
    let kind = &page_context.classified_kinds[order];
    let block_type = kind.block_type.clone();
    let sub_type = kind.sub_type.clone();
    let tags = kind.tags.clone();
    let kind_metadata = kind.metadata.clone();

    let bbox = tighten_text_bbox(
        &normalize_bbox(block.get("block_bbox")),
        &text,
        &block_type,
        &sub_type,
    );
    let segments = build_segments(&text, &raw_label);
    let lines = build_lines(&bbox, &segments, &text, &raw_label, &block_type, &sub_type);
    let lines_value = Value::Array(lines.clone());
    let explicit_line_texts: Vec<String> = py_splitlines(&text)
        .into_iter()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    let line_texts: Vec<String> = if explicit_line_texts.len() >= 2 {
        explicit_line_texts
    } else {
        line_texts_from_lines(&lines_value)
    };

    let metadata = build_block_metadata(page_context, order, &kind_metadata);
    let source = build_source(block, page_context.page_index, &raw_label, &bbox, &text, order);

    let role_rule = paddle_text_role_rule(&raw_label, &block_type, &sub_type);
    let layout_role = role_rule.layout_role.clone();
    let semantic_role = role_rule.semantic_role.clone();
    let structure_role = role_rule.structure_role.clone();
    let mut text_flow = classify_text_flow_for_role(&text, &lines_value, &semantic_role, &structure_role);

    let line_texts_value = Value::Array(line_texts.iter().map(|s| Value::String(s.clone())).collect());
    let toc_entries = if sub_type == "table_of_contents" {
        build_toc_entries(&lines_value, &line_texts_value)
    } else {
        vec![]
    };
    if !toc_entries.is_empty() {
        text_flow = TEXT_FLOW_PRESERVE_LINES;
    }

    let mut metadata = metadata;
    let metadata_obj = metadata.as_object_mut().expect("metadata object");
    metadata_obj.insert("structure_role".to_string(), Value::String(structure_role.clone()));
    metadata_obj.insert("layout_role".to_string(), Value::String(layout_role.clone()));
    metadata_obj.insert("semantic_role".to_string(), Value::String(semantic_role.clone()));
    metadata_obj.insert("policy_translate".to_string(), Value::Bool(role_rule.translate));

    let mut content = Map::new();
    content.insert("kind".to_string(), Value::String(block_type.clone()));
    content.insert("text".to_string(), Value::String(text.clone()));
    if !line_texts.is_empty() {
        content.insert(
            "line_texts".to_string(),
            Value::Array(line_texts.iter().map(|s| Value::String(s.clone())).collect()),
        );
        content.insert("text_flow".to_string(), Value::String(text_flow.to_string()));
    }
    if !toc_entries.is_empty() {
        content.insert("toc_entries".to_string(), Value::Array(toc_entries));
    }

    json!({
        "block_id": format!("p{:03}-b{:04}", page_context.page_index + 1, order),
        "page_index": page_context.page_index,
        "order": order,
        "reading_order": order,
        "block_type": block_type,
        "sub_type": sub_type,
        "bbox": Value::Array(bbox.iter().map(|f| Value::from(*f)).collect()),
        "geometry": json!({ "bbox": Value::Array(bbox.iter().map(|f| Value::from(*f)).collect()) }),
        "content": Value::Object(content),
        "text": text,
        "lines": lines,
        "segments": segments,
        "tags": Value::Array(tags.iter().map(|t| Value::String(t.clone())).collect()),
        "layout_role": layout_role,
        "semantic_role": semantic_role,
        "structure_role": structure_role,
        "policy": json!({
            "translate": role_rule.translate,
            "translate_reason": role_rule.translate_reason,
        }),
        "derived": build_derived(&raw_label, &sub_type),
        "metadata": metadata,
        "source": source,
        "provenance": build_provenance(&source, &raw_label),
    })
}
