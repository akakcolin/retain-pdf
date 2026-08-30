//! N11e: native `build_overlay_page_specs` — the overlay/dual bundle key
//! (`entrypoints/run_render_delegate.py::build_bundle`): per selected page, the
//! page geometry from the RAW source PDF (`read_source_page_sizes` == fitz
//! `page.rect` dimensions) plus the `RenderBlock` DTOs for that page's items.
//!
//! The blocks run the identical layout pipeline as `blocks.build_render_blocks`:
//! `seed_render_fields` -> `build_block_payloads` -> order by
//! `(inner_bbox[1], inner_bbox[0])` -> `apply_body_payload_pipeline` (font-unify
//! stage set selected by `font_unify_mode`) + `unify_annotation_fonts` (unless
//! mode == "off") + `recover_underfilled_annotation_density` ->
//! `mark_adjacent_collision_risk` -> `emit_render_blocks` (re-orders by payload
//! `index`). Mutations propagate to the original payload dicts (the Python path
//! reorders dict references, not copies), so write-back is by original index.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Result};
use mupdf::Document;
use rendering_core::item::Item;
use rendering_core::layout::collision::mark_adjacent_collision_risk;
use rendering_core::layout::render_item::seed_render_fields;
use rendering_core::payload::block_seed::build_block_payloads;
use rendering_core::payload::body_pipeline::apply_body_payload_pipeline;
use rendering_core::payload::body_policy_facade::{
    recover_underfilled_annotation_density, unify_annotation_fonts,
};
use rendering_core::payload::emit::emit_render_blocks;
use rendering_reader::PdfDocument as _;
use serde_json::{json, Value};

/// `build_overlay_page_specs`: per selected page, page_rect dims from the source
/// PDF plus the page's RenderBlock DTOs. Out-of-range / unreadable pages are
/// skipped (the delegate's `if page_idx in source_sizes` gate).
pub fn build_overlay_page_specs(
    source_pdf_path: &Path,
    prepared_pages: &BTreeMap<i64, Vec<Value>>,
    font_unify_mode: &str,
) -> Result<Vec<Value>> {
    let doc = Document::open(source_pdf_path).map_err(|e| {
        anyhow!(
            "open source pdf for overlay page sizes {}: {e}",
            source_pdf_path.display()
        )
    })?;
    let mut specs = Vec::new();
    for (&page_idx, items) in prepared_pages {
        if page_idx < 0 {
            continue;
        }
        let Ok(page_rect) = doc.page_rect(page_idx) else { continue };
        let width = page_rect.x1 - page_rect.x0;
        let height = page_rect.y1 - page_rect.y0;
        let blocks = build_render_blocks(items, width, height, font_unify_mode);
        specs.push(json!({
            "page_index": page_idx,
            "page_width_pt": width,
            "page_height_pt": height,
            "blocks": blocks,
        }));
    }
    Ok(specs)
}

/// `blocks.build_render_blocks` over one page's translated items, producing the
/// `RenderBlock` DTO array (the delegate converts via `_as_render_blocks` ->
/// `_render_block_to_dict`, whose fields match `emit_render_blocks` output).
fn build_render_blocks(items: &[Value], page_width: f64, page_height: f64, font_unify_mode: &str) -> Vec<Value> {
    let mut seeded = items.to_vec();
    for item in &mut seeded {
        seed_render_fields(item);
    }
    let typed: Vec<Item> = seeded.iter().map(Item::from_json_value).collect();
    let (mut block_payloads, page_text_width_med) =
        build_block_payloads(&typed, &seeded, Some(page_width), Some(page_height));

    let keys: Vec<(f64, f64)> = block_payloads
        .iter()
        .map(|p| {
            let arr = p.get("inner_bbox").and_then(Value::as_array);
            let get = |k: usize| -> f64 {
                arr.and_then(|a| a.get(k))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
            };
            (get(1), get(0))
        })
        .collect();
    let mut order: Vec<usize> = (0..block_payloads.len()).collect();
    order.sort_by(|&a, &b| keys[a].partial_cmp(&keys[b]).unwrap_or(std::cmp::Ordering::Equal));

    let mut ordered: Vec<Value> = order.iter().map(|&i| block_payloads[i].clone()).collect();
    apply_body_payload_pipeline(&mut ordered, page_text_width_med, None, font_unify_mode);
    if font_unify_mode != "off" {
        unify_annotation_fonts(&mut ordered);
    }
    recover_underfilled_annotation_density(&mut ordered);
    mark_adjacent_collision_risk(&mut ordered);
    for (k, &i) in order.iter().enumerate() {
        block_payloads[i] = ordered[k].clone();
    }
    emit_render_blocks(&block_payloads)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::*;
    use serde_json::json;

    const GOLDEN_ROOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/samples/golden-pdfs"
    );

    fn paragraph_item(item_id: &str, page_idx: i64, bbox: [f64; 4]) -> Value {
        json!({
            "item_id": item_id,
            "page_idx": page_idx,
            "block_type": "text",
            "block_kind": "text",
            "layout_role": "paragraph",
            "semantic_role": "body",
            "bbox": bbox,
            "source_text": "source text",
            "translated_text": "译文",
            "protected_source_text": "source text",
            "protected_translated_text": "译文",
            "should_translate": true,
            "final_status": "translated",
            "translation_unit_id": item_id,
            "translation_unit_kind": "single",
            "translation_unit_protected_source_text": "source text",
            "translation_unit_protected_translated_text": "译文",
            "translation_unit_formula_map": [],
            "formula_map": [],
            "protected_map": [],
            "continuation_group": "",
            "group_protected_source_text": "",
            "group_formula_map": [],
            "group_protected_translated_text": "",
            "group_translated_text": "",
            "_render_policy": {"overlay_fill": "sampled", "cleanup_mode": "delete_text"},
            "_render_cover_fill": [1.0, 1.0, 1.0],
            "_render_text_color": [0.0, 0.0, 0.0],
            "lines": [{"bbox": bbox, "spans": [{"type": "text", "content": "source text", "bbox": bbox}]}],
        })
    }

    #[test]
    fn skips_out_of_range_pages() {
        // A real (multi-page) doc: negative indices are dropped by the gate and
        // a page index past the last page fails `page_rect`, so no specs emit.
        let source = Path::new(GOLDEN_ROOT).join("2.pdf");
        let mut pages: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
        pages.insert(-1, vec![paragraph_item("n001", -1, [0.0, 0.0, 10.0, 10.0])]);
        pages.insert(100, vec![paragraph_item("p100", 100, [0.0, 0.0, 10.0, 10.0])]);
        let specs = build_overlay_page_specs(&source, &pages, "role_min").unwrap();
        assert!(specs.is_empty());
    }

    #[test]
    fn emits_blocks_sorted_by_index() {
        // `emit_render_blocks` re-orders by payload `index` (input order), so
        // the emitted blocks follow the items array even when the body pipeline
        // processes them y-sorted internally.
        let source = Path::new(GOLDEN_ROOT).join("2.pdf");
        let mut pages: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
        pages.insert(0, vec![
            paragraph_item("p001-b001", 0, [40.0, 40.0, 360.0, 80.0]),
            paragraph_item("p001-b002", 0, [40.0, 140.0, 360.0, 180.0]),
        ]);
        let specs = build_overlay_page_specs(&source, &pages, "role_min").unwrap();
        assert_eq!(specs.len(), 1);
        let spec = &specs[0];
        assert_eq!(spec["page_index"], 0);
        assert!(spec["page_width_pt"].as_f64().unwrap() > 0.0);
        assert!(spec["page_height_pt"].as_f64().unwrap() > 0.0);
        let blocks = spec["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["source_item_id"], "p001-b001");
        assert_eq!(blocks[1]["source_item_id"], "p001-b002");
        // The block DTO carries the sampled cover fill / text color through the
        // color-adapted item into the emitted block.
        assert_eq!(blocks[0]["cover_fill"], json!([1.0, 1.0, 1.0]));
        assert_eq!(blocks[0]["text_color"], json!([0.0, 0.0, 0.0]));
    }
}
