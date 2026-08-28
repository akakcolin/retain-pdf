use anyhow::{anyhow, Result};

use crate::models::domain::{now_iso, JobRuntimeState, JobStatusKind};
use crate::ocr_provider::OcrProviderKind;
use crate::worker_command::{build_worker_stage_command, WorkerStageCommand};

use super::{
    clear_job_failure, execute_process_job, job_artifacts_mut, sync_runtime_state,
    ProcessRuntimeDeps,
};
use super::support::{fail_missing_source_pdf, fail_ocr_transport, save_ocr_job};
use super::transport::prepare_local_upload_source;
use super::workspace::OcrWorkspace;

/// Skip the OCR provider entirely and build the translation source document
/// from the PDF's embedded text layer.
pub(super) async fn execute_text_layer_extraction(
    deps: ProcessRuntimeDeps,
    mut job: JobRuntimeState,
    output_job_id_override: Option<String>,
    parent_job_id: Option<String>,
) -> Result<JobRuntimeState> {
    job.status = JobStatusKind::Running;
    if job.started_at.is_none() {
        job.started_at = Some(now_iso());
    }
    job.updated_at = now_iso();
    job.stage = Some("extract_text_layer".to_string());
    job.stage_detail = Some("跳过 OCR，直接提取 PDF 文本层".to_string());
    clear_job_failure(&mut job);
    sync_runtime_state(&mut job);
    save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;

    let workspace = OcrWorkspace::prepare(
        &deps.persist.output_root,
        &mut job,
        &OcrProviderKind::Mineru,
        output_job_id_override,
    )?;

    let Some(source_pdf_path) =
        prepare_local_upload_source(deps.db.as_ref(), &mut job, &workspace.source_dir)?
    else {
        let err = anyhow!("跳过 OCR 需要一个已上传的本地 PDF 源文件");
        fail_ocr_transport(&mut job, &err);
        save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;
        return Ok(job);
    };

    if !source_pdf_path.exists() {
        fail_missing_source_pdf(&mut job, &source_pdf_path);
        save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;
        return Ok(job);
    }

    job_artifacts_mut(&mut job).source_pdf = Some(source_pdf_path.to_string_lossy().to_string());

    let output_json_path = workspace.job_paths.ocr_dir.join("normalized_document_v1.json");
    job.command = build_worker_stage_command(
        &deps.worker_command_runtime(),
        &job.request_payload,
        &workspace.job_paths,
        WorkerStageCommand::ExtractTextLayer {
            source_pdf_path: &source_pdf_path,
            output_json_path: &output_json_path,
        },
    )?;
    job.stage = Some("extract_text_layer".to_string());
    job.stage_detail = Some("正在从 PDF 文本层提取翻译源".to_string());
    job.updated_at = now_iso();
    sync_runtime_state(&mut job);
    save_ocr_job(&deps, &job, parent_job_id.as_deref()).await?;

    execute_process_job(deps, job, &[]).await
}
