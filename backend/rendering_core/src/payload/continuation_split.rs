// Port of services/rendering/layout/payload/continuation_split.py.

use crate::payload::formula_cost::token_units;
use crate::payload::text_common::{tokenize_protected_text, trim_joined_tokens, SPLIT_PUNCTUATION};
use std::collections::HashMap;

pub const CONTINUATION_REBALANCE_MAX_PASSES: usize = 3;
pub const CONTINUATION_REBALANCE_TOKEN_WINDOW: i64 = 80;
pub const CONTINUATION_REBALANCE_TARGET_TOLERANCE: f64 = 3.5;
pub const CONTINUATION_REBALANCE_IMBALANCE_TRIGGER: f64 = 12.0;
pub const CONTINUATION_REBALANCE_PUNCTUATION_PENALTY: f64 = 1.75;
pub const CONTINUATION_REBALANCE_NON_PUNCT_MIN_MOVE_UNITS: f64 = 18.0;

fn range_cost(prefix_costs: &[f64], start: usize, end: usize) -> f64 {
    prefix_costs[end] - prefix_costs[start]
}

fn probe_has_split_punctuation(tokens: &[String], start: usize, end: usize) -> bool {
    let mut probe = end;
    while probe > start && tokens[probe - 1].chars().all(|c| c.is_whitespace()) {
        probe -= 1;
    }
    if probe <= start {
        return false;
    }
    let tok = &tokens[probe - 1];
    SPLIT_PUNCTUATION.iter().any(|p| tok.ends_with(p))
}

fn trim_range_edges(tokens: &[String], start: usize, end: usize) -> (usize, usize) {
    let mut s = start;
    let mut e = end;
    while s < e && tokens[s].chars().all(|c| c.is_whitespace()) {
        s += 1;
    }
    while e > s && tokens[e - 1].chars().all(|c| c.is_whitespace()) {
        e -= 1;
    }
    (s, e)
}

fn range_text(tokens: &[String], start: usize, end: usize) -> String {
    let (s, e) = trim_range_edges(tokens, start, end);
    trim_joined_tokens(&tokens[s..e])
}

fn candidate_rebalance_positions(
    tokens: &[String],
    _prefix_costs: &[f64],
    start: usize,
    end: usize,
    ideal_end: i64,
) -> Vec<usize> {
    let mut positions: Vec<usize> = Vec::new();
    let left = (start as i64 + 1).max(ideal_end - CONTINUATION_REBALANCE_TOKEN_WINDOW as i64);
    let right = (end as i64 - 1).min(ideal_end + CONTINUATION_REBALANCE_TOKEN_WINDOW as i64);
    if left <= right {
        for probe in left..=right {
            positions.push(probe as usize);
        }
    }
    for probe in (start + 1)..end {
        if probe_has_split_punctuation(tokens, start, probe) {
            positions.push(probe);
        }
    }
    positions.push(start + 1);
    positions.push(end - 1);
    positions.sort_unstable();
    positions.dedup();
    positions.into_iter().filter(|p| start < *p && *p < end).collect()
}

fn rebalance_chunk_ranges(
    tokens: &[String],
    prefix_costs: &[f64],
    capacities: &[f64],
    ranges: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    if ranges.len() <= 1 {
        return ranges.to_vec();
    }
    let normalized_capacities: Vec<f64> = capacities.iter().map(|v| v.max(1.0)).collect();
    let mut rebalanced = ranges.to_vec();
    for _ in 0..CONTINUATION_REBALANCE_MAX_PASSES {
        let mut changed = false;
        for index in 0..(rebalanced.len() - 1) {
            let (left_start, left_end) = rebalanced[index];
            let (right_start, right_end) = rebalanced[index + 1];
            if left_start >= left_end || right_start >= right_end {
                continue;
            }
            let left_cost = range_cost(prefix_costs, left_start, left_end);
            let right_cost = range_cost(prefix_costs, right_start, right_end);
            let combined_cost = left_cost + right_cost;
            if combined_cost <= 0.0 {
                continue;
            }
            let left_target = combined_cost * normalized_capacities[index]
                / (normalized_capacities[index] + normalized_capacities[index + 1]);
            let imbalance = left_cost - left_target;
            if imbalance <= CONTINUATION_REBALANCE_IMBALANCE_TRIGGER {
                continue;
            }
            let left_ratio = left_cost / normalized_capacities[index];
            let right_ratio = right_cost / normalized_capacities[index + 1];
            if left_ratio <= right_ratio + 0.08 {
                continue;
            }

            let mut cumulative = 0.0;
            let mut ideal_probe = left_end as i64 - 1;
            let mut probe = left_end as i64 - 1;
            while probe > left_start as i64 {
                cumulative += token_units(&tokens[probe as usize], &HashMap::new());
                if cumulative >= imbalance {
                    ideal_probe = probe;
                    break;
                }
                probe -= 1;
            }

            let mut best_probe: Option<usize> = None;
            let mut best_score: Option<f64> = None;
            for probe in candidate_rebalance_positions(tokens, prefix_costs, left_start, left_end, ideal_probe) {
                let moved_cost = range_cost(prefix_costs, probe, left_end);
                if moved_cost <= 0.0 {
                    continue;
                }
                if moved_cost < CONTINUATION_REBALANCE_NON_PUNCT_MIN_MOVE_UNITS
                    && !probe_has_split_punctuation(tokens, left_start, probe)
                {
                    continue;
                }
                let next_left_cost = range_cost(prefix_costs, left_start, probe);
                let next_right_cost = range_cost(prefix_costs, probe, left_end) + right_cost;
                let target_delta = (next_left_cost - left_target).abs();
                let ratio_delta = ((next_left_cost / normalized_capacities[index])
                    - (next_right_cost / normalized_capacities[index + 1]))
                    .abs();
                let punctuation_penalty = if probe_has_split_punctuation(tokens, left_start, probe) {
                    0.0
                } else {
                    CONTINUATION_REBALANCE_PUNCTUATION_PENALTY
                };
                let score = target_delta + ratio_delta * 6.0 + punctuation_penalty;
                if best_score.is_none() || score < best_score.unwrap() {
                    best_score = Some(score);
                    best_probe = Some(probe);
                }
            }
            let best_probe = match best_probe {
                Some(p) => p,
                None => continue,
            };
            let next_left_cost = range_cost(prefix_costs, left_start, best_probe);
            if (next_left_cost - left_cost).abs() <= CONTINUATION_REBALANCE_TARGET_TOLERANCE {
                continue;
            }
            rebalanced[index] = (left_start, best_probe);
            rebalanced[index + 1] = (best_probe, right_end);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    rebalanced
}

pub fn split_protected_text_for_boxes(
    protected_text: &str,
    formula_map: &[(String, String)],
    capacities: &[f64],
    preferred_weights: Option<&[f64]>,
    _direct_math_mode: bool,
) -> Vec<String> {
    if capacities.len() <= 1 {
        return vec![protected_text.trim().to_string()];
    }
    let tokens = tokenize_protected_text(protected_text);
    if tokens.is_empty() {
        return vec![String::new(); capacities.len()];
    }
    let formula_lookup: HashMap<String, String> = formula_map
        .iter()
        .map(|(placeholder, formula_text)| (placeholder.clone(), formula_text.clone()))
        .collect();
    let token_costs: Vec<f64> = tokens.iter().map(|t| token_units(t, &formula_lookup)).collect();
    let mut prefix_costs = vec![0.0f64];
    for cost in &token_costs {
        prefix_costs.push(prefix_costs[prefix_costs.len() - 1] + cost);
    }
    let mut remaining_cost: f64 = token_costs.iter().sum();
    if remaining_cost <= 0.0 {
        let mut chunks = vec![String::new(); capacities.len()];
        chunks[0] = trim_joined_tokens(&tokens);
        return chunks;
    }

    let capacity_weights: Vec<f64> = capacities.iter().map(|c| c.max(1.0)).collect();
    let mut total_preferred: f64 = match preferred_weights {
        Some(weights) => weights.iter().map(|v| v.max(1.0)).sum(),
        None => capacity_weights.iter().sum(),
    };
    let preferred_costs: Vec<f64> = match preferred_weights {
        Some(weights) => weights.iter().map(|v| v.max(1.0)).collect(),
        None => capacity_weights.clone(),
    };

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut cursor = 0usize;
    for (box_index, capacity) in capacities.iter().enumerate() {
        let current_capacity = capacity.max(1.0);
        if box_index == capacities.len() - 1 {
            ranges.push((cursor, tokens.len()));
            break;
        }
        let remaining_boxes = capacities.len() - box_index - 1;
        let max_end = tokens.len() - remaining_boxes;
        let share = preferred_costs[box_index] / total_preferred.max(1.0);
        let share_target = remaining_cost * share;
        let soft_target = share_target.min(current_capacity * 0.98);

        let mut anchor = cursor + 1;
        while anchor < max_end && range_cost(&prefix_costs, cursor, anchor) < soft_target {
            anchor += 1;
        }

        let mut candidate_positions: Vec<usize> = Vec::new();
        let left = (cursor as i64 + 1).max(anchor as i64 - 24);
        let right = (max_end as i64).min(anchor as i64 + 24);
        if left <= right {
            for probe in left..=right {
                candidate_positions.push(probe as usize);
            }
        }
        for probe in (cursor + 1)..=(max_end) {
            if probe_has_split_punctuation(&tokens, cursor, probe) {
                candidate_positions.push(probe);
            }
        }
        candidate_positions.push(cursor + 1);
        candidate_positions.push(max_end);
        candidate_positions.sort_unstable();
        candidate_positions.dedup();

        let remaining_capacity_after: f64 = capacity_weights[box_index + 1..].iter().sum();
        let mut best_end = cursor + 1;
        let mut best_score: Option<f64> = None;
        let (current_overflow_weight, future_overflow_weight) =
            if remaining_capacity_after > 0.0 && current_capacity >= remaining_capacity_after * 2.0 {
                (28.0, 140.0)
            } else {
                (72.0, 108.0)
            };
        for probe in candidate_positions {
            if probe <= cursor || probe > max_end {
                continue;
            }
            let current_cost = range_cost(&prefix_costs, cursor, probe);
            let future_cost = remaining_cost - current_cost;
            let current_overflow = (current_cost - current_capacity * 1.01).max(0.0);
            let future_overflow = if remaining_boxes > 0 {
                (future_cost - remaining_capacity_after * 1.03).max(0.0)
            } else {
                0.0
            };
            let target_delta = (current_cost - share_target).abs();
            let underfill = (current_capacity * 0.55 - current_cost).max(0.0);
            let punctuation_penalty =
                if probe_has_split_punctuation(&tokens, cursor, probe) { 0.0 } else { 1.25 };
            let score = current_overflow * current_overflow_weight
                + future_overflow * future_overflow_weight
                + target_delta
                + underfill * 0.1
                + punctuation_penalty;
            if best_score.is_none() || score < best_score.unwrap() {
                best_score = Some(score);
                best_end = probe;
            }
        }

        ranges.push((cursor, best_end));
        remaining_cost = (remaining_cost - range_cost(&prefix_costs, cursor, best_end)).max(0.0);
        total_preferred = (total_preferred - preferred_costs[box_index]).max(1.0);
        cursor = best_end;
    }

    while ranges.len() < capacities.len() {
        ranges.push((tokens.len(), tokens.len()));
    }

    let rebalanced = rebalance_chunk_ranges(&tokens, &prefix_costs, capacities, &ranges[..capacities.len()]);
    let mut chunks: Vec<String> = rebalanced
        .iter()
        .take(capacities.len())
        .map(|&(start, end)| range_text(&tokens, start, end))
        .collect();
    while chunks.len() < capacities.len() {
        chunks.push(String::new());
    }
    chunks.truncate(capacities.len());
    chunks
}
