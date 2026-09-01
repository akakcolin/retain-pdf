use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::models::JobRecord;

pub const LOG_TAIL_LIMIT: usize = 40;

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: T,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            code: 0,
            message: "ok".to_string(),
            data,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatusKind {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

/// Full status state persisted in the `jobs.status_json` blob — the single
/// source of truth for status plus the derived stage/progress/error fields.
/// Legacy rows keep a bare `"succeeded"`-style enum string; both shapes decode
/// through `JobStatusJson` (rows.rs). The five top-level jobs columns are
/// physically retained for compatibility reads but never written anymore.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct JobStatusState {
    pub status: JobStatusKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_current: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_total: Option<i64>,
}

impl JobStatusState {
    pub fn from_job_record(record: &JobRecord) -> Self {
        Self {
            status: record.status.clone(),
            stage: record.stage.clone(),
            stage_detail: record.stage_detail.clone(),
            error: record.error.clone(),
            progress_current: record.progress_current,
            progress_total: record.progress_total,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowKind {
    Book,
    Ocr,
    Translate,
    Render,
}

impl Default for WorkflowKind {
    fn default() -> Self {
        Self::Book
    }
}

impl WorkflowKind {
    pub fn job_api_prefix(&self) -> &'static str {
        match self {
            Self::Ocr => "/api/v1/ocr/jobs",
            Self::Book | Self::Translate | Self::Render => "/api/v1/jobs",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UploadRecord {
    pub upload_id: String,
    pub filename: String,
    pub stored_path: String,
    pub bytes: u64,
    pub page_count: u32,
    pub uploaded_at: String,
    pub developer_mode: bool,
    /// sha256(文件字节),即 documents.document_id;空串表示旧记录未回填
    #[serde(default)]
    pub content_hash: String,
}

#[derive(Debug, Serialize)]
pub struct UploadView {
    pub upload_id: String,
    pub filename: String,
    pub bytes: u64,
    pub page_count: u32,
    pub uploaded_at: String,
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub fn build_job_id() -> String {
    let ts = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let rand = format!("{:06x}", fastrand::u32(..=0xFFFFFF));
    format!("{ts}-{rand}")
}
