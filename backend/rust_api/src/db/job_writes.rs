use anyhow::Result;
use rusqlite::params;

use crate::models::domain::{JobArtifactRecord, JobSnapshot, JobStatusState};
use crate::storage_paths::{collect_job_artifact_entries, normalize_job_paths_for_storage};

use super::Db;

impl Db {
    pub fn save_job(&self, job: &JobSnapshot) -> Result<()> {
        let mut stored_job = job.clone();
        stored_job.sync_runtime_state();
        normalize_job_paths_for_storage(&self.data_root, &mut stored_job)?;
        let artifacts_json = stored_job
            .artifacts
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let artifact_entries = collect_job_artifact_entries(&stored_job, &self.data_root)?;
        let runtime_json = stored_job
            .runtime
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let failure_json = stored_job
            .failure
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let status_json =
            serde_json::to_string(&JobStatusState::from_job_record(&stored_job.record))?;
        tx.execute(
            r#"
            INSERT INTO jobs (
                job_id, workflow, status_json, created_at, updated_at, started_at, finished_at,
                upload_id, pid, command_json, request_json,
                log_tail_json, result_json, runtime_json, failure_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(job_id) DO UPDATE SET
                workflow=excluded.workflow,
                status_json=excluded.status_json,
                created_at=excluded.created_at,
                updated_at=excluded.updated_at,
                started_at=excluded.started_at,
                finished_at=excluded.finished_at,
                upload_id=excluded.upload_id,
                pid=excluded.pid,
                command_json=excluded.command_json,
                request_json=excluded.request_json,
                log_tail_json=excluded.log_tail_json,
                result_json=excluded.result_json,
                runtime_json=excluded.runtime_json,
                failure_json=excluded.failure_json
            "#,
            params![
                stored_job.job_id,
                serde_json::to_string(&stored_job.workflow)?,
                status_json,
                stored_job.created_at,
                stored_job.updated_at,
                stored_job.started_at,
                stored_job.finished_at,
                stored_job.upload_id,
                stored_job.pid.map(|v| v as i64),
                serde_json::to_string(&stored_job.command)?,
                serde_json::to_string(&stored_job.request_payload)?,
                serde_json::to_string(&stored_job.log_tail)?,
                serde_json::to_string(&stored_job.result)?,
                runtime_json,
                failure_json,
            ],
        )?;
        persist_job_artifacts(&tx, &stored_job.job_id, artifacts_json, &artifact_entries)?;
        tx.commit()?;
        Ok(())
    }
}

fn persist_job_artifacts(
    tx: &rusqlite::Transaction<'_>,
    job_id: &str,
    artifacts_json: Option<String>,
    artifact_entries: &[JobArtifactRecord],
) -> Result<()> {
    if let Some(artifacts_json) = artifacts_json {
        tx.execute(
            r#"
            INSERT INTO artifacts (job_id, artifacts_json)
            VALUES (?1, ?2)
            ON CONFLICT(job_id) DO UPDATE SET
                artifacts_json=excluded.artifacts_json
            "#,
            params![job_id, artifacts_json],
        )?;
    } else {
        tx.execute("DELETE FROM artifacts WHERE job_id = ?1", params![job_id])?;
    }
    tx.execute(
        "DELETE FROM job_artifact_entries WHERE job_id = ?1",
        params![job_id],
    )?;
    for item in artifact_entries {
        tx.execute(
            r#"
            INSERT INTO job_artifact_entries (
                job_id, artifact_key, artifact_group, artifact_kind, relative_path,
                file_name, content_type, ready, size_bytes, checksum, source_stage,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                item.job_id,
                item.artifact_key,
                item.artifact_group,
                item.artifact_kind,
                item.relative_path,
                item.file_name,
                item.content_type,
                if item.ready { 1 } else { 0 },
                item.size_bytes.map(|value| value as i64),
                item.checksum,
                item.source_stage,
                item.created_at,
                item.updated_at,
            ],
        )?;
    }
    Ok(())
}
