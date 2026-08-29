// Port of services/rendering/layout/payload/emit.py — the `payload_to_render_block`
// / `emit_render_blocks` assembler at the C3-N2 emit boundary. Inputs are the raw
// block-payload JSON objects from `build_block_payloads` (+ body-pipeline keys);
// outputs are `RenderBlock` DTO dicts matching `_native._render_block_to_dict`.

use serde_json::{json, Value};

use crate::inline_content::build_item_render_markdown;
use crate::item::{formula_map, Item};
use crate::layout::line_structure::preserved_line_boxes_for_item;
use crate::payload::fit_typst::resolve_typst_binary_fit;
use crate::payload::formula_safety::formula_safe_inner_bbox;
use crate::payload::policy_compat::item_uses_white_overlay_fill;
use crate::payload::text_common::build_plain_text_from_text;
use crate::payload::toc_structure::render_toc_entries_for_item;

/// `f64_list`: a JSON array of numbers (ints/floats) as `Vec<f64>`, else `None`.
fn f64_list(value: &Value) -> Option<Vec<f64>> {
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

fn payload_f64(payload: &Value, key: &str, default: f64) -> f64 {
    payload
        .get(key)
        .and_then(|v| v.as_f64())
        .unwrap_or(default)
}

fn payload_bool(payload: &Value, key: &str) -> bool {
    payload.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn payload_string(payload: &Value, key: &str, default: &str) -> String {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

/// `str(payload["item"].get("item_id") or "")` — strings pass through, numbers
/// stringify, missing/null become empty.
fn source_item_id(item: &Value) -> String {
    match item.get("item_id") {
        Some(v) if v.is_string() => v.as_str().unwrap().to_string(),
        Some(v) if v.is_number() => v.to_string(),
        _ => String::new(),
    }
}

/// `payload_to_render_block`.
pub fn payload_to_render_block(payload: &Value) -> Value {
    let item_value = payload.get("item").cloned().unwrap_or(Value::Null);
    let translated_text = payload_string(payload, "translated_text", "");
    let formula_entries = formula_map(payload.get("formula_map"));
    let font_size_pt = payload_f64(payload, "font_size_pt", 0.0);

    let inner_bbox = payload.get("inner_bbox").and_then(f64_list).unwrap_or_default();
    let (safe_inner_bbox, _formula_insets) =
        formula_safe_inner_bbox(&inner_bbox, &translated_text, &formula_entries, font_size_pt);

    let title_fit = payload.get("title_fit");
    let (fit_to_box, fit_single_line, fit_min_font_size_pt, fit_max_font_size_pt, fit_min_leading_em,
         fit_max_height_pt, fit_target_width_pt, fit_target_height_pt) =
        if let Some(fit) = title_fit.filter(|v| v.is_object()) {
            (
                fit.get("fit_to_box").and_then(|v| v.as_bool()).unwrap_or(false),
                fit.get("fit_single_line").and_then(|v| v.as_bool()).unwrap_or(false),
                fit.get("fit_min_font_size_pt").and_then(|v| v.as_f64()).unwrap_or(0.0),
                fit.get("fit_max_font_size_pt").and_then(|v| v.as_f64()).unwrap_or(0.0),
                fit.get("fit_min_leading_em").and_then(|v| v.as_f64()).unwrap_or(0.0),
                fit.get("fit_max_height_pt").and_then(|v| v.as_f64()).unwrap_or(0.0),
                fit.get("fit_target_width_pt").and_then(|v| v.as_f64()).unwrap_or(0.0),
                fit.get("fit_target_height_pt").and_then(|v| v.as_f64()).unwrap_or(0.0),
            )
        } else {
            let mut item = Item::from_json_value(&item_value);
            if safe_inner_bbox.len() == 4 {
                item.render_inner_bbox = Some([
                    safe_inner_bbox[0],
                    safe_inner_bbox[1],
                    safe_inner_bbox[2],
                    safe_inner_bbox[3],
                ]);
            }
            item.is_body_text_candidate = payload_bool(payload, "is_body");
            item.dense_small_box = payload_bool(payload, "dense_small_box");
            item.heavy_dense_small_box = payload_bool(payload, "heavy_dense_small_box");
            let leading_em = payload_f64(payload, "leading_em", 0.0);
            let fit = resolve_typst_binary_fit(
                &item,
                &translated_text,
                &formula_entries,
                font_size_pt,
                leading_em,
                payload_f64(payload, "_relaxed_fit_height_pt", 0.0),
                payload_f64(payload, "_short_body_inherited_font_floor_pt", 0.0),
                payload.get("page_body_font_size_pt").and_then(|v| v.as_f64()),
                payload_bool(payload, "prefer_typst_fit"),
                payload_bool(payload, "adjacent_collision_risk"),
                payload.get("adjacent_available_height_pt").and_then(|v| v.as_f64()),
            );
            let mut fit_to_box = fit.fit_to_box;
            let mut fit_min_font_size_pt = fit.min_font_size_pt;
            let mut fit_min_leading_em = fit.min_leading_em;
            let mut fit_max_height_pt = fit.max_height_pt;
            if payload_bool(payload, "_body_font_unified")
                && !payload_bool(payload, "prefer_typst_fit")
                && !fit_to_box
            {
                fit_to_box = false;
                fit_min_font_size_pt = 0.0;
                fit_min_leading_em = 0.0;
                fit_max_height_pt = 0.0;
            }
            if payload_bool(payload, "_allow_short_text_bbox_overflow") {
                fit_to_box = false;
                fit_min_font_size_pt = 0.0;
                fit_min_leading_em = 0.0;
                fit_max_height_pt = 0.0;
            }
            (
                fit_to_box,
                false,
                fit_min_font_size_pt,
                0.0,
                fit_min_leading_em,
                fit_max_height_pt,
                0.0,
                0.0,
            )
        };

    let preserve_line_breaks = payload_bool(payload, "preserve_line_breaks");
    let render_kind = payload_string(payload, "render_kind", "");
    let fit_to_box_effective = fit_to_box && render_kind == "markdown" && !preserve_line_breaks;

    let preserved_line_boxes = preserved_line_boxes_for_item(&item_value, &translated_text);
    let toc_entries = render_toc_entries_for_item(&item_value, &translated_text);
    let markdown_text = build_item_render_markdown(&item_value, &translated_text, &formula_entries);
    let plain_text = build_plain_text_from_text(&translated_text);

    let index = payload.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    let text_color = payload
        .get("text_color")
        .and_then(f64_list)
        .unwrap_or_else(|| vec![0.0, 0.0, 0.0]);
    let cover_fill = payload
        .get("cover_fill")
        .and_then(f64_list)
        .unwrap_or_else(|| vec![1.0, 1.0, 1.0]);
    let skip_reason = if payload_bool(payload, "adjacent_collision_risk") {
        "adjacent_collision_risk".to_string()
    } else {
        String::new()
    };
    let math_map = payload
        .get("formula_map")
        .cloned()
        .unwrap_or(Value::Array(vec![]));

    json!({
        "block_id": format!("item-{index}"),
        "bbox": payload.get("bbox").cloned().unwrap_or(Value::Array(vec![])),
        "cover_bbox": payload.get("cover_bbox").cloned().unwrap_or(Value::Array(vec![])),
        "inner_bbox": payload.get("inner_bbox").cloned().unwrap_or(Value::Array(vec![])),
        "markdown_text": markdown_text,
        "plain_text": plain_text,
        "render_kind": render_kind,
        "font_size_pt": font_size_pt,
        "leading_em": payload_f64(payload, "leading_em", 0.0),
        "font_weight": payload_string(payload, "font_weight", "regular"),
        "fit_to_box": fit_to_box_effective,
        "fit_single_line": fit_single_line,
        "fit_min_font_size_pt": fit_min_font_size_pt,
        "fit_max_font_size_pt": fit_max_font_size_pt,
        "fit_min_leading_em": fit_min_leading_em,
        "fit_max_height_pt": fit_max_height_pt,
        "fit_target_width_pt": fit_target_width_pt,
        "fit_target_height_pt": fit_target_height_pt,
        "fit_shift_up_pt": 0.0,
        "first_line_indent_pt": payload_f64(payload, "first_line_indent_pt", 0.0),
        "justify_text": payload_bool(payload, "is_body") && render_kind == "markdown",
        "text_color": text_color,
        "cover_fill": cover_fill,
        "use_cover_fill": item_uses_white_overlay_fill(&item_value),
        "math_map": math_map,
        "skip_reason": skip_reason,
        "source_item_id": source_item_id(&item_value),
        "preserve_line_breaks": preserve_line_breaks,
        "preserved_line_boxes": if preserved_line_boxes.is_empty() {
            Value::Array(vec![])
        } else {
            Value::Array(preserved_line_boxes)
        },
        "toc_entries": if toc_entries.is_empty() {
            Value::Array(vec![])
        } else {
            Value::Array(toc_entries)
        },
    })
}

/// `emit_render_blocks`: order by payload `index`, then assemble.
pub fn emit_render_blocks(block_payloads: &[Value]) -> Vec<Value> {
    let mut ordered: Vec<&Value> = block_payloads.iter().collect();
    ordered.sort_by_key(|p| p.get("index").and_then(|v| v.as_i64()).unwrap_or(i64::MAX));
    ordered.into_iter().map(payload_to_render_block).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_payload(index: i64) -> Value {
        json!({
            "index": index,
            "item": {
                "item_id": "b001",
                "math_mode": "placeholder",
                "formula_map": [],
            },
            "bbox": [72.0, 50.0, 540.0, 90.0],
            "cover_bbox": [72.0, 50.0, 540.0, 90.0],
            "inner_bbox": [72.0, 50.0, 540.0, 90.0],
            "translated_text": "第一章",
            "formula_map": [],
            "render_kind": "markdown",
            "font_size_pt": 16.0,
            "leading_em": 0.3,
            "first_line_indent_pt": 0.0,
            "font_weight": "bold",
            "page_body_font_size_pt": null,
            "is_body": false,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "prefer_typst_fit": false,
            "title_fit": null,
            "preserve_line_breaks": false,
            "adjacent_collision_risk": false,
            "adjacent_available_height_pt": null,
            "text_color": [0.1, 0.2, 0.3],
            "cover_fill": [0.9, 0.8, 0.7],
        })
    }

    #[test]
    fn emits_plain_title_block() {
        let block = payload_to_render_block(&base_payload(3));
        assert_eq!(block["block_id"], "item-3");
        assert_eq!(block["source_item_id"], "b001");
        assert_eq!(block["markdown_text"], "第一章");
        assert_eq!(block["plain_text"], "第一章");
        assert_eq!(block["font_weight"], "bold");
        assert_eq!(block["text_color"], json!([0.1, 0.2, 0.3]));
        assert_eq!(block["cover_fill"], json!([0.9, 0.8, 0.7]));
        assert_eq!(block["fit_to_box"], false);
        assert_eq!(block["fit_single_line"], false);
        assert_eq!(block["preserved_line_boxes"], json!([]));
        assert_eq!(block["toc_entries"], json!([]));
        assert_eq!(block["math_map"], json!([]));
        assert_eq!(block["fit_shift_up_pt"], 0.0);
    }

    #[test]
    fn title_fit_branch_reads_fit_fields() {
        let mut payload = base_payload(0);
        payload["title_fit"] = json!({
            "fit_to_box": true,
            "fit_single_line": true,
            "fit_min_font_size_pt": 9.5,
            "fit_max_font_size_pt": 18.0,
            "fit_min_leading_em": 0.25,
            "fit_max_height_pt": 80.0,
            "fit_target_width_pt": 400.0,
            "fit_target_height_pt": 90.0,
        });
        let block = payload_to_render_block(&payload);
        assert_eq!(block["fit_single_line"], true);
        assert_eq!(block["fit_min_font_size_pt"], 9.5);
        assert_eq!(block["fit_max_font_size_pt"], 18.0);
        assert_eq!(block["fit_target_width_pt"], 400.0);
    }

    #[test]
    fn fit_decision_path_matches_python_reference() {
        // Same payload probed against Python payload_to_render_block: the
        // resolve_typst_binary_fit branch must reproduce its exact numbers.
        let payload = json!({
            "index": 2,
            "item": {
                "item_id": "b003",
                "math_mode": "placeholder",
                "semantic_role": "body",
                "layout_role": "paragraph",
                "bbox": [72.0, 140.0, 460.0, 200.0],
                "lines": [
                    {"bbox": [72.0, 140.0, 460.0, 160.0], "spans": []},
                    {"bbox": [72.0, 160.0, 460.0, 180.0], "spans": []},
                    {"bbox": [72.0, 180.0, 460.0, 200.0], "spans": []},
                ],
                "source_text": "This is a fairly long body paragraph with multiple lines of flowing English text.",
                "translated_text": "这是一个相当长的正文段落，包含多行流动的中文文本内容。",
                "protected_translated_text": "这是一个相当长的正文段落，包含多行流动的中文文本内容。",
                "formula_map": [],
            },
            "bbox": [72.0, 140.0, 460.0, 200.0],
            "cover_bbox": [72.0, 140.0, 460.0, 200.0],
            "inner_bbox": [72.0, 140.0, 460.0, 200.0],
            "translated_text": "这是一个相当长的正文段落，包含多行流动的中文文本内容。",
            "formula_map": [],
            "render_kind": "markdown",
            "font_size_pt": 12.0,
            "leading_em": 0.35,
            "first_line_indent_pt": 0.0,
            "font_weight": "regular",
            "page_body_font_size_pt": 12.0,
            "is_body": true,
            "dense_small_box": true,
            "heavy_dense_small_box": false,
            "prefer_typst_fit": true,
            "title_fit": null,
            "preserve_line_breaks": false,
            "adjacent_collision_risk": false,
            "adjacent_available_height_pt": null,
            "text_color": [0.0, 0.0, 0.0],
            "cover_fill": [1.0, 1.0, 1.0],
        });
        let block = payload_to_render_block(&payload);
        assert_eq!(block["fit_to_box"], true);
        assert_eq!(block["fit_min_font_size_pt"], 11.14);
        assert_eq!(block["fit_min_leading_em"], 0.35);
        assert_eq!(block["fit_max_height_pt"], 60.0);
        assert_eq!(block["fit_max_font_size_pt"], 0.0);
        assert_eq!(block["fit_single_line"], false);
        assert_eq!(block["justify_text"], true);
        assert_eq!(block["markdown_text"], "这是一个相当长的正文段落，包含多行流动的中文文本内容。");
        assert_eq!(block["source_item_id"], "b003");
        assert_eq!(block["block_id"], "item-2");
    }

    #[test]
    fn preserve_line_breaks_matches_python_reference() {
        let mut payload = base_payload(4);
        payload["item"] = json!({
            "item_id": "b005",
            "math_mode": "placeholder",
            "formula_map": [],
            "_render_preserve_line_breaks": true,
            "lines": [
                {"bbox": [72.0, 250.0, 200.0, 260.0]},
                {"bbox": [72.0, 260.0, 200.0, 270.0]},
            ],
        });
        payload["translated_text"] = json!("保留\n换行");
        payload["preserve_line_breaks"] = json!(true);
        payload["render_kind"] = json!("plain_line");
        let block = payload_to_render_block(&payload);
        assert_eq!(block["preserve_line_breaks"], true);
        assert_eq!(block["render_kind"], "plain_line");
        assert_eq!(block["fit_to_box"], false);
        assert_eq!(block["markdown_text"], "保留\n换行");
        assert_eq!(
            block["preserved_line_boxes"],
            json!([
                {"text": "保留", "bbox": [72.0, 250.0, 200.0, 260.0]},
                {"text": "换行", "bbox": [72.0, 260.0, 200.0, 270.0]},
            ])
        );
    }

    #[test]
    fn emit_render_blocks_sorts_by_index() {
        let p3 = base_payload(3);
        let p1 = base_payload(1);
        let p2 = base_payload(2);
        let blocks = emit_render_blocks(&[p3, p1, p2]);
        assert_eq!(blocks[0]["block_id"], "item-1");
        assert_eq!(blocks[1]["block_id"], "item-2");
        assert_eq!(blocks[2]["block_id"], "item-3");
    }
}
