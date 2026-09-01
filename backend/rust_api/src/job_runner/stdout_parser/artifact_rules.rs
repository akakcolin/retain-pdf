use serde_json::Value;

use crate::models::domain::JobSnapshot;

use super::artifact_fields::{artifact_field_from_key, ArtifactField, ARTIFACT_LABEL_RULES};
use super::{job_artifacts_mut, ocr_provider_diagnostics_mut, parse_labeled_value};

pub(super) fn apply_artifact_line(job: &mut JobSnapshot, line: &str) {
    if apply_structured_artifact_event(job, line) {
        return;
    }
    for (label, field) in ARTIFACT_LABEL_RULES {
        if let Some(value) = parse_labeled_value(line, label) {
            apply_artifact_field(job, *field, value);
        }
    }
}

fn apply_structured_artifact_event(job: &mut JobSnapshot, line: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return false;
    };
    if value
        .get("event_type")
        .and_then(Value::as_str)
        .map(str::trim)
        != Some("artifact_published")
    {
        return false;
    }
    let Some(payload) = value.get("payload").and_then(Value::as_object) else {
        return false;
    };
    let Some(artifact_key) = payload
        .get("artifact_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
    else {
        return false;
    };
    let Some(path) = payload
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
    else {
        return false;
    };
    let Some(field) = artifact_field_from_key(artifact_key) else {
        return false;
    };
    apply_artifact_field(job, field, path);
    true
}

fn apply_artifact_field(job: &mut JobSnapshot, field: ArtifactField, value: &str) {
    match field {
        ArtifactField::JobRoot => job_artifacts_mut(job).job_root = Some(value.to_string()),
        // source_pdf 的真值由 OCR/翻译阶段直接写入（原始上传稿）。渲染阶段
        // render_rs 也会打印 `source pdf: <渲染基底>`（bbox-text-stripped 输入），
        // 这里只应在尚未设置时接收，避免把对照阅读用的原稿覆盖成去字渲染基底。
        ArtifactField::SourcePdf => {
            let artifacts = job_artifacts_mut(job);
            if artifacts.source_pdf.is_none() {
                artifacts.source_pdf = Some(value.to_string());
            }
        }
        ArtifactField::LayoutJson => {
            let value = value.to_string();
            job_artifacts_mut(job).layout_json = Some(value.clone());
            ocr_provider_diagnostics_mut(job).artifacts.layout_json = Some(value);
        }
        ArtifactField::NormalizedDocumentJson => {
            let value = value.to_string();
            job_artifacts_mut(job).normalized_document_json = Some(value.clone());
            ocr_provider_diagnostics_mut(job)
                .artifacts
                .normalized_document_json = Some(value);
        }
        ArtifactField::NormalizationReportJson => {
            let value = value.to_string();
            job_artifacts_mut(job).normalization_report_json = Some(value.clone());
            ocr_provider_diagnostics_mut(job)
                .artifacts
                .normalization_report_json = Some(value);
        }
        ArtifactField::ProviderRawDir => {
            job_artifacts_mut(job).provider_raw_dir = Some(value.to_string())
        }
        ArtifactField::ProviderZip => {
            let value = value.to_string();
            job_artifacts_mut(job).provider_zip = Some(value.clone());
            ocr_provider_diagnostics_mut(job)
                .artifacts
                .provider_bundle_zip = Some(value);
        }
        ArtifactField::ProviderSummaryJson => {
            job_artifacts_mut(job).provider_summary_json = Some(value.to_string())
        }
        ArtifactField::SchemaVersion => {
            job_artifacts_mut(job).schema_version = Some(value.to_string())
        }
        ArtifactField::TranslationsDir => {
            job_artifacts_mut(job).translations_dir = Some(value.to_string())
        }
        ArtifactField::OutputPdf => job_artifacts_mut(job).output_pdf = Some(value.to_string()),
        ArtifactField::Summary => job_artifacts_mut(job).summary = Some(value.to_string()),
        ArtifactField::EventsJsonl => job_artifacts_mut(job).events_jsonl = Some(value.to_string()),
        ArtifactField::BatchId => {
            ocr_provider_diagnostics_mut(job).handle.batch_id = Some(value.to_string())
        }
        ArtifactField::TaskId => {
            ocr_provider_diagnostics_mut(job).handle.task_id = Some(value.to_string())
        }
        ArtifactField::FullZipUrl => {
            ocr_provider_diagnostics_mut(job).artifacts.full_zip_url = Some(value.to_string())
        }
    }
}
