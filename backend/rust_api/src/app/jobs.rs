use std::sync::Arc;

use tracing::warn;

use crate::job_runner::{spawn_job, ProcessRuntimeDeps};
use crate::services::job_launcher::JobLaunchDeps;
use crate::services::jobs::{
    build_jobs_facade, CommandJobsDeps, ControlDeps, JobSubmitDeps, JobsFacade, QueryJobsDeps,
    ReplayDeps, SnapshotBuildDeps, UploadStoreDeps,
};
use crate::services::runtime_gateway::JobRuntimeLauncher;

use super::state::AppState;

fn build_process_runtime_deps(state: &AppState) -> ProcessRuntimeDeps {
    ProcessRuntimeDeps::new(
        state.config.clone(),
        state.db.clone(),
        state.canceled_jobs.clone(),
        state.job_slots.clone(),
    )
}

/// Requeue and immediately re-drive queued jobs stranded by a previous run.
///
/// Startup-only: call this once, from the real server boot path and not from
/// `build_state` (which many unit tests call directly against fixtures). See
/// `state_recovery::requeue_stuck_queued_jobs` for why re-driving is safe only
/// at startup.
pub fn requeue_stuck_queued_jobs_at_startup(state: &AppState) -> usize {
    match super::state_recovery::requeue_stuck_queued_jobs(&state.config, &state.db) {
        Ok(job_ids) => {
            let requeued = job_ids.len();
            for job_id in job_ids {
                spawn_job(build_process_runtime_deps(state), job_id);
            }
            requeued
        }
        Err(error) => {
            warn!("startup failed to requeue stuck queued jobs: {error:#}");
            0
        }
    }
}

pub fn build_jobs_facade_from_state(state: &AppState) -> JobsFacade<'_> {
    let runtime_state = state.clone();
    let launcher = JobLaunchDeps::new(
        state.db.as_ref(),
        &state.config.data_root,
        &state.config.output_root,
        JobRuntimeLauncher::new(Arc::new(move |job_id| {
            spawn_job(build_process_runtime_deps(&runtime_state), job_id)
        })),
    );
    let snapshot = SnapshotBuildDeps::new(state.db.as_ref(), state.config.job_snapshot_runtime());
    let uploads = UploadStoreDeps::new(
        state.db.as_ref(),
        &state.config.uploads_dir,
        state.config.upload_max_bytes,
        state.config.upload_max_pages,
        state.config.upload_max_complexity,
        &state.config.render_rs_bin,
    );
    let submit = JobSubmitDeps::new(snapshot, uploads, launcher);
    let control = ControlDeps::new(
        state.db.as_ref(),
        &state.config.job_runner,
        &state.config.data_root,
        &state.config.output_root,
        &state.canceled_jobs,
    );
    let replay = ReplayDeps::new(
        &state.config.project_root,
        &state.config.scripts_dir,
        &state.config.python_bin,
        &state.config.data_root,
        &state.config.render_rs_bin,
    );
    build_jobs_facade(
        CommandJobsDeps::new(state.db.as_ref(), submit, control),
        QueryJobsDeps::new(
            state.db.as_ref(),
            &state.config.data_root,
            &state.config.downloads_dir,
            &state.downloads_lock,
            replay,
        ),
    )
}
