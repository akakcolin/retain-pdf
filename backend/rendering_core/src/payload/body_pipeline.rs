// Port of services/rendering/layout/payload/body_pipeline.py — the C3-N3 body
// payload pipeline orchestrator. `ordered_payloads` is the real payload vec;
// `body_payloads` holds clones of the body entries so leaf policies mutate them
// in place (mirroring Python's shared-dict references). `body_indices` maps each
// clone index back to its position in `ordered_payloads`.

use serde_json::Value;

use crate::layout::body_context::resolve_body_targets;
use crate::layout::line_structure::fit_preserved_line_block_metrics;
use crate::layout::payload_dict::{payload_bool, payload_f64, payload_string};
use crate::payload::body_policy_facade as body_policy;
use crate::util::median_f64;

/// Stages that mutate `body_payloads` (the clones) directly.
#[derive(Clone, Copy)]
enum CloneStage {
    Tighten,
    MarkForceFit,
    GrowUnderfilled,
    RecoverDensity,
    RestoreComfortLeading,
    HarmonizeLong,
    SmoothAdjacent,
    RefitLeadingAfterUnify,
}

/// Stages that read `body_payloads` as anchors and mutate `ordered_payloads`.
#[derive(Clone, Copy)]
enum OrderedStage {
    InheritShort,
    UnifySimilar,
    InheritLowHeight,
    RelaxShort,
    HarmonizeUnderfilled,
    ApplyPageAnchor,
}

#[derive(Clone, Copy)]
enum StageAny {
    Clone(CloneStage),
    Ordered(OrderedStage),
}

fn sync_clones_to_ordered(body_payloads: &[Value], ordered_payloads: &mut Vec<Value>, body_indices: &[usize]) {
    for (k, &idx) in body_indices.iter().enumerate() {
        ordered_payloads[idx] = body_payloads[k].clone();
    }
}

fn sync_ordered_to_clones(body_payloads: &mut Vec<Value>, ordered_payloads: &[Value], body_indices: &[usize]) {
    for (k, &idx) in body_indices.iter().enumerate() {
        body_payloads[k] = ordered_payloads[idx].clone();
    }
}

fn run_clone_stage(
    stage: CloneStage,
    body_payloads: &mut Vec<Value>,
    body_font_median: f64,
    body_density_target: f64,
    body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
) {
    match stage {
        CloneStage::Tighten => {
            body_policy::tighten_body_payloads(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        CloneStage::MarkForceFit => {
            body_policy::mark_force_fit_dense_outliers(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        CloneStage::GrowUnderfilled => {
            body_policy::grow_underfilled_body_payloads(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        CloneStage::RecoverDensity => {
            body_policy::recover_underfilled_body_density(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        CloneStage::RestoreComfortLeading => {
            body_policy::restore_comfort_body_leading(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        CloneStage::HarmonizeLong => {
            body_policy::harmonize_long_body_payloads(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        CloneStage::SmoothAdjacent => {
            body_policy::smooth_adjacent_body_payloads(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        CloneStage::RefitLeadingAfterUnify => {
            body_policy::refit_body_leading_after_font_unify(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
    }
}

fn run_ordered_stage(
    stage: OrderedStage,
    body_payloads: &mut Vec<Value>,
    body_font_median: f64,
    body_density_target: f64,
    body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
    book_body_font_target: Option<f64>,
) {
    match stage {
        OrderedStage::InheritShort => {
            body_policy::inherit_short_body_fonts(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        OrderedStage::UnifySimilar => {
            body_policy::unify_similar_body_fonts(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
                book_body_font_target,
            );
        }
        OrderedStage::InheritLowHeight => {
            body_policy::inherit_low_height_body_fonts(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        OrderedStage::RelaxShort => {
            body_policy::relax_short_body_context_heights(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        OrderedStage::HarmonizeUnderfilled => {
            body_policy::harmonize_underfilled_body_fonts(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
        OrderedStage::ApplyPageAnchor => {
            body_policy::apply_page_body_font_anchor(
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
        }
    }
}

fn run_stage_any(
    stage: StageAny,
    body_payloads: &mut Vec<Value>,
    body_indices: &[usize],
    body_font_median: f64,
    body_density_target: f64,
    body_pressure_median: f64,
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
    book_body_font_target: Option<f64>,
) {
    match stage {
        StageAny::Clone(c) => {
            run_clone_stage(
                c,
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
            );
            sync_clones_to_ordered(body_payloads, ordered_payloads, body_indices);
        }
        StageAny::Ordered(o) => {
            run_ordered_stage(
                o,
                body_payloads,
                body_font_median,
                body_density_target,
                body_pressure_median,
                ordered_payloads,
                page_text_width_med,
                book_body_font_target,
            );
            sync_ordered_to_clones(body_payloads, ordered_payloads, body_indices);
        }
    }
}

fn refit_preserved_line_payloads(body_payloads: &mut Vec<Value>) {
    for payload in body_payloads {
        if !payload_bool(payload, "preserve_line_breaks") {
            continue;
        }
        // Python `or` is truthiness-based: an empty inner_bbox falls through to
        // bbox, and an empty bbox falls through to `[]`.
        let inner = payload
            .get("inner_bbox")
            .and_then(|v| v.as_array())
            .and_then(|a| {
                if a.is_empty() {
                    None
                } else {
                    Some(a.iter().filter_map(|v| v.as_f64()).collect::<Vec<f64>>())
                }
            })
            .or_else(|| {
                payload
                    .get("bbox")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_f64()).collect::<Vec<f64>>())
            })
            .unwrap_or_default();
        let translated = payload_string(payload, "translated_text", "");
        let current_font = payload_f64(payload, "font_size_pt", 0.0);
        let current_leading = payload_f64(payload, "leading_em", 0.0);
        let (font, leading) = fit_preserved_line_block_metrics(&inner, &translated, current_font, current_leading);
        let obj = payload.as_object_mut().expect("payload is an object");
        obj.insert("font_size_pt".to_string(), Value::from(font));
        obj.insert("leading_em".to_string(), Value::from(leading));
    }
}

/// `apply_body_payload_pipeline`: run the body font/leading policy stages over
/// the body entries of `ordered_payloads`, in place.
pub fn apply_body_payload_pipeline(
    ordered_payloads: &mut Vec<Value>,
    page_text_width_med: f64,
    book_body_font_target: Option<f64>,
    font_unify_mode: &str,
) {
    let body_indices: Vec<usize> = ordered_payloads
        .iter()
        .enumerate()
        .filter(|(_, p)| payload_bool(p, "is_body"))
        .map(|(idx, _)| idx)
        .collect();
    if body_indices.is_empty() {
        return;
    }
    let mut body_payloads: Vec<Value> = body_indices.iter().map(|&idx| ordered_payloads[idx].clone()).collect();

    let (mut body_font_median, body_density_target, body_pressure_median) = resolve_body_targets(&mut body_payloads);
    sync_clones_to_ordered(&body_payloads, ordered_payloads, &body_indices);

    for stage in [StageAny::Clone(CloneStage::Tighten), StageAny::Clone(CloneStage::MarkForceFit)] {
        run_stage_any(
            stage,
            &mut body_payloads,
            &body_indices,
            body_font_median,
            body_density_target,
            body_pressure_median,
            ordered_payloads,
            page_text_width_med,
            book_body_font_target,
        );
    }

    body_font_median = median_f64(&body_payloads.iter().map(|p| payload_f64(p, "font_size_pt", 0.0)).collect::<Vec<_>>());

    let stages: Vec<StageAny> = if font_unify_mode == "off" {
        vec![
            StageAny::Ordered(OrderedStage::InheritShort),
            StageAny::Ordered(OrderedStage::InheritLowHeight),
            StageAny::Ordered(OrderedStage::RelaxShort),
            StageAny::Clone(CloneStage::GrowUnderfilled),
            StageAny::Ordered(OrderedStage::HarmonizeUnderfilled),
            StageAny::Clone(CloneStage::RecoverDensity),
            StageAny::Ordered(OrderedStage::ApplyPageAnchor),
            StageAny::Clone(CloneStage::RestoreComfortLeading),
            StageAny::Clone(CloneStage::HarmonizeLong),
            StageAny::Clone(CloneStage::SmoothAdjacent),
            StageAny::Clone(CloneStage::RefitLeadingAfterUnify),
        ]
    } else {
        vec![
            StageAny::Ordered(OrderedStage::InheritShort),
            StageAny::Ordered(OrderedStage::UnifySimilar),
            StageAny::Ordered(OrderedStage::InheritLowHeight),
            StageAny::Ordered(OrderedStage::RelaxShort),
            StageAny::Clone(CloneStage::GrowUnderfilled),
            StageAny::Ordered(OrderedStage::HarmonizeUnderfilled),
            StageAny::Clone(CloneStage::RecoverDensity),
            StageAny::Ordered(OrderedStage::ApplyPageAnchor),
            StageAny::Clone(CloneStage::RestoreComfortLeading),
            StageAny::Clone(CloneStage::HarmonizeLong),
            StageAny::Clone(CloneStage::SmoothAdjacent),
            StageAny::Ordered(OrderedStage::UnifySimilar),
            StageAny::Clone(CloneStage::RecoverDensity),
            StageAny::Clone(CloneStage::RefitLeadingAfterUnify),
        ]
    };
    for stage in stages {
        run_stage_any(
            stage,
            &mut body_payloads,
            &body_indices,
            body_font_median,
            body_density_target,
            body_pressure_median,
            ordered_payloads,
            page_text_width_med,
            book_body_font_target,
        );
    }

    refit_preserved_line_payloads(&mut body_payloads);
    sync_clones_to_ordered(&body_payloads, ordered_payloads, &body_indices);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEXT_A: &str = "这是正文段落一，包含足够多的文字用于测试正文管道各阶段的平滑与统一效果，避免被过滤跳过。";
    const TEXT_B: &str = "这是正文段落二，其字号和行距与相邻段落略有差异以便观察相邻平滑的效果变化。";
    const TEXT_C: &str = "这是较短的第三段正文，用于观察短段落继承与恢复密度的行为是否符合预期。";
    const TEXT_D: &str = "这是正文段落四，承载足够多文字以确保满足锚点候选的宽度与行数门槛要求。";

    fn body_payload(x0: f64, y0: f64, x1: f64, y1: f64, font: f64, leading: f64, text: &str, lines: usize) -> Value {
        let item_lines: Vec<Value> = (0..lines)
            .map(|i| {
                json!({
                    "bbox": [x0, y0 + i as f64 * (y1 - y0) / lines as f64, x1, y0 + (i + 1) as f64 * (y1 - y0) / lines as f64],
                    "spans": [{"type": "text", "text": text}],
                })
            })
            .collect();
        json!({
            "inner_bbox": [x0, y0, x1, y1],
            "translated_text": text,
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": leading,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "prefer_typst_fit": false,
            "item": {"source_text": text, "lines": item_lines},
        })
    }

    fn title_payload(x0: f64, y0: f64, x1: f64, y1: f64, font: f64) -> Value {
        json!({
            "inner_bbox": [x0, y0, x1, y1],
            "translated_text": "章节标题",
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": 0.3,
            "render_kind": "markdown",
            "is_body": false,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "prefer_typst_fit": false,
            "item": {},
        })
    }

    fn fixture() -> Vec<Value> {
        vec![
            title_payload(50.0, 0.0, 250.0, 30.0, 18.0),
            body_payload(50.0, 40.0, 250.0, 140.0, 12.0, 0.44, TEXT_A, 4),
            body_payload(50.0, 141.0, 250.0, 241.0, 13.0, 0.58, TEXT_B, 4),
            body_payload(50.0, 242.0, 250.0, 300.0, 12.5, 0.5, TEXT_C, 2),
            body_payload(50.0, 301.0, 250.0, 401.0, 11.0, 0.4, TEXT_D, 4),
        ]
    }

    fn f(payload: &Value, key: &str) -> f64 {
        payload.get(key).and_then(|v| v.as_f64()).unwrap_or(-1.0)
    }

    fn b(payload: &Value, key: &str) -> bool {
        payload.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
    }

    fn growth_target(payload: &Value) -> Option<f64> {
        payload
            .get("_body_font_growth_decision")
            .and_then(|v| v.as_object())
            .and_then(|d| d.get("target_font_pt"))
            .and_then(|v| v.as_f64())
    }

    #[test]
    fn pipeline_matches_python_ground_truth() {
        let mut payloads = fixture();
        apply_body_payload_pipeline(&mut payloads, 200.0, None, "role_min");

        // Unify converges every font to 12.0 (title included, as in Python).
        for payload in &payloads {
            assert_eq!(f(payload, "font_size_pt"), 12.0, "font converged");
            assert_eq!(f(payload, "page_body_font_size_pt"), 12.0, "page anchor annotated");
            assert!(b(payload, "_body_font_unified"), "unify flag set");
        }

        // Title untouched for leading, but annotated.
        assert_eq!(f(&payloads[0], "leading_em"), 0.3);
        assert!(growth_target(&payloads[0]).is_none());

        // Body leading + growth decisions match the Python ground truth.
        assert_eq!(f(&payloads[1], "leading_em"), 0.68);
        assert_eq!(growth_target(&payloads[1]), Some(12.23));
        assert_eq!(f(&payloads[2], "leading_em"), 0.7);
        assert_eq!(growth_target(&payloads[2]), Some(12.09));
        assert_eq!(f(&payloads[3], "leading_em"), 0.57);
        assert!(growth_target(&payloads[3]).is_none());
        assert_eq!(f(&payloads[4], "leading_em"), 0.7);
        assert_eq!(growth_target(&payloads[4]), Some(12.64));
    }

    #[test]
    fn empty_body_pipeline_is_noop() {
        let mut payloads = vec![title_payload(50.0, 0.0, 250.0, 30.0, 18.0)];
        let before = payloads[0].clone();
        apply_body_payload_pipeline(&mut payloads, 200.0, None, "off");
        assert_eq!(payloads[0], before);
    }

    #[test]
    fn off_mode_matches_python_ground_truth() {
        let mut payloads = fixture();
        apply_body_payload_pipeline(&mut payloads, 200.0, None, "off");

        // Off mode never sets the unify marker (title included).
        for payload in &payloads {
            assert!(!b(payload, "_body_font_unified"), "off mode skips unify");
        }

        // Fonts/leads/growth decisions match the Python `FONT_UNIFY_MODE=off`
        // ground truth for this fixture (title flows through ordered stages).
        assert_eq!(f(&payloads[0], "font_size_pt"), 13.91);
        assert_eq!(f(&payloads[0], "leading_em"), 0.3);
        assert!(growth_target(&payloads[0]).is_none());
        assert_eq!(f(&payloads[1], "font_size_pt"), 13.91);
        assert_eq!(f(&payloads[1], "leading_em"), 0.68);
        assert_eq!(growth_target(&payloads[1]), Some(12.23));
        assert_eq!(f(&payloads[2], "font_size_pt"), 12.81);
        assert_eq!(f(&payloads[2], "leading_em"), 0.7);
        assert_eq!(growth_target(&payloads[2]), Some(12.38));
        assert_eq!(f(&payloads[3], "font_size_pt"), 12.76);
        assert_eq!(f(&payloads[3], "leading_em"), 0.56);
        assert!(growth_target(&payloads[3]).is_none());
        assert_eq!(f(&payloads[4], "font_size_pt"), 13.1);
        assert_eq!(f(&payloads[4], "leading_em"), 0.59);
        assert!(growth_target(&payloads[4]).is_none());
    }
}
