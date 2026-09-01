use anyhow::{Context, Result};
use rusqlite::params;

use crate::models::domain::{
    CreateJobInput, JobFailureInfo, JobSnapshot, JobStatusKind, JobStatusState, WorkflowKind,
};

use super::rows::{parse_status_kind, row_to_job_snapshot, JOB_SELECT_SQL, STATUS_JSON_STATUS_EXPR};
use super::{Db, JobProcessRecord};

/// One job row shaped for the /metrics aggregation (see `metrics.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobMetricRow {
    pub job_id: String,
    /// snake_case status (`queued`/`running`/`succeeded`/`failed`/`canceled`).
    pub status: String,
    pub command: Vec<String>,
    /// `runtime_json.renderer` (`render_rs`/`python`); None for non-render jobs
    /// and for jobs started before the field existed.
    pub renderer: Option<String>,
}

impl Db {
    pub fn get_job(&self, job_id: &str) -> Result<JobSnapshot> {
        let conn = self.connect()?;
        let job = conn
            .query_row(
                &format!("{JOB_SELECT_SQL} WHERE jobs.job_id = ?1"),
                params![job_id],
                row_to_job_snapshot,
            )
            .with_context(|| format!("job not found: {job_id}"))?;
        Ok(job)
    }

    pub fn list_jobs(
        &self,
        limit: u32,
        offset: u32,
        status: Option<&JobStatusKind>,
        workflow: Option<&WorkflowKind>,
    ) -> Result<Vec<JobSnapshot>> {
        let conn = self.connect()?;
        let status_filter = status.map(|s| status_str(s.clone()));
        let workflow_json = workflow.map(serde_json::to_string).transpose()?;
        let base_sql = JOB_SELECT_SQL;
        let query = match (status_filter.as_ref(), workflow_json.as_ref()) {
            (Some(_), Some(_)) => format!("{base_sql} WHERE {STATUS_JSON_STATUS_EXPR} = ?1 AND jobs.workflow = ?2 ORDER BY jobs.updated_at DESC LIMIT ?3 OFFSET ?4"),
            (Some(_), None) => format!("{base_sql} WHERE {STATUS_JSON_STATUS_EXPR} = ?1 ORDER BY jobs.updated_at DESC LIMIT ?2 OFFSET ?3"),
            (None, Some(_)) => format!("{base_sql} WHERE jobs.workflow = ?1 ORDER BY jobs.updated_at DESC LIMIT ?2 OFFSET ?3"),
            (None, None) => format!("{base_sql} ORDER BY jobs.updated_at DESC LIMIT ?1 OFFSET ?2"),
        };
        let mut stmt = conn.prepare(&query)?;
        let rows = match (status_filter.as_ref(), workflow_json.as_ref()) {
            (Some(status_filter), Some(workflow_json)) => stmt.query_map(
                params![status_filter, workflow_json, limit as i64, offset as i64],
                row_to_job_snapshot,
            )?,
            (Some(status_filter), None) => stmt.query_map(
                params![status_filter, limit as i64, offset as i64],
                row_to_job_snapshot,
            )?,
            (None, Some(workflow_json)) => stmt.query_map(
                params![workflow_json, limit as i64, offset as i64],
                row_to_job_snapshot,
            )?,
            (None, None) => {
                stmt.query_map(params![limit as i64, offset as i64], row_to_job_snapshot)?
            }
        };
        let mut jobs = Vec::new();
        for row in rows {
            match row {
                Ok(job) => jobs.push(job),
                Err(error) => {
                    eprintln!("[db] skipping malformed job row during list_jobs: {error}");
                }
            }
        }
        Ok(jobs)
    }

    pub fn list_jobs_with_status(&self, status: &JobStatusKind) -> Result<Vec<JobSnapshot>> {
        let conn = self.connect()?;
        let status_filter = status_str(status.clone());
        let query = format!(
            "{JOB_SELECT_SQL} WHERE {STATUS_JSON_STATUS_EXPR} = ?1 ORDER BY jobs.updated_at DESC"
        );
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params![status_filter], row_to_job_snapshot)?;
        let mut jobs = Vec::new();
        for row in rows {
            match row {
                Ok(job) => jobs.push(job),
                Err(error) => {
                    eprintln!(
                        "[db] skipping malformed job row during list_jobs_with_status: {error}"
                    );
                }
            }
        }
        Ok(jobs)
    }

    pub fn delete_job(&self, job_id: &str) -> Result<bool> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM events WHERE job_id = ?1", params![job_id])?;
        let changed = conn.execute("DELETE FROM jobs WHERE job_id = ?1", params![job_id])?;
        Ok(changed > 0)
    }

    pub fn list_job_process_records_with_status(
        &self,
        status: &JobStatusKind,
    ) -> Result<Vec<JobProcessRecord>> {
        let conn = self.connect()?;
        let status_filter = status_str(status.clone());
        let mut stmt = conn.prepare(
            r#"
            SELECT job_id, pid,
                   CASE WHEN json_type(status_json) = 'object'
                        THEN json_extract(status_json, '$.stage') ELSE stage END AS stage,
                   updated_at
            FROM jobs
            WHERE COALESCE(json_extract(status_json, '$.status'), json_extract(status_json, '$')) = ?1
            ORDER BY updated_at DESC
            "#,
        )?;
        let rows = stmt.query_map(params![status_filter], |row| {
            Ok(JobProcessRecord {
                job_id: row.get(0)?,
                pid: row.get::<_, Option<i64>>(1)?.map(|value| value as u32),
                stage: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    /// Raw-DB recovery for a stale `Running` job whose row cannot be parsed
    /// into a `JobSnapshot` (malformed `workflow`/`request_json`). Applies a
    /// surgical UPDATE (status/runtime/failure/pid/finished_at only) and
    /// returns the recovered terminal snapshot so the caller can emit events
    /// on top — closing the replay audit's known-gap for recovery paths.
    pub fn recover_stale_running_job(
        &self,
        job_id: &str,
        detail: &str,
        timestamp: &str,
        failure_category: &str,
        failure_code: &str,
    ) -> Result<JobSnapshot> {
        let conn = self.connect()?;
        let mut current = JobSnapshot::new(job_id.to_string(), CreateJobInput::default(), vec![]);
        current.updated_at = timestamp.to_string();
        current.status = JobStatusKind::Failed;
        current.stage = Some("failed".to_string());
        current.stage_detail = Some("startup stale running job recovered".to_string());
        current.error = Some(detail.to_string());
        current.finished_at = Some(timestamp.to_string());
        current.pid = None;
        current.append_log(&format!("ERROR: {detail}"));
        current.replace_failure_info(Some(JobFailureInfo {
            stage: "startup_recovery".to_string(),
            category: failure_category.to_string(),
            code: None,
            failed_stage: Some("startup_recovery".to_string()),
            failure_code: Some(failure_code.to_string()),
            failure_category: Some("internal".to_string()),
            provider_stage: None,
            provider_code: None,
            summary: "后端启动时回收了遗留 running 任务".to_string(),
            root_cause: Some(detail.to_string()),
            retryable: true,
            upstream_host: None,
            provider: None,
            suggestion: Some("该任务对应的 worker 已不在运行；请重新提交或手动重试".to_string()),
            last_log_line: Some(detail.to_string()),
            raw_excerpt: Some(detail.to_string()),
            raw_error_excerpt: Some(detail.to_string()),
            raw_diagnostic: None,
            ai_diagnostic: None,
        }));
        current.sync_runtime_state();
        let status_json = serde_json::to_string(&JobStatusState::from_job_record(&current.record))?;
        let runtime_json = current
            .runtime
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let failure_json = current
            .failure
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        conn.execute(
            r#"
            UPDATE jobs
            SET status_json = ?1,
                updated_at = ?2,
                finished_at = ?3,
                pid = NULL,
                runtime_json = ?4,
                failure_json = ?5
            WHERE job_id = ?6
            "#,
            params![
                status_json,
                timestamp,
                timestamp,
                runtime_json,
                failure_json,
                job_id,
            ],
        )?;
        Ok(current)
    }

    pub fn count_jobs_with_status(&self, status: &JobStatusKind) -> Result<i64> {
        let conn = self.connect()?;
        let status_filter = status_str(status.clone());
        let count = conn.query_row(
            &format!("SELECT COUNT(*) FROM jobs WHERE {STATUS_JSON_STATUS_EXPR} = ?1"),
            params![status_filter],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(count)
    }

    /// Lightweight per-job rows for the /metrics endpoint. Skips rows with
    /// malformed status/runtime JSON rather than failing the whole query.
    pub fn list_job_metric_rows(&self) -> Result<Vec<JobMetricRow>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT job_id, status_json, command_json, runtime_json FROM jobs",
        )?;
        let rows = stmt.query_map([], |row| {
            let status_json: String = row.get(1)?;
            let command_json: String = row.get(2)?;
            let runtime_json: Option<String> = row.get(3)?;
            let status = parse_status_kind(&status_json)
                .map(status_str)
                .unwrap_or_else(|_| "unknown".to_string());
            let renderer = runtime_json
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|value| value.get("renderer")?.as_str().map(str::to_string));
            Ok(JobMetricRow {
                job_id: row.get(0)?,
                status,
                command: serde_json::from_str(&command_json).unwrap_or_default(),
                renderer,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

pub(super) fn status_str(status: JobStatusKind) -> String {
    match status {
        JobStatusKind::Queued => "queued",
        JobStatusKind::Running => "running",
        JobStatusKind::Succeeded => "succeeded",
        JobStatusKind::Failed => "failed",
        JobStatusKind::Canceled => "canceled",
    }
    .to_string()
}
