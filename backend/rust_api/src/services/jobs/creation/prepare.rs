use crate::error::AppError;
use crate::models::domain::{JobSnapshot, ResolvedJobSpec, UploadRecord, WorkflowKind};
use crate::models::request::CreateJobInput;
use crate::services::glossaries::resolve_task_glossary_request;
use crate::services::job_validation::{
    validate_mineru_upload_limits, validate_ocr_provider_request, validate_provider_credentials,
    validate_render_options, validate_translation_credentials,
};
use crate::storage_paths::resolve_data_path;

use super::context::SnapshotBuildDeps;
use super::offline_defaults::apply_offline_defaults;
use super::upload::load_upload_or_404;

pub(super) struct PreparedTranslationUpload {
    pub(super) spec: ResolvedJobSpec,
}

pub(super) struct PreparedTranslateOnlyInput {
    pub(super) spec: ResolvedJobSpec,
}

pub(super) struct PreparedRenderInput {
    pub(super) spec: ResolvedJobSpec,
}

#[derive(Debug)]
pub(super) struct PreparedOcrInput {
    pub(super) spec: ResolvedJobSpec,
}

pub(super) fn prepare_full_pipeline_input(
    ctx: &SnapshotBuildDeps<'_>,
    input: &CreateJobInput,
) -> Result<PreparedTranslationUpload, AppError> {
    let mut input = resolve_task_glossary_request(ctx.db, input)?;
    apply_offline_defaults(&mut input, ctx.config.offline_mode);
    validate_render_options(&input)?;
    if !input.source.artifact_job_id.trim().is_empty() {
        validate_translation_credentials(&input, ctx.config.offline_mode)?;
        let source_job = load_artifact_job(ctx, &input.source.artifact_job_id)?;
        ensure_ocr_artifacts_ready_for_translation(ctx, &source_job)?;
        return Ok(PreparedTranslationUpload {
            spec: ResolvedJobSpec::from_input(input),
        });
    }
    let _ = require_translation_upload(ctx, &input)?;
    Ok(PreparedTranslationUpload {
        spec: ResolvedJobSpec::from_input(input),
    })
}

pub(super) fn prepare_translate_only_input(
    ctx: &SnapshotBuildDeps<'_>,
    input: &CreateJobInput,
) -> Result<PreparedTranslateOnlyInput, AppError> {
    let mut input = resolve_task_glossary_request(ctx.db, input)?;
    apply_offline_defaults(&mut input, ctx.config.offline_mode);
    validate_render_options(&input)?;
    if input.source.artifact_job_id.trim().is_empty() {
        let _ = require_translation_upload(ctx, &input)?;
    } else {
        validate_translation_credentials(&input, ctx.config.offline_mode)?;
        let source_job = load_artifact_job(ctx, &input.source.artifact_job_id)?;
        ensure_ocr_artifacts_ready_for_translation(ctx, &source_job)?;
    }
    let mut spec = ResolvedJobSpec::from_input(input);
    spec.workflow = WorkflowKind::Translate;
    Ok(PreparedTranslateOnlyInput { spec })
}

pub(super) fn prepare_render_input(
    ctx: &SnapshotBuildDeps<'_>,
    input: &CreateJobInput,
) -> Result<PreparedRenderInput, AppError> {
    if input.source.artifact_job_id.trim().is_empty() {
        return Err(AppError::bad_request(
            "source.artifact_job_id is required for render workflow",
        ));
    }
    if ctx.db.get_job(&input.source.artifact_job_id).is_err() {
        return Err(AppError::not_found(format!(
            "artifact job not found: {}",
            input.source.artifact_job_id
        )));
    }
    validate_render_options(input)?;
    let mut spec = ResolvedJobSpec::from_input(input.clone());
    spec.workflow = WorkflowKind::Render;
    Ok(PreparedRenderInput { spec })
}

pub(super) fn prepare_ocr_input(
    ctx: &SnapshotBuildDeps<'_>,
    input: &CreateJobInput,
    upload: Option<&UploadRecord>,
) -> Result<PreparedOcrInput, AppError> {
    let mut input = input.clone();
    apply_offline_defaults(&mut input, ctx.config.offline_mode);
    validate_ocr_provider_request(&input)?;
    let resolved_upload = match upload {
        Some(upload) => Some(upload.clone()),
        None if !input.source.upload_id.trim().is_empty() => {
            Some(load_upload_or_404(ctx.db, &input.source.upload_id)?)
        }
        None => None,
    };
    if resolved_upload.is_none() && input.source.source_url.trim().is_empty() {
        return Err(AppError::bad_request(
            "either file, upload_id, or source_url is required",
        ));
    }

    let mut resolved = ResolvedJobSpec::from_input(input.clone());
    resolved.workflow = WorkflowKind::Ocr;
    if let Some(upload) = resolved_upload.as_ref() {
        resolved.source.upload_id = upload.upload_id.clone();
        validate_mineru_upload_limits(&input, upload, ctx.config.provider_limits)?;
    }
    Ok(PreparedOcrInput { spec: resolved })
}

fn require_translation_upload(
    ctx: &SnapshotBuildDeps<'_>,
    input: &CreateJobInput,
) -> Result<UploadRecord, AppError> {
    if input.source.upload_id.trim().is_empty() {
        return Err(AppError::bad_request("upload_id is required"));
    }
    validate_provider_credentials(input, ctx.config.offline_mode)?;
    validate_render_options(input)?;
    let upload = load_upload_or_404(ctx.db, &input.source.upload_id)?;
    validate_mineru_upload_limits(input, &upload, ctx.config.provider_limits)?;
    Ok(upload)
}

fn load_artifact_job(
    ctx: &SnapshotBuildDeps<'_>,
    artifact_job_id: &str,
) -> Result<JobSnapshot, AppError> {
    ctx.db
        .get_job(artifact_job_id)
        .map_err(|_| AppError::not_found(format!("artifact job not found: {artifact_job_id}")))
}

fn ensure_ocr_artifacts_ready_for_translation(
    ctx: &SnapshotBuildDeps<'_>,
    source_job: &JobSnapshot,
) -> Result<(), AppError> {
    let source_label = &source_job.job_id;
    let artifacts = source_job.artifacts.as_ref().ok_or_else(|| {
        AppError::bad_request(format!("artifact job has no artifacts: {source_label}"))
    })?;
    require_artifact_file(
        ctx,
        artifacts.normalized_document_json.as_deref(),
        "normalized_document_json",
        source_label,
    )?;
    require_artifact_file(
        ctx,
        artifacts.source_pdf.as_deref(),
        "source_pdf",
        source_label,
    )?;
    if let Some(layout_json) = artifacts.layout_json.as_deref() {
        require_artifact_file(ctx, Some(layout_json), "layout_json", source_label)?;
    }
    Ok(())
}

fn require_artifact_file(
    ctx: &SnapshotBuildDeps<'_>,
    raw: Option<&str>,
    artifact_key: &str,
    source_label: &str,
) -> Result<(), AppError> {
    let raw = raw.ok_or_else(|| {
        AppError::bad_request(format!("{source_label} is missing {artifact_key}"))
    })?;
    let path = resolve_data_path(ctx.config.data_root, raw).map_err(|err| {
        AppError::bad_request(format!(
            "invalid {artifact_key} path for {source_label}: {err}"
        ))
    })?;
    if !path.is_file() {
        return Err(AppError::bad_request(format!(
            "{artifact_key} not found for {source_label}: {}",
            path.display()
        )));
    }
    Ok(())
}
