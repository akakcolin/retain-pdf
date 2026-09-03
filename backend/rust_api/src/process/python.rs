//! Unified python subprocess builder for the spawn sites.
//!
//! Before this module existed each spawn site hand-rolled its argv, env, and
//! cwd: `upload.rs`/`preview.rs`/`side_by_side.rs` passed no env at all, some
//! sites set `PYTHONUNBUFFERED` and data-root vars manually, and the Console
//! entrypoint mode spawned a PATH-resolved wrapper without configuring PATH.
//! Everything funnels through [`PythonCommand`] so the sites agree on the
//! baseline env and the platform python default.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

pub const DATA_ROOT_ENV: &str = "RUST_API_DATA_ROOT";
pub const OUTPUT_ROOT_ENV: &str = "RUST_API_OUTPUT_ROOT";
pub const LEGACY_OUTPUT_ROOT_ENV: &str = "OUTPUT_ROOT";
pub const PYTHONUNBUFFERED_ENV: &str = "PYTHONUNBUFFERED";

/// Platform-appropriate python binary name for the `PYTHON_BIN` default.
///
/// Modern POSIX systems ship `python3`; only Windows commonly exposes `python`.
pub fn platform_python_bin() -> &'static str {
    if cfg!(windows) {
        "python"
    } else {
        "python3"
    }
}

/// Environment pairs shared by every python worker subprocess.
pub fn worker_env(data_root: &Path, output_root: &Path) -> Vec<(String, String)> {
    vec![
        (
            DATA_ROOT_ENV.to_string(),
            data_root.to_string_lossy().into_owned(),
        ),
        (
            OUTPUT_ROOT_ENV.to_string(),
            output_root.to_string_lossy().into_owned(),
        ),
        (
            LEGACY_OUTPUT_ROOT_ENV.to_string(),
            output_root.to_string_lossy().into_owned(),
        ),
        (PYTHONUNBUFFERED_ENV.to_string(), "1".to_string()),
    ]
}

/// Prefix the directory containing `python_bin` onto `PATH` so pip console-script
/// wrappers (which are installed next to the interpreter) resolve by name.
///
/// Returns `None` when `python_bin` has no directory component (a bare name is
/// already resolved via `PATH`), so callers can treat it as "no fix needed".
pub fn prepend_python_bin_dir_to_path(python_bin: &Path) -> Option<String> {
    let parent = python_bin.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    let existing = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::env::split_paths(&existing).collect();
    paths.insert(0, parent.to_path_buf());
    std::env::join_paths(paths)
        .ok()
        .map(|joined| joined.to_string_lossy().into_owned())
}

/// Non-fatal startup probe: warns (never errors) if the configured python
/// binary is not runnable, so a broken config surfaces at boot instead of as a
/// runtime job 500.
pub async fn probe_python_binary(python_bin: &str) {
    let probe = tokio::process::Command::new(python_bin)
        .args(["-c", "import sys; print(sys.executable)"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match tokio::time::timeout(Duration::from_secs(5), probe).await {
        Ok(Ok(output)) if output.status.success() => {
            tracing::debug!("configured python binary `{python_bin}` probe ok");
        }
        Ok(Ok(output)) => {
            tracing::warn!(
                "configured python binary `{python_bin}` is not runnable (exit {}); jobs that spawn python will fail at runtime",
                output.status
            );
        }
        Ok(Err(err)) => {
            tracing::warn!(
                "configured python binary `{python_bin}` could not be spawned: {err}; jobs that spawn python will fail at runtime"
            );
        }
        Err(_) => {
            tracing::warn!(
                "configured python binary `{python_bin}` probe timed out; jobs that spawn python may fail at runtime"
            );
        }
    }
}

/// Builder for a python subprocess. Defaults `PYTHONUNBUFFERED=1`.
#[derive(Clone, Debug)]
pub struct PythonCommand {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    current_dir: Option<PathBuf>,
    stdout_piped: bool,
    stderr_piped: bool,
}

impl PythonCommand {
    pub fn new(python_bin: impl Into<String>) -> Self {
        Self {
            program: python_bin.into(),
            args: Vec::new(),
            env: vec![(PYTHONUNBUFFERED_ENV.to_string(), "1".to_string())],
            current_dir: None,
            stdout_piped: false,
            stderr_piped: false,
        }
    }

    /// Run a python script file: `python <path> [args...]`.
    pub fn script(mut self, path: &Path) -> Self {
        self.args.push(path.to_string_lossy().into_owned());
        self
    }

    /// Run inline python code: `python -c <code> [args...]`.
    pub fn inline(mut self, code: &str) -> Self {
        self.args.push("-c".to_string());
        self.args.push(code.to_string());
        self
    }

    pub fn arg(mut self, arg: impl AsRef<std::ffi::OsStr>) -> Self {
        self.args.push(arg.as_ref().to_string_lossy().into_owned());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set the child's working directory.
    pub fn current_dir(mut self, path: &Path) -> Self {
        self.current_dir = Some(path.to_path_buf());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Emit `RUST_API_DATA_ROOT` / `RUST_API_OUTPUT_ROOT` / `OUTPUT_ROOT`.
    pub fn data_roots(mut self, data_root: &Path, output_root: &Path) -> Self {
        self.env.push((
            DATA_ROOT_ENV.to_string(),
            data_root.to_string_lossy().into_owned(),
        ));
        self.env.push((
            OUTPUT_ROOT_ENV.to_string(),
            output_root.to_string_lossy().into_owned(),
        ));
        self.env.push((
            LEGACY_OUTPUT_ROOT_ENV.to_string(),
            output_root.to_string_lossy().into_owned(),
        ));
        self
    }

    pub fn stdout_piped(mut self) -> Self {
        self.stdout_piped = true;
        self
    }

    pub fn stderr_piped(mut self) -> Self {
        self.stderr_piped = true;
        self
    }

    pub fn to_std_command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        self.apply(&mut command);
        command
    }

    pub fn to_tokio_command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.program);
        self.apply(&mut command);
        command
    }

    fn apply(&self, command: &mut impl CommandSink) {
        for arg in &self.args {
            command.arg(arg);
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        if let Some(dir) = &self.current_dir {
            command.current_dir(dir);
        }
        if self.stdout_piped {
            command.stdout_piped();
        }
        if self.stderr_piped {
            command.stderr_piped();
        }
    }
}

trait CommandSink {
    fn arg(&mut self, value: &str);
    fn env(&mut self, key: &str, value: &str);
    fn current_dir(&mut self, path: &Path);
    fn stdout_piped(&mut self);
    fn stderr_piped(&mut self);
}

impl CommandSink for std::process::Command {
    fn arg(&mut self, value: &str) {
        self.arg(value);
    }

    fn env(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }

    fn current_dir(&mut self, path: &Path) {
        self.current_dir(path);
    }

    fn stdout_piped(&mut self) {
        self.stdout(Stdio::piped());
    }

    fn stderr_piped(&mut self) {
        self.stderr(Stdio::piped());
    }
}

impl CommandSink for tokio::process::Command {
    fn arg(&mut self, value: &str) {
        self.arg(value);
    }

    fn env(&mut self, key: &str, value: &str) {
        self.env(key, value);
    }

    fn current_dir(&mut self, path: &Path) {
        self.current_dir(path);
    }

    fn stdout_piped(&mut self) {
        self.stdout(Stdio::piped());
    }

    fn stderr_piped(&mut self) {
        self.stderr(Stdio::piped());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn env_has(env: &[(String, String)], key: &str, value: &str) -> bool {
        env.iter().any(|(k, v)| k == key && v == value)
    }

    #[test]
    fn script_mode_argv_and_default_unbuffered() {
        let built = PythonCommand::new("python3")
            .script(Path::new("/tmp/run.py"))
            .arg("--spec")
            .arg("/tmp/s.json");
        assert_eq!(built.program, "python3");
        assert_eq!(built.args, vec!["/tmp/run.py", "--spec", "/tmp/s.json"]);
        assert!(env_has(&built.env, PYTHONUNBUFFERED_ENV, "1"));
    }

    #[test]
    fn inline_mode_argv() {
        let built = PythonCommand::new("python3").inline("print(1)");
        assert_eq!(built.args, vec!["-c", "print(1)"]);
    }

    #[test]
    fn data_roots_env_sets_three_pairs() {
        let built = PythonCommand::new("python3").data_roots(Path::new("/d"), Path::new("/o"));
        assert!(env_has(&built.env, DATA_ROOT_ENV, "/d"));
        assert!(env_has(&built.env, OUTPUT_ROOT_ENV, "/o"));
        assert!(env_has(&built.env, LEGACY_OUTPUT_ROOT_ENV, "/o"));
    }

    #[test]
    fn current_dir_and_piped_flags() {
        let built = PythonCommand::new("python3")
            .current_dir(Path::new("/cwd"))
            .stdout_piped()
            .stderr_piped();
        assert_eq!(built.current_dir.as_deref(), Some(Path::new("/cwd")));
        assert!(built.stdout_piped);
        assert!(built.stderr_piped);
    }

    #[test]
    fn to_std_command_applies_program_args_env_and_cwd() {
        let command = PythonCommand::new("python3")
            .script(Path::new("/tmp/run.py"))
            .current_dir(Path::new("/cwd"))
            .to_std_command();
        assert_eq!(command.get_program(), "python3");
        let args: Vec<&OsStr> = command.get_args().collect();
        assert_eq!(args, vec![OsStr::new("/tmp/run.py")]);
        let envs: Vec<(&OsStr, Option<&OsStr>)> = command.get_envs().collect();
        assert!(envs
            .iter()
            .any(|(k, v)| *k == OsStr::new(PYTHONUNBUFFERED_ENV) && *v == Some(OsStr::new("1"))));
        assert_eq!(
            command.get_current_dir().map(|p| p.to_path_buf()),
            Some(PathBuf::from("/cwd"))
        );
    }

    #[test]
    fn platform_python_bin_matches_os() {
        #[cfg(not(windows))]
        assert_eq!(platform_python_bin(), "python3");
        #[cfg(windows)]
        assert_eq!(platform_python_bin(), "python");
    }

    #[test]
    fn worker_env_has_four_pairs() {
        let env = worker_env(Path::new("/d"), Path::new("/o"));
        assert_eq!(env.len(), 4);
        assert!(env_has(&env, DATA_ROOT_ENV, "/d"));
        assert!(env_has(&env, OUTPUT_ROOT_ENV, "/o"));
        assert!(env_has(&env, LEGACY_OUTPUT_ROOT_ENV, "/o"));
        assert!(env_has(&env, PYTHONUNBUFFERED_ENV, "1"));
    }

    #[test]
    fn prepend_absolute_python_path_puts_parent_first() {
        let value = prepend_python_bin_dir_to_path(Path::new("/opt/venv/bin/python"))
            .expect("prepend absolute path");
        assert!(
            value.starts_with("/opt/venv/bin"),
            "parent dir should be prepended: {value}"
        );
    }

    #[test]
    fn prepend_bare_python_name_returns_none() {
        assert!(prepend_python_bin_dir_to_path(Path::new("python3")).is_none());
    }
}
