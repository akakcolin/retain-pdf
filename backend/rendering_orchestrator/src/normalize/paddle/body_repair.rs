// Port of `provider_adapters/paddle/body_repair.py` — cross-column body-block
// repair for double-column pages: empty slots get split carryover text from a
// neighboring absorber donor.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::block_labels::block_bbox;

const MIN_EMPTY_SLOT_AREA: f64 = 8_000.0;
const MIN_EMPTY_SLOT_HEIGHT: f64 = 48.0;
const MIN_SAME_BAND_DONOR_TEXT_LENGTH: usize = 12;
const MIN_CARRYOVER_DONOR_TEXT_LENGTH: usize = 48;
const BODY_LABEL_WHITELIST: [&str; 2] = ["text", "abstract"];

/// `repair_body_cross_column_blocks` → (repaired_blocks, repair_metadata, repair_summary).
pub fn repair_body_cross_column_blocks(
    parsing_res_list: &[Value],
    column_signals: &Value,
) -> (Vec<Value>, BTreeMap<i64, Map<String, Value>>, Value) {
    let mut repaired_blocks: Vec<Value> = parsing_res_list.to_vec();
    let mut repair_metadata: BTreeMap<i64, Map<String, Value>> = BTreeMap::new();
    let mut repaired_pairs: Vec<Value> = Vec::new();
    let mut visited_orders: Vec<i64> = Vec::new();

    let mut body_records = build_body_records(&repaired_blocks, column_signals);
    let mut body_by_order: BTreeMap<i64, usize> = BTreeMap::new();
    for (index, record) in body_records.iter().enumerate() {
        body_by_order.insert(record.order, index);
    }
    if !looks_like_double_column_body_page(&body_records) {
        let summary = build_summary(&repaired_pairs, &repair_metadata);
        return (repaired_blocks, repair_metadata, summary);
    }

    let empty_slots: Vec<usize> = body_records
        .iter()
        .enumerate()
        .filter(|(_, r)| is_repairable_empty_slot(r))
        .map(|(i, _)| i)
        .collect();

    // Strategy A: first slot in a column is empty, previous column's last body ends mid-sentence.
    for &slot_index in &empty_slots {
        let slot_order = body_records[slot_index].order;
        if visited_orders.contains(&slot_order) {
            continue;
        }
        let donor = find_column_carryover_donor(
            &body_records,
            slot_index,
            &visited_orders,
        );
        if donor.is_none() {
            continue;
        }
        let donor_index = donor.expect("checked");
        let donor_order = body_records[donor_index].order;
        if apply_repair(
            &mut repaired_blocks,
            &mut repair_metadata,
            &mut repaired_pairs,
            &mut visited_orders,
            &body_records[donor_index],
            &body_records[slot_index],
            "column_carryover",
        ) {
            if let Some(idx) = body_by_order.get(&donor_order).copied() {
                body_records[idx].text = repaired_blocks[donor_order as usize]
                    .get("block_content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if let Some(idx) = body_by_order.get(&slot_order).copied() {
                body_records[idx].text = repaired_blocks[slot_order as usize]
                    .get("block_content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }
    }

    // Strategy B: same-band empty slot, likely one-to-one absorbed peer.
    for &slot_index in &empty_slots {
        let slot_order = body_records[slot_index].order;
        if visited_orders.contains(&slot_order) {
            continue;
        }
        let donor = find_same_band_donor(&body_records, slot_index, &visited_orders);
        if donor.is_none() {
            continue;
        }
        let donor_index = donor.expect("checked");
        let donor_order = body_records[donor_index].order;
        if apply_repair(
            &mut repaired_blocks,
            &mut repair_metadata,
            &mut repaired_pairs,
            &mut visited_orders,
            &body_records[donor_index],
            &body_records[slot_index],
            "same_band",
        ) {
            if let Some(idx) = body_by_order.get(&donor_order).copied() {
                body_records[idx].text = repaired_blocks[donor_order as usize]
                    .get("block_content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
            if let Some(idx) = body_by_order.get(&slot_order).copied() {
                body_records[idx].text = repaired_blocks[slot_order as usize]
                    .get("block_content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
            }
        }
    }

    let summary = build_summary(&repaired_pairs, &repair_metadata);
    (repaired_blocks, repair_metadata, summary)
}

fn build_summary(repaired_pairs: &[Value], repair_metadata: &BTreeMap<i64, Map<String, Value>>) -> Value {
    json!({
        "body_repair_pair_count": repaired_pairs.len(),
        "body_repair_pairs": repaired_pairs,
        "body_repair_block_count": repair_metadata
            .values()
            .filter(|meta| meta.get("provider_body_repair_applied").and_then(Value::as_bool) == Some(true))
            .count(),
    })
}

fn apply_repair(
    repaired_blocks: &mut [Value],
    repair_metadata: &mut BTreeMap<i64, Map<String, Value>>,
    repaired_pairs: &mut Vec<Value>,
    visited_orders: &mut Vec<i64>,
    donor: &BodyRecord,
    slot: &BodyRecord,
    strategy: &str,
) -> bool {
    let donor_text = repaired_blocks[donor.order as usize]
        .get("block_content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let slot_text = repaired_blocks[slot.order as usize]
        .get("block_content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if donor_text.is_empty() || !slot_text.is_empty() {
        return false;
    }
    let Some((donor_repaired_text, slot_repaired_text, split_index)) =
        split_absorbed_text(&donor_text, &donor.bbox, &slot.bbox)
    else {
        mark_failed_attempt(repair_metadata, donor.order, slot.order, "unsafe_split", strategy);
        return false;
    };
    if donor_repaired_text.is_empty() || slot_repaired_text.is_empty() {
        mark_failed_attempt(repair_metadata, donor.order, slot.order, "unsafe_split", strategy);
        return false;
    }
    if let Some(obj) = repaired_blocks[donor.order as usize].as_object_mut() {
        obj.insert("block_content".to_string(), Value::String(donor_repaired_text.clone()));
    }
    if let Some(obj) = repaired_blocks[slot.order as usize].as_object_mut() {
        obj.insert("block_content".to_string(), Value::String(slot_repaired_text.clone()));
    }
    if !visited_orders.contains(&donor.order) {
        visited_orders.push(donor.order);
    }
    if !visited_orders.contains(&slot.order) {
        visited_orders.push(slot.order);
    }
    repaired_pairs.push(json!({
        "absorber_order": donor.order,
        "peer_order": slot.order,
        "split_index": split_index,
        "strategy": strategy,
    }));
    let donor_meta = repair_metadata.entry(donor.order).or_default();
    donor_meta.insert("provider_body_repair_attempted".into(), Value::Bool(true));
    donor_meta.insert("provider_body_repair_applied".into(), Value::Bool(true));
    donor_meta.insert("provider_body_repair_role".into(), "absorber".into());
    donor_meta.insert("provider_body_repair_strategy".into(), Value::String(strategy.to_string()));
    donor_meta.insert("provider_body_repair_peer_order".into(), Value::from(slot.order));
    donor_meta.insert("provider_suspected_peer_order".into(), Value::from(slot.order));
    donor_meta.insert("provider_body_repair_split_index".into(), Value::from(split_index as i64));
    donor_meta.insert(
        "provider_body_repair_original_text_length".into(),
        Value::from(donor_text.chars().count() as i64),
    );
    donor_meta.insert(
        "provider_body_repair_final_text_length".into(),
        Value::from(donor_repaired_text.chars().count() as i64),
    );
    let slot_meta = repair_metadata.entry(slot.order).or_default();
    slot_meta.insert("provider_body_repair_attempted".into(), Value::Bool(true));
    slot_meta.insert("provider_body_repair_applied".into(), Value::Bool(true));
    slot_meta.insert("provider_body_repair_role".into(), "peer".into());
    slot_meta.insert("provider_body_repair_strategy".into(), Value::String(strategy.to_string()));
    slot_meta.insert("provider_body_repair_peer_order".into(), Value::from(donor.order));
    slot_meta.insert("provider_suspected_peer_order".into(), Value::from(donor.order));
    slot_meta.insert("provider_body_repair_split_index".into(), Value::from(split_index as i64));
    slot_meta.insert("provider_body_repair_original_text_length".into(), Value::from(0));
    slot_meta.insert(
        "provider_body_repair_final_text_length".into(),
        Value::from(slot_repaired_text.chars().count() as i64),
    );
    true
}

fn mark_failed_attempt(
    repair_metadata: &mut BTreeMap<i64, Map<String, Value>>,
    donor_order: i64,
    slot_order: i64,
    reason: &str,
    strategy: &str,
) {
    let donor_meta = repair_metadata.entry(donor_order).or_default();
    donor_meta.insert("provider_body_repair_attempted".into(), Value::Bool(true));
    donor_meta.insert("provider_body_repair_applied".into(), Value::Bool(false));
    donor_meta.insert("provider_body_repair_reason".into(), Value::String(reason.to_string()));
    donor_meta.insert("provider_body_repair_strategy".into(), Value::String(strategy.to_string()));
    donor_meta.insert("provider_suspected_peer_order".into(), Value::from(slot_order));
    let slot_meta = repair_metadata.entry(slot_order).or_default();
    slot_meta.insert("provider_body_repair_attempted".into(), Value::Bool(true));
    slot_meta.insert("provider_body_repair_applied".into(), Value::Bool(false));
    slot_meta.insert("provider_body_repair_reason".into(), Value::String(reason.to_string()));
    slot_meta.insert("provider_body_repair_strategy".into(), Value::String(strategy.to_string()));
    slot_meta.insert("provider_suspected_peer_order".into(), Value::from(donor_order));
}

#[derive(Clone, Debug)]
struct BodyRecord {
    order: i64,
    bbox: Vec<f64>,
    text: String,
    empty_text: bool,
    column_guess: String,
    area: f64,
    y0: f64,
    y1: f64,
}

fn build_body_records(parsing_res_list: &[Value], column_signals: &Value) -> Vec<BodyRecord> {
    let body_flow_start = body_flow_start_order(parsing_res_list);
    let mut result = Vec::new();
    for (order, block) in parsing_res_list.iter().enumerate() {
        if !is_body_candidate(parsing_res_list, order, body_flow_start) {
            continue;
        }
        let Some(bbox) = block_bbox(block) else {
            continue;
        };
        let text = block
            .get("block_content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let column_guess = column_guess_for_record(order as i64, &bbox, column_signals).to_string();
        result.push(BodyRecord {
            order: order as i64,
            area: bbox_capacity(&bbox),
            bbox: bbox.clone(),
            y0: bbox[1],
            y1: bbox[3],
            text: text.clone(),
            empty_text: text.chars().count() < 2,
            column_guess,
        });
    }
    result
}

fn looks_like_double_column_body_page(body_records: &[BodyRecord]) -> bool {
    let left: Vec<&BodyRecord> = body_records.iter().filter(|r| r.column_guess == "left").collect();
    let right: Vec<&BodyRecord> = body_records.iter().filter(|r| r.column_guess == "right").collect();
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left.len() >= 2 && right.len() >= 2 {
        return true;
    }
    left.iter().chain(right.iter()).any(|r| r.empty_text)
}

fn is_repairable_empty_slot(record: &BodyRecord) -> bool {
    record.empty_text
        && record.area >= MIN_EMPTY_SLOT_AREA
        && bbox_height(&record.bbox) >= MIN_EMPTY_SLOT_HEIGHT
        && (record.column_guess == "left" || record.column_guess == "right")
}

fn find_same_band_donor(
    body_records: &[BodyRecord],
    slot_index: usize,
    visited_orders: &[i64],
) -> Option<usize> {
    let slot = &body_records[slot_index];
    if has_column_carryover_candidate(body_records, slot_index, visited_orders) {
        return None;
    }
    let mut candidates: Vec<usize> = Vec::new();
    for (index, record) in body_records.iter().enumerate() {
        if visited_orders.contains(&record.order) || record.order == slot.order {
            continue;
        }
        if record.empty_text || record.column_guess == slot.column_guess {
            continue;
        }
        if record.text.chars().count() < MIN_SAME_BAND_DONOR_TEXT_LENGTH {
            continue;
        }
        if !vertical_overlap(&record.bbox, &slot.bbox) {
            continue;
        }
        candidates.push(index);
    }
    candidates
        .into_iter()
        .max_by(|&a, &b| body_records[a].area.partial_cmp(&body_records[b].area).expect("finite"))
}

fn find_column_carryover_donor(
    body_records: &[BodyRecord],
    slot_index: usize,
    visited_orders: &[i64],
) -> Option<usize> {
    let slot = &body_records[slot_index];
    if !(slot.column_guess == "left" || slot.column_guess == "right") {
        return None;
    }
    if !is_first_slot_in_column(slot, body_records) {
        return None;
    }
    if !has_same_column_body_context(slot, body_records) {
        return None;
    }
    resolve_column_carryover_candidate(slot, body_records, visited_orders)
}

fn is_first_slot_in_column(slot: &BodyRecord, body_records: &[BodyRecord]) -> bool {
    let same_column_records: Vec<&BodyRecord> = body_records
        .iter()
        .filter(|r| r.column_guess == slot.column_guess && is_substantive_body_record(r))
        .collect();
    if same_column_records.is_empty() {
        return false;
    }
    let first = same_column_records
        .iter()
        .min_by(|&&a, &&b| a.y0.partial_cmp(&b.y0).expect("finite").then(a.order.cmp(&b.order)))
        .map(|r| *r)
        .expect("non-empty");
    slot.order == first.order
}

fn has_same_column_body_context(slot: &BodyRecord, body_records: &[BodyRecord]) -> bool {
    for record in body_records {
        if record.order == slot.order || record.column_guess != slot.column_guess {
            continue;
        }
        if record.empty_text {
            continue;
        }
        if record.text.chars().count() < MIN_CARRYOVER_DONOR_TEXT_LENGTH {
            continue;
        }
        let gap = vertical_gap(&slot.bbox, &record.bbox);
        if gap <= 96.0 {
            return true;
        }
    }
    false
}

fn has_column_carryover_candidate(
    body_records: &[BodyRecord],
    slot_index: usize,
    visited_orders: &[i64],
) -> bool {
    find_column_carryover_donor(body_records, slot_index, visited_orders).is_some()
}

fn resolve_column_carryover_candidate(
    slot: &BodyRecord,
    body_records: &[BodyRecord],
    visited_orders: &[i64],
) -> Option<usize> {
    let opposite_column = if slot.column_guess == "left" { "right" } else { "left" };
    let mut prior_opposite: Vec<&BodyRecord> = body_records
        .iter()
        .filter(|r| {
            r.column_guess == opposite_column
                && !r.empty_text
                && !visited_orders.contains(&r.order)
                && r.order < slot.order
        })
        .collect();
    if !prior_opposite.is_empty() {
        prior_opposite.sort_by(|a, b| a.order.cmp(&b.order));
        let donor = prior_opposite.last().copied().expect("non-empty");
        if donor.text.chars().count() >= MIN_CARRYOVER_DONOR_TEXT_LENGTH {
            return Some(index_of(donor, body_records));
        }
    }
    let opposite_records: Vec<&BodyRecord> = body_records
        .iter()
        .filter(|r| {
            r.column_guess == opposite_column && !r.empty_text && !visited_orders.contains(&r.order)
        })
        .collect();
    if opposite_records.is_empty() {
        return None;
    }
    let donor = opposite_records
        .iter()
        .max_by(|&&a, &&b| {
            a.y1.partial_cmp(&b.y1).expect("finite").then(a.order.cmp(&b.order))
        })
        .map(|r| *r)
        .expect("non-empty");
    if donor.text.chars().count() < MIN_CARRYOVER_DONOR_TEXT_LENGTH {
        return None;
    }
    Some(index_of(donor, body_records))
}

fn index_of(record: &BodyRecord, body_records: &[BodyRecord]) -> usize {
    body_records.iter().position(|r| std::ptr::eq(r, record)).expect("record in list")
}

fn is_substantive_body_record(record: &BodyRecord) -> bool {
    record.area >= MIN_EMPTY_SLOT_AREA || record.text.chars().count() >= MIN_CARRYOVER_DONOR_TEXT_LENGTH
}

fn is_body_candidate(parsing_res_list: &[Value], order: usize, body_flow_start: i64) -> bool {
    if order >= parsing_res_list.len() {
        return false;
    }
    let raw_label = parsing_res_list[order]
        .get("block_label")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !BODY_LABEL_WHITELIST.contains(&raw_label.as_str()) {
        return false;
    }
    if body_flow_start >= 0 && (order as i64) < body_flow_start {
        return false;
    }
    true
}

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
        if label == "paragraph_title" && !text.is_empty() && text != "abstract" {
            return order as i64;
        }
        if BODY_LABEL_WHITELIST.contains(&label.as_str()) && !text.is_empty()
            && !looks_like_front_matter_text(&text)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn text_block(label: &str, content: &str, bbox: [f64; 4]) -> Value {
        json!({
            "block_label": label,
            "block_content": content,
            "block_bbox": bbox,
        })
    }

    #[test]
    fn split_absorbed_text_short_text_returns_none() {
        assert_eq!(split_absorbed_text("short", &[0.0, 0.0, 100.0, 50.0], &[100.0, 0.0, 200.0, 50.0]), None);
    }

    #[test]
    fn split_absorbed_text_even_capacity_splits_in_half() {
        let (donor, peer, index) = split_absorbed_text(
            "the paragraph text is long enough to split across two balanced boxes",
            &[0.0, 0.0, 200.0, 100.0],
            &[200.0, 0.0, 400.0, 100.0],
        )
        .expect("split");
        assert!(!donor.is_empty() && !peer.is_empty());
        assert!(index > 0);
        assert_eq!(format!("{donor} {peer}"), "the paragraph text is long enough to split across two balanced boxes");
    }

    #[test]
    fn choose_split_index_handles_small_target_without_underflow() {
        // target_index < max(8, len/6): the pre-fix `target - 8` would underflow.
        let index = choose_split_index("abcdefghij klmnop", 2);
        assert_eq!(index, Some(10));
        assert_eq!(choose_split_index("ab cd", 1), None);
        assert_eq!(choose_split_index("x", 0), None);
    }

    #[test]
    fn repair_same_band_moves_text_into_empty_slot() {
        let blocks = json!([
            text_block("paragraph_title", "Methods", [40.0, 60.0, 560.0, 90.0]),
            text_block("text", "", [40.0, 110.0, 250.0, 170.0]),
            text_block(
                "text",
                "A long donor paragraph with enough text to split across the empty peer slot in the neighboring column.",
                [350.0, 110.0, 560.0, 170.0],
            ),
            text_block("text", "More body text left column.", [40.0, 180.0, 250.0, 240.0]),
            text_block("text", "More body text right column.", [350.0, 180.0, 560.0, 240.0]),
        ]);
        let signals = json!({ "split_x": 300.0, "block_signals": {} });
        let (repaired, metadata, summary) = repair_body_cross_column_blocks(blocks.as_array().unwrap(), &signals);
        assert_eq!(summary["body_repair_pair_count"], 1);
        assert_eq!(summary["body_repair_block_count"], 2);
        let pair = &summary["body_repair_pairs"][0];
        assert_eq!(pair["strategy"], "same_band");
        assert_eq!(pair["absorber_order"], 2);
        assert_eq!(pair["peer_order"], 1);
        // slot absorbed the carryover text; absorber was trimmed.
        let slot_text = repaired[1]["block_content"].as_str().unwrap();
        assert!(!slot_text.is_empty());
        assert!(repaired[2]["block_content"].as_str().unwrap().len() < 105);
        let absorber_meta = metadata.get(&2).unwrap();
        assert_eq!(absorber_meta["provider_body_repair_role"], "absorber");
        assert_eq!(absorber_meta["provider_body_repair_applied"], true);
        let peer_meta = metadata.get(&1).unwrap();
        assert_eq!(peer_meta["provider_body_repair_role"], "peer");
        assert_eq!(peer_meta["provider_suspected_peer_order"], 2);
    }

    #[test]
    fn repair_noop_on_single_column_body() {
        let blocks = json!([
            text_block("text", "Single column full width paragraph.", [40.0, 100.0, 560.0, 160.0]),
            text_block("footer", "Footer", [40.0, 800.0, 560.0, 830.0]),
        ]);
        let signals = json!({ "split_x": 300.0, "block_signals": {} });
        let (_, metadata, summary) = repair_body_cross_column_blocks(blocks.as_array().unwrap(), &signals);
        assert_eq!(summary["body_repair_pair_count"], 0);
        assert!(metadata.is_empty());
    }

    #[test]
    fn column_guess_from_signal_and_split_x() {
        let signals = json!({
            "split_x": 300.0,
            "block_signals": { "0": { "provider_column_index_guess": "left" } },
        });
        assert_eq!(column_guess_for_record(0, &[40.0, 0.0, 250.0, 50.0], &signals), "left");
        assert_eq!(column_guess_for_record(1, &[350.0, 0.0, 560.0, 50.0], &signals), "right");
        assert_eq!(column_guess_for_record(9, &[40.0, 0.0, 250.0, 50.0], &signals), "left");
        let no_split = json!({ "block_signals": {} });
        assert_eq!(column_guess_for_record(0, &[40.0, 0.0, 250.0, 50.0], &no_split), "unknown");
    }

    #[test]
    fn body_flow_start_order_skips_front_matter() {
        let blocks = json!([
            text_block("doc_title", "Title", [40.0, 40.0, 560.0, 90.0]),
            text_block("abstract", "Abstract text.", [40.0, 110.0, 560.0, 150.0]),
            text_block("paragraph_title", "Introduction", [40.0, 170.0, 560.0, 200.0]),
            text_block("text", "Body starts here.", [40.0, 220.0, 560.0, 260.0]),
        ]);
        assert_eq!(body_flow_start_order(blocks.as_array().unwrap()), 2);
    }
}

/// `_split_absorbed_text` → (donor_text, peer_text, split_index).
fn split_absorbed_text(absorber_text: &str, absorber_bbox: &[f64], peer_bbox: &[f64]) -> Option<(String, String, usize)> {
    let normalized = absorber_text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() < 16 {
        return None;
    }
    let absorber_capacity = bbox_capacity(absorber_bbox);
    let peer_capacity = bbox_capacity(peer_bbox);
    let total_capacity = absorber_capacity + peer_capacity;
    if total_capacity <= 0.0 {
        return None;
    }
    let peer_ratio = (peer_capacity / total_capacity).max(0.18).min(0.55);
    let donor_prefix_ratio = 1.0 - peer_ratio;
    let target_index = ((normalized.chars().count() as f64) * donor_prefix_ratio).floor() as usize;
    let split_index = choose_split_index(&normalized, target_index)?;
    let chars: Vec<char> = normalized.chars().collect();
    let donor_text: String = chars[..split_index].iter().collect::<String>().trim().to_string();
    let peer_text: String = chars[split_index..].iter().collect::<String>().trim().to_string();
    if donor_text.chars().count() < 6 || peer_text.chars().count() < 6 {
        return None;
    }
    Some((donor_text, peer_text, split_index))
}

fn choose_split_index(text: &str, target_index: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let length = chars.len();
    if length < 2 {
        return None;
    }
    let lower = 4usize.max(target_index.saturating_sub((8).max(length / 6)));
    let upper = length.saturating_sub(4).min(target_index + (8).max(length / 6));
    if lower >= upper {
        return None;
    }
    let mut preferred: Vec<usize> = Vec::new();
    for (index, c) in chars.iter().enumerate() {
        if *c == '\n' || c.is_whitespace() {
            preferred.push(index);
        }
    }
    let candidates: Vec<usize> = preferred
        .into_iter()
        .filter(|&i| lower <= i && i <= upper)
        .collect();
    if candidates.is_empty() {
        return None;
    }
    candidates
        .into_iter()
        .min_by(|&a, &b| {
            (a as i64 - target_index as i64).abs().cmp(&(b as i64 - target_index as i64).abs())
        })
}

fn column_guess_for_record(order: i64, bbox: &[f64], column_signals: &Value) -> &'static str {
    if bbox.len() != 4 {
        return "unknown";
    }
    let block_signals = column_signals.get("block_signals").and_then(Value::as_object);
    let signal = block_signals
        .and_then(|m| m.get(&order.to_string()))
        .and_then(Value::as_object);
    let signal_guess = signal
        .and_then(|m| m.get("provider_column_index_guess"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if matches!(signal_guess.as_str(), "left" | "right" | "full") {
        return match signal_guess.as_str() {
            "left" => "left",
            "right" => "right",
            _ => "full",
        };
    }
    let split_x = column_signals
        .get("split_x")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if split_x <= 0.0 {
        return "unknown";
    }
    if center_x(bbox) <= split_x {
        "left"
    } else {
        "right"
    }
}

fn looks_like_front_matter_text(text: &str) -> bool {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_lowercase();
    !compact.is_empty() && compact.starts_with("keywords:")
}

fn center_x(bbox: &[f64]) -> f64 {
    if bbox.len() == 4 {
        (bbox[0] + bbox[2]) / 2.0
    } else {
        0.0
    }
}

fn bbox_capacity(bbox: &[f64]) -> f64 {
    if bbox.len() != 4 {
        return 0.0;
    }
    let width = (bbox[2] - bbox[0]).max(1.0);
    let height = (bbox[3] - bbox[1]).max(1.0);
    width * height
}

fn bbox_height(bbox: &[f64]) -> f64 {
    if bbox.len() != 4 {
        return 0.0;
    }
    (bbox[3] - bbox[1]).max(0.0)
}

fn vertical_overlap(a: &[f64], b: &[f64]) -> bool {
    if a.len() != 4 || b.len() != 4 {
        return false;
    }
    let overlap = a[3].min(b[3]) - a[1].max(b[1]);
    let min_height = ((a[3] - a[1]).max(1.0)).min((b[3] - b[1]).max(1.0));
    overlap >= min_height * 0.25
}

fn vertical_gap(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != 4 || b.len() != 4 {
        return f64::INFINITY;
    }
    if vertical_overlap(a, b) {
        return 0.0;
    }
    if a[3] < b[1] {
        return b[1] - a[3];
    }
    if b[3] < a[1] {
        return a[1] - b[3];
    }
    0.0
}
