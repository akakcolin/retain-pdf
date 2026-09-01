use crate::config::WorkerCommandRuntimeConfig;
use std::path::Path;

use super::command_builder::{CommandBuilder, PythonEntrypoint};

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
    _render_mode: &str,
) -> Vec<String> {
    render_rs_command(config, spec_path)
}

/// render_rs 全量接管: every render stage runs through the native `render_rs`
/// orchestrator (`bundle_builder::build_bundle` resolves `auto` and rejects
/// anything else). The Python render fallback and the
/// `RETAINPDF_RENDER_ORCHESTRATOR_RS/OFF` escape valves are retired.

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
            run_translate_only_script: scripts,
            render_rs_bin,
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
    fn extract_text_layer_command_always_routes_native() {
        let cmd = extract_text_layer_command(
            &test_command_config(Path::new("/opt/bin/render_rs")),
            Path::new("/tmp/extract.spec.json"),
        );
        assert_eq!(cmd[0], "/opt/bin/render_rs");
        assert_eq!(cmd[1], "--extract-text-layer");
        assert_eq!(cmd[2], "--spec");
        assert_eq!(cmd[3], "/tmp/extract.spec.json");
    }

    #[test]
    fn normalize_ocr_command_always_routes_native() {
        let cmd = normalize_ocr_command(
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
    fn render_only_command_always_routes_render_rs() {
        for mode in ["typst", "typst_visual", "overlay", "dual", "auto", "bogus"] {
            let cmd = render_only_command(
                &test_command_config(Path::new("/opt/bin/render_rs")),
                Path::new("/tmp/spec.json"),
                mode,
            );
            assert_eq!(cmd[0], "/opt/bin/render_rs", "mode {mode}");
            assert_eq!(cmd[1], "--spec", "mode {mode}");
            assert_eq!(cmd[2], "/tmp/spec.json", "mode {mode}");
        }
    }
}

pub(super) fn extract_text_layer_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    native_extract_text_layer_command(config, spec_path)
}

/// C5-N1 routing decision: the skip-OCR text-layer extraction runs natively by
/// default (render_rs `--extract-text-layer`); the python worker is retired.
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

/// C5-N2a..C5-N2d: every registered OCR provider (mineru / paddle) normalizes
/// through the native `render_rs --normalize-ocr` worker. The python normalize
/// worker and the `should_route_normalize_native` fallback are retired; a new
/// provider lands a native adapter in `rendering_orchestrator/src/normalize/`.
pub(super) fn normalize_ocr_command(
    config: &WorkerCommandRuntimeConfig<'_>,
    spec_path: &Path,
) -> Vec<String> {
    native_normalize_ocr_command(config, spec_path)
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
