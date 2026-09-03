use crate::models::domain::{now_iso, JobRuntimeState, JobStatusKind};
use crate::ocr_provider::parse_provider_kind;
use crate::worker_command::{build_worker_stage_command, WorkerStageCommand};
use anyhow::Result;

use super::{
    clear_canceled_runtime_artifacts, clear_job_failure, execute_process_job, job_artifacts_mut,
    sync_runtime_state, ProcessRuntimeDeps,
};

mod artifacts;
mod bundle_download;
mod bundle_download_retry;
mod bundle_events;
mod bundle_ready_wait;
mod bundle_retry_policy;
mod local;
mod markdown_bundle;
mod mineru;
mod mineru_polling;
mod mineru_retry;
mod mineru_status_handlers;
mod paddle;
mod paddle_errors;
mod paddle_markdown;
mod paddle_payload;
mod page_subset;
mod polling;
mod provider_result;
mod provider_transport;
mod status;
mod support;
mod text_layer;
mod transport;
mod workspace;

use super::cancel_registry::is_cancel_requested_with_registry;
use provider_transport::execute_provider_transport;
pub use support::sync_parent_with_ocr_child;
use support::{fail_missing_source_pdf, fail_ocr_transport, save_ocr_job};
use text_layer::{prepare_text_layer_extraction, TextLayerPreparation};
use workspace::OcrWorkspace;

pub async fn execute_ocr_job(
    deps: ProcessRuntimeDeps,
    mut job: JobRuntimeState,
    output_job_id_override: Option<String>,
    parent_job_id: Option<String>,
) -> Result<JobRuntimeState> {
    if job.request_payload.ocr.skip_ocr {
        let prepared =
            prepare_text_layer_extraction(&deps, job, output_job_id_override, parent_job_id)
                .await?;
        // 回交 process runner 的决策统一收敛在编排层。
        return match prepared {
            TextLayerPreparation::Ready(job) => execute_process_job(deps, job, &[]).await,
            TextLayerPreparation::Terminal(job) => Ok(job),
        };
    }
    let provider_kind = parse_provider_kind(&job.request_payload.ocr.provider);
    job.status = JobStatusKind::Running;
    if job.started_at.is_none() {
        job.started_at = Some(now_iso());
    }
    job.updated_at = now_iso();
    job.stage = Some("ocr_upload".to_string());
    job.stage_detail = Some("OCR provider transport 启动中".to_string());
    clear_job_failure(&mut job);
    sync_runtime_state(&mut job);
    save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;

    let workspace = OcrWorkspace::prepare(
        &deps.persist.output_root,
        &mut job,
        &provider_kind,
        output_job_id_override,
    )?;
    save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;

    let source_pdf_path = match execute_provider_transport(
        &deps,
        &mut job,
        &provider_kind,
        &workspace,
        parent_job_id.as_deref(),
    )
    .await
    {
        Ok(path) => path,
        Err(err) => {
            fail_ocr_transport(&mut job, &err);
            return Ok(job);
        }
    };

    if is_cancel_requested_with_registry(deps.canceled_jobs.as_ref(), &job.job_id).await {
        job.status = JobStatusKind::Canceled;
        job.stage = Some("canceled".to_string());
        job.stage_detail = Some("OCR 任务已取消".to_string());
        job.updated_at = now_iso();
        job.finished_at = Some(now_iso());
        clear_canceled_runtime_artifacts(&mut job);
        clear_job_failure(&mut job);
        sync_runtime_state(&mut job);
        save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;
        return Ok(job);
    }

    if !source_pdf_path.exists() {
        fail_missing_source_pdf(&mut job, &source_pdf_path);
        save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;
        return Ok(job);
    }

    let source_pdf_string = source_pdf_path.to_string_lossy().to_string();
    job_artifacts_mut(&mut job).source_pdf = Some(source_pdf_string);

    job.command = build_worker_stage_command(
        &deps.worker_command_runtime(),
        &job.request_payload,
        &workspace.job_paths,
        WorkerStageCommand::NormalizeOcr {
            source_json_path: &workspace.layout_json_path,
            source_pdf_path: &source_pdf_path,
            provider_result_json_path: &workspace.provider_result_json_path,
            provider_zip_path: &workspace.provider_zip_path,
            provider_raw_dir: &workspace.provider_raw_dir,
        },
    )?;
    job.stage = Some("normalizing".to_string());
    job.stage_detail = Some("OCR provider 已完成，开始标准化 document.v1".to_string());
    job.updated_at = now_iso();
    sync_runtime_state(&mut job);
    save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;

    execute_process_job(deps, job, &[]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::JobSnapshot;
    use crate::models::request::CreateJobInput;

    #[test]
    fn fail_missing_source_pdf_marks_job_failed_with_clear_detail() {
        let mut job = JobSnapshot::new(
            "job-missing-source-pdf".to_string(),
            CreateJobInput::default(),
            vec!["python".to_string()],
        )
        .into_runtime();
        let missing = std::path::Path::new("/definitely/missing/source.pdf");

        fail_missing_source_pdf(&mut job, missing);

        assert_eq!(job.status, JobStatusKind::Failed);
        assert_eq!(job.stage.as_deref(), Some("failed"));
        assert_eq!(
            job.stage_detail.as_deref(),
            Some("OCR 已完成，但任务源 PDF 缺失")
        );
        let failure = job.failure.as_ref().expect("failure");
        assert_eq!(failure.category, "source_pdf_missing");
        assert_eq!(failure.summary, "源 PDF 缺失");
        assert!(!failure.retryable);
    }
}
