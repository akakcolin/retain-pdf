// Port of services/rendering/layout/payload/geometry_adjustments.py — the
// effective-inner-bbox builder consumed by `collect_page_seed_metrics`.

use crate::item::Item;
use crate::layout::render_item::get_render_inner_bbox;
use crate::semantics::{block_kind, is_bodylike_block, is_title_like_block};
use crate::typography::geometry::inner_bbox;
use crate::util::{median_f64, py_round};
use std::collections::{HashMap, HashSet};

const BODY_TIGHT_GAP_MAX_INSET_RATIO: f64 = 0.03;
const BODY_TIGHT_GAP_MIN_INSET_PT: f64 = 0.35;
const BODY_TIGHT_GAP_MIN_TARGET_PT: f64 = 1.2;
const BODY_TIGHT_GAP_MAX_TARGET_PT: f64 = 4.0;
const TITLE_BODY_LEFT_TOLERANCE_PT: f64 = 18.0;
const TITLE_BODY_WIDTH_MAX_SCALE: f64 = 1.18;
const SHORT_BODY_REGION_MIN_ANCHORS: usize = 2;
const SHORT_BODY_REGION_X_TOLERANCE_PAGE_RATIO: f64 = 0.10;
const SHORT_BODY_REGION_MAX_HEIGHT_RATIO: f64 = 0.72;
const SHORT_BODY_REGION_MAX_WIDTH_RATIO: f64 = 0.78;
const SHORT_BODY_REGION_TOP_EXPAND_RATIO: f64 = 0.05;
const SHORT_BODY_REGION_RIGHT_EXPAND_RATIO: f64 = 0.30;
const SHORT_BODY_REGION_MIN_GAP_PT: f64 = 1.0;

fn same_text_column(first: &[f64], second: &[f64], page_width: Option<f64>) -> bool {
    let first_width = (first[2] - first[0]).max(1.0);
    let second_width = (second[2] - second[0]).max(1.0);
    let overlap = (first[2].min(second[2]) - first[0].max(second[0])).max(0.0);
    if overlap >= first_width.min(second_width) * 0.55 {
        return true;
    }
    let tolerance = 18.0_f64.max(page_width.unwrap_or(0.0) * 0.035);
    (first[0] - second[0]).abs() <= tolerance
}

fn next_same_column_box(
    current: &[f64],
    later_indices: &[usize],
    effective: &HashMap<usize, Vec<f64>>,
    page_width: Option<f64>,
) -> Option<Vec<f64>> {
    for &index in later_indices {
        let candidate = &effective[&index];
        if same_text_column(current, candidate, page_width) {
            return Some(candidate.clone());
        }
    }
    None
}

fn apply_body_tight_gap_inset(
    effective: &mut HashMap<usize, Vec<f64>>,
    body_flags: &HashMap<usize, bool>,
    page_width: Option<f64>,
    locked_indices: &HashSet<usize>,
) {
    let body_indices: Vec<usize> = effective
        .keys()
        .cloned()
        .filter(|i| body_flags.get(i).copied().unwrap_or(false))
        .collect();
    if body_indices.len() < 2 {
        return;
    }
    let heights: Vec<f64> = body_indices
        .iter()
        .map(|i| (effective[i][3] - effective[i][1]).max(0.0))
        .collect();
    let mut positive: Vec<f64> = heights.iter().cloned().filter(|h| *h > 0.0).collect();
    if positive.is_empty() {
        positive.push(0.0);
    }
    let median_height = median_f64(&positive);
    if median_height <= 0.0 {
        return;
    }
    let target_gap = BODY_TIGHT_GAP_MAX_TARGET_PT.min(BODY_TIGHT_GAP_MIN_TARGET_PT.max(median_height * 0.08));

    let mut ordered: Vec<usize> = body_indices;
    ordered.sort_by(|&a, &b| {
        effective[&a][1]
            .partial_cmp(&effective[&b][1])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(effective[&a][0].partial_cmp(&effective[&b][0]).unwrap_or(std::cmp::Ordering::Equal))
    });
    for position in 0..ordered.len() {
        let current_index = ordered[position];
        if locked_indices.contains(&current_index) {
            continue;
        }
        let current = effective[&current_index].clone();
        let nxt = next_same_column_box(&current, &ordered[position + 1..], effective, page_width);
        let nxt = match nxt {
            Some(v) => v,
            None => continue,
        };
        let gap = nxt[1] - current[3];
        if gap <= -target_gap || gap >= target_gap {
            continue;
        }
        let tightness = 1.0_f64.min((target_gap - gap) / target_gap.max(0.01));
        let current_height = (current[3] - current[1]).max(0.0);
        if current_height <= 0.0 {
            continue;
        }
        let total_inset = current_height * BODY_TIGHT_GAP_MAX_INSET_RATIO * tightness;
        if total_inset < BODY_TIGHT_GAP_MIN_INSET_PT {
            continue;
        }
        let inset_each_side = (current_height * 0.08).min(total_inset / 2.0);
        if inset_each_side > 0.0 && current_height - inset_each_side * 2.0 >= 8.0 {
            let box_mut = effective.get_mut(&current_index).unwrap();
            box_mut[1] = py_round(box_mut[1] + inset_each_side, 3);
            box_mut[3] = py_round(box_mut[3] - inset_each_side, 3);
        }
    }
}

fn is_body_region_text_item(item: &Item) -> bool {
    if is_title_like_block(item) {
        return false;
    }
    if block_kind(item) != "text" && !is_bodylike_block(item) {
        return false;
    }
    let layout_role = item.layout_role.as_deref().unwrap_or("").trim().to_lowercase();
    let semantic_role = item.semantic_role.as_deref().unwrap_or("").trim().to_lowercase();
    matches!(layout_role.as_str(), "" | "paragraph" | "list_item")
        && matches!(semantic_role.as_str(), "" | "body" | "abstract")
}

fn previous_region_anchors(
    current: &[f64],
    previous_indices: &[usize],
    effective: &HashMap<usize, Vec<f64>>,
    page_width: Option<f64>,
) -> Vec<Vec<f64>> {
    let x_tolerance = 18.0_f64.max(page_width.unwrap_or(0.0) * SHORT_BODY_REGION_X_TOLERANCE_PAGE_RATIO);
    let mut anchors: Vec<Vec<f64>> = Vec::new();
    for &index in previous_indices.iter().rev() {
        let candidate = &effective[&index];
        if candidate[3] > current[1] {
            continue;
        }
        if (candidate[0] - current[0]).abs() > x_tolerance {
            continue;
        }
        if candidate[2] <= current[2] {
            continue;
        }
        if !same_text_column(candidate, current, page_width) {
            continue;
        }
        anchors.push(candidate.clone());
        if anchors.len() >= SHORT_BODY_REGION_MIN_ANCHORS {
            break;
        }
    }
    anchors
}

fn apply_short_body_region_expansion(
    effective: &mut HashMap<usize, Vec<f64>>,
    translated_items: &[Item],
    body_flags: &HashMap<usize, bool>,
    page_width: Option<f64>,
    locked_indices: &HashSet<usize>,
) {
    let anchor_indices: Vec<usize> = effective
        .keys()
        .cloned()
        .filter(|i| body_flags.get(i).copied().unwrap_or(false))
        .collect();
    let candidate_indices: Vec<usize> = effective
        .keys()
        .cloned()
        .filter(|i| body_flags.get(i).copied().unwrap_or(false) || is_body_region_text_item(&translated_items[*i]))
        .collect();
    if anchor_indices.len() < SHORT_BODY_REGION_MIN_ANCHORS
        || candidate_indices.len() < SHORT_BODY_REGION_MIN_ANCHORS + 1
    {
        return;
    }

    let mut body_heights: Vec<f64> = Vec::new();
    let mut body_widths: Vec<f64> = Vec::new();
    for &index in &anchor_indices {
        let box_values = &effective[&index];
        let h = (box_values[3] - box_values[1]).max(0.0);
        let w = (box_values[2] - box_values[0]).max(0.0);
        if h > 0.0 {
            body_heights.push(h);
        }
        if w > 0.0 {
            body_widths.push(w);
        }
    }
    if body_heights.is_empty() {
        body_heights.push(0.0);
    }
    if body_widths.is_empty() {
        body_widths.push(0.0);
    }
    let median_height = median_f64(&body_heights);
    let median_width = median_f64(&body_widths);
    if median_height <= 0.0 || median_width <= 0.0 {
        return;
    }

    let mut ordered: Vec<usize> = candidate_indices;
    ordered.sort_by(|&a, &b| {
        effective[&a][1]
            .partial_cmp(&effective[&b][1])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(effective[&a][0].partial_cmp(&effective[&b][0]).unwrap_or(std::cmp::Ordering::Equal))
    });
    let anchor_set: HashSet<usize> = anchor_indices.into_iter().collect();
    for position in 0..ordered.len() {
        let current_index = ordered[position];
        if locked_indices.contains(&current_index) {
            continue;
        }
        if anchor_set.contains(&current_index) {
            continue;
        }
        let current = effective[&current_index].clone();
        let current_height = (current[3] - current[1]).max(0.0);
        let current_width = (current[2] - current[0]).max(0.0);
        if current_height <= 0.0 || current_width <= 0.0 {
            continue;
        }
        if current_height > median_height * SHORT_BODY_REGION_MAX_HEIGHT_RATIO {
            continue;
        }
        if current_width > median_width * SHORT_BODY_REGION_MAX_WIDTH_RATIO {
            continue;
        }
        let previous_indices: Vec<usize> = ordered[..position]
            .iter()
            .cloned()
            .filter(|i| anchor_set.contains(i))
            .collect();
        let anchors = previous_region_anchors(&current, &previous_indices, effective, page_width);
        if anchors.len() < SHORT_BODY_REGION_MIN_ANCHORS {
            continue;
        }

        let previous_bottom = anchors
            .iter()
            .filter(|a| a[3] <= current[1])
            .map(|a| a[3])
            .fold(0.0_f64, f64::max);
        let max_up = (current[1] - previous_bottom - SHORT_BODY_REGION_MIN_GAP_PT).max(0.0);
        let top_expand = (current_height * SHORT_BODY_REGION_TOP_EXPAND_RATIO).min(max_up);
        let box_mut = effective.get_mut(&current_index).unwrap();
        if top_expand > 0.0 {
            box_mut[1] = py_round(box_mut[1] - top_expand, 3);
        }
        let anchor_right = anchors.iter().map(|a| a[2]).fold(0.0_f64, f64::max);
        let page_right = match page_width {
            Some(w) if w > 0.0 => w - 4.0,
            _ => anchor_right,
        };
        let target_right = anchor_right
            .min(page_right)
            .min(current[2] + current_width * SHORT_BODY_REGION_RIGHT_EXPAND_RATIO);
        if target_right > current[2] + 0.5 {
            box_mut[2] = py_round(target_right, 3);
        }
    }
}

fn apply_title_body_width_alignment(
    effective: &mut HashMap<usize, Vec<f64>>,
    translated_items: &[Item],
    body_flags: &HashMap<usize, bool>,
    page_width: Option<f64>,
    locked_indices: &HashSet<usize>,
) {
    let body_boxes: Vec<Vec<f64>> = effective
        .iter()
        .filter(|(i, _)| body_flags.get(i).copied().unwrap_or(false))
        .map(|(_, b)| b.clone())
        .collect();
    if body_boxes.is_empty() {
        return;
    }
    for (index, item) in translated_items.iter().enumerate() {
        if !effective.contains_key(&index) || !is_title_like_block(item) {
            continue;
        }
        if locked_indices.contains(&index) {
            continue;
        }
        let title = effective[&index].clone();
        let title_width = (title[2] - title[0]).max(0.0);
        if title_width <= 0.0 {
            continue;
        }
        let candidates: Vec<&Vec<f64>> = body_boxes
            .iter()
            .filter(|b| {
                b[1] >= title[1]
                    && (b[0] - title[0]).abs() <= TITLE_BODY_LEFT_TOLERANCE_PT
                    && b[2] > title[2]
            })
            .collect();
        if candidates.is_empty() {
            continue;
        }
        let mut target_right = candidates.iter().map(|b| b[2]).fold(0.0_f64, f64::max);
        if let Some(w) = page_width {
            if w > 0.0 {
                target_right = target_right.min(w - 4.0);
            }
        }
        let max_right = title[0] + title_width * TITLE_BODY_WIDTH_MAX_SCALE;
        let box_mut = effective.get_mut(&index).unwrap();
        box_mut[2] = py_round(box_mut[2].max(target_right.min(max_right)), 3);
    }
}

/// `build_effective_inner_bboxes`: per-index inner bbox (render-cached when
/// present) after tight-gap inset, short-body-region expansion and title-body
/// width alignment.
pub fn build_effective_inner_bboxes(
    translated_items: &[Item],
    body_flags: &HashMap<usize, bool>,
    page_width: Option<f64>,
) -> HashMap<usize, Vec<f64>> {
    let mut effective: HashMap<usize, Vec<f64>> = HashMap::new();
    for (index, item) in translated_items.iter().enumerate() {
        let inner = inner_bbox(item);
        if inner.len() != 4 {
            continue;
        }
        match get_render_inner_bbox(item) {
            Some(cached) => {
                effective.insert(index, vec![cached[0], cached[1], cached[2], cached[3]]);
            }
            None => {
                effective.insert(index, inner);
            }
        }
    }
    if effective.is_empty() {
        return effective;
    }
    let locked_indices: HashSet<usize> = translated_items
        .iter()
        .enumerate()
        .filter(|(_, item)| get_render_inner_bbox(item).is_some())
        .map(|(i, _)| i)
        .collect();
    apply_body_tight_gap_inset(&mut effective, body_flags, page_width, &locked_indices);
    apply_short_body_region_expansion(&mut effective, translated_items, body_flags, page_width, &locked_indices);
    apply_title_body_width_alignment(&mut effective, translated_items, body_flags, page_width, &locked_indices);
    effective
}
