//! Page-spec → redaction-item conversion (port of `redaction_items.py`,
//! `redaction_plan.py::redaction_item_from_render_block`, and the
//! `block_view.py` helpers restricted to the keys the chain consumes) plus the
//! visual-profile fill key-order matching
//! (`visual_profile/runtime.py::background_fill_for_item`).
//!
//! The Python shim sends the ORIGINAL translated items plus the raw
//! `RenderPageSpec` JSON and a flat first-wins fill map; this module performs
//! the page-spec replacement the shim used to precompute in 7R-7, so the
//! bridge consumes page specs natively. The formula-source pages are the
//! un-replaced originals (production `stage.py` reads
//! `translated_pages[page_index]` for formula detection).

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

use super::dto::RedactionItem;

/// `models.py::RenderLayoutBlock` — only the keys the conversion chain reads
/// (`block_view.py::layout_block_to_render_block` maps `cover_bbox =
/// background_rect`, `markdown_text = content_text`, `render_kind =
/// content_kind`).
#[derive(Debug, Clone, Deserialize)]
pub struct RenderLayoutBlock {
    #[serde(default)]
    pub block_id: String,
    #[serde(default)]
    pub background_rect: Vec<f64>,
    #[serde(default)]
    pub content_rect: Vec<f64>,
    #[serde(default)]
    pub content_kind: String,
    #[serde(default)]
    pub content_text: String,
    #[serde(default)]
    pub plain_text: String,
}

/// `models.py::RenderPageSpec` — only `page_index` + `blocks` are consumed
/// (remaining fields are ignored by serde).
#[derive(Debug, Clone, Deserialize)]
pub struct RenderPageSpec {
    #[serde(default)]
    pub page_index: i32,
    #[serde(default)]
    pub blocks: Vec<RenderLayoutBlock>,
}

/// `block_view.py::render_block_protected_text`: `markdown_text if
/// render_kind == "markdown" else plain_text`.
fn protected_text(block: &RenderLayoutBlock) -> String {
    if block.content_kind == "markdown" {
        block.content_text.clone()
    } else {
        block.plain_text.clone()
    }
}

/// `redaction_plan.py::redaction_item_from_render_block`. `**source_item`
/// inherits every source field; the merge overrides the id/kind/text/bbox
/// keys production sets. `source_item_id = source_item.get("item_id")` maps an
/// absent/empty source id to `None` (the corpus never carries an explicit
/// empty `item_id`). `render_protected_text` mirrors `protected_translated_text`
/// (production sets both to `render_block_protected_text`); `_render_block_id`
/// and `_render_block_index` are dropped from the DTO consistently on both sides.
fn redaction_item_from_render_block(
    block: &RenderLayoutBlock,
    source_item: Option<&RedactionItem>,
) -> RedactionItem {
    let protected = protected_text(block);
    let mut out = source_item.cloned().unwrap_or_default();
    out.item_id = block.block_id.clone();
    out.source_item_id = source_item
        .filter(|s| !s.item_id.is_empty())
        .map(|s| s.item_id.clone());
    out.block_kind = "render_block".to_string();
    out.block_type = "render_block".to_string();
    let source_text = source_item
        .map(|s| s.source_text.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            source_item
                .map(|s| s.protected_source_text.as_str())
                .filter(|s| !s.is_empty())
        });
    out.source_text = source_text.map(str::to_string).unwrap_or_else(|| block.plain_text.clone());
    out.translated_text = block.plain_text.clone();
    out.protected_translated_text = protected.clone();
    out.render_protected_text = protected;
    out.bbox = Some(block.background_rect.clone());
    out
}

/// `redaction_items.py::redaction_items_from_layout_blocks`. `source_by_id`
/// compresses duplicate ids (later wins), mirroring the Python dict
/// comprehension. The `raw_id.is_ascii_digit()` vs Python `.isdigit()`
/// (non-ASCII digits) is a documented micro-divergence unreachable in the
/// corpus.
pub fn redaction_items_from_layout_blocks(
    translated_items: &[RedactionItem],
    blocks: &[RenderLayoutBlock],
) -> Vec<RedactionItem> {
    let mut source_by_id: HashMap<String, usize> = HashMap::new();
    for (index, item) in translated_items.iter().enumerate() {
        source_by_id.insert(item.item_id.clone(), index);
    }
    let mut out = Vec::new();
    for block in blocks {
        if block.background_rect.len() != 4 {
            continue;
        }
        let mut source_item: Option<&RedactionItem> = None;
        if let Some(raw_id) = block.block_id.strip_prefix("item-") {
            if let Some(&i) = source_by_id.get(raw_id) {
                source_item = Some(&translated_items[i]);
            } else if !raw_id.is_empty() && raw_id.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(i) = raw_id.parse::<usize>() {
                    source_item = translated_items.get(i);
                }
            }
        }
        out.push(redaction_item_from_render_block(block, source_item));
    }
    out
}

/// `runtime.py::background_fill_for_item`: key order `source_item_id →
/// item_id → block_id`, empty/absent keys skipped, first hit returned.
pub fn background_fill_for_item(
    item: &RedactionItem,
    fill_map: &HashMap<String, [f64; 3]>,
) -> Option<[f64; 3]> {
    if let Some(id) = &item.source_item_id {
        if !id.is_empty() {
            if let Some(f) = fill_map.get(id) {
                return Some(*f);
            }
        }
    }
    if !item.item_id.is_empty() {
        if let Some(f) = fill_map.get(&item.item_id) {
            return Some(*f);
        }
    }
    if !item.block_id.is_empty() {
        if let Some(f) = fill_map.get(&item.block_id) {
            return Some(*f);
        }
    }
    None
}

/// Bridge-facing preparation: for every page, formula source = the original
/// items (no fill); redaction = page-spec-replaced items (or the originals
/// when the page has no spec), each annotated with `visual_profile_fill` from
/// the key-order match. `specs_by_page` later-wins on duplicate page indices.
pub fn apply_page_specs_and_fills(
    translated_pages: &BTreeMap<i32, Vec<RedactionItem>>,
    page_specs: &[RenderPageSpec],
    fill_map: &HashMap<String, [f64; 3]>,
) -> (BTreeMap<i32, Vec<RedactionItem>>, BTreeMap<i32, Vec<RedactionItem>>) {
    let specs_by_page: HashMap<i32, &RenderPageSpec> =
        page_specs.iter().map(|s| (s.page_index, s)).collect();
    let mut redaction = BTreeMap::new();
    let mut formula_source = BTreeMap::new();
    for (&page_index, items) in translated_pages {
        formula_source.insert(page_index, items.clone());
        let base = match specs_by_page.get(&page_index) {
            Some(spec) => redaction_items_from_layout_blocks(items, &spec.blocks),
            None => items.clone(),
        };
        let filled = base
            .into_iter()
            .map(|mut item| {
                item.visual_profile_fill = background_fill_for_item(&item, fill_map);
                item
            })
            .collect();
        redaction.insert(page_index, filled);
    }
    (redaction, formula_source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(json: &str) -> RedactionItem {
        serde_json::from_str(json).expect("parse")
    }

    fn block(block_id: &str, rect: &[f64], kind: &str, content: &str) -> RenderLayoutBlock {
        RenderLayoutBlock {
            block_id: block_id.to_string(),
            background_rect: rect.to_vec(),
            content_rect: rect.to_vec(),
            content_kind: kind.to_string(),
            content_text: content.to_string(),
            plain_text: content.to_string(),
        }
    }

    #[test]
    fn block_replacement_produces_render_item() {
        let items = vec![item(r#"{"bbox":[10.0,100.0,50.0,120.0],"translated_text":"Alpha"}"#)];
        let blocks = vec![block("block-0", &[50.0, 85.0, 320.0, 200.0], "text", "Alpha Beta")];
        let out = redaction_items_from_layout_blocks(&items, &blocks);
        assert_eq!(out.len(), 1);
        let it = &out[0];
        assert_eq!(it.item_id, "block-0");
        assert_eq!(it.source_item_id, None);
        assert_eq!(it.block_kind, "render_block");
        assert_eq!(it.block_type, "render_block");
        assert_eq!(it.source_text, "Alpha Beta");
        assert_eq!(it.translated_text, "Alpha Beta");
        assert_eq!(it.protected_translated_text, "Alpha Beta");
        assert_eq!(it.bbox.as_deref(), Some(&[50.0, 85.0, 320.0, 200.0][..]));
        assert_eq!(it.visual_profile_fill, None);
    }

    #[test]
    fn source_item_merge_preserves_source_fields() {
        let items = vec![item(
            r#"{"bbox":[10.0,100.0,50.0,120.0],"translated_text":"Alpha","item_id":"line-0",
                "block_kind":"text","block_type":"paragraph","raw_block_type":"text",
                "normalized_sub_type":"body","continuation_group":"grp-1",
                "source_text":"Original","protected_source_text":"Protected",
                "translation_unit_translated_text":"TU","_force_visual_cover_only":true}"#,
        )];
        let blocks = vec![block("item-line-0", &[50.0, 85.0, 320.0, 200.0], "text", "Alpha Beta")];
        let out = redaction_items_from_layout_blocks(&items, &blocks);
        let it = &out[0];
        assert_eq!(it.item_id, "item-line-0");
        assert_eq!(it.source_item_id, Some("line-0".to_string()));
        assert_eq!(it.block_kind, "render_block");
        assert_eq!(it.block_type, "render_block");
        assert_eq!(it.source_text, "Original");
        assert_eq!(it.raw_block_type, "text");
        assert_eq!(it.normalized_sub_type, "body");
        assert_eq!(it.continuation_group.as_deref(), Some("grp-1"));
        assert!(it.force_visual_cover_only);
        assert_eq!(it.translation_unit_translated_text, "TU");
        assert_eq!(it.bbox.as_deref(), Some(&[50.0, 85.0, 320.0, 200.0][..]));
    }

    #[test]
    fn by_index_fallback_and_text_fallback() {
        let items = vec![
            item(r#"{"bbox":[1.0,1.0,2.0,2.0],"translated_text":"x"}"#),
            item(r#"{"bbox":[3.0,3.0,4.0,4.0],"translated_text":"y"}"#),
        ];
        // "item-1" is not in by-id (ids are empty) but "1" is a digit -> index 1.
        let blocks = vec![block("item-1", &[5.0, 5.0, 6.0, 6.0], "text", "Block Text")];
        let out = redaction_items_from_layout_blocks(&items, &blocks);
        let it = &out[0];
        // Source index 1 has empty item_id -> None; source text empty -> block text.
        assert_eq!(it.source_item_id, None);
        assert_eq!(it.source_text, "Block Text");
        assert_eq!(it.translated_text, "Block Text");
    }

    #[test]
    fn dict_compression_later_wins() {
        let items = vec![
            item(r#"{"bbox":[1.0,1.0,2.0,2.0],"translated_text":"first","item_id":"dup","source_text":"A"}"#),
            item(r#"{"bbox":[3.0,3.0,4.0,4.0],"translated_text":"second","item_id":"dup","source_text":"B"}"#),
        ];
        let blocks = vec![block("item-dup", &[5.0, 5.0, 6.0, 6.0], "text", "Block Text")];
        let out = redaction_items_from_layout_blocks(&items, &blocks);
        assert_eq!(out[0].source_text, "B");
        assert_eq!(out[0].source_item_id, Some("dup".to_string()));
    }

    #[test]
    fn non_four_bbox_filtered() {
        let items = vec![item(r#"{"bbox":[0.0,0.0,1.0,1.0],"translated_text":"x"}"#)];
        let blocks = vec![block("bad", &[1.0, 2.0, 3.0], "text", "no")];
        assert!(redaction_items_from_layout_blocks(&items, &blocks).is_empty());
    }

    #[test]
    fn markdown_protected_text() {
        let items: Vec<RedactionItem> = vec![];
        let blocks = vec![block("block-0", &[1.0, 2.0, 3.0, 4.0], "markdown", "<md>")];
        let out = redaction_items_from_layout_blocks(&items, &blocks);
        assert_eq!(out[0].translated_text, "<md>");
        assert_eq!(out[0].protected_translated_text, "<md>");
        assert_eq!(out[0].source_text, "<md>");
    }

    #[test]
    fn fill_key_order_source_item_id_priority() {
        let mut map = HashMap::new();
        map.insert("a".to_string(), [1.0, 0.0, 0.0]);
        map.insert("b".to_string(), [0.0, 1.0, 0.0]);
        map.insert("c".to_string(), [0.0, 0.0, 1.0]);
        let it = item(r#"{"source_item_id":"a","item_id":"b","block_id":"c"}"#);
        assert_eq!(background_fill_for_item(&it, &map), Some([1.0, 0.0, 0.0]));
        let it = item(r#"{"item_id":"b","block_id":"c"}"#);
        assert_eq!(background_fill_for_item(&it, &map), Some([0.0, 1.0, 0.0]));
        let it = item(r#"{"block_id":"c"}"#);
        assert_eq!(background_fill_for_item(&it, &map), Some([0.0, 0.0, 1.0]));
    }

    #[test]
    fn fill_misses_and_empty_table() {
        let empty = HashMap::new();
        let it = item(r#"{"source_item_id":"a","item_id":"b","block_id":"c"}"#);
        assert_eq!(background_fill_for_item(&it, &empty), None);
        let mut map = HashMap::new();
        map.insert("b".to_string(), [0.2, 0.4, 0.6]);
        let miss = item(r#"{"source_item_id":"z"}"#);
        assert_eq!(background_fill_for_item(&miss, &map), None);
        let no_ids = item(r#"{"translated_text":"x"}"#);
        assert_eq!(background_fill_for_item(&no_ids, &map), None);
    }

    #[test]
    fn apply_pages_replaces_and_fills() {
        let items_0 = vec![item(r#"{"bbox":[10.0,100.0,50.0,120.0],"translated_text":"Alpha"}"#)];
        let items_1 = vec![item(r#"{"bbox":[10.0,200.0,50.0,220.0],"translated_text":"Gamma"}"#)];
        let pages: BTreeMap<i32, Vec<RedactionItem>> =
            [(0, items_0.clone()), (1, items_1.clone())].into_iter().collect();
        let specs = vec![RenderPageSpec {
            page_index: 0,
            blocks: vec![block("block-0", &[50.0, 85.0, 320.0, 200.0], "text", "Alpha Beta")],
        }];
        let mut fill_map = HashMap::new();
        fill_map.insert("block-0".to_string(), [0.1, 0.2, 0.3]);
        let (redaction, formula) = apply_page_specs_and_fills(&pages, &specs, &fill_map);
        // Formula source is the original items (no fill).
        assert_eq!(formula[&0], items_0);
        assert_eq!(formula[&1], items_1);
        // Page 0 replaced + filled; page 1 untouched.
        assert_eq!(redaction[&0].len(), 1);
        assert_eq!(redaction[&0][0].item_id, "block-0");
        assert_eq!(redaction[&0][0].visual_profile_fill, Some([0.1, 0.2, 0.3]));
        assert_eq!(redaction[&1], items_1);
        assert_eq!(redaction[&1][0].visual_profile_fill, None);
    }
}
