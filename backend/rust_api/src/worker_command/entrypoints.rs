use crate::config::WorkerCommandRuntimeConfig;
use std::path::Path;

use super::command_builder::{CommandBuilder, PythonEntrypoint};

#[cfg(test)]
pub(super) fn provider_case_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    let mut cmd = CommandBuilder::new(
        config.python_bin,
        config.python_entrypoint_mode,
        &PythonEntrypoint::new(
            config.run_provider_case_script,
            "retainpdf-run-provider-case",
        ),
        true,
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}

pub(super) fn provider_ocr_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    let mut cmd = CommandBuilder::new(
        config.python_bin,
        config.python_entrypoint_mode,
        &PythonEntrypoint::new(config.run_provider_ocr_script, "retainpdf-run-provider-ocr"),
        true,
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}

pub(super) fn translate_only_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    let mut cmd = CommandBuilder::new(
        config.python_bin,
        config.python_entrypoint_mode,
        &PythonEntrypoint::new(
            config.run_translate_only_script,
            "retainpdf-run-translate-only",
        ),
        true,
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}

pub(super) fn render_only_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    if orchestrator_gate_enabled() {
        return render_rs_command(spec_path);
    }
    let mut cmd = CommandBuilder::new(
        config.python_bin,
        config.python_entrypoint_mode,
        &PythonEntrypoint::new(config.run_render_only_script, "retainpdf-run-render-only"),
        true,
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}

/// Test-gated C1 switch (not wired to production config): when
/// `RETAINPDF_RENDER_ORCHESTRATOR_RS=1` the render stage is delegated to the
/// native `render_rs` orchestrator instead of `python3 run_render_only.py`.
/// The binary comes from `RETAIN_PDF_RENDER_RS_BIN` (default `render_rs` on
/// PATH); it rejects non-typst render modes with a clear exit.
const RENDER_ORCHESTRATOR_GATE_ENV: &str = "RETAINPDF_RENDER_ORCHESTRATOR_RS";
const RENDER_RS_BIN_ENV: &str = "RETAIN_PDF_RENDER_RS_BIN";

fn orchestrator_gate_enabled() -> bool {
    std::env::var(RENDER_ORCHESTRATOR_GATE_ENV).as_deref() == Ok("1")
}

fn render_rs_command(spec_path: &Path) -> Vec<String> {
    let bin = std::env::var(RENDER_RS_BIN_ENV).unwrap_or_else(|_| "render_rs".to_string());
    vec![bin, "--spec".to_string(), spec_path.to_string_lossy().into_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_rs_command_shape() {
        let cmd = render_rs_command(Path::new("/tmp/spec.json"));
        assert_eq!(cmd[0], "render_rs");
        assert_eq!(cmd[1], "--spec");
        assert_eq!(cmd[2], "/tmp/spec.json");
    }
}

pub(super) fn extract_text_layer_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    let mut cmd = CommandBuilder::new(
        config.python_bin,
        config.python_entrypoint_mode,
        &PythonEntrypoint::new(
            config.run_extract_text_layer_script,
            "retainpdf-run-extract-text-layer",
        ),
        false,
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}

pub(super) fn normalize_ocr_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    let mut cmd = CommandBuilder::new(
        config.python_bin,
        config.python_entrypoint_mode,
        &PythonEntrypoint::new(
            config.run_normalize_ocr_script,
            "retainpdf-run-normalize-ocr",
        ),
        false,
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}
