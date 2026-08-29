// Port of services/rendering/policy/typography_decision.py — the decision DTOs
// the body-pipeline (C3-N3) stages write onto payload dicts and re-read later.
// Works on the raw JSON payload dict (the typed `Item` does not carry
// `_body_*` decision keys).

use serde_json::{json, Value};

use crate::util::py_round;

pub const FONT_GROWTH_DECISION_KEY: &str = "_body_font_growth_decision";
pub const LEADING_DECISION_KEY: &str = "_body_leading_decision";
pub const PAGE_ANCHOR_DECISION_KEY: &str = "_page_body_anchor_decision";
pub const VERTICAL_BUDGET_KEY: &str = "_body_vertical_budget";

#[derive(Debug, Clone)]
pub struct FontGrowthDecision {
    pub seed_font_pt: f64,
    pub target_font_pt: f64,
    pub slack_ratio: f64,
    pub reason: String,
}

impl FontGrowthDecision {
    pub fn new(seed_font_pt: f64, target_font_pt: f64, slack_ratio: f64) -> Self {
        Self {
            seed_font_pt,
            target_font_pt,
            slack_ratio,
            reason: "underfilled_body".to_string(),
        }
    }

    pub fn grew_pt(&self) -> f64 {
        (self.target_font_pt - self.seed_font_pt).max(0.0)
    }

    pub fn to_payload(&self) -> Value {
        json!({
            "seed_font_pt": py_round(self.seed_font_pt, 3),
            "target_font_pt": py_round(self.target_font_pt, 3),
            "grew_pt": py_round(self.grew_pt(), 3),
            "slack_ratio": py_round(self.slack_ratio, 3),
            "reason": self.reason,
        })
    }
}

#[derive(Debug, Clone)]
pub struct LeadingDecision {
    pub leading_em: f64,
    pub target_density: f64,
    pub leading_cap_em: f64,
    pub refit_after_font_unify: bool,
}

impl LeadingDecision {
    pub fn new(leading_em: f64, target_density: f64, leading_cap_em: f64) -> Self {
        Self {
            leading_em,
            target_density,
            leading_cap_em,
            refit_after_font_unify: false,
        }
    }

    pub fn to_payload(&self) -> Value {
        json!({
            "leading_em": py_round(self.leading_em, 3),
            "target_density": py_round(self.target_density, 3),
            "leading_cap_em": py_round(self.leading_cap_em, 3),
            "refit_after_font_unify": self.refit_after_font_unify,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PageBodyAnchorDecision {
    pub target_font_pt: f64,
    pub applied: bool,
}

impl PageBodyAnchorDecision {
    pub fn to_payload(&self) -> Value {
        json!({
            "target_font_pt": py_round(self.target_font_pt, 3),
            "applied": self.applied,
        })
    }
}

#[derive(Debug, Clone)]
pub struct VerticalBudget {
    pub font_growth_pt: f64,
    pub leading_growth_em: f64,
    pub target_density: f64,
    pub leading_cap_em: f64,
}

impl VerticalBudget {
    pub fn to_payload(&self) -> Value {
        json!({
            "font_growth_pt": py_round(self.font_growth_pt.max(0.0), 3),
            "leading_growth_em": py_round(self.leading_growth_em.max(0.0), 3),
            "target_density": py_round(self.target_density, 3),
            "leading_cap_em": py_round(self.leading_cap_em, 3),
        })
    }
}

fn payload_f64(payload: &Value, key: &str, default: f64) -> f64 {
    payload.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
}

pub fn set_font_growth_decision(payload: &mut Value, decision: &FontGrowthDecision) {
    let as_obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    as_obj.insert(FONT_GROWTH_DECISION_KEY.to_string(), decision.to_payload());
    as_obj.insert(
        "_body_underfill_seed_font_pt".to_string(),
        json!(py_round(decision.seed_font_pt, 2)),
    );
    as_obj.insert(
        "_body_underfill_font_grew_pt".to_string(),
        json!(py_round(decision.grew_pt(), 2)),
    );
    as_obj.insert(
        "_body_underfill_font_slack_ratio".to_string(),
        json!(py_round(decision.slack_ratio, 3)),
    );
}

fn decision_dict<'a>(payload: &'a Value, key: &str) -> Option<&'a serde_json::Map<String, Value>> {
    payload.get(key).and_then(|v| v.as_object())
}

pub fn font_growth_grew_pt(payload: &Value) -> f64 {
    if let Some(decision) = decision_dict(payload, FONT_GROWTH_DECISION_KEY) {
        return decision.get("grew_pt").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }
    payload_f64(payload, "_body_underfill_font_grew_pt", 0.0)
}

pub fn font_growth_seed_font_pt(payload: &Value, fallback: f64) -> f64 {
    if let Some(decision) = decision_dict(payload, FONT_GROWTH_DECISION_KEY) {
        return decision
            .get("seed_font_pt")
            .and_then(|v| v.as_f64())
            .filter(|v| *v != 0.0)
            .unwrap_or(fallback);
    }
    let direct = payload_f64(payload, "_body_underfill_seed_font_pt", 0.0);
    if direct == 0.0 {
        fallback
    } else {
        direct
    }
}

pub fn font_growth_slack_ratio(payload: &Value) -> f64 {
    if let Some(decision) = decision_dict(payload, FONT_GROWTH_DECISION_KEY) {
        return decision.get("slack_ratio").and_then(|v| v.as_f64()).unwrap_or(0.0);
    }
    payload_f64(payload, "_body_underfill_font_slack_ratio", 0.0)
}

pub fn set_leading_decision(payload: &mut Value, decision: &LeadingDecision) {
    let as_obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    as_obj.insert(LEADING_DECISION_KEY.to_string(), decision.to_payload());
    as_obj.insert(
        "_body_dynamic_leading_cap_em".to_string(),
        json!(py_round(decision.leading_cap_em, 2)),
    );
    as_obj.insert(
        "_body_leading_target_density".to_string(),
        json!(py_round(decision.target_density, 3)),
    );
    if decision.refit_after_font_unify {
        as_obj.insert("_body_leading_refit_after_font_unify".to_string(), json!(true));
    }
}

pub fn leading_refit_after_font_unify(payload: &Value) -> bool {
    if let Some(decision) = decision_dict(payload, LEADING_DECISION_KEY) {
        if decision.get("refit_after_font_unify").and_then(|v| v.as_bool()).unwrap_or(false) {
            return true;
        }
    }
    payload
        .get("_body_leading_refit_after_font_unify")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn set_page_body_anchor_decision(payload: &mut Value, decision: &PageBodyAnchorDecision) {
    let as_obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    as_obj.insert(PAGE_ANCHOR_DECISION_KEY.to_string(), decision.to_payload());
    as_obj.insert(
        "_page_body_anchor_font_pt".to_string(),
        json!(py_round(decision.target_font_pt, 2)),
    );
    if decision.applied {
        as_obj.insert("_page_body_anchor_font_applied".to_string(), json!(true));
    }
}

pub fn set_vertical_budget(payload: &mut Value, budget: &VerticalBudget) {
    let as_obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    as_obj.insert(VERTICAL_BUDGET_KEY.to_string(), budget.to_payload());
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn leading_decision_round_trips() {
        let mut payload = json!({"font_size_pt": 12.0});
        set_leading_decision(
            &mut payload,
            &LeadingDecision {
                leading_em: 0.34,
                target_density: 0.8,
                leading_cap_em: 0.66,
                refit_after_font_unify: true,
            },
        );
        assert!(leading_refit_after_font_unify(&payload));
        assert_eq!(payload["_body_dynamic_leading_cap_em"], json!(0.66));
        assert_eq!(payload["_body_leading_target_density"], json!(0.8));
        assert!(!leading_refit_after_font_unify(&json!({})));
    }

    #[test]
    fn font_growth_reads_written_values() {
        let mut payload = json!({});
        set_font_growth_decision(&mut payload, &FontGrowthDecision::new(11.0, 12.0, 0.5));
        assert_eq!(font_growth_grew_pt(&payload), 1.0);
        assert_eq!(font_growth_seed_font_pt(&payload, 99.0), 11.0);
        assert_eq!(font_growth_slack_ratio(&payload), 0.5);
    }
}
