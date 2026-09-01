// Differential (golden replay) tests: the C3-N3 body policy pipeline's
// observable contract — per-block final font_size/leading plus the typography
// decision keys. Snapshots the pipeline BEFORE the Phase B TypographyCapacity
// reparameterization so behavior changes surface as corpus drift instead of
// silent numeric churn.
//
// Regenerate the golden corpus (after an intentional, reviewed behavior change):
//   cargo test -p rendering_core --test differential_policy regen_policy_corpus \
//     -- --ignored --nocapture > tests/layout_policy_corpus.json

mod common;

use common::*;
use rendering_core::payload::body_pipeline::apply_body_payload_pipeline;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;

const FONT_GROWTH: &str = "_body_font_growth_decision";
const LEADING: &str = "_body_leading_decision";
const PAGE_ANCHOR: &str = "_page_body_anchor_decision";

// --- corpus loading ----------------------------------------------------------

#[derive(Deserialize)]
struct PolicyCase {
    input: Value,
    expected: Value,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct PolicyCorpus {
    schema: String,
    seed: u64,
    cases: Vec<PolicyCase>,
}

fn policy_corpus() -> &'static PolicyCorpus {
    static C: OnceLock<PolicyCorpus> = OnceLock::new();
    C.get_or_init(|| {
        let raw = include_str!("layout_policy_corpus.json");
        serde_json::from_str(raw).expect("failed to parse layout_policy_corpus.json")
    })
}

// --- observable contract -----------------------------------------------------

fn extract_contract(payload: &Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("font_size_pt".to_string(), payload.get("font_size_pt").cloned().unwrap_or(Value::Null));
    out.insert("leading_em".to_string(), payload.get("leading_em").cloned().unwrap_or(Value::Null));
    for key in ["page_body_font_size_pt", "_body_font_unified", FONT_GROWTH, LEADING, PAGE_ANCHOR] {
        if let Some(v) = payload.get(key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    Value::Object(out)
}

fn run_pipeline(input: &Value) -> Vec<Value> {
    let mut payloads: Vec<Value> = from_value(&input["ordered_payloads"]);
    let page_text_width_med: f64 = from_value(&input["page_text_width_med"]);
    let book_target: Option<f64> = from_value(&input["book_body_font_target"]);
    let mode: String = from_value(&input["font_unify_mode"]);
    apply_body_payload_pipeline(&mut payloads, page_text_width_med, book_target, &mode);
    payloads
}

fn assert_contract_matches(case_idx: usize, actual: &[Value], expected_per_block: &[Value]) {
    assert_eq!(
        actual.len(),
        expected_per_block.len(),
        "case {case_idx}: block count mismatch"
    );
    for (b, (actual_block, expected_block)) in actual.iter().zip(expected_per_block.iter()).enumerate() {
        let contract = extract_contract(actual_block);
        let exp_font = expected_block.get("font_size_pt").and_then(|v| v.as_f64());
        let act_font = contract.get("font_size_pt").and_then(|v| v.as_f64());
        if let (Some(a), Some(e)) = (act_font, exp_font) {
            assert_close_f64(a, e);
        }
        let exp_leading = expected_block.get("leading_em").and_then(|v| v.as_f64());
        let act_leading = contract.get("leading_em").and_then(|v| v.as_f64());
        if let (Some(a), Some(e)) = (act_leading, exp_leading) {
            assert_close_f64(a, e);
        }
        for key in ["page_body_font_size_pt", "_body_font_unified", FONT_GROWTH, LEADING, PAGE_ANCHOR] {
            assert_eq!(
                contract.get(key),
                expected_block.get(key),
                "case {case_idx} block {b} key {key}"
            );
        }
    }
}

#[test]
fn policy_pipeline_contract_replay() {
    for (i, case) in policy_corpus().cases.iter().enumerate() {
        let expected_per_block: Vec<Value> = from_value(&case.expected["per_block"]);
        let actual = run_pipeline(&case.input);
        assert_contract_matches(i, &actual, &expected_per_block);
    }
}

// --- corpus fixtures (shared with the regen test) -----------------------------

const TEXT_A: &str =
    "这是正文段落一，包含足够多的文字用于测试正文管道各阶段的平滑与统一效果，避免被过滤跳过。";
const TEXT_B: &str = "这是正文段落二，其字号和行距与相邻段落略有差异以便观察相邻平滑的效果变化。";
const TEXT_C: &str = "这是较短的第三段正文，用于观察短段落继承与恢复密度的行为是否符合预期。";
const TEXT_D: &str = "这是正文段落四，承载足够多文字以确保满足锚点候选的宽度与行数门槛要求。";
const DENSE_TEXT: &str =
    "这一段落在非常狭小的框内塞入了大量密密麻麻的中文文字内容用于触发密集小框的收紧与强制拟合路径执行确保密度压力被正确感知。";
const PRESERVED_TEXT: &str = "第一行文字内容\n第二行独立文字\n第三行收尾文字内容";

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

fn with_flag(mut v: Value, key: &str, value: bool) -> Value {
    v.as_object_mut().expect("payload is an object").insert(key.to_string(), json!(value));
    v
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

fn annotation_payload(x0: f64, y0: f64, x1: f64, y1: f64, font: f64, role: &str) -> Value {
    json!({
        "inner_bbox": [x0, y0, x1, y1],
        "translated_text": "图注或脚注文字示例",
        "formula_map": [],
        "font_size_pt": font,
        "leading_em": 0.35,
        "render_kind": "markdown",
        "is_body": false,
        "dense_small_box": false,
        "heavy_dense_small_box": false,
        "prefer_typst_fit": false,
        "item": {"layout_role": role},
    })
}

fn single_column_fixture() -> Vec<Value> {
    vec![
        title_payload(50.0, 0.0, 250.0, 30.0, 18.0),
        body_payload(50.0, 40.0, 250.0, 140.0, 12.0, 0.44, TEXT_A, 4),
        body_payload(50.0, 141.0, 250.0, 241.0, 13.0, 0.58, TEXT_B, 4),
        body_payload(50.0, 242.0, 250.0, 300.0, 12.5, 0.5, TEXT_C, 2),
        body_payload(50.0, 301.0, 250.0, 401.0, 11.0, 0.4, TEXT_D, 4),
    ]
}

fn two_column_fixture() -> Vec<Value> {
    vec![
        title_payload(50.0, 0.0, 290.0, 30.0, 18.0),
        body_payload(50.0, 40.0, 160.0, 190.0, 12.0, 0.5, TEXT_A, 4),
        body_payload(170.0, 40.0, 290.0, 190.0, 12.0, 0.5, TEXT_B, 4),
        body_payload(50.0, 191.0, 160.0, 300.0, 12.0, 0.5, TEXT_C, 4),
        body_payload(170.0, 191.0, 290.0, 300.0, 12.0, 0.5, TEXT_D, 4),
    ]
}

fn dense_and_short_fixture() -> Vec<Value> {
    vec![
        title_payload(50.0, 0.0, 250.0, 30.0, 18.0),
        with_flag(
            body_payload(50.0, 40.0, 120.0, 130.0, 12.0, 0.44, DENSE_TEXT, 4),
            "dense_small_box",
            true,
        ),
        with_flag(
            body_payload(130.0, 40.0, 250.0, 130.0, 12.0, 0.44, TEXT_A, 4),
            "heavy_dense_small_box",
            true,
        ),
        body_payload(50.0, 131.0, 250.0, 155.0, 11.0, 0.4, TEXT_C, 1),
        body_payload(50.0, 156.0, 250.0, 256.0, 12.0, 0.5, TEXT_B, 4),
    ]
}

fn preserved_lines_fixture() -> Vec<Value> {
    vec![
        body_payload(50.0, 40.0, 250.0, 140.0, 12.0, 0.5, TEXT_A, 4),
        with_flag(
            body_payload(50.0, 141.0, 250.0, 241.0, 12.0, 0.5, PRESERVED_TEXT, 3),
            "preserve_line_breaks",
            true,
        ),
        body_payload(50.0, 242.0, 250.0, 342.0, 12.0, 0.5, TEXT_D, 4),
    ]
}

fn annotation_fixture() -> Vec<Value> {
    vec![
        title_payload(50.0, 0.0, 250.0, 30.0, 18.0),
        body_payload(50.0, 40.0, 250.0, 140.0, 12.0, 0.44, TEXT_A, 4),
        annotation_payload(50.0, 141.0, 250.0, 161.0, 9.0, "caption"),
        annotation_payload(50.0, 162.0, 250.0, 182.0, 8.5, "footnote"),
        body_payload(50.0, 183.0, 250.0, 283.0, 12.0, 0.5, TEXT_D, 4),
    ]
}

fn corpus_cases() -> Vec<Value> {
    let mk = |payloads: Vec<Value>, width: f64, mode: &str| -> Value {
        json!({
            "input": {
                "ordered_payloads": payloads,
                "page_text_width_med": width,
                "book_body_font_target": null,
                "font_unify_mode": mode,
            }
        })
    };
    vec![
        mk(single_column_fixture(), 200.0, "role_min"),
        mk(single_column_fixture(), 200.0, "off"),
        mk(vec![title_payload(50.0, 0.0, 250.0, 30.0, 18.0)], 200.0, "off"),
        mk(two_column_fixture(), 110.0, "role_min"),
        mk(two_column_fixture(), 110.0, "off"),
        mk(dense_and_short_fixture(), 200.0, "role_min"),
        mk(dense_and_short_fixture(), 200.0, "off"),
        mk(preserved_lines_fixture(), 200.0, "role_min"),
        mk(preserved_lines_fixture(), 200.0, "off"),
        mk(annotation_fixture(), 200.0, "role_min"),
    ]
}

#[test]
#[ignore = "regenerates tests/layout_policy_corpus.json; not run in CI"]
fn regen_policy_corpus() {
    let mut cases: Vec<Value> = Vec::new();
    for case in corpus_cases() {
        let actual = run_pipeline(&case["input"]);
        let per_block: Vec<Value> = actual.iter().map(extract_contract).collect();
        let mut full = case;
        full.as_object_mut().expect("case is an object").insert(
            "expected".to_string(),
            json!({"per_block": per_block}),
        );
        cases.push(full);
    }
    let corpus = json!({
        "schema": "retainpdf_diff_corpus_v1",
        "seed": 0,
        "cases": cases,
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("layout_policy_corpus.json");
    let text = serde_json::to_string_pretty(&corpus).expect("serialize corpus");
    std::fs::write(&path, text).expect("write layout_policy_corpus.json");
    println!("wrote {}", path.display());
}
