#[cfg(unix)]
use std::io;
use std::path::Path;
#[cfg(windows)]
use std::process::Command as StdCommand;
use std::process::Stdio;
use std::time::Instant;

#[cfg(windows)]
use anyhow::anyhow;
use anyhow::{Context, Result};
use tokio::process::{Child, Command};
use tokio::time::{sleep, Duration};

use crate::config::{PythonWorkerEntrypointMode, WorkerProcessRuntimeConfig};
use crate::models::domain::JobRuntimeState;
use crate::ocr_provider::{provider_token, provider_token_env_name, require_supported_provider};
use crate::process::python::{prepend_python_bin_dir_to_path, worker_env};

pub(super) fn spawn_worker_process(
    config: &WorkerProcessRuntimeConfig<'_>,
    job: &JobRuntimeState,
) -> Result<Child> {
    let mut command = Command::new(&job.command[0]);
    command
        .args(&job.command[1..])
        .envs(worker_env(config.data_root, config.output_root))
        .current_dir(config.project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if config.python_entrypoint_mode == PythonWorkerEntrypointMode::Console {
        // Console entrypoints are pip console-script wrappers resolved by name,
        // so the interpreter's own directory must be reachable on PATH.
        if let Some(path) = prepend_python_bin_dir_to_path(Path::new(config.python_bin)) {
            command.env("PATH", path);
        }
    }
    apply_job_credentials(&mut command, job);
    configure_child_process(&mut command);

    let program = job.command.first().cloned().unwrap_or_default();
    command
        .spawn()
        .with_context(|| format!("failed to spawn python worker: {program}"))
}

/// Resolve the renderer label recorded on a render job's runtime info.
/// Rendering is native-only, so every Render contract is `render_rs`;
/// non-render workers yield None.
pub(super) fn renderer_label(command: &[String]) -> Option<&'static str> {
    use super::process_contract::WorkerContract;
    if WorkerContract::from_command(command) != WorkerContract::Render {
        return None;
    }
    Some("render_rs")
}

fn apply_job_credentials(command: &mut Command, job: &JobRuntimeState) {
    if !job.request_payload.translation.api_key.trim().is_empty() {
        command.env(
            "RETAIN_TRANSLATION_API_KEY",
            job.request_payload.translation.api_key.trim(),
        );
    }
    if let Ok(provider_kind) = require_supported_provider(&job.request_payload.ocr.provider) {
        let token = provider_token(&provider_kind, &job.request_payload.ocr);
        if !token.is_empty() {
            if let Some(env_name) = provider_token_env_name(&provider_kind) {
                command.env(env_name, token);
            }
        }
    }
}

#[cfg(unix)]
fn configure_child_process(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn configure_child_process(_command: &mut Command) {}

/// Checks whether a process with the given pid is still alive.
///
/// Uses `kill(pid, 0)` (POSIX signal 0), which sends no signal but still
/// performs existence/permission checks: it returns success (or `EPERM`,
/// meaning the process exists but is owned by someone else) when the pid is
/// alive, and `ESRCH` when it is not. This works identically on Linux and
/// macOS, unlike checking for a `/proc/{pid}` entry (macOS has no `/proc`,
/// so that check always reported processes as dead).
#[cfg(unix)]
pub(crate) fn worker_process_exists(pid: u32) -> bool {
    let pid = pid as libc::pid_t;
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    // EPERM means the process exists (owned by someone else); ESRCH means
    // no such process. Any other errno is treated conservatively as "does
    // not exist" so we don't get stuck if something else goes wrong.
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub(crate) fn worker_process_exists(_pid: u32) -> bool {
    false
}

pub async fn terminate_job_process_tree(
    pid: u32,
    grace_secs: u64,
    poll_interval_ms: u64,
) -> Result<()> {
    #[cfg(windows)]
    {
        terminate_job_process_tree_windows(pid)
    }

    #[cfg(unix)]
    {
        let group_pid = -(pid as i32);
        let deadline = Instant::now() + Duration::from_secs(grace_secs);
        let poll_interval = Duration::from_millis(poll_interval_ms);
        let _ = unsafe { libc::kill(group_pid, libc::SIGTERM) };
        while Instant::now() < deadline {
            if !worker_process_exists(pid) {
                return Ok(());
            }
            sleep(poll_interval).await;
        }
        let _ = unsafe { libc::kill(group_pid, libc::SIGKILL) };
        Ok(())
    }
}

/// Synchronous counterpart to [`terminate_job_process_tree`] for callers
/// that run before/outside the async runtime (e.g. startup state
/// reconciliation). Sends SIGTERM to the process group, polls for exit with
/// a blocking sleep, and escalates to SIGKILL once the grace period elapses.
pub(crate) fn terminate_job_process_tree_blocking(
    pid: u32,
    grace_secs: u64,
    poll_interval_ms: u64,
) -> Result<()> {
    #[cfg(windows)]
    {
        terminate_job_process_tree_windows(pid)
    }

    #[cfg(unix)]
    {
        let group_pid = -(pid as i32);
        let deadline = Instant::now() + Duration::from_secs(grace_secs);
        let poll_interval = Duration::from_millis(poll_interval_ms);
        let _ = unsafe { libc::kill(group_pid, libc::SIGTERM) };
        while Instant::now() < deadline {
            if !worker_process_exists(pid) {
                return Ok(());
            }
            std::thread::sleep(poll_interval);
        }
        let _ = unsafe { libc::kill(group_pid, libc::SIGKILL) };
        Ok(())
    }
}

#[cfg(windows)]
fn terminate_job_process_tree_windows(pid: u32) -> Result<()> {
    let status = StdCommand::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to invoke taskkill")?;
    if status.success() {
        return Ok(());
    }
    Err(anyhow!("taskkill failed for pid={pid}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::renderer_label;
    use super::worker_process_exists;

    #[test]
    fn renderer_label_marks_render_contracts_as_render_rs() {
        let render_rs_cmd = vec![
            "/opt/bin/render_rs".to_string(),
            "--spec".to_string(),
            "spec.json".to_string(),
        ];
        assert_eq!(renderer_label(&render_rs_cmd), Some("render_rs"));
    }

    #[test]
    fn renderer_label_none_for_non_render_workers() {
        let normalize_cmd = vec![
            "/opt/bin/python3".to_string(),
            "run_translate_only.py".to_string(),
        ];
        assert_eq!(renderer_label(&normalize_cmd), None);
        let unknown_cmd = vec!["/opt/bin/python3".to_string(), "custom.py".to_string()];
        assert_eq!(renderer_label(&unknown_cmd), None);
    }

    #[test]
    fn worker_process_exists_true_for_current_process() {
        // The current process is always alive, and this must work without
        // /proc (e.g. on macOS), so it's a direct regression test for the
        // `kill(pid, 0)`-based existence check.
        assert!(worker_process_exists(std::process::id()));
    }

    #[test]
    fn worker_process_exists_false_for_absurd_pid() {
        // 999_999 is well above the default max pid on both Linux and
        // macOS and matches the value used by the state_recovery
        // "dead pid" tests, so it's exceedingly unlikely to collide with a
        // real running process in CI.
        assert!(!worker_process_exists(999_999));
    }
}
