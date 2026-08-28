//! Delegation: spawn the thin Python prepare/page-specs entrypoint
//! (`entrypoints/run_render_delegate.py`) to produce the render bundle, then
//! load it. The Python binary is overridable via `RETAIN_PDF_PYTHON_BIN`
//! (default `python3`); the delegate script path is overridable via
//! `RETAIN_PDF_RENDER_DELEGATE_SCRIPT` or `RETAIN_PDF_ENTRYPOINTS_DIR`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bundle::RenderBundle;

pub fn run_delegate(spec_path: &Path, bundle_out: &Path) -> anyhow::Result<RenderBundle> {
    let script = resolve_delegate_script()?;
    let python_bin =
        std::env::var("RETAIN_PDF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    if let Some(parent) = bundle_out.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut cmd = Command::new(&python_bin);
    cmd.arg(&script)
        .arg("--spec")
        .arg(spec_path)
        .arg("--bundle-out")
        .arg(bundle_out);
    // The delegate needs `backend/scripts` on PYTHONPATH (its own module dir's
    // parent); prepend it so the `services`/`foundation` packages resolve.
    if let Some(scripts_dir) = scripts_dir_of(&script) {
        let existing = std::env::var("PYTHONPATH").unwrap_or_default();
        let combined = if existing.trim().is_empty() {
            scripts_dir.to_string_lossy().into_owned()
        } else {
            format!("{}:{}", scripts_dir.to_string_lossy(), existing)
        };
        cmd.env("PYTHONPATH", combined);
    }

    let output = cmd.output().map_err(|e| {
        anyhow::anyhow!("failed to spawn render delegate {script:?} via {python_bin}: {e}")
    })?;
    if !output.status.success() {
        anyhow::bail!(
            "render delegate failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if !bundle_out.exists() {
        anyhow::bail!(
            "render delegate exited 0 but no bundle at {}",
            bundle_out.display()
        );
    }
    RenderBundle::load(bundle_out)
}

fn scripts_dir_of(script: &Path) -> Option<PathBuf> {
    // entrypoints/run_render_delegate.py -> parent is `entrypoints`,
    // its parent is `backend/scripts`.
    let parent = script.parent()?;
    let scripts = parent.parent()?;
    Some(scripts.to_path_buf())
}

fn resolve_delegate_script() -> anyhow::Result<PathBuf> {
    if let Ok(env_path) = std::env::var("RETAIN_PDF_RENDER_DELEGATE_SCRIPT") {
        let path = PathBuf::from(env_path);
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!(
            "RETAIN_PDF_RENDER_DELEGATE_SCRIPT set but missing: {}",
            path.display()
        );
    }
    if let Ok(dir) = std::env::var("RETAIN_PDF_ENTRYPOINTS_DIR") {
        let path = PathBuf::from(dir).join("run_render_delegate.py");
        if path.exists() {
            return Ok(path);
        }
    }
    for candidate in ["entrypoints/run_render_delegate.py", "backend/scripts/entrypoints/run_render_delegate.py"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }
    anyhow::bail!(
        "run_render_delegate.py not found; set RETAIN_PDF_RENDER_DELEGATE_SCRIPT or RETAIN_PDF_ENTRYPOINTS_DIR"
    )
}
