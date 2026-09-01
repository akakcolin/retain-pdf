use std::path::{Component, Path};

use crate::models::api::RetryStageKind;
use crate::models::domain::{JobArtifacts, JobSnapshot, JobStatusKind, WorkflowKind};
use crate::storage_paths::{resolve_data_path, TRANSLATION_MANIFEST_FILE_NAME};

#[derive(Debug, Clone)]
pub(crate) struct JobStagePlan {
    pub stage: RetryStageKind,
    pub label: String,
    pub can_retry: bool,
    pub disabled_reason: String,
    pub will_reuse: Vec<String>,
    pub will_rerun: Vec<String>,
    pub retry_workflow: WorkflowKind,
    pub danger: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct JobResumePlan {
    pub can_resume: bool,
    pub from_stage: Option<String>,
    pub resume_workflow: Option<WorkflowKind>,
    pub reuses_artifacts: Vec<String>,
    pub reruns_stages: Vec<String>,
    pub reason: Option<String>,
}

pub(crate) fn stage_plans(job: &JobSnapshot, data_root: &Path) -> Vec<JobStagePlan> {
    vec![
        stage_plan(job, RetryStageKind::Ocr, data_root),
        stage_plan(job, RetryStageKind::Translation, data_root),
        stage_plan(job, RetryStageKind::Render, data_root),
    ]
}

pub(crate) fn stage_plan(
    job: &JobSnapshot,
    stage: RetryStageKind,
    data_root: &Path,
) -> JobStagePlan {
    let availability = StageArtifactAvailability::from_job(job, data_root);
    let running = matches!(job.status, JobStatusKind::Queued | JobStatusKind::Running);
    let mut plan = base_stage_plan(stage, &availability);

    if running {
        plan.can_retry = false;
        plan.disabled_reason =
            "job is queued or running; cancel it before retrying a stage".to_string();
    } else if !plan.can_retry {
        plan.disabled_reason = disabled_reason_for_stage(&plan.stage, &availability);
    }
    plan
}

pub(crate) fn resume_plan(job: &JobSnapshot, data_root: &Path) -> JobResumePlan {
    let availability = StageArtifactAvailability::from_job(job, data_root);
    if availability.translations_available {
        return JobResumePlan {
            can_resume: true,
            from_stage: Some("render".to_string()),
            resume_workflow: Some(WorkflowKind::Render),
            reuses_artifacts: vec![
                "source_pdf".to_string(),
                "translations_dir".to_string(),
                "normalized_document_json".to_string(),
            ],
            reruns_stages: vec!["rendering".to_string()],
            reason: None,
        };
    }
    if availability.ocr_available {
        return JobResumePlan {
            can_resume: true,
            from_stage: Some("translate".to_string()),
            resume_workflow: Some(WorkflowKind::Book),
            reuses_artifacts: vec![
                "source_pdf".to_string(),
                "normalized_document_json".to_string(),
                "normalization_report_json".to_string(),
            ],
            reruns_stages: vec!["translation".to_string(), "rendering".to_string()],
            reason: None,
        };
    }
    JobResumePlan {
        can_resume: false,
        from_stage: None,
        resume_workflow: None,
        reuses_artifacts: Vec::new(),
        reruns_stages: Vec::new(),
        reason: Some(resume_unavailable_reason(&availability)),
    }
}

pub(crate) fn stage_name(stage: &RetryStageKind) -> &'static str {
    match stage {
        RetryStageKind::Ocr => "ocr",
        RetryStageKind::Translation => "translation",
        RetryStageKind::Render => "render",
    }
}

fn has_request_source(job: &JobSnapshot) -> bool {
    !job.request_payload.source.upload_id.trim().is_empty()
        || !job.request_payload.source.source_url.trim().is_empty()
}

fn base_stage_plan(
    stage: RetryStageKind,
    availability: &StageArtifactAvailability,
) -> JobStagePlan {
    match stage {
        RetryStageKind::Ocr => JobStagePlan {
            stage,
            label: "重试 OCR".to_string(),
            can_retry: availability.source_retryable_from_request,
            disabled_reason: String::new(),
            will_reuse: vec!["source_pdf".to_string()],
            will_rerun: vec![
                "ocr".to_string(),
                "translation".to_string(),
                "render".to_string(),
            ],
            retry_workflow: WorkflowKind::Book,
            danger: true,
        },
        RetryStageKind::Translation => JobStagePlan {
            stage,
            label: "重试翻译".to_string(),
            can_retry: availability.ocr_available,
            disabled_reason: String::new(),
            will_reuse: vec!["source_pdf".to_string(), "ocr_result".to_string()],
            will_rerun: vec!["translation".to_string(), "render".to_string()],
            retry_workflow: WorkflowKind::Book,
            danger: false,
        },
        RetryStageKind::Render => JobStagePlan {
            stage,
            label: "重新渲染".to_string(),
            can_retry: availability.translations_available,
            disabled_reason: String::new(),
            will_reuse: vec![
                "source_pdf".to_string(),
                "ocr_result".to_string(),
                "translation_result".to_string(),
            ],
            will_rerun: vec!["render".to_string()],
            retry_workflow: WorkflowKind::Render,
            danger: false,
        },
    }
}

fn disabled_reason_for_stage(
    stage: &RetryStageKind,
    availability: &StageArtifactAvailability,
) -> String {
    match stage {
        RetryStageKind::Ocr if !availability.has_request_source => {
            "OCR retry currently requires the original upload_id or source_url on the job"
                .to_string()
        }
        RetryStageKind::Ocr => "source PDF is not available".to_string(),
        RetryStageKind::Translation => {
            "need source_pdf and normalized_document_json to retry translation".to_string()
        }
        RetryStageKind::Render => {
            "need source_pdf and translations_dir to retry render".to_string()
        }
    }
}

fn resume_unavailable_reason(availability: &StageArtifactAvailability) -> String {
    if !availability.source_available {
        "need source_pdf before resuming a job".to_string()
    } else {
        "need translations_dir+source_pdf or normalized_document_json+source_pdf".to_string()
    }
}

#[derive(Debug)]
struct StageArtifactAvailability {
    has_request_source: bool,
    source_available: bool,
    source_retryable_from_request: bool,
    ocr_available: bool,
    translations_available: bool,
}

impl StageArtifactAvailability {
    fn from_job(job: &JobSnapshot, data_root: &Path) -> Self {
        let artifacts = job.artifacts.as_ref();
        let has_request_source = has_request_source(job);
        let source_artifact_available = has_artifact(artifacts, |item| &item.source_pdf);
        let source_available = has_request_source || source_artifact_available;
        Self {
            has_request_source,
            source_available,
            source_retryable_from_request: source_available && has_request_source,
            ocr_available: source_artifact_available
                && has_artifact(artifacts, |item| &item.normalized_document_json),
            // 不仅要求 translations_dir 路径非空，还要求其内容完整（manifest 声明的
            // 每页文件存在且非空）。断电/中断可能留下半个 translated 目录，若仅按路径
            // 存在判定可续跑，resume 会信任残缺产物并产出损坏结果——这里校验失败即
            // 降级到 translate 分支，而不是信任。
            translations_available: source_artifact_available
                && has_artifact(artifacts, |item| &item.translations_dir)
                && translations_dir_intact(job, data_root),
        }
    }
}

/// translations_dir 是否完整：`translation-manifest.json` 存在且可解析，且其中
/// 声明的每个页文件都存在且非空。任一环节失败都视为不完整（吞错降级，绝不报错）。
fn translations_dir_intact(job: &JobSnapshot, data_root: &Path) -> bool {
    let Some(rel) = job
        .artifacts
        .as_ref()
        .and_then(|item| item.translations_dir.as_deref())
    else {
        return false;
    };
    let rel = rel.trim();
    if rel.is_empty() {
        return false;
    }
    let Ok(dir) = resolve_data_path(data_root, rel) else {
        return false;
    };
    let manifest_path = dir.join(TRANSLATION_MANIFEST_FILE_NAME);
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let Some(pages) = manifest.get("pages").and_then(serde_json::Value::as_array) else {
        return false;
    };
    if pages.is_empty() {
        return false;
    }
    pages.iter().all(|page| {
        let Some(path) = page
            .get("path")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return false;
        };
        let path = Path::new(path);
        // 拒绝绝对路径与 `..` 逃逸：门禁只应读 translations_dir 内的文件，
        // 对齐 Python writer 的逃逸防护，防止恶意 manifest 让门禁读取目录外文件。
        if path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return false;
        }
        std::fs::metadata(dir.join(path))
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false)
    })
}

fn has_artifact(
    artifacts: Option<&JobArtifacts>,
    pick: impl Fn(&JobArtifacts) -> &Option<String>,
) -> bool {
    artifacts
        .and_then(|item| pick(item).as_deref())
        .map(str::trim)
        .is_some_and(|item| !item.is_empty())
}
