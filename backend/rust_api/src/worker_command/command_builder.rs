use std::path::Path;

use crate::config::PythonWorkerEntrypointMode;

pub(super) struct CommandBuilder {
    parts: Vec<String>,
}

impl CommandBuilder {
    pub(super) fn new(
        python_bin: &str,
        mode: PythonWorkerEntrypointMode,
        entrypoint: &PythonEntrypoint<'_>,
    ) -> Self {
        // Script mode is `[python, script, ...]`: the script path stays at
        // argv index 1, which `WorkerContract::from_command` depends on.
        // Unbuffered output needs no `-u` flag here because the spawner
        // always sets `PYTHONUNBUFFERED=1` (see `worker_env`).
        let parts = match mode {
            PythonWorkerEntrypointMode::Script => {
                vec![
                    python_bin.to_string(),
                    entrypoint.script_path.to_string_lossy().to_string(),
                ]
            }
            PythonWorkerEntrypointMode::Console => vec![entrypoint.console_command.to_string()],
        };
        Self { parts }
    }

    pub(super) fn arg(&mut self, name: &str, value: impl ToString) {
        self.parts.push(name.to_string());
        self.parts.push(value.to_string());
    }

    pub(super) fn path_arg(&mut self, name: &str, value: &Path) {
        self.arg(name, value.to_string_lossy());
    }

    pub(super) fn finish(self) -> Vec<String> {
        self.parts
    }
}

pub(super) struct PythonEntrypoint<'a> {
    script_path: &'a Path,
    console_command: &'static str,
}

impl<'a> PythonEntrypoint<'a> {
    pub(super) fn new(script_path: &'a Path, console_command: &'static str) -> Self {
        Self {
            script_path,
            console_command,
        }
    }
}
