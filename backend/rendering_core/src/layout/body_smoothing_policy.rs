// Port of services/rendering/layout/payload/body_smoothing_policy.py — the
// adjacent-body font/leading smoothing pass (C3-N3). Operates on the raw JSON
// block-payload dicts, mutating in place.

use serde_json::Value;

use crate::layout::body_common::payload_is_continuation_member;
use crate::layout::body_context::{
    is_same_column_adjacent_body_pair, payload_center_x, payload_inner_bottom, payload_inner_top,
    smooth_adjacent_body_pair,
};
use crate::layout::payload_dict::inner_bbox4;

/// `smooth_adjacent_body_payloads`: for each body block, smooth it against the
/// best same-column neighbor directly below it, once per pair.
pub fn smooth_adjacent_body_payloads(body_payloads: &mut Vec<Value>, page_text_width_med: f64) {
    let top_left = |idx: usize| {
        let bbox = inner_bbox4(&body_payloads[idx]).unwrap_or([0.0; 4]);
        (bbox[1], bbox[0])
    };
    let mut order: Vec<usize> = (0..body_payloads.len()).collect();
    order.sort_by(|&a, &b| {
        let (ta, la) = top_left(a);
        let (tb, lb) = top_left(b);
        ta.partial_cmp(&tb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut smoothed: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for i in 0..order.len() {
        let current_idx = order[i];
        if payload_is_continuation_member(&body_payloads[current_idx]) {
            continue;
        }
        let mut best_next: Option<usize> = None;
        let mut best_key: Option<(f64, f64)> = None;
        for &nxt_idx in &order[i + 1..] {
            if payload_is_continuation_member(&body_payloads[nxt_idx]) {
                continue;
            }
            if !is_same_column_adjacent_body_pair(&body_payloads[current_idx], &body_payloads[nxt_idx], page_text_width_med) {
                continue;
            }
            let gap = (-4.0_f64).max(payload_inner_top(&body_payloads[nxt_idx]) - payload_inner_bottom(&body_payloads[current_idx]));
            let center_delta = (payload_center_x(&body_payloads[current_idx]) - payload_center_x(&body_payloads[nxt_idx])).abs();
            let key = (gap, center_delta);
            if best_key.is_none() || key < best_key.unwrap() {
                best_key = Some(key);
                best_next = Some(nxt_idx);
            }
        }
        let Some(best_next) = best_next else {
            continue;
        };
        let pair_key = (current_idx.min(best_next), current_idx.max(best_next));
        if smoothed.contains(&pair_key) {
            continue;
        }
        let (lo, hi) = if current_idx < best_next {
            (current_idx, best_next)
        } else {
            (best_next, current_idx)
        };
        let (left, right) = body_payloads.split_at_mut(hi);
        let a = &mut left[lo];
        let b = &mut right[0];
        smooth_adjacent_body_pair(a, b);
        smoothed.insert(pair_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(top: f64, bottom: f64, font: f64, leading: f64, text: &str) -> Value {
        json!({
            "inner_bbox": [50.0, top, 250.0, bottom],
            "translated_text": text,
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": leading,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {},
        })
    }

    const ZH_A: &str = "这是一段足够长的中文文本内容，用于测试相邻正文块之间的字号和行距平滑。";
    const ZH_B: &str = "这是第二段足够长的中文文本内容，它的密度与前面一段略有差异以便观察平滑效果。";

    #[test]
    fn smooths_adjacent_same_column_pair() {
        let mut payloads = vec![payload(0.0, 40.0, 11.0, 0.44, ZH_A), payload(41.0, 90.0, 12.5, 0.58, ZH_B)];
        smooth_adjacent_body_payloads(&mut payloads, 200.0);
        let font0: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        let font1: f64 = payloads[1]["font_size_pt"].as_f64().unwrap();
        assert!((font0 - font1).abs() < 1.0);
        assert!(font0 > 11.0);
    }

    #[test]
    fn different_column_pair_untouched() {
        let mut payloads = vec![
            payload(0.0, 40.0, 11.0, 0.44, ZH_A),
            json!({
                "inner_bbox": [400.0, 41.0, 600.0, 90.0],
                "translated_text": ZH_B,
                "formula_map": [],
                "font_size_pt": 12.5,
                "leading_em": 0.58,
                "render_kind": "markdown",
                "is_body": true,
                "dense_small_box": false,
                "heavy_dense_small_box": false,
                "item": {},
            }),
        ];
        smooth_adjacent_body_payloads(&mut payloads, 200.0);
        assert_eq!(payloads[0]["font_size_pt"], json!(11.0));
        assert_eq!(payloads[1]["font_size_pt"], json!(12.5));
    }
}
