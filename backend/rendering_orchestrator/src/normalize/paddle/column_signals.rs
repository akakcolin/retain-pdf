// Port of `provider_adapters/paddle/column_signals.py` — column layout detection
// over the page's parsed blocks, producing per-block merge/reading-order signals
// and the page/document summaries.

use serde_json::{json, Map, Value};

use super::block_labels::block_bbox;

const TEXTISH_LABELS: [&str; 11] = [
    "abstract",
    "aside_text",
    "doc_title",
    "figure_title",
    "footnote",
    "header",
    "number",
    "paragraph_title",
    "reference_content",
    "text",
    "vision_footnote",
];

const NON_BODY_SIGNAL_LABELS: [&str; 8] = [
    "aside_text",
    "footnote",
    "footer",
    "footer_image",
    "header",
    "header_image",
    "number",
    "vision_footnote",
];

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite values"));
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

fn column_guess(bbox: &[f64], page_width: f64) -> &'static str {
    if bbox.len() != 4 || page_width <= 0.0 {
        return "unknown";
    }
    let width = (bbox[2] - bbox[0]).max(0.0);
    if width >= page_width * 0.62 {
        return "full";
    }
    let center_x = (bbox[0] + bbox[2]) / 2.0;
    if center_x <= page_width / 2.0 {
        "left"
    } else {
        "right"
    }
}

fn is_column_sized(bbox: &[f64], page_width: f64) -> bool {
    if bbox.len() != 4 || page_width <= 0.0 {
        return false;
    }
    let width = bbox[2] - bbox[0];
    let width_ratio = width / page_width;
    0.18 <= width_ratio && width_ratio <= 0.48
}

fn vertical_overlap(a: &[f64], b: &[f64]) -> bool {
    if a.len() != 4 || b.len() != 4 {
        return false;
    }
    let overlap = a[3].min(b[3]) - a[1].max(b[1]);
    let min_height = ((a[3] - a[1]).max(1.0)).min((b[3] - b[1]).max(1.0));
    overlap >= min_height * 0.25
}

fn candidate_neighbors(records: &[BlockRecord], order: usize) -> Vec<&BlockRecord> {
    let mut result = Vec::new();
    for offset in [-1i64, 1i64] {
        let target = order as i64 + offset;
        for record in records {
            if record.order as i64 == target {
                result.push(record);
                break;
            }
        }
    }
    result
}

fn looks_like_sparse_double_column(
    left_slot_count: usize,
    right_slot_count: usize,
    records: &[BlockRecord],
) -> bool {
    if left_slot_count >= 2 && right_slot_count >= 2 {
        return true;
    }
    if left_slot_count < 1 || right_slot_count < 1 {
        return false;
    }
    records.iter().any(|r| r.empty_text && (r.column_guess == "left" || r.column_guess == "right"))
}

struct BlockRecord {
    order: usize,
    bbox: Vec<f64>,
    label: String,
    empty_text: bool,
    column_guess: String,
}

/// `analyze_page_column_signals`.
pub fn analyze_page_column_signals(parsing_res_list: &[Value], page_width: f64) -> Value {
    let mut valid_blocks: Vec<BlockRecord> = Vec::new();
    let mut left_centers: Vec<f64> = Vec::new();
    let mut right_centers: Vec<f64> = Vec::new();
    for (order, block) in parsing_res_list.iter().enumerate() {
        let Some(bbox) = block_bbox(block) else {
            continue;
        };
        let label = block
            .get("block_label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let text = block
            .get("block_content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let width = bbox[2] - bbox[0];
        let center_x = (bbox[0] + bbox[2]) / 2.0;
        let column_guess = column_guess(&bbox, page_width).to_string();
        valid_blocks.push(BlockRecord {
            order,
            bbox: bbox.clone(),
            label: label.clone(),
            empty_text: text.chars().count() < 2,
            column_guess: column_guess.clone(),
        });
        if TEXTISH_LABELS.contains(&label.as_str()) && !text.is_empty() && page_width > 0.0 {
            let width_ratio = width / page_width;
            if (0.18..=0.48).contains(&width_ratio) {
                if column_guess == "left" {
                    left_centers.push(center_x);
                } else if column_guess == "right" {
                    right_centers.push(center_x);
                }
            }
        }
    }

    let left_slot_count = valid_blocks
        .iter()
        .filter(|r| TEXTISH_LABELS.contains(&r.label.as_str()) && r.column_guess == "left"
            && is_column_sized(&r.bbox, page_width))
        .count();
    let right_slot_count = valid_blocks
        .iter()
        .filter(|r| TEXTISH_LABELS.contains(&r.label.as_str()) && r.column_guess == "right"
            && is_column_sized(&r.bbox, page_width))
        .count();

    let mode: &str;
    let split_x: f64;
    if page_width <= 0.0
        || ((left_centers.len() < 2 || right_centers.len() < 2)
            && !looks_like_sparse_double_column(left_slot_count, right_slot_count, &valid_blocks))
    {
        mode = "single";
        split_x = if page_width > 0.0 { page_width / 2.0 } else { 0.0 };
    } else {
        mode = "double";
        split_x = if !left_centers.is_empty() && !right_centers.is_empty() {
            (median(&left_centers) + median(&right_centers)) / 2.0
        } else if page_width > 0.0 {
            page_width / 2.0
        } else {
            0.0
        };
    }

    let mut suspicious_orders: Vec<usize> = Vec::new();
    let mut peer_orders: Vec<(usize, usize)> = Vec::new();
    let mut empty_bbox_orders: Vec<usize> = Vec::new();
    let mut absorber_orders: Vec<usize> = Vec::new();
    if mode == "double" {
        for current in &valid_blocks {
            if current.empty_text
                || !matches!(current.column_guess.as_str(), "left" | "right")
                || NON_BODY_SIGNAL_LABELS.contains(&current.label.as_str())
            {
                continue;
            }
            for neighbor in candidate_neighbors(&valid_blocks, current.order) {
                if !neighbor.empty_text {
                    continue;
                }
                if NON_BODY_SIGNAL_LABELS.contains(&neighbor.label.as_str()) {
                    continue;
                }
                if neighbor.column_guess == current.column_guess {
                    continue;
                }
                if !matches!(neighbor.column_guess.as_str(), "left" | "right") {
                    continue;
                }
                if !vertical_overlap(&current.bbox, &neighbor.bbox) {
                    continue;
                }
                if !suspicious_orders.contains(&current.order) {
                    suspicious_orders.push(current.order);
                }
                if !suspicious_orders.contains(&neighbor.order) {
                    suspicious_orders.push(neighbor.order);
                }
                peer_orders.push((current.order, neighbor.order));
                peer_orders.push((neighbor.order, current.order));
                if !absorber_orders.contains(&current.order) {
                    absorber_orders.push(current.order);
                }
                if !empty_bbox_orders.contains(&neighbor.order) {
                    empty_bbox_orders.push(neighbor.order);
                }
            }
        }
    }

    let mut block_signals: Map<String, Value> = Map::new();
    for record in &valid_blocks {
        let suspicious = suspicious_orders.contains(&record.order);
        let peer_order = peer_orders
            .iter()
            .find(|(o, _)| *o == record.order)
            .map(|(_, p)| *p as i64);
        let text_missing_but_bbox_present = empty_bbox_orders.contains(&record.order);
        let peer_block_absorbed_text = absorber_orders.contains(&record.order);
        let mut signals = Map::new();
        signals.insert("provider_cross_column_merge_suspected".into(), Value::Bool(suspicious));
        signals.insert("provider_reading_order_unreliable".into(), Value::Bool(suspicious));
        signals.insert("provider_structure_unreliable".into(), Value::Bool(suspicious));
        signals.insert(
            "provider_text_missing_but_bbox_present".into(),
            Value::Bool(text_missing_but_bbox_present),
        );
        signals.insert(
            "provider_peer_block_absorbed_text".into(),
            Value::Bool(peer_block_absorbed_text),
        );
        signals.insert(
            "provider_suspected_peer_order".into(),
            peer_order.map(Value::from).unwrap_or(Value::Null),
        );
        signals.insert(
            "provider_column_layout_mode".into(),
            Value::String(if page_width > 0.0 { mode.to_string() } else { "unknown".into() }),
        );
        signals.insert(
            "provider_column_index_guess".into(),
            Value::String(record.column_guess.clone()),
        );
        block_signals.insert(record.order.to_string(), Value::Object(signals));
    }

    suspicious_orders.sort_unstable();
    empty_bbox_orders.sort_unstable();
    absorber_orders.sort_unstable();
    let layout_mode = if page_width > 0.0 { mode.to_string() } else { "unknown".into() };
    json!({
        "column_layout_mode": layout_mode,
        "split_x": if split_x != 0.0 { round_to_3(split_x) } else { 0.0 },
        "suspected_orders": Value::Array(suspicious_orders.iter().map(|o| Value::from(*o as i64)).collect()),
        "suspected_count": suspicious_orders.len(),
        "empty_bbox_orders": Value::Array(empty_bbox_orders.iter().map(|o| Value::from(*o as i64)).collect()),
        "absorber_orders": Value::Array(absorber_orders.iter().map(|o| Value::from(*o as i64)).collect()),
        "block_signals": Value::Object(block_signals),
    })
}

fn round_to_3(x: f64) -> f64 {
    rendering_core::rect::round_to_digits(x, 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(label: &str, text: &str, bbox: [f64; 4]) -> Value {
        json!({
            "block_label": label,
            "block_content": text,
            "block_bbox": bbox,
        })
    }

    #[test]
    fn single_column_blocks_detect_single_mode() {
        let blocks = json!([
            block("doc_title", "Title", [40.0, 40.0, 560.0, 90.0]),
            block("text", "A full-width body paragraph.", [40.0, 110.0, 560.0, 170.0]),
            block("footer", "Footer", [40.0, 800.0, 560.0, 830.0]),
        ]);
        let signals = analyze_page_column_signals(blocks.as_array().unwrap(), 600.0);
        assert_eq!(signals["column_layout_mode"], "single");
        assert_eq!(signals["suspected_count"], 0);
        assert_eq!(signals["split_x"], 300.0);
    }

    #[test]
    fn double_column_blocks_detect_split_x() {
        let blocks = json!([
            block("text", "Left col one.", [40.0, 100.0, 250.0, 150.0]),
            block("text", "Right col one.", [350.0, 100.0, 560.0, 150.0]),
            block("text", "Left col two.", [40.0, 170.0, 250.0, 220.0]),
            block("text", "Right col two.", [350.0, 170.0, 560.0, 220.0]),
        ]);
        let signals = analyze_page_column_signals(blocks.as_array().unwrap(), 600.0);
        assert_eq!(signals["column_layout_mode"], "double");
        let split_x = signals["split_x"].as_f64().unwrap();
        // left centers 145, right centers 455 -> (145 + 455) / 2 = 300.
        assert!((split_x - 300.0).abs() < 1e-6);
    }

    #[test]
    fn empty_peer_in_opposite_column_flags_suspicious() {
        let blocks = json!([
            block("text", "Absorber text runs here.", [40.0, 100.0, 250.0, 150.0]),
            block("text", "", [350.0, 100.0, 560.0, 150.0]),
        ]);
        let signals = analyze_page_column_signals(blocks.as_array().unwrap(), 600.0);
        assert_eq!(signals["column_layout_mode"], "double");
        assert_eq!(signals["suspected_orders"], json!([0, 1]));
        assert_eq!(signals["empty_bbox_orders"], json!([1]));
        assert_eq!(signals["absorber_orders"], json!([0]));
        let block_signals = signals["block_signals"].as_object().unwrap();
        let zero = block_signals["0"].as_object().unwrap();
        assert_eq!(zero["provider_cross_column_merge_suspected"], true);
        assert_eq!(zero["provider_peer_block_absorbed_text"], true);
        assert_eq!(zero["provider_suspected_peer_order"], 1);
        assert_eq!(zero["provider_column_index_guess"], "left");
        let one = block_signals["1"].as_object().unwrap();
        assert_eq!(one["provider_text_missing_but_bbox_present"], true);
        assert_eq!(one["provider_peer_block_absorbed_text"], false);
    }

    #[test]
    fn missing_bbox_blocks_are_skipped() {
        let signals = analyze_page_column_signals(&[json!({"block_label": "text"})], 600.0);
        assert_eq!(signals["column_layout_mode"], "single");
        assert!(signals["block_signals"].as_object().unwrap().is_empty());
    }

    #[test]
    fn summarize_document_aggregates_page_counts() {
        let pages = json!([
            {
                "page_index": 0,
                "metadata": {
                    "column_layout_mode": "double",
                    "suspected_cross_column_merge_block_count": 2,
                    "suspected_block_ids": ["p001-b0000", "p001-b0001"],
                    "text_missing_but_bbox_present_count": 1,
                    "text_missing_but_bbox_present_block_ids": ["p001-b0001"],
                    "peer_block_absorbed_text_count": 1,
                    "peer_block_absorbed_text_block_ids": ["p001-b0001"],
                },
            },
            {
                "page_index": 1,
                "metadata": { "column_layout_mode": "single" },
            },
        ]);
        let summary = summarize_document_column_signals(pages.as_array().unwrap());
        assert_eq!(summary["provider"], "paddle");
        assert_eq!(summary["suspicious_cross_column_merge_pages"], 1);
        assert_eq!(summary["suspicious_cross_column_merge_blocks"], 2);
        let page0 = summary["pages"][0].as_object().unwrap();
        assert_eq!(page0["column_layout_mode"], "double");
        assert_eq!(page0["suspected_block_ids"], json!(["p001-b0000", "p001-b0001"]));
        assert_eq!(summary["pages"][1]["column_layout_mode"], "single");
    }
}

/// `summarize_document_column_signals`.
pub fn summarize_document_column_signals(pages: &[Value]) -> Value {
    let mut page_summaries: Vec<Value> = Vec::new();
    let mut suspicious_pages = 0usize;
    let mut suspicious_blocks = 0usize;
    for page in pages {
        let metadata = page.get("metadata").and_then(Value::as_object).cloned().unwrap_or_default();
        let count = metadata
            .get("suspected_cross_column_merge_block_count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let empty_bbox_count = metadata
            .get("text_missing_but_bbox_present_count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let absorber_count = metadata
            .get("peer_block_absorbed_text_count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if count > 0 {
            suspicious_pages += 1;
            suspicious_blocks += count as usize;
        }
        page_summaries.push(json!({
            "page_index": page.get("page_index").and_then(Value::as_i64).unwrap_or(0),
            "column_layout_mode": metadata.get("column_layout_mode").and_then(Value::as_str).unwrap_or("unknown"),
            "suspected_cross_column_merge_block_count": count,
            "suspected_block_ids": metadata.get("suspected_block_ids").cloned().unwrap_or_else(|| Value::Array(vec![])),
            "text_missing_but_bbox_present_count": empty_bbox_count,
            "text_missing_but_bbox_present_block_ids": metadata.get("text_missing_but_bbox_present_block_ids").cloned().unwrap_or_else(|| Value::Array(vec![])),
            "peer_block_absorbed_text_count": absorber_count,
            "peer_block_absorbed_text_block_ids": metadata.get("peer_block_absorbed_text_block_ids").cloned().unwrap_or_else(|| Value::Array(vec![])),
        }));
    }
    json!({
        "provider": "paddle",
        "suspicious_cross_column_merge_pages": suspicious_pages,
        "suspicious_cross_column_merge_blocks": suspicious_blocks,
        "pages": page_summaries,
    })
}
