// Port of services/rendering/layout/payload/block_seed_payload_factory.py —
// `build_seed_payload_for_item`. The typography-memory store is not ported, so
// the deterministic no-memory path always runs (memory_hit = false) and each
// payload still carries its `_typography_memory_key` feature hash.

use crate::font_roles::resolve_font_weight;
use crate::item::Item;
use crate::layout::block_seed_body_policy::{
    adjust_body_seed_font_size, is_dense_small_box, is_heavy_dense_small_box, is_wide_aspect_body_text,
    relax_wide_aspect_body_leading,
};
use crate::layout::body_context::page_box_area_ratio;
use crate::layout::fit::{fit_translated_block_metrics, solve_title_fit, TitleFitDecision};
use crate::layout::line_structure::fit_preserved_line_block_metrics;
use crate::layout::render_item::{
    get_render_first_line_indent_pt, get_render_formula_map, get_render_protected_text,
    set_render_inner_bbox,
};
use crate::layout::typography_capacity::build_typography_feature;
use crate::leading_fit::normalize_leading_em_for_font_size;
use crate::leading_fit::{
    BODY_LEADING_FLOOR_MIN, BODY_LEADING_MAX, BODY_LEADING_MIN, BODY_LEADING_SIZE_ADJUST,
    NON_BODY_LEADING_FLOOR_MIN, NON_BODY_LEADING_MAX, NON_BODY_LEADING_MIN, NON_BODY_LEADING_SIZE_ADJUST,
};
use crate::payload::block_seed_metrics::PageSeedMetrics;
use crate::payload::text_common::{
    is_flag_like_plain_text_block, layout_density_ratio, translation_density_ratio, COMPACT_SCALE,
    HEAVY_COMPACT_RATIO,
};
use crate::semantics::is_title_like_block;
use crate::title_fit_limits::resolve_title_fill_max_font_size_pt;
use crate::typography::geometry::{cover_bbox, inner_bbox};
use crate::typography::line_metrics::{bbox_height, bbox_width};
use crate::util::py_round;

/// First non-empty formula-map array among the raw item's four candidate keys,
/// mirroring `get_render_formula_map`'s `or`-chain for the payload's raw value.
fn raw_formula_map_json(raw_item: &serde_json::Value) -> serde_json::Value {
    for key in [
        "render_formula_map",
        "translation_unit_formula_map",
        "group_formula_map",
        "formula_map",
    ] {
        if let Some(arr) = raw_item.get(key).and_then(|v| v.as_array()) {
            if !arr.is_empty() {
                return serde_json::Value::Array(arr.clone());
            }
        }
    }
    serde_json::Value::Array(Vec::new())
}

fn array_value(values: &[f64]) -> serde_json::Value {
    serde_json::Value::Array(values.iter().map(|v| serde_json::Value::from(*v)).collect())
}

fn title_fit_json(decision: &Option<TitleFitDecision>) -> serde_json::Value {
    let decision = match decision {
        Some(d) => d,
        None => return serde_json::Value::Null,
    };
    let mut map = serde_json::Map::new();
    map.insert("font_size_pt".to_string(), serde_json::Value::from(decision.font_size_pt));
    map.insert("leading_em".to_string(), serde_json::Value::from(decision.leading_em));
    map.insert("fit_to_box".to_string(), serde_json::Value::Bool(decision.fit_to_box));
    map.insert("fit_single_line".to_string(), serde_json::Value::Bool(decision.fit_single_line));
    map.insert("fit_min_font_size_pt".to_string(), serde_json::Value::from(decision.fit_min_font_size_pt));
    map.insert("fit_max_font_size_pt".to_string(), serde_json::Value::from(decision.fit_max_font_size_pt));
    map.insert("fit_min_leading_em".to_string(), serde_json::Value::from(decision.fit_min_leading_em));
    map.insert("fit_max_height_pt".to_string(), serde_json::Value::from(decision.fit_max_height_pt));
    map.insert("fit_target_width_pt".to_string(), serde_json::Value::from(decision.fit_target_width_pt));
    map.insert("fit_target_height_pt".to_string(), serde_json::Value::from(decision.fit_target_height_pt));
    serde_json::Value::Object(map)
}

/// `item.get("_render_text_color")` when present, else the given default. The
/// Python default is an int triple, so the fallback emits integers.
fn raw_color(raw_item: &serde_json::Value, key: &str, default: [i64; 3]) -> serde_json::Value {
    match raw_item.get(key) {
        Some(v) if v.is_array() => v.clone(),
        _ => serde_json::Value::Array(default.iter().map(|x| serde_json::Value::from(*x)).collect()),
    }
}

pub fn build_seed_payload_for_item(
    index: usize,
    item: &Item,
    raw_item: &serde_json::Value,
    metrics: &PageSeedMetrics,
    page_width: Option<f64>,
    page_height: Option<f64>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let translated_text = get_render_protected_text(item);
    let bbox: [f64; 4] = item.bbox?;
    if translated_text.is_empty() {
        return None;
    }
    let bbox_slice = vec![bbox[0], bbox[1], bbox[2], bbox[3]];
    let use_raw_text_bbox = item.use_raw_text_bbox;

    let (mut font_size_pt, mut leading_em) =
        metrics.base_metrics.get(&index).copied().unwrap_or((0.0, 0.0));
    let formula_map = get_render_formula_map(item);
    let mut title_fit: Option<TitleFitDecision> = None;

    let density_ratio = translation_density_ratio(item, &translated_text);
    let page_box_area_ratio = page_box_area_ratio(&bbox_slice, page_width, page_height);
    let raw_inner_bbox = inner_bbox(item);
    let item_inner_bbox = metrics
        .effective_inner_bboxes
        .get(&index)
        .cloned()
        .unwrap_or(raw_inner_bbox);
    let preserve_line_breaks = item.render_preserve_line_breaks;
    let seed_line_step = (font_size_pt * 1.02).max(font_size_pt * (1.0 + leading_em));
    let layout_density = layout_density_ratio(&item_inner_bbox, &translated_text, font_size_pt, seed_line_step);
    let dense_small_box = is_dense_small_box(density_ratio, layout_density, page_box_area_ratio);
    let heavy_dense_small_box =
        is_heavy_dense_small_box(density_ratio, layout_density, page_box_area_ratio, HEAVY_COMPACT_RATIO);
    let block_height = bbox_height(item);
    let block_width = bbox_width(item);
    let body_like_single_line = metrics.body_flags.get(&index).copied().unwrap_or(false);
    let wide_aspect_body_text = is_wide_aspect_body_text(body_like_single_line, block_width, block_height);

    let memory_feature = build_typography_feature(
        item,
        &translated_text,
        font_size_pt,
        leading_em,
        page_width,
        page_height,
        metrics.page_text_width_med,
        body_like_single_line,
        dense_small_box,
        heavy_dense_small_box,
        wide_aspect_body_text,
        preserve_line_breaks,
    );
    let title_like = is_title_like_block(item);
    // Native typography-memory store not ported: always the deterministic
    // no-memory path (matches Python with RETAIN_RENDER_TYPOGRAPHY_MEMORY=0).
    let memory_hit = false;
    if !memory_hit {
        font_size_pt = adjust_body_seed_font_size(
            font_size_pt,
            metrics.page_body_font_size_pt,
            body_like_single_line,
            dense_small_box,
            heavy_dense_small_box,
            wide_aspect_body_text,
        );
    }

    if title_like {
        let mut title_item = item.clone();
        set_render_inner_bbox(&mut title_item, [item_inner_bbox[0], item_inner_bbox[1], item_inner_bbox[2], item_inner_bbox[3]]);
        title_fit = solve_title_fit(
            &title_item,
            &translated_text,
            &formula_map,
            font_size_pt,
            leading_em,
            resolve_title_fill_max_font_size_pt(item, font_size_pt),
        );
        if let Some(fit) = &title_fit {
            font_size_pt = fit.font_size_pt;
            leading_em = fit.leading_em;
        }
    }

    if dense_small_box && !metrics.body_flags.get(&index).copied().unwrap_or(false) && !title_like && !memory_hit {
        font_size_pt = py_round(font_size_pt * COMPACT_SCALE, 2);
        leading_em = py_round(leading_em * COMPACT_SCALE, 2);
    }

    if !title_like {
        let mut fit_item = item.clone();
        fit_item.is_body_text_candidate = body_like_single_line;
        fit_item.dense_small_box = dense_small_box;
        fit_item.heavy_dense_small_box = heavy_dense_small_box;
        fit_item.wide_aspect_body_text = wide_aspect_body_text;
        set_render_inner_bbox(&mut fit_item, [item_inner_bbox[0], item_inner_bbox[1], item_inner_bbox[2], item_inner_bbox[3]]);
        let (fit_font, fit_leading) = fit_translated_block_metrics(
            &fit_item,
            &translated_text,
            &formula_map,
            font_size_pt,
            leading_em,
            if body_like_single_line { metrics.page_body_font_size_pt } else { None },
        );
        font_size_pt = fit_font;
        leading_em = fit_leading;
    }

    if body_like_single_line && !title_like {
        leading_em = normalize_leading_em_for_font_size(
            font_size_pt,
            leading_em,
            metrics.page_body_font_size_pt.unwrap_or(metrics.page_font_size),
            BODY_LEADING_MIN,
            BODY_LEADING_MAX,
            BODY_LEADING_SIZE_ADJUST,
            Some(BODY_LEADING_FLOOR_MIN),
        );
    } else if !title_like {
        leading_em = normalize_leading_em_for_font_size(
            font_size_pt,
            leading_em,
            metrics.page_font_size,
            NON_BODY_LEADING_MIN,
            NON_BODY_LEADING_MAX,
            NON_BODY_LEADING_SIZE_ADJUST,
            Some(NON_BODY_LEADING_FLOOR_MIN),
        );
    }

    if wide_aspect_body_text {
        leading_em = relax_wide_aspect_body_leading(&item_inner_bbox, &translated_text, &formula_map, font_size_pt, leading_em);
    }
    if preserve_line_breaks {
        let (lf, ll) = fit_preserved_line_block_metrics(&item_inner_bbox, &translated_text, font_size_pt, leading_em);
        font_size_pt = lf;
        leading_em = ll;
    }
    let item_cover_bbox = cover_bbox(item);
    let render_kind = if item.force_plain_line || is_flag_like_plain_text_block(item) {
        "plain_line"
    } else {
        "markdown"
    };
    let raw_bbox = raw_item
        .get("bbox")
        .cloned()
        .unwrap_or_else(|| array_value(&bbox_slice));

    let mut payload = serde_json::Map::new();
    payload.insert("index".to_string(), serde_json::Value::from(index));
    payload.insert("item".to_string(), raw_item.clone());
    payload.insert("bbox".to_string(), raw_bbox);
    payload.insert("cover_bbox".to_string(), array_value(&item_cover_bbox));
    payload.insert(
        "inner_bbox".to_string(),
        if use_raw_text_bbox {
            raw_item.get("bbox").cloned().unwrap_or_else(|| array_value(&bbox_slice))
        } else {
            array_value(&item_inner_bbox)
        },
    );
    payload.insert("translated_text".to_string(), serde_json::Value::String(translated_text));
    payload.insert("formula_map".to_string(), raw_formula_map_json(raw_item));
    payload.insert("render_kind".to_string(), serde_json::Value::String(render_kind.to_string()));
    payload.insert("font_size_pt".to_string(), serde_json::Value::from(font_size_pt));
    payload.insert("leading_em".to_string(), serde_json::Value::from(leading_em));
    payload.insert("first_line_indent_pt".to_string(), serde_json::Value::from(get_render_first_line_indent_pt(item)));
    payload.insert("font_weight".to_string(), serde_json::Value::String(resolve_font_weight(item)));
    payload.insert(
        "page_body_font_size_pt".to_string(),
        if body_like_single_line {
            match metrics.page_body_font_size_pt {
                Some(v) => serde_json::Value::from(v),
                None => serde_json::Value::Null,
            }
        } else {
            serde_json::Value::Null
        },
    );
    payload.insert("is_body".to_string(), serde_json::Value::Bool(body_like_single_line));
    payload.insert("page_box_area_ratio".to_string(), serde_json::Value::from(page_box_area_ratio));
    payload.insert("dense_small_box".to_string(), serde_json::Value::Bool(dense_small_box));
    payload.insert("heavy_dense_small_box".to_string(), serde_json::Value::Bool(heavy_dense_small_box));
    payload.insert("wide_aspect_body_text".to_string(), serde_json::Value::Bool(wide_aspect_body_text));
    payload.insert(
        "prefer_typst_fit".to_string(),
        serde_json::Value::Bool(body_like_single_line && dense_small_box),
    );
    payload.insert("title_fit".to_string(), title_fit_json(&title_fit));
    payload.insert("preserve_line_breaks".to_string(), serde_json::Value::Bool(preserve_line_breaks));
    payload.insert("adjacent_collision_risk".to_string(), serde_json::Value::Bool(false));
    payload.insert("adjacent_available_height_pt".to_string(), serde_json::Value::Null);
    payload.insert("text_color".to_string(), raw_color(raw_item, "_render_text_color", [0, 0, 0]));
    payload.insert("cover_fill".to_string(), raw_color(raw_item, "_render_cover_fill", [1, 1, 1]));
    if let Some(feature) = &memory_feature {
        payload.insert("_typography_memory_key".to_string(), serde_json::Value::String(feature.key.clone()));
    }
    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::block_seed_metrics::collect_page_seed_metrics;
    use serde_json::json;

    fn title_item() -> Item {
        Item {
            block_type: Some("title".into()),
            layout_role: Some("title".into()),
            translated_text: "A Short Heading".into(),
            source_text: "A Short Heading".into(),
            bbox: Some([0.0, 0.0, 300.0, 60.0]),
            lines: vec![crate::item::Line { bbox: Some([0.0, 0.0, 300.0, 20.0]), spans: vec![] }],
            ..Default::default()
        }
    }

    fn raw_title() -> serde_json::Value {
        json!({
            "block_type": "title",
            "layout_role": "title",
            "translated_text": "A Short Heading",
            "source_text": "A Short Heading",
            "bbox": [0.0, 0.0, 300.0, 60.0],
            "lines": [{"bbox": [0.0, 0.0, 300.0, 20.0], "spans": []}]
        })
    }

    #[test]
    fn empty_text_returns_none() {
        let item = Item { bbox: Some([0.0, 0.0, 10.0, 10.0]), ..Default::default() };
        let metrics = collect_page_seed_metrics(&[], None);
        assert!(build_seed_payload_for_item(0, &item, &json!({"bbox": [0.0, 0.0, 10.0, 10.0]}), &metrics, None, None).is_none());
    }

    #[test]
    fn missing_bbox_returns_none() {
        let item = Item { translated_text: "x".into(), ..Default::default() };
        let metrics = collect_page_seed_metrics(&[], None);
        assert!(build_seed_payload_for_item(0, &item, &json!({}), &metrics, None, None).is_none());
    }

    #[test]
    fn title_payload_shape() {
        let items = vec![title_item()];
        let raw_items = vec![raw_title()];
        let metrics = collect_page_seed_metrics(&items, None);
        let payload = build_seed_payload_for_item(0, &items[0], &raw_items[0], &metrics, None, None).unwrap();
        assert_eq!(payload.get("index").unwrap(), &json!(0));
        assert_eq!(payload.get("is_body").unwrap(), &json!(false));
        assert_eq!(payload.get("font_weight").unwrap(), &json!("bold"));
        assert_eq!(payload.get("render_kind").unwrap(), &json!("markdown"));
        assert!(payload.get("title_fit").unwrap().is_object());
        assert!(payload.contains_key("_typography_memory_key"));
        assert_eq!(payload.get("item").unwrap(), &raw_items[0]);
    }
}
