// Port of `provider_adapters/paddle/page_trace.py` — layout-det box lookup,
// per-block layout trace attach, and the page metadata trace.

use serde_json::{json, Map, Value};

use super::super::common::{normalize_polygon, py_int, to_string_value};

/// `build_layout_box_lookup` — box dicts keyed by their 4-float coordinate.
pub type LayoutBoxLookup = Vec<(Vec<f64>, Value)>;

/// `build_layout_box_lookup` — boxes with a 4-element `coordinate` list.
pub fn build_layout_box_lookup(layout_boxes: &Value) -> LayoutBoxLookup {
    let mut lookup: LayoutBoxLookup = Vec::new();
    if let Some(boxes) = layout_boxes.as_array() {
        for box_value in boxes {
            let Some(coordinate) = box_value.get("coordinate") else { continue };
            let Some(coords) = coordinate.as_array() else { continue };
            if coords.len() != 4 {
                continue;
            }
            let key: Vec<f64> = coords.iter().map(|item| item.as_f64().unwrap_or(0.0)).collect();
            lookup.push((key, box_value.clone()));
        }
    }
    lookup
}

fn lookup_get<'a>(lookup: &'a LayoutBoxLookup, bbox: &[f64]) -> Option<&'a Value> {
    if bbox.len() != 4 {
        return None;
    }
    let key: Vec<f64> = bbox.to_vec();
    lookup.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

fn polygon_value(polygon: &[Vec<f64>]) -> Value {
    Value::Array(
        polygon
            .iter()
            .map(|pair| Value::Array(pair.iter().map(|f| Value::from(*f)).collect()))
            .collect(),
    )
}

/// `attach_layout_trace` — matched layout-det box → trace keys.
pub fn attach_layout_trace(
    metadata: &mut Map<String, Value>,
    bbox: &[f64],
    layout_box_lookup: &LayoutBoxLookup,
) {
    match lookup_get(layout_box_lookup, bbox) {
        None => {
            metadata.insert("layout_det_matched".into(), Value::Bool(false));
        }
        Some(matched) => {
            metadata.insert("layout_det_matched".into(), Value::Bool(true));
            metadata.insert("layout_det_label".into(), Value::String(to_string_value(matched.get("label"))));
            metadata.insert("layout_det_cls_id".into(), matched.get("cls_id").cloned().unwrap_or(Value::Null));
            metadata.insert("layout_det_score".into(), matched.get("score").cloned().unwrap_or(Value::Null));
            metadata.insert("layout_det_order".into(), matched.get("order").cloned().unwrap_or(Value::Null));
            metadata.insert(
                "layout_det_polygon".into(),
                polygon_value(&normalize_polygon(matched.get("polygon_points"))),
            );
        }
    }
}

fn object_or_empty(value: Option<&Value>) -> Value {
    match value {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        _ => Value::Object(Map::new()),
    }
}

fn str_or_unknown(value: Option<&Value>) -> String {
    let s = to_string_value(value);
    if s.is_empty() { "unknown".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_box_lookup_keeps_only_four_coord_boxes() {
        let boxes = json!([
            {"coordinate": [1.0, 2.0, 3.0, 4.0], "label": "text"},
            {"coordinate": [5, 6, 7], "label": "short"},
            {"coordinate": "nope", "label": "str"},
            {"label": "missing"},
        ]);
        let lookup = build_layout_box_lookup(&boxes);
        assert_eq!(lookup.len(), 1);
        assert_eq!(lookup[0].0, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn attach_layout_trace_match_and_miss() {
        let boxes = json!([
            {
                "coordinate": [10.0, 20.0, 300.0, 60.0],
                "label": "title",
                "cls_id": 1,
                "score": 0.98,
                "order": 0,
                "polygon_points": [[10, 20], [300, 20], [300, 60], [10, 60]],
            }
        ]);
        let lookup = build_layout_box_lookup(&boxes);
        let mut matched = Map::new();
        attach_layout_trace(&mut matched, &[10.0, 20.0, 300.0, 60.0], &lookup);
        assert_eq!(matched["layout_det_matched"], true);
        assert_eq!(matched["layout_det_label"], "title");
        assert_eq!(matched["layout_det_cls_id"], 1);
        assert_eq!(matched["layout_det_score"], 0.98);
        assert_eq!(matched["layout_det_order"], 0);
        assert_eq!(matched["layout_det_polygon"], json!([[10.0, 20.0], [300.0, 20.0], [300.0, 60.0], [10.0, 60.0]]));

        let mut missed = Map::new();
        attach_layout_trace(&mut missed, &[99.0, 99.0, 199.0, 199.0], &lookup);
        assert_eq!(missed["layout_det_matched"], false);
    }

    #[test]
    fn build_page_trace_full_shape() {
        let page_payload = json!({
            "inputImage": "raw/page_0.png",
            "markdown": {"text": "markdown body", "images": {"a.png": "out/a.png"}},
            "outputImages": {"1": "out_0.png"},
        });
        let pruned = json!({
            "page_count": 2,
            "model_settings": {"enable_body_repair": true},
            "layout_det_res": {
                "boxes": [{"coordinate": [10, 20, 30, 40], "label": "text", "score": 0.5}],
            },
        });
        let column_signals = json!({
            "column_layout_mode": "double",
            "split_x": 300.5,
            "suspected_count": 1,
            "suspected_orders": [1],
        });
        let block_ids: Vec<String> = vec!["p001-b0000".to_string(), "p001-b0001".to_string()];
        let trace = build_page_trace(&page_payload, &pruned, "preprocessed/page_0.png", Some(&column_signals), &block_ids);
        assert_eq!(trace["input_image"], "raw/page_0.png");
        assert_eq!(trace["preprocessed_image"], "preprocessed/page_0.png");
        assert_eq!(trace["raw_unit"], "px");
        assert_eq!(trace["provider_page_count"], 2);
        assert_eq!(trace["model_settings"]["enable_body_repair"], true);
        assert_eq!(trace["markdown"]["text_length"], 13);
        assert_eq!(trace["markdown"]["images"]["a.png"], "out/a.png");
        assert_eq!(trace["column_layout_mode"], "double");
        assert_eq!(trace["column_split_x"], 300.5);
        assert_eq!(trace["suspected_cross_column_merge_block_count"], 1);
        assert_eq!(trace["suspected_block_ids"], json!(["p001-b0001"]));
        assert_eq!(trace["layout_det_res"]["box_count"], 1);
        assert_eq!(trace["layout_det_res"]["boxes"][0]["coordinate"], json!([10.0, 20.0, 30.0, 40.0]));
    }

    #[test]
    fn build_page_trace_defaults_for_empty_inputs() {
        let trace = build_page_trace(&json!({}), &json!({}), "", None, &[]);
        assert_eq!(trace["input_image"], "");
        assert_eq!(trace["provider_page_count"], 0);
        assert_eq!(trace["column_layout_mode"], "unknown");
        assert_eq!(trace["suspected_cross_column_merge_block_count"], 0);
        assert_eq!(trace["suspected_block_ids"], json!([]));
        assert_eq!(trace["layout_det_res"]["box_count"], 0);
    }
}

/// `build_page_trace` — page-level metadata trace dict.
pub fn build_page_trace(
    page_payload: &Value,
    pruned: &Value,
    preprocessed_image: &str,
    column_signals: Option<&Value>,
    block_ids: &[String],
) -> Value {
    let markdown = page_payload.get("markdown").cloned().unwrap_or_else(|| json!({}));
    let layout_boxes: Vec<Value> = pruned
        .get("layout_det_res")
        .and_then(|r| r.get("boxes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let column_signals = column_signals.cloned().unwrap_or_else(|| json!({}));
    let suspected_orders: Vec<i64> = column_signals
        .get("suspected_orders")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    let suspected_block_ids: Vec<Value> = suspected_orders
        .into_iter()
        .filter(|order| *order >= 0 && (*order as usize) < block_ids.len())
        .map(|order| Value::String(block_ids[order as usize].clone()))
        .collect();

    let markdown_text = to_string_value(markdown.get("text"));

    let boxes_value: Vec<Value> = layout_boxes
        .iter()
        .filter(|b| b.is_object())
        .map(|box_value| {
            let coordinate: Vec<Value> = match box_value.get("coordinate").and_then(Value::as_array) {
                Some(arr) if !arr.is_empty() => arr
                    .iter()
                    .take(4)
                    .map(|item| Value::from(item.as_f64().unwrap_or(0.0)))
                    .collect(),
                _ => vec![Value::from(0.0); 4],
            };
            json!({
                "label": to_string_value(box_value.get("label")),
                "cls_id": box_value.get("cls_id").cloned().unwrap_or(Value::Null),
                "score": box_value.get("score").cloned().unwrap_or(Value::Null),
                "order": box_value.get("order").cloned().unwrap_or(Value::Null),
                "coordinate": coordinate,
                "polygon_points": polygon_value(&normalize_polygon(box_value.get("polygon_points"))),
            })
        })
        .collect();

    json!({
        "input_image": to_string_value(page_payload.get("inputImage")),
        "preprocessed_image": preprocessed_image,
        "raw_unit": "px",
        "provider_page_count": pruned.get("page_count").and_then(py_int).unwrap_or(0),
        "model_settings": object_or_empty(pruned.get("model_settings")),
        "markdown": {
            "text": markdown_text,
            "text_length": markdown_text.chars().count(),
            "images": object_or_empty(markdown.get("images")),
        },
        "output_images": object_or_empty(page_payload.get("outputImages")),
        "column_layout_mode": str_or_unknown(column_signals.get("column_layout_mode")),
        "column_split_x": column_signals.get("split_x").and_then(Value::as_f64).unwrap_or(0.0),
        "suspected_cross_column_merge_block_count": column_signals
            .get("suspected_count")
            .and_then(py_int)
            .unwrap_or(0),
        "suspected_block_ids": suspected_block_ids,
        "layout_det_res": {
            "box_count": layout_boxes.len(),
            "boxes": boxes_value,
        },
    })
}
