//! Native `normalize_ocr` worker mirroring `entrypoints/run_normalize_ocr.py`
//! (`services/document_schema/normalize_pipeline.py::main`): load the
//! `normalize.stage.v1` spec, adapt the raw provider layout JSON into
//! `document.v1`, apply defaults + contract enrichment, rescale geometry to the
//! source PDF, rebuild paddle-style line geometry, and persist the compact
//! document + pretty report with the production stdout labels.
//!
//! C5-N2a/C5-N2b/C5-N2c support the `mineru`, `mineru_content_list_v2` and
//! `paddle` provider adapters; the generic_flat_ocr adapter is a separate batch
//! and stays on the python subprocess until then.

pub mod adapter_content_list_v2;
pub mod adapter_mineru;
pub mod common;
pub mod contract;
pub mod defaults;
pub mod formula_protection;
pub mod paddle;
pub mod paddle_rebuild;
pub mod reporting;
pub mod rescale;
pub mod spec;
pub mod validator;
pub mod version;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use self::adapter_content_list_v2::{
    build_content_list_v2_document, PROVIDER_MINERU_CONTENT_LIST_V2,
};
use self::adapter_mineru::{build_mineru_document, PROVIDER_MINERU};
use self::contract::enrich_document_contract_v1;
use self::paddle::{build_paddle_document, looks_like_paddle_layout, PROVIDER_PADDLE};
use self::defaults::apply_document_defaults_with_report;
use self::paddle_rebuild::post_rescale_rebuild_paddle_text_geometry;
use self::reporting::build_normalization_summary;
use self::rescale::rescale_document_geometry_to_pdf;
use self::spec::NormalizeStageSpec;
use self::validator::build_validation_report;
use self::version::{DOCUMENT_SCHEMA_FILE_NAME, DOCUMENT_SCHEMA_REPORT_FILE_NAME};

/// `foundation/shared/job_dirs.py` directory names (ocr dir under the job root).
const OCR_DIR_NAME: &str = "ocr";
const NORMALIZED_DIR_NAME: &str = "normalized";

fn resolve(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `str(value or "")` for an optional spec path — an empty string path is falsy.
fn optional_path_str(opt: &Option<PathBuf>) -> Option<String> {
    opt.as_ref()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.display().to_string())
}

fn read_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read source json: {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse source json: {}", path.display()))
}

fn save_json_compact(path: &Path, payload: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create output dir: {}", parent.display()))?;
    }
    let text = serde_json::to_string(payload)?;
    std::fs::write(path, text).with_context(|| format!("write json: {}", path.display()))
}

fn save_json_pretty(path: &Path, payload: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create output dir: {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(payload)?;
    std::fs::write(path, text).with_context(|| format!("write json: {}", path.display()))
}

/// `looks_like_generic_flat_ocr` — payload.provider == "generic_flat_ocr" + pages list.
fn looks_like_generic_flat_ocr(payload: &Value) -> bool {
    payload.is_object()
        && payload.get("provider").and_then(Value::as_str) == Some("generic_flat_ocr")
        && payload.get("pages").map_or(false, Value::is_array)
}

/// `looks_like_mineru_content_list_v2` — a list-of-lists-of-blocks payload.
fn looks_like_mineru_content_list_v2(payload: &Value) -> bool {
    let Some(pages) = payload.as_array() else {
        return false;
    };
    if pages.is_empty() {
        return true;
    }
    let Some(first_page) = pages[0].as_array() else {
        return false;
    };
    if first_page.is_empty() {
        return true;
    }
    let Some(first_block) = first_page[0].as_object() else {
        return false;
    };
    first_block.contains_key("type") && first_block.contains_key("content")
}

fn looks_like_mineru_layout(payload: &Value) -> bool {
    let Some(pdf_info) = payload.get("pdf_info") else {
        return false;
    };
    let Some(pages) = pdf_info.as_array() else {
        return false;
    };
    if pages.is_empty() {
        return true;
    }
    pages[0]
        .as_object()
        .map_or(false, |page| page.contains_key("para_blocks"))
}

/// `detect_ocr_provider_with_report` restricted to the adapters the native path
/// can identify (generic_flat_ocr, mineru_content_list_v2, mineru) — the mineru
/// layout payload resolves on the third attempt, matching the reference report.
fn detect_provider_with_report(payload: &Value) -> Value {
    let mut attempts: Vec<Value> = Vec::new();
    let gf = looks_like_generic_flat_ocr(payload);
    attempts.push(json!({ "provider": "generic_flat_ocr", "matched": gf }));
    if gf {
        return json!({ "matched": true, "provider": "generic_flat_ocr", "attempts": attempts });
    }
    let mcl2 = looks_like_mineru_content_list_v2(payload);
    attempts.push(json!({ "provider": "mineru_content_list_v2", "matched": mcl2 }));
    if mcl2 {
        return json!({ "matched": true, "provider": "mineru_content_list_v2", "attempts": attempts });
    }
    let mineru = looks_like_mineru_layout(payload);
    attempts.push(json!({ "provider": PROVIDER_MINERU, "matched": mineru }));
    if mineru {
        return json!({ "matched": true, "provider": PROVIDER_MINERU, "attempts": attempts });
    }
    let paddle = looks_like_paddle_layout(payload);
    attempts.push(json!({ "provider": PROVIDER_PADDLE, "matched": paddle }));
    if paddle {
        return json!({ "matched": true, "provider": PROVIDER_PADDLE, "attempts": attempts });
    }
    json!({ "matched": false, "provider": "", "attempts": attempts })
}

/// `adapt_path_to_document_v1_with_report` (mineru-gated) — builder -> defaults
/// -> contract, returning the document and its normalization report.
fn adapt_document_with_report(
    source_json: &Path,
    document_id: &str,
    provider: &str,
    provider_version: &str,
    payload: &Value,
) -> Result<(Value, Value)> {
    let mut document = if provider == PROVIDER_MINERU {
        build_mineru_document(payload, document_id, source_json, provider_version)
    } else if provider == PROVIDER_MINERU_CONTENT_LIST_V2 {
        build_content_list_v2_document(payload, document_id, source_json, provider_version)
    } else if provider == PROVIDER_PADDLE {
        build_paddle_document(payload, document_id, source_json, provider_version)
    } else {
        anyhow::bail!("unsupported native OCR provider adapter: {provider}");
    };
    let defaults_report = apply_document_defaults_with_report(&mut document);
    enrich_document_contract_v1(&mut document);

    let detection = detect_provider_with_report(payload);
    let detected_provider = detection
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !provider.is_empty() && !detected_provider.is_empty() && detected_provider != provider {
        anyhow::bail!(
            "Explicit OCR provider does not match detected provider: \
             provider={provider} detected={detected_provider}. \
             Pass allow_provider_mismatch=True only for a configured raw-provider override."
        );
    }
    let validation = build_validation_report(&document).map_err(anyhow::Error::msg)?;
    let report = json!({
        "source_json_path": source_json.display().to_string(),
        "document_id": document_id,
        "provider": provider,
        "provider_version": provider_version,
        "defaults": defaults_report,
        "validation": validation,
        "detected_provider": if detected_provider.is_empty() { provider } else { detected_provider.as_str() },
        "detection": detection,
        "provider_was_explicit": !provider.is_empty(),
        "provider_mismatch_allowed": false,
    });
    Ok((document, report))
}

/// `_refresh_report_for_final_document` — re-validate and refresh defaults
/// counts against the final (rescaled/rebuild) document.
fn refresh_report_for_final_document(report: &Value, document: &Value) -> Result<Value> {
    let mut refreshed = report.clone();
    let pages = document.get("pages").and_then(Value::as_array);
    let pages_seen = pages.map_or(0, |p| p.len());
    let blocks_seen = pages.map_or(0, |pages| {
        pages
            .iter()
            .map(|page| page.get("blocks").and_then(Value::as_array).map_or(0, |b| b.len()))
            .sum()
    });
    if let Some(defaults) = refreshed.get_mut("defaults") {
        if let Some(obj) = defaults.as_object_mut() {
            obj.insert("pages_seen".to_string(), Value::from(pages_seen));
            obj.insert("blocks_seen".to_string(), Value::from(blocks_seen));
        }
    }
    refreshed["validation"] = build_validation_report(document).map_err(anyhow::Error::msg)?;
    Ok(refreshed)
}

/// `entrypoints/run_normalize_ocr.py::main` — full normalize worker.
pub fn normalize_ocr(spec_path: &Path) -> Result<Value> {
    let spec = NormalizeStageSpec::load(spec_path)?;
    let provider = spec.inputs.provider.trim().to_lowercase();
    let source_json = resolve(&spec.inputs.source_json);
    let source_pdf = resolve(&spec.inputs.source_pdf);
    if !source_json.exists() {
        anyhow::bail!("source json not found: {}", source_json.display());
    }
    if !source_pdf.exists() {
        anyhow::bail!("source pdf not found: {}", source_pdf.display());
    }
    let job_root = resolve(&spec.job.job_root);
    let ocr_dir = job_root.join(OCR_DIR_NAME);
    let normalized_dir = ocr_dir.join(NORMALIZED_DIR_NAME);
    let normalized_json_path = normalized_dir.join(DOCUMENT_SCHEMA_FILE_NAME);
    let normalized_report_json_path = normalized_dir.join(DOCUMENT_SCHEMA_REPORT_FILE_NAME);
    let document_id = job_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let provider_version = spec.inputs.provider_version.trim().to_string();

    let payload = read_json(&source_json)?;
    let (mut document, report) = adapt_document_with_report(
        &source_json,
        &document_id,
        &provider,
        &provider_version,
        &payload,
    )?;
    rescale_document_geometry_to_pdf(&mut document, &source_pdf)?;
    post_rescale_rebuild_paddle_text_geometry(&mut document);
    let report = refresh_report_for_final_document(&report, &document)?;

    save_json_compact(&normalized_json_path, &document)?;
    save_json_pretty(&normalized_report_json_path, &report)?;

    let validation = report
        .get("validation")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let summary = build_normalization_summary(&report);

    let provider_raw_dir = optional_path_str(&spec.inputs.provider_raw_dir)
        .unwrap_or_else(|| ocr_dir.display().to_string());
    let provider_zip = optional_path_str(&spec.inputs.provider_zip).unwrap_or_default();
    let provider_summary = optional_path_str(&spec.inputs.provider_result_json)
        .unwrap_or_else(|| source_json.display().to_string());

    println!("job root: {}", job_root.display());
    println!("source pdf: {}", source_pdf.display());
    println!("layout json: {}", source_json.display());
    println!("normalized document json: {}", normalized_json_path.display());
    println!("normalization report json: {}", normalized_report_json_path.display());
    println!("provider raw dir: {}", provider_raw_dir);
    println!("provider zip: {}", provider_zip);
    println!("provider summary json: {}", provider_summary);
    println!(
        "normalized document validated: schema={} version={} pages={} blocks={} path={}",
        validation.get("schema").and_then(Value::as_str).unwrap_or(""),
        validation.get("schema_version").and_then(Value::as_str).unwrap_or(""),
        validation.get("page_count").and_then(Value::as_i64).unwrap_or(0),
        validation.get("block_count").and_then(Value::as_i64).unwrap_or(0),
        normalized_json_path.display()
    );
    println!(
        "normalized document report: provider={} detected={} pages_observed={} blocks_observed={} \
         defaulted_document_fields={} defaulted_page_fields={} defaulted_block_fields={} path={}",
        summary.get("provider").and_then(Value::as_str).unwrap_or(""),
        summary.get("detected_provider").and_then(Value::as_str).unwrap_or(""),
        summary.get("pages_observed").and_then(Value::as_i64).unwrap_or(0),
        summary.get("blocks_observed").and_then(Value::as_i64).unwrap_or(0),
        summary.get("defaulted_document_fields").and_then(Value::as_i64).unwrap_or(0),
        summary.get("defaulted_page_fields").and_then(Value::as_i64).unwrap_or(0),
        summary.get("defaulted_block_fields").and_then(Value::as_i64).unwrap_or(0),
        normalized_report_json_path.display()
    );
    println!("schema version: document.v1");
    Ok(document)
}
