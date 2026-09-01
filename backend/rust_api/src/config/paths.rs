use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::env_vars::env_path;

#[derive(Clone, Debug)]
pub struct RuntimePathsConfig {
    pub project_root: PathBuf,
    pub rust_api_root: PathBuf,
    pub data_root: PathBuf,
    pub scripts_dir: PathBuf,
    pub run_translate_only_script: PathBuf,
    pub run_failure_ai_diagnosis_script: PathBuf,
    pub render_rs_bin: PathBuf,
    pub uploads_dir: PathBuf,
    pub downloads_dir: PathBuf,
    pub jobs_db_path: PathBuf,
    pub output_root: PathBuf,
    pub auth_config_path: PathBuf,
}

impl RuntimePathsConfig {
    pub fn from_env() -> Result<Self> {
        let default_rust_api_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let rust_api_root = env_path("RUST_API_ROOT").unwrap_or(default_rust_api_root);
        let default_project_root = infer_project_root(&rust_api_root)?;
        let project_root = env_path("RUST_API_PROJECT_ROOT").unwrap_or(default_project_root);
        let scripts_dir =
            env_path("RUST_API_SCRIPTS_DIR").unwrap_or_else(|| default_scripts_dir(&project_root));
        Self::from_roots(project_root, rust_api_root, scripts_dir)
    }

    pub fn from_desktop(resource_root: PathBuf, data_root: PathBuf) -> Self {
        let scripts_dir = resource_root.join("scripts");
        Self::from_roots_unchecked(
            resource_root,
            data_root.join("rust_api"),
            data_root,
            scripts_dir,
        )
    }

    fn from_roots(
        project_root: PathBuf,
        rust_api_root: PathBuf,
        scripts_dir: PathBuf,
    ) -> Result<Self> {
        // Relative env paths (e.g. RUST_API_DATA_ROOT=../../data) must be
        // absolutized against the process cwd. Otherwise uploads_dir becomes
        // "../../data/uploads/..." and DB storage rejects parent-relative paths.
        let project_root = absolutize_path(&project_root);
        let rust_api_root = absolutize_path(&rust_api_root);
        let scripts_dir = absolutize_path(&scripts_dir);
        let data_root = absolutize_path(&resolve_data_root(&project_root));
        Ok(Self::from_roots_unchecked(
            project_root,
            rust_api_root,
            data_root,
            scripts_dir,
        ))
    }

    fn from_roots_unchecked(
        project_root: PathBuf,
        rust_api_root: PathBuf,
        data_root: PathBuf,
        scripts_dir: PathBuf,
    ) -> Self {
        let uploads_dir = data_root.join("uploads");
        let downloads_dir = data_root.join("downloads");
        let jobs_db_path = data_root.join("db").join("jobs.db");
        let output_root = data_root.join("jobs");
        let auth_config_path = rust_api_root.join("auth.local.json");
        let render_rs_bin = resolve_render_rs_bin(&project_root);

        Self {
            project_root,
            rust_api_root,
            data_root,
            scripts_dir: scripts_dir.clone(),
            run_translate_only_script: scripts_dir
                .join("entrypoints")
                .join("run_translate_only.py"),
            run_failure_ai_diagnosis_script: scripts_dir
                .join("entrypoints")
                .join("diagnose_failure_with_ai.py"),
            render_rs_bin,
            uploads_dir,
            downloads_dir,
            jobs_db_path,
            output_root,
            auth_config_path,
        }
    }
}

pub fn create_runtime_dirs(paths: &RuntimePathsConfig) -> Result<()> {
    std::fs::create_dir_all(&paths.data_root)?;
    std::fs::create_dir_all(&paths.uploads_dir)?;
    std::fs::create_dir_all(&paths.downloads_dir)?;
    std::fs::create_dir_all(&paths.output_root)?;
    if let Some(parent) = paths.jobs_db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Locate the native `render_rs` orchestrator binary.
///
/// `RETAIN_PDF_RENDER_RS_BIN` overrides everything; otherwise probe the repo
/// build output, then the desktop bundle layout (`app/backend/bin/render_rs`,
/// where `project_root` == the app backend root), then fall back to `render_rs`
/// on PATH.
fn resolve_render_rs_bin(project_root: &Path) -> PathBuf {
    if let Some(override_bin) = env_path("RETAIN_PDF_RENDER_RS_BIN") {
        return override_bin;
    }
    let suffix = std::env::consts::EXE_SUFFIX;
    for candidate in [
        project_root
            .join("backend")
            .join("rendering_orchestrator")
            .join("target")
            .join("release")
            .join(format!("render_rs{suffix}")),
        project_root
            .join("backend")
            .join("rendering_orchestrator")
            .join("target")
            .join("debug")
            .join(format!("render_rs{suffix}")),
        project_root
            .join("backend")
            .join("bin")
            .join(format!("render_rs{suffix}")),
        project_root.join("bin").join(format!("render_rs{suffix}")),
    ] {
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("render_rs")
}

fn infer_project_root(rust_api_root: &Path) -> Result<PathBuf> {
    let parent = rust_api_root
        .parent()
        .context("rust_api must live under the repository root or backend/")?;
    if parent.file_name().and_then(|v| v.to_str()) == Some("backend") {
        return parent
            .parent()
            .context("backend must live directly under repository root")
            .map(Path::to_path_buf);
    }
    Ok(parent.to_path_buf())
}

fn default_scripts_dir(project_root: &Path) -> PathBuf {
    let backend_scripts = project_root.join("backend").join("scripts");
    if backend_scripts.exists() {
        backend_scripts
    } else {
        project_root.join("scripts")
    }
}

fn resolve_data_root(project_root: &Path) -> PathBuf {
    env_path("RUST_API_DATA_ROOT")
        .or_else(|| env_path("RUST_API_DATA_DIR"))
        .unwrap_or_else(|| project_root.join("data"))
}

/// Resolve a possibly-relative path to an absolute path.
///
/// Prefer `canonicalize` when the path already exists so `..` segments collapse.
/// Fall back to `cwd.join(path)` without requiring the target to exist yet
/// (e.g. a brand-new data root on first boot).
fn absolutize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => {
            let joined = cwd.join(path);
            joined.canonicalize().unwrap_or(joined)
        }
        Err(_) => path.to_path_buf(),
    }
}
