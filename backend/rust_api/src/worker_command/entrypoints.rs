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
    let mut cmd = CommandBuilder::new(
        config.python_bin,
        config.python_entrypoint_mode,
        &PythonEntrypoint::new(config.run_render_only_script, "retainpdf-run-render-only"),
        true,
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
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
