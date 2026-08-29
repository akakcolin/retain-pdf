// Port of services/rendering/layout/payload/body_font_dense_policy.py — the
// dense-block font tightening/force-fit policy (C3-N3). Operates on the raw
// JSON block-payload dicts, mutating in place.

use serde_json::Value;

use crate::item::formula_map;
use crate::layout::body_common::payload_density;
use crate::layout::payload_dict::{inner_bbox4, payload_bool, payload_f64, payload_string};
use crate::payload::capacity::text_demand_units;
use crate::util::py_round;

pub const BODY_PRESSURE_TIGHTEN_TRIGGER: f64 = 1.38;
pub const BODY_PRESSURE_TIGHTEN_TRIGGER_HIGH: f64 = 1.30;
pub const BODY_FINAL_FORCE_FIT_DENSITY: f64 = 1.12;
pub const BODY_FONT_SMOOTH_BAND_PT: f64 = 0.16;
pub const BODY_DENSE_FONT_MAX_PT: f64 = 10.35;
pub const BODY_HEAVY_DENSE_FONT_MAX_PT: f64 = 10.2;

/// `tighten_body_payloads`: clamp each body font toward the page median and
/// shrink high-pressure dense blocks that still overflow the density target.
pub fn tighten_body_payloads(
    body_payloads: &mut Vec<Value>,
    body_font_median: f64,
    body_density_target: f64,
    body_pressure_median: f64,
) {
    for payload in body_payloads {
        let mut smooth_floor = payload_f64(payload, "font_size_pt", 0.0);
        let mut smooth_cap = body_font_median + BODY_FONT_SMOOTH_BAND_PT;
        if payload_bool(payload, "heavy_dense_small_box") {
            smooth_cap = smooth_cap.min(BODY_HEAVY_DENSE_FONT_MAX_PT);
        } else if payload_bool(payload, "dense_small_box") {
            smooth_cap = smooth_cap.min(BODY_DENSE_FONT_MAX_PT);
        }
        if smooth_cap < smooth_floor {
            smooth_floor = smooth_cap;
        }
        let smooth = py_round(smooth_floor.max(payload_f64(payload, "font_size_pt", 0.0)).min(smooth_cap), 2);
        let obj = payload.as_object_mut().expect("payload is an object");
        obj.insert("font_size_pt".to_string(), Value::from(smooth));

        let bbox = inner_bbox4(payload).unwrap_or([0.0; 4]);
        let inner_height = (bbox[3] - bbox[1]).max(8.0);
        let inner_width = (bbox[2] - bbox[0]).max(8.0);
        let demand = text_demand_units(&payload_string(payload, "translated_text", ""), &formula_map(payload.get("formula_map")));
        let pressure = demand / (inner_width * inner_height).max(1.0);
        let pressure_ratio = if body_pressure_median > 0.0 {
            pressure / body_pressure_median.max(1e-6)
        } else {
            1.0
        };
        let mut density = payload_density(payload, None, None);
        let pressure_trigger = if density > body_density_target + 0.03 {
            BODY_PRESSURE_TIGHTEN_TRIGGER_HIGH
        } else {
            BODY_PRESSURE_TIGHTEN_TRIGGER
        };

        if pressure_ratio > pressure_trigger
            && payload_bool(payload, "dense_small_box")
            && density > body_density_target + 0.04
        {
            let steps = ((pressure_ratio - pressure_trigger) / 0.26).ceil().clamp(1.0, 3.0);
            let font = payload_f64(payload, "font_size_pt", 0.0);
            let leading = payload_f64(payload, "leading_em", 0.0);
            let shrink_floor = font.min(body_font_median - 0.34);
            let obj = payload.as_object_mut().expect("payload is an object");
            obj.insert("font_size_pt".to_string(), Value::from(py_round(shrink_floor.max(font - steps * 0.08), 2)));
            obj.insert("leading_em".to_string(), Value::from(py_round(0.52_f64.max(leading - 0.01 * steps.min(2.0)), 2)));
            obj.insert("prefer_typst_fit".to_string(), Value::Bool(true));
            density = payload_density(payload, None, None);
        }

        if density > body_density_target + 0.06
            && payload_bool(payload, "dense_small_box")
            && pressure_ratio > BODY_PRESSURE_TIGHTEN_TRIGGER_HIGH
        {
            payload
                .as_object_mut()
                .expect("payload is an object")
                .insert("prefer_typst_fit".to_string(), Value::Bool(true));
        } else if !payload_bool(payload, "dense_small_box")
            && !payload_bool(payload, "heavy_dense_small_box")
            && density < body_density_target - 0.12
            && pressure_ratio < 0.94
        {
            let steps = ((body_density_target - density) / 0.12).ceil().clamp(1.0, 2.0);
            let font = payload_f64(payload, "font_size_pt", 0.0);
            let grown = (body_font_median + 0.08).min(font + steps * 0.04);
            let obj = payload.as_object_mut().expect("payload is an object");
            obj.insert("font_size_pt".to_string(), Value::from(py_round(grown, 2)));
        }
    }
}

/// `mark_force_fit_dense_outliers`: force-fit heavy dense blocks whose density
/// still exceeds the final force-fit threshold.
pub fn mark_force_fit_dense_outliers(body_payloads: &mut Vec<Value>) {
    for payload in body_payloads {
        if payload_bool(payload, "heavy_dense_small_box") && payload_density(payload, None, None) > BODY_FINAL_FORCE_FIT_DENSITY {
            payload
                .as_object_mut()
                .expect("payload is an object")
                .insert("prefer_typst_fit".to_string(), Value::Bool(true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(font: f64, dense: bool, heavy: bool) -> Value {
        json!({
            "inner_bbox": [50.0, 0.0, 250.0, 120.0],
            "translated_text": "a",
            "formula_map": [],
            "font_size_pt": font,
            "leading_em": 0.5,
            "render_kind": "markdown",
            "is_body": true,
            "dense_small_box": dense,
            "heavy_dense_small_box": heavy,
            "prefer_typst_fit": false,
            "item": {},
        })
    }

    #[test]
    fn clamps_font_toward_median_band() {
        // Font above the median band gets pulled down to the band cap (12.0 + 0.16).
        let mut payloads = vec![payload(13.0, false, false)];
        tighten_body_payloads(&mut payloads, 12.0, 0.9, 1.0);
        let font: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        assert!((font - 12.0).abs() <= BODY_FONT_SMOOTH_BAND_PT);
    }

    #[test]
    fn heavy_dense_caps_band() {
        let mut payloads = vec![payload(12.0, true, false)];
        tighten_body_payloads(&mut payloads, 13.0, 0.9, 1.0);
        let font: f64 = payloads[0]["font_size_pt"].as_f64().unwrap();
        assert!(font <= BODY_DENSE_FONT_MAX_PT);
    }

    #[test]
    fn force_fit_marks_heavy_dense_outlier() {
        // Heavy dense block with long text in a short box overflows force-fit density.
        let mut p = payload(12.0, false, true);
        p.as_object_mut().unwrap().insert("translated_text".to_string(), json!(DENSE_TEXT));
        p.as_object_mut().unwrap().insert("inner_bbox".to_string(), json!([50.0, 0.0, 250.0, 40.0]));
        let mut payloads = vec![p];
        mark_force_fit_dense_outliers(&mut payloads);
        assert!(payloads[0].get("prefer_typst_fit").map(|v| v.as_bool().unwrap_or(false)).unwrap_or(false));
    }

    const DENSE_TEXT: &str = "这是一段很长的中文文字内容，它被用作正文的密集排版测试用例。我们需要足够多的字符来填充这个版面，让文本能够稳定地在给定的宽度内折行并占据足够的垂直空间，从而模拟真实的正文块在页面上的表现。";
}
