use anyhow::Result;
use std::path::Path;

use crate::job_runner::{job_artifacts_mut, ocr_provider_diagnostics_mut, ProcessRuntimeDeps};
use crate::models::domain::{now_iso, JobRuntimeState};
use crate::ocr_provider::local::LocalPaddlexClient;
use crate::ocr_provider::{OcrErrorCategory, OcrProviderErrorInfo};

use super::artifacts::persist_provider_result;
use super::paddle_markdown::materialize_paddle_markdown_artifacts;
use super::save_ocr_job;
use super::status::record_provider_trace;

pub(super) async fn run_local_ocr_transport_local(
    deps: &ProcessRuntimeDeps,
    job: &mut JobRuntimeState,
    client: &LocalPaddlexClient,
    upload_path: &Path,
    provider_result_json_path: &Path,
    job_root: &Path,
    parent_job_id: Option<&str>,
) -> Result<()> {
    let result = client
        .layout_parse(upload_path)
        .await
        .map_err(|err| attach_local_paddlex_runtime_error(job, err, "layout_parse"))?;
    if let Some(log_id) = result.log_id.clone() {
        ocr_provider_diagnostics_mut(job).handle.task_id = Some(log_id.clone());
        job.append_log(&format!("task_id: {log_id}"));
        record_provider_trace(job, Some(log_id));
    }
    job.stage = Some("ocr_processing".to_string());
    job.stage_detail = Some("本地 PaddleX 解析完成，结果已就绪".to_string());
    job.updated_at = now_iso();
    save_ocr_job(deps, job, parent_job_id).await?;
    persist_provider_result(job, provider_result_json_path, &result.payload).await?;
    if let Some(markdown_path) =
        materialize_paddle_markdown_artifacts(&result.payload, job_root).await?
    {
        job.append_log(&format!("published markdown: {}", markdown_path.display()));
    }
    Ok(())
}

fn attach_local_paddlex_runtime_error(
    job: &mut JobRuntimeState,
    err: anyhow::Error,
    stage: &str,
) -> anyhow::Error {
    if let Some(provider_err) =
        err.downcast_ref::<crate::ocr_provider::local::LocalPaddlexProviderError>()
    {
        apply_local_error(
            job,
            provider_err.info().clone(),
            provider_err.stage_detail(),
        );
        return err;
    }
    let info = OcrProviderErrorInfo {
        category: OcrErrorCategory::ProviderFailed,
        provider_code: None,
        provider_message: Some(err.to_string()),
        operator_hint: Some("请结合 job 日志和本地 PaddleX 服务状态继续排查".to_string()),
        trace_id: job_artifacts_mut(job).provider_trace_id.clone(),
        http_status: None,
    };
    apply_local_error(job, info, format!("本地 PaddleX {stage} 失败: {}", err));
    err
}

fn apply_local_error(job: &mut JobRuntimeState, info: OcrProviderErrorInfo, stage_detail: String) {
    if let Some(trace_id) = info.trace_id.clone() {
        job_artifacts_mut(job).provider_trace_id = Some(trace_id);
    }
    ocr_provider_diagnostics_mut(job).last_error = Some(info);
    if !stage_detail.trim().is_empty() {
        job.stage_detail = Some(stage_detail);
    }
}
