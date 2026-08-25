use std::path::PathBuf;

use crate::constants::AI_SERVICE_PORT;

pub struct BackendEnvInput {
    pub ai_service_root: PathBuf,
    pub api_port: u16,
    pub backend_root: PathBuf,
    pub bundled_font_path: PathBuf,
    pub bundled_python_home: String,
    pub bundled_python_import_paths: Vec<PathBuf>,
    pub bundled_title_bold_font_path: PathBuf,
    pub bundled_typst_font_dir: PathBuf,
    pub data_root: PathBuf,
    pub desktop_api_key: String,
    pub inherit_host_pythonpath: bool,
    pub python_command: String,
    pub rust_api_root: PathBuf,
    pub scripts_dir: PathBuf,
    pub simple_port: u16,
    pub typst_bin: Option<PathBuf>,
    pub typst_package_cache_path: PathBuf,
    pub typst_package_path: PathBuf,
}

pub fn build_backend_env(input: BackendEnvInput) -> Vec<(String, String)> {
    let host_pythonpath = if input.inherit_host_pythonpath {
        std::env::var("PYTHONPATH").unwrap_or_default()
    } else {
        String::new()
    };

    let mut env = vec![
        ("RUST_API_BIND_HOST".to_string(), "127.0.0.1".to_string()),
        ("RUST_API_PORT".to_string(), input.api_port.to_string()),
        ("RUST_API_SIMPLE_PORT".to_string(), input.simple_port.to_string()),
        ("RUST_API_KEYS".to_string(), input.desktop_api_key.clone()),
        (
            "RUST_API_DATA_ROOT".to_string(),
            input.data_root.to_string_lossy().into_owned(),
        ),
        (
            "RUST_API_ROOT".to_string(),
            input.rust_api_root.to_string_lossy().into_owned(),
        ),
        (
            "RUST_API_NORMAL_MAX_BYTES".to_string(),
            (200 * 1024 * 1024).to_string(),
        ),
        ("RUST_API_NORMAL_MAX_PAGES".to_string(), "300".to_string()),
        (
            "RUST_API_PROJECT_ROOT".to_string(),
            input.backend_root.to_string_lossy().into_owned(),
        ),
        (
            "RUST_API_SCRIPTS_DIR".to_string(),
            input.scripts_dir.to_string_lossy().into_owned(),
        ),
        (
            "RUST_API_AI_SERVICE_BASE".to_string(),
            format!("http://127.0.0.1:{AI_SERVICE_PORT}"),
        ),
        ("PYTHON_BIN".to_string(), input.python_command.clone()),
        (
            "PYTHONPATH".to_string(),
            build_pythonpath(
                &input.scripts_dir,
                &input.ai_service_root,
                &input.bundled_python_import_paths,
                &host_pythonpath,
            ),
        ),
        ("PYTHONUNBUFFERED".to_string(), "1".to_string()),
        ("PYTHONUTF8".to_string(), "1".to_string()),
        ("PYTHONDONTWRITEBYTECODE".to_string(), "1".to_string()),
        ("PDF_TRANSLATOR_TRUST_ENV_PROXY".to_string(), "1".to_string()),
        (
            "RETAIN_PDF_FONT_PATH".to_string(),
            input.bundled_font_path.to_string_lossy().into_owned(),
        ),
        (
            "RETAIN_PDF_TITLE_BOLD_FONT_PATH".to_string(),
            input.bundled_title_bold_font_path.to_string_lossy().into_owned(),
        ),
        (
            "RETAIN_PDF_TYPST_FONT_DIRS".to_string(),
            input.bundled_typst_font_dir.to_string_lossy().into_owned(),
        ),
        (
            "RETAIN_PDF_TYPST_FONT_FAMILY".to_string(),
            "Source Han Serif SC".to_string(),
        ),
        (
            "TYPST_PACKAGE_CACHE_PATH".to_string(),
            input.typst_package_cache_path.to_string_lossy().into_owned(),
        ),
        ("RETAIN_AI_HOST".to_string(), "127.0.0.1".to_string()),
        ("RETAIN_AI_PORT".to_string(), AI_SERVICE_PORT.to_string()),
        ("RETAIN_AI_API_KEYS".to_string(), input.desktop_api_key.clone()),
        ("RETAIN_AI_RUST_API_KEY".to_string(), input.desktop_api_key),
        (
            "RETAIN_AI_RUST_API_BASE".to_string(),
            format!("http://127.0.0.1:{}", input.api_port),
        ),
        (
            "RETAIN_AI_DATA_ROOT".to_string(),
            input.data_root.to_string_lossy().into_owned(),
        ),
    ];

    if input.typst_package_path.exists() {
        env.push((
            "TYPST_PACKAGE_PATH".to_string(),
            input.typst_package_path.to_string_lossy().into_owned(),
        ));
    }
    if !input.bundled_python_home.is_empty() {
        env.push(("PYTHONHOME".to_string(), input.bundled_python_home));
    }
    if let Some(typst_bin) = input.typst_bin {
        if typst_bin.exists() {
            env.push((
                "TYPST_BIN".to_string(),
                typst_bin.to_string_lossy().into_owned(),
            ));
        }
    }

    env
}

fn build_pythonpath(
    scripts_dir: &PathBuf,
    ai_service_root: &PathBuf,
    import_paths: &[PathBuf],
    host_pythonpath: &str,
) -> String {
    let mut parts = vec![
        scripts_dir.to_string_lossy().into_owned(),
        ai_service_root.to_string_lossy().into_owned(),
    ];
    for path in import_paths {
        parts.push(path.to_string_lossy().into_owned());
    }
    if !host_pythonpath.is_empty() {
        parts.push(host_pythonpath.to_string());
    }
    let separator = if cfg!(windows) { ";" } else { ":" };
    parts.join(separator)
}
