//! `pipeline_summary.json` writer mirroring `render_only.py` (lines 109/142-163).
//! The file's existence at the job artifacts path is what the rust_api worker
//! output-contract checks (`validate_render_outputs`); the field set mirrors the
//! Python summary so the job-detail UI and diagnostics layer read the same shape.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::spec::{RenderStageSpec, RENDER_STAGE_SCHEMA_VERSION};

pub const PIPELINE_SUMMARY_FILE_NAME: &str = "pipeline_summary.json";

#[allow(clippy::too_many_arguments)]
pub fn write_pipeline_summary(
    spec: &RenderStageSpec,
    output_pdf: &Path,
    source_pdf: &Path,
    mode: &str,
    page_count: usize,
    elapsed_seconds: f64,
    render_diagnostics: &Value,
    events_jsonl: &Path,
) -> anyhow::Result<PathBuf> {
    let summary_path = spec.job.job_root.join("artifacts").join(PIPELINE_SUMMARY_FILE_NAME);
    if let Some(parent) = summary_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let summary = json!({
        "job_root": spec.job.job_root.to_string_lossy(),
        "source_pdf": source_pdf.to_string_lossy(),
        "translations_dir": spec.inputs.translations_dir.to_string_lossy(),
        "translation_manifest": translation_manifest_str(spec),
        "output_pdf": output_pdf.to_string_lossy(),
        "pages_processed": page_count,
        "render_elapsed": elapsed_seconds,
        "total_elapsed": elapsed_seconds,
        "render_mode": mode,
        "effective_render_mode": mode,
        "renderer": "render_rs",
        "pdf_compress_dpi": spec.params.pdf_compress_dpi(),
        "render_diagnostics": render_diagnostics,
        "events_jsonl": events_jsonl.to_string_lossy(),
        "invocation": {
            "stage": "render",
            "stage_spec_schema_version": RENDER_STAGE_SCHEMA_VERSION,
        },
    });
    std::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)?;
    Ok(summary_path)
}

fn translation_manifest_str(spec: &RenderStageSpec) -> String {
    spec.inputs
        .translation_manifest
        .as_deref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::spec::{RenderStageInputs, RenderStageParams, RenderStageSpec, StageJobRef};

    fn unique_root() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("render-rs-summary-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn writes_pipeline_summary_to_job_artifacts_dir() {
        let root = unique_root();
        let job_root = root.join("jobs").join("job-test");
        let spec = RenderStageSpec {
            schema_version: crate::spec::RENDER_STAGE_SCHEMA_VERSION.to_string(),
            stage: "render".to_string(),
            job: StageJobRef {
                job_id: "job-test".to_string(),
                job_root: job_root.clone(),
                workflow: "book".to_string(),
            },
            inputs: RenderStageInputs {
                source_pdf: job_root.join("source/in.pdf"),
                translations_dir: job_root.join("translated"),
                translation_manifest: Some(job_root.join("translated/translation_manifest.json")),
            },
            params: RenderStageParams {
                render_mode: Some("typst".to_string()),
                ..RenderStageParams::default()
            },
        };
        let output_pdf = job_root.join("rendered/out.pdf");
        let diagnostics = serde_json::json!({
            "typst_cover_fallback_pages": {"count": 1, "head": [0], "tail": []},
            "typst_cover_fallback_items": {"count": 0, "head": [], "tail": []},
            "save_elapsed_seconds": 0.25,
        });
        let events_path = job_root.join("logs/pipeline_events.jsonl");

        let path = write_pipeline_summary(
            &spec,
            &output_pdf,
            &spec.inputs.source_pdf,
            "typst",
            3,
            1.5,
            &diagnostics,
            &events_path,
        )
        .expect("write summary");

        assert_eq!(path, job_root.join("artifacts").join(PIPELINE_SUMMARY_FILE_NAME));
        assert!(path.is_file());
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read summary"))
                .expect("parse summary");
        assert_eq!(value["pages_processed"], 3);
        assert_eq!(value["render_mode"], "typst");
        assert_eq!(value["effective_render_mode"], "typst");
        assert_eq!(value["renderer"], "render_rs");
        assert_eq!(
            value["translation_manifest"],
            spec.inputs.translation_manifest.unwrap().to_string_lossy().into_owned()
        );
        assert_eq!(value["render_diagnostics"], diagnostics);
        assert_eq!(value["events_jsonl"], events_path.to_string_lossy().into_owned());
        let _ = std::fs::remove_dir_all(&root);
    }
}
