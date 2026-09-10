use anyhow::Result;
use tracing::warn;

use crate::config::AppConfig;
use crate::db::Db;
use crate::job_events::{emit_job_events_with_previous, persist_job_with_resources};
use crate::job_runner::{terminate_job_process_tree_blocking, worker_process_exists};
use crate::models::domain::{now_iso, JobFailureInfo, JobStatusKind};

/// Why a `Running`-status job found at startup is being reconciled.
///
/// The pid recorded for a `Running` job is only ever written by us right
/// before spawning that job's worker (and cleared once it finishes), and the
/// worker is placed in its own process group via `setpgid` at spawn time
/// (see `configure_child_process`), so terminating `-pid` only reaches the
/// process(es) that job itself spawned. There's still a theoretical
/// PID-reuse race (the recorded pid could have exited and been recycled by
/// an unrelated process before we get here), but the process-group scoping
/// keeps the blast radius of a false-positive kill limited to that reused
/// pid's own group rather than anything else on the system, and this only
/// runs once at startup against jobs the DB itself says were left running.
enum StaleReason {
    /// No pid was ever recorded for this job.
    NoPid,
    /// The recorded pid is no longer alive.
    Dead(u32),
    /// The recorded pid is still alive: the worker was orphaned by an
    /// unclean shutdown/restart and is left running detached, with nothing
    /// left to consume its stdout or ever mark the job finished.
    Orphaned(u32),
}

/// Re-drive queued jobs that no driver task owns any more.
///
/// Startup-only contract: the job runtime is in-process (see
/// `app::jobs::build_jobs_facade_from_state`), so right after a (re)start no
/// task exists for any queued job and every id returned here is safe to
/// re-drive exactly once. This is the queued counterpart to
/// [`reconcile_stale_running_jobs`] — without it a job that was persisted but
/// never launched stays `queued` forever. A queued job whose recorded worker
/// pid is still alive is left alone: that worker outlived the API and still
/// owns the job.
///
/// Never call this outside startup: a live driver may own the job and a
/// second driver would duplicate execution.
pub(super) fn requeue_stuck_queued_jobs(config: &AppConfig, db: &Db) -> Result<Vec<String>> {
    let queued = db.list_job_process_records_with_status(&JobStatusKind::Queued)?;
    let timestamp = now_iso();
    let mut requeued = Vec::new();
    for record in queued {
        if let Some(pid) = record.pid.filter(|pid| worker_process_exists(*pid)) {
            warn!(
                "startup found queued job {} with live worker pid {pid}; leaving it to that worker",
                record.job_id
            );
            continue;
        }
        match db.get_job(&record.job_id) {
            Ok(mut job) => {
                job.updated_at = timestamp.clone();
                job.stage_detail =
                    Some("启动时发现无人驱动的排队任务，已重新入队自动续跑".to_string());
                job.append_log(
                    "WARN: startup found queued job with no driver; requeued for automatic resume",
                );
                job.sync_runtime_state();
                if let Err(error) =
                    persist_job_with_resources(db, &config.data_root, &config.output_root, &job)
                {
                    warn!(
                        "startup failed to mark requeued job {}: {error:#}",
                        record.job_id
                    );
                }
            }
            Err(error) => {
                warn!(
                    "startup found stuck queued job {} but failed to load it: {error:#}",
                    record.job_id
                );
            }
        }
        requeued.push(record.job_id);
    }
    if !requeued.is_empty() {
        warn!(
            "startup reconciliation requeued {} stuck queued job(s)",
            requeued.len()
        );
    }
    Ok(requeued)
}

pub(super) fn reconcile_stale_running_jobs(config: &AppConfig, db: &Db) -> Result<usize> {
    let running_jobs = db.list_job_process_records_with_status(&JobStatusKind::Running)?;
    let mut reconciled = 0usize;
    for job_record in running_jobs {
        let reason = match job_record.pid {
            Some(pid) if worker_process_exists(pid) => StaleReason::Orphaned(pid),
            Some(pid) => StaleReason::Dead(pid),
            None => StaleReason::NoPid,
        };

        if let StaleReason::Orphaned(pid) = reason {
            warn!(
                "startup found live orphaned worker process pid={pid} for job {} still running; terminating its process tree before recovering job state",
                job_record.job_id
            );
            if let Err(error) = terminate_job_process_tree_blocking(
                pid,
                config.job_runner.worker_terminate_grace_secs,
                config.job_runner.worker_terminate_poll_ms,
            ) {
                warn!(
                    "failed to terminate orphaned worker process pid={pid} for job {}: {error:#}",
                    job_record.job_id
                );
            }
        }

        let (detail, failure_category, failure_code) = match reason {
            StaleReason::Orphaned(pid) => (
                format!(
                    "后端启动时发现遗留 running 任务，worker 进程 {pid} 仍在运行（孤儿进程），已终止该进程"
                ),
                "worker_orphaned_after_restart",
                "worker_orphaned_after_restart",
            ),
            StaleReason::Dead(pid) => (
                format!("后端启动时发现遗留 running 任务，但 worker 进程 {pid} 已不存在"),
                "worker_process_missing",
                "worker_process_missing",
            ),
            StaleReason::NoPid => (
                "后端启动时发现遗留 running 任务，但未记录 worker pid".to_string(),
                "worker_process_missing",
                "worker_process_missing",
            ),
        };
        let timestamp = now_iso();
        match db.get_job(&job_record.job_id) {
            Ok(mut job) => {
                job.append_log(&format!("ERROR: {detail}"));
                job.status = JobStatusKind::Failed;
                job.stage = Some("failed".to_string());
                job.stage_detail = Some("startup stale running job recovered".to_string());
                job.error = Some(detail.clone());
                job.updated_at = timestamp.clone();
                job.finished_at = Some(timestamp.clone());
                job.pid = None;
                job.sync_runtime_state();
                job.replace_failure_info(Some(JobFailureInfo {
                    stage: "startup_recovery".to_string(),
                    category: failure_category.to_string(),
                    code: None,
                    failed_stage: Some("startup_recovery".to_string()),
                    failure_code: Some(failure_code.to_string()),
                    failure_category: Some("internal".to_string()),
                    provider_stage: None,
                    provider_code: None,
                    summary: "后端启动时回收了遗留 running 任务".to_string(),
                    root_cause: Some(detail.clone()),
                    retryable: true,
                    upstream_host: None,
                    provider: None,
                    suggestion: Some(
                        "该任务对应的 worker 已不在运行；请重新提交或手动重试".to_string(),
                    ),
                    last_log_line: Some(detail.clone()),
                    raw_excerpt: Some(detail.clone()),
                    raw_error_excerpt: Some(detail.clone()),
                    raw_diagnostic: None,
                    ai_diagnostic: None,
                }));
                persist_job_with_resources(db, &config.data_root, &config.output_root, &job)?;
            }
            Err(error) => {
                warn!(
                    "startup reconciliation fell back to raw DB recovery for {}: {}",
                    job_record.job_id, error
                );
                let recovered = db.recover_stale_running_job(
                    &job_record.job_id,
                    &detail,
                    &timestamp,
                    failure_category,
                    failure_code,
                )?;
                // The stored row was unparseable, so emit events from a
                // synthetic running→failed transition instead of the real
                // previous snapshot. This keeps the replay audit consistent
                // (terminal event present) rather than a known-gap.
                let mut previous = recovered.clone();
                previous.status = JobStatusKind::Running;
                previous.stage = job_record.stage.clone();
                previous.stage_detail = None;
                previous.error = None;
                previous.failure = None;
                previous.finished_at = None;
                previous.pid = job_record.pid;
                emit_job_events_with_previous(
                    db,
                    &config.data_root,
                    &config.output_root,
                    &previous,
                    &recovered,
                );
            }
        }
        reconciled += 1;
        warn!(
            "recovered stale running job during startup: {}",
            job_record.job_id
        );
    }
    if reconciled > 0 {
        warn!("startup reconciliation recovered {reconciled} stale running job(s)");
    }
    Ok(reconciled)
}
