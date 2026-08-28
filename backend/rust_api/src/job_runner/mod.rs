use crate::models::domain::{JobRuntimeState, JobSnapshot};

mod artifact_requirements;
mod cancel_registry;
mod execution_queue;
mod lifecycle;
mod ocr_flow;
mod pipeline_plan;
pub(crate) mod process_contract;
mod process_runner;
mod render_flow;
mod render_flow_artifacts;
mod runtime_deps;
mod runtime_state;
mod stage_contract;
mod stdout_parser;
mod translation_flow;
mod worker_process;

pub(crate) use cancel_registry::{clear_cancel_request_with_registry, request_cancel_with_registry};
pub(crate) use lifecycle::spawn_job;
pub(crate) use process_runner::execute_process_job;
pub(crate) use runtime_deps::{JobPersistDeps, ProcessRuntimeDeps};
pub(crate) use runtime_state::{
    attach_job_paths, attach_job_provider_failure, clear_canceled_runtime_artifacts,
    clear_job_failure, job_artifacts_mut, ocr_provider_diagnostics_mut, refresh_job_failure,
    register_job_retry, sync_runtime_state,
};
pub use worker_process::terminate_job_process_tree;
pub(crate) use worker_process::{terminate_job_process_tree_blocking, worker_process_exists};

pub(crate) fn format_error_chain(err: &anyhow::Error) -> String {
    let causes: Vec<String> = err
        .chain()
        .map(|cause| cause.to_string().trim().to_string())
        .filter(|cause| !cause.is_empty())
        .collect();
    if causes.is_empty() {
        return "unknown error".to_string();
    }
    if causes.len() == 1 {
        return causes[0].clone();
    }
    let mut message = causes[0].clone();
    message.push_str("\nCaused by:");
    for cause in causes.iter().skip(1) {
        message.push_str("\n- ");
        message.push_str(cause);
    }
    message
}

pub(crate) trait LogAppend {
    fn push_job_log(&mut self, line: &str);
}

impl LogAppend for JobSnapshot {
    fn push_job_log(&mut self, line: &str) {
        self.append_log(line);
    }
}

impl LogAppend for JobRuntimeState {
    fn push_job_log(&mut self, line: &str) {
        self.append_log(line);
    }
}

fn append_error_chain_log<T: LogAppend>(job: &mut T, err: &anyhow::Error) {
    for (idx, cause) in err.chain().enumerate() {
        let text = cause.to_string().trim().to_string();
        if text.is_empty() {
            continue;
        }
        if idx == 0 {
            job.push_job_log(&format!("ERROR: {text}"));
        } else {
            job.push_job_log(&format!("CAUSE[{idx}]: {text}"));
        }
    }
}
