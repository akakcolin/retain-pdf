// Port of services/rendering/layout/payload/body_leading_policy.py — the
// body-leading restore/refit policy that spends remaining vertical slack on
// body line spacing (C3-N3, after font unify). Operates on the raw JSON
// block-payload dicts, mutating in place.

use serde_json::Value;

use crate::layout::body_common::required_lines;
use crate::layout::body_leading_solver::{annotate_body_vertical_budget, solve_body_leading};
use crate::layout::payload_dict::{payload_bool, payload_f64, payload_string};
use crate::layout::typography_decision::{leading_refit_after_font_unify, set_leading_decision, LeadingDecision};
use crate::layout::typography_policy as typography;
use crate::util::{median_f64, py_round};

/// `restore_comfort_body_leading`: spend remaining vertical slack on body
/// leading, recording the vertical-budget annotation and the leading decision.
pub fn restore_comfort_body_leading(body_payloads: &mut Vec<Value>) {
    let baseline = page_body_leading_baseline(body_payloads);
    for payload in body_payloads {
        if !eligible_for_body_leading(payload) {
            continue;
        }
        ensure_multiline_comfort_floor(payload);
        let Some(solution) = solve_body_leading(payload, baseline) else {
            continue;
        };
        if payload_f64(payload, "leading_em", 0.0) >= solution.leading_em {
            continue;
        }
        annotate_body_vertical_budget(payload, &solution);
        payload
            .as_object_mut()
            .expect("payload is an object")
            .insert("leading_em".to_string(), Value::from(solution.leading_em));
        set_leading_decision(
            payload,
            &LeadingDecision::new(solution.leading_em, solution.target_density, solution.leading_cap_em),
        );
    }
}

/// `refit_body_leading_after_font_unify`: identical to the restore pass but
/// records `refit_after_font_unify` on the decision (the font-unify path).
pub fn refit_body_leading_after_font_unify(body_payloads: &mut Vec<Value>) {
    let baseline = page_body_leading_baseline(body_payloads);
    for payload in body_payloads {
        if !eligible_for_body_leading(payload) {
            continue;
        }
        ensure_multiline_comfort_floor(payload);
        let Some(solution) = solve_body_leading(payload, baseline) else {
            continue;
        };
        if payload_f64(payload, "leading_em", 0.0) >= solution.leading_em {
            continue;
        }
        annotate_body_vertical_budget(payload, &solution);
        payload
            .as_object_mut()
            .expect("payload is an object")
            .insert("leading_em".to_string(), Value::from(solution.leading_em));
        set_leading_decision(
            payload,
            &LeadingDecision {
                leading_em: solution.leading_em,
                target_density: solution.target_density,
                leading_cap_em: solution.leading_cap_em,
                refit_after_font_unify: true,
            },
        );
    }
}

fn eligible_for_body_leading(payload: &Value) -> bool {
    if payload_string(payload, "render_kind", "") != "markdown" {
        return false;
    }
    if payload_bool(payload, "dense_small_box")
        || payload_bool(payload, "heavy_dense_small_box")
        || payload_bool(payload, "prefer_typst_fit")
    {
        return false;
    }
    true
}

fn page_body_leading_baseline(body_payloads: &[Value]) -> Option<f64> {
    let mut values = Vec::new();
    for payload in body_payloads {
        if !eligible_for_body_leading(payload) {
            continue;
        }
        let leading = payload_f64(payload, "leading_em", 0.0);
        if leading <= 0.0 || leading_refit_after_font_unify(payload) {
            continue;
        }
        values.push(leading);
    }
    if values.len() < 2 {
        return None;
    }
    Some(py_round(median_f64(&values), 2))
}

fn ensure_multiline_comfort_floor(payload: &mut Value) {
    if required_lines(payload) < 3 {
        return;
    }
    let current = payload_f64(payload, "leading_em", 0.0);
    if current >= typography::BODY_COMFORT_LEADING_MIN {
        return;
    }
    let obj = payload.as_object_mut().expect("payload is an object");
    obj.insert(
        "leading_em".to_string(),
        Value::from(py_round(typography::BODY_COMFORT_LEADING_MIN, 2)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body_payload(leading_em: f64, translated: &str) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 350.0, 120.0],
            "translated_text": translated,
            "formula_map": [],
            "font_size_pt": 12.0,
            "leading_em": leading_em,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": false,
            "heavy_dense_small_box": false,
            "item": {
                "bbox": [50.0, 0.0, 350.0, 120.0],
                "lines": [
                    {"bbox": [50.0, 0.0, 350.0, 20.0], "spans": [{"type": "text", "text": "第一行"}]},
                    {"bbox": [50.0, 24.0, 350.0, 44.0], "spans": [{"type": "text", "text": "第二行"}]},
                    {"bbox": [50.0, 48.0, 350.0, 68.0], "spans": [{"type": "text", "text": "第三行"}]},
                ],
            },
        })
    }

    #[test]
    fn restore_spends_slack_on_short_text() {
        let mut payloads = vec![body_payload(0.44, "短文本")];
        restore_comfort_body_leading(&mut payloads);
        let leading: f64 = payloads[0]["leading_em"].as_f64().unwrap();
        assert!(leading > 0.44);
        assert!(payloads[0].get("_body_leading_decision").is_some());
        assert!(payloads[0]["_body_vertical_budget"]["target_density"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn skips_dense_payload() {
        let mut payload = body_payload(0.44, "短文本");
        payload.as_object_mut().unwrap().insert("dense_small_box".to_string(), json!(true));
        let mut payloads = vec![payload];
        restore_comfort_body_leading(&mut payloads);
        assert_eq!(payloads[0]["leading_em"], json!(0.44));
        assert!(payloads[0].get("_body_leading_decision").is_none());
    }

    #[test]
    fn refit_marks_decision_flag() {
        let mut payloads = vec![body_payload(0.44, "短文本")];
        refit_body_leading_after_font_unify(&mut payloads);
        assert_eq!(payloads[0]["_body_leading_decision"]["refit_after_font_unify"], json!(true));
    }

    #[test]
    fn baseline_requires_two_values() {
        assert!(page_body_leading_baseline(&[body_payload(0.44, "短文本")]).is_none());
        let two = vec![body_payload(0.44, "短文本"), body_payload(0.5, "另一段")];
        let baseline = page_body_leading_baseline(&two).unwrap();
        assert!(baseline > 0.44);
    }

    #[test]
    fn multiline_comfort_floor_applied() {
        // Estimated 4 lines with a low leading: floor lifts to BODY_COMFORT_LEADING_MIN.
        const MULTILINE: &str =
            "这是一段足够长的中文段落文字内容，它需要能够在给定的宽度之内被排版成多行文字才能满足段落的自然阅读习惯。这里再补充一部分内容让文本继续变长，直到它能够稳定的产生三行以上的估计行数。";
        let mut payloads = vec![body_payload(0.44, MULTILINE)];
        ensure_multiline_comfort_floor(&mut payloads[0]);
        assert_eq!(payloads[0]["leading_em"], json!(0.56));
    }
}
