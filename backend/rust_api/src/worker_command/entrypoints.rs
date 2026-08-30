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
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}

pub(super) fn render_only_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
    render_mode: &str,
) -> Vec<String> {
    let force_on = std::env::var(RENDER_ORCHESTRATOR_FORCE_ON_ENV).as_deref() == Ok("1");
    let force_off = std::env::var(RENDER_ORCHESTRATOR_FORCE_OFF_ENV).as_deref() == Ok("1");
    if should_route_render_rs(render_mode, force_on, force_off) {
        return render_rs_command(config, spec_path);
    }
    let mut cmd = CommandBuilder::new(
        config.python_bin,
        config.python_entrypoint_mode,
        &PythonEntrypoint::new(config.run_render_only_script, "retainpdf-run-render-only"),
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}

/// C3 production takeover: typst/typst_visual/overlay/dual/auto renders run
/// through the native `render_rs` orchestrator by default (the delegate
/// resolves `auto` and rejects anything else). `RETAINPDF_RENDER_ORCHESTRATOR_RS=1`
/// (legacy C1 test gate) forces native for any mode; `RETAINPDF_RENDER_ORCHESTRATOR_OFF=1`
/// forces the python flow.
const RENDER_ORCHESTRATOR_FORCE_ON_ENV: &str = "RETAINPDF_RENDER_ORCHESTRATOR_RS";
const RENDER_ORCHESTRATOR_FORCE_OFF_ENV: &str = "RETAINPDF_RENDER_ORCHESTRATOR_OFF";

fn should_route_render_rs(render_mode: &str, force_on: bool, force_off: bool) -> bool {
    if force_off {
        return false;
    }
    if force_on {
        return true;
    }
    matches!(render_mode, "typst" | "typst_visual" | "overlay" | "dual" | "auto")
}

fn render_rs_command(config: &WorkerCommandRuntimeConfig<'_>, spec_path: &Path) -> Vec<String> {
    let bin = config.render_rs_bin.to_string_lossy().into_owned();
    vec![bin, "--spec".to_string(), spec_path.to_string_lossy().into_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PythonWorkerEntrypointMode;

    fn test_command_config(render_rs_bin: &Path) -> WorkerCommandRuntimeConfig<'_> {
        let scripts = Path::new("/tmp/scripts");
        WorkerCommandRuntimeConfig {
            python_bin: "python",
            python_entrypoint_mode: PythonWorkerEntrypointMode::Script,
            run_provider_case_script: scripts,
            run_provider_ocr_script: scripts,
            run_normalize_ocr_script: scripts,
            run_extract_text_layer_script: scripts,
            run_translate_only_script: scripts,
            run_render_only_script: scripts,
            render_rs_bin,
            render_rs_delegate_script: scripts,
        }
    }

    #[test]
    fn render_rs_command_shape() {
        let cmd = render_rs_command(&test_command_config(Path::new("/opt/bin/render_rs")), Path::new("/tmp/spec.json"));
        assert_eq!(cmd[0], "/opt/bin/render_rs");
        assert_eq!(cmd[1], "--spec");
        assert_eq!(cmd[2], "/tmp/spec.json");
    }

    #[test]
    fn native_extract_text_layer_command_shape() {
        let cmd = native_extract_text_layer_command(
            &test_command_config(Path::new("/opt/bin/render_rs")),
            Path::new("/tmp/extract.spec.json"),
        );
        assert_eq!(cmd[0], "/opt/bin/render_rs");
        assert_eq!(cmd[1], "--extract-text-layer");
        assert_eq!(cmd[2], "--spec");
        assert_eq!(cmd[3], "/tmp/extract.spec.json");
    }

    #[test]
    fn should_route_extract_native_defaults_to_native_and_off_falls_back() {
        assert!(should_route_extract_native(false));
        assert!(!should_route_extract_native(true));
    }

    #[test]
    fn python_extract_text_layer_command_shape() {
        let cmd = python_extract_text_layer_command(
            &test_command_config(Path::new("/opt/bin/render_rs")),
            Path::new("/tmp/extract.spec.json"),
        );
        assert_eq!(cmd[0], "python");
        assert_eq!(cmd[1], "/tmp/scripts");
        assert_eq!(cmd[2], "--spec");
        assert_eq!(cmd[3], "/tmp/extract.spec.json");
    }

    #[test]
    fn should_route_normalize_native_defaults_to_mineru_and_off_falls_back() {
        assert!(should_route_normalize_native("mineru", false));
        assert!(!should_route_normalize_native("paddle", false));
        assert!(!should_route_normalize_native("mineru_content_list_v2", false));
        assert!(!should_route_normalize_native("mineru", true));
    }

    #[test]
    fn native_normalize_ocr_command_shape() {
        let cmd = native_normalize_ocr_command(
            &test_command_config(Path::new("/opt/bin/render_rs")),
            Path::new("/tmp/normalize.spec.json"),
        );
        assert_eq!(cmd[0], "/opt/bin/render_rs");
        assert_eq!(cmd[1], "--normalize-ocr");
        assert_eq!(cmd[2], "--spec");
        assert_eq!(cmd[3], "/tmp/normalize.spec.json");
    }

    #[test]
    fn script_mode_puts_script_at_index_1_for_worker_contract() {
        // WorkerContract::from_command reads the .py script path at argv
        // index 1, so Script mode must be `[python, script, --spec, ...]`
        // with no `-u` flag shifting the script.
        let config = test_command_config(Path::new("/opt/bin/render_rs"));
        let cmd = translate_only_command(&config, Path::new("/tmp/spec.json"));
        assert_eq!(cmd[0], "python");
        assert_eq!(cmd[1], "/tmp/scripts");
        assert_eq!(cmd[2], "--spec");
        assert_eq!(cmd[3], "/tmp/spec.json");
        assert!(!contains(&cmd, "-u"));
    }

    fn contains(cmd: &[String], value: &str) -> bool {
        cmd.iter().any(|arg| arg == value)
    }

    #[test]
    fn should_route_render_rs_defaults_to_native_for_render_rs_modes() {
        for mode in ["typst", "typst_visual", "overlay", "dual", "auto"] {
            assert!(should_route_render_rs(mode, false, false), "mode {mode}");
        }
    }

    #[test]
    fn should_route_render_rs_keeps_unknown_modes_on_python() {
        assert!(!should_route_render_rs("bogus", false, false));
    }

    #[test]
    fn should_route_render_rs_force_flags() {
        assert!(should_route_render_rs("overlay", true, false));
        assert!(!should_route_render_rs("typst", true, true));
        assert!(!should_route_render_rs("typst", false, true));
    }
}

pub(super) fn extract_text_layer_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    let force_off = std::env::var(RENDER_ORCHESTRATOR_FORCE_OFF_ENV).as_deref() == Ok("1");
    if should_route_extract_native(force_off) {
        return native_extract_text_layer_command(config, spec_path);
    }
    python_extract_text_layer_command(config, spec_path)
}

/// C5-N1 routing decision: the skip-OCR text-layer extraction runs natively by
/// default; `RETAINPDF_RENDER_ORCHESTRATOR_OFF=1` falls back to the python worker.
fn should_route_extract_native(force_off: bool) -> bool {
    !force_off
}

fn native_extract_text_layer_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    vec![
        config.render_rs_bin.to_string_lossy().into_owned(),
        "--extract-text-layer".to_string(),
        "--spec".to_string(),
        spec_path.to_string_lossy().into_owned(),
    ]
}

fn python_extract_text_layer_command(
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
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}

pub(super) fn normalize_ocr_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
    provider: &str,
) -> Vec<String> {
    let force_off = std::env::var(RENDER_ORCHESTRATOR_FORCE_OFF_ENV).as_deref() == Ok("1");
    if should_route_normalize_native(provider, force_off) {
        return native_normalize_ocr_command(config, spec_path);
    }
    python_normalize_ocr_command(config, spec_path)
}

/// C5-N2a routing decision: the mineru-provider normalize worker runs natively
/// by default; other providers (mineru_content_list_v2, paddle) stay on the
/// python worker until their adapters land. `RETAINPDF_RENDER_ORCHESTRATOR_OFF=1`
/// forces the python flow.
fn should_route_normalize_native(provider: &str, force_off: bool) -> bool {
    !force_off && provider == "mineru"
}

fn native_normalize_ocr_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    vec![
        config.render_rs_bin.to_string_lossy().into_owned(),
        "--normalize-ocr".to_string(),
        "--spec".to_string(),
        spec_path.to_string_lossy().into_owned(),
    ]
}

fn python_normalize_ocr_command(
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
    );
    cmd.path_arg("--spec", spec_path);
    cmd.finish()
}
