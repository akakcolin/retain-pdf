//! Port of `output/typst/compiler.py`: orchestrate the `typst` CLI to compile
//! an emitted `.typ` source into a PDF.
//!
//! Only the command assembly and process runner are ported; the `typst` binary
//! itself is never invoked from CI (the differential generators compile on the
//! Python side and embed the resulting PDF).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_TYPST_BIN: &str = "/snap/bin/typst";
pub const DEFAULT_COMPILE_TIMEOUT_SECONDS: f64 = 600.0;

/// Environment + defaults captured once per compile request.
#[derive(Debug, Clone)]
pub struct CompileContext {
    pub typ_bin: String,
    pub timeout_seconds: f64,
    /// `fonts.BACKEND_FONTS_DIR` when it exists; Python auto-prepends it.
    pub backends_fonts_dir: Option<PathBuf>,
    /// `RETAIN_PDF_TYPST_FONT_DIRS` value (path-separated), if set.
    pub env_font_dirs: Option<String>,
}

impl Default for CompileContext {
    fn default() -> Self {
        CompileContext {
            typ_bin: resolve_typst_bin(None, None),
            timeout_seconds: DEFAULT_COMPILE_TIMEOUT_SECONDS,
            backends_fonts_dir: None,
            env_font_dirs: None,
        }
    }
}

/// Port of `shared._resolve_typst_bin` (env `TYPST_BIN`, then `which typst`,
/// then the snap default).
pub fn resolve_typst_bin(env_typst_bin: Option<&str>, discovered: Option<&str>) -> String {
    if let Some(explicit) = env_typst_bin {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(path) = discovered {
        if !path.is_empty() {
            return path.to_string();
        }
    }
    DEFAULT_TYPST_BIN.to_string()
}

/// Port of `_resolved_font_paths`: backends fonts dir first (if it exists),
/// then env dirs, then explicit paths, deduplicated.
pub fn resolved_font_paths(
    backends_fonts_dir: Option<&Path>,
    env_font_dirs: Option<&str>,
    font_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let mut resolved: Vec<PathBuf> = Vec::new();
    if let Some(dir) = backends_fonts_dir {
        if dir.exists() && !resolved.iter().any(|p| p == dir) {
            resolved.push(dir.to_path_buf());
        }
    }
    if let Some(raw) = env_font_dirs {
        for item in raw.split(':') {
            let value = item.trim();
            if value.is_empty() {
                continue;
            }
            let path = PathBuf::from(value);
            if !resolved.contains(&path) {
                resolved.push(path);
            }
        }
    }
    for item in font_paths {
        if !resolved.contains(item) {
            resolved.push(item.clone());
        }
    }
    resolved
}

/// `_typst_compile_command` (no `--root`) plus the `--root` variant.
pub fn typst_compile_command(
    typ_path: &Path,
    pdf_path: &Path,
    ctx: &CompileContext,
    root: Option<&Path>,
    font_paths: &[PathBuf],
) -> Vec<String> {
    let mut command = vec![ctx.typ_bin.clone(), "compile".to_string()];
    if let Some(root) = root {
        command.push("--root".to_string());
        command.push(root.to_string_lossy().into_owned());
    }
    for font_path in resolved_font_paths(ctx.backends_fonts_dir.as_deref(), ctx.env_font_dirs.as_deref(), font_paths) {
        command.push("--font-path".to_string());
        command.push(font_path.to_string_lossy().into_owned());
    }
    command.push(typ_path.to_string_lossy().into_owned());
    command.push(pdf_path.to_string_lossy().into_owned());
    command
}

/// Port of `_resolved_common_root`: the longest common path of the (lexically
/// normalized) inputs, falling back to `fallback_root` when they share nothing.
pub fn resolved_common_root(paths_to_cover: &[PathBuf], fallback_root: &Path) -> PathBuf {
    if paths_to_cover.is_empty() {
        return fallback_root.to_path_buf();
    }
    let normalized: Vec<PathBuf> = paths_to_cover
        .iter()
        .map(|entry| lexical_normalize(entry))
        .collect();
    let mut common: Vec<std::path::Component> = normalized[0].components().collect();
    for path in &normalized[1..] {
        let own: Vec<std::path::Component> = path.components().collect();
        let mut idx = 0;
        while idx < common.len()
            && idx < own.len()
            && common[idx] == own[idx]
        {
            idx += 1;
        }
        common.truncate(idx);
        if common.is_empty() {
            break;
        }
    }
    let joined: PathBuf = common.iter().collect();
    if joined.as_os_str().is_empty() {
        fallback_root.to_path_buf()
    } else {
        joined
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `_run_typst_compile`: spawn the CLI, poll until exit or timeout, and
/// surface a `TypstCompileError` on timeout / spawn failure / non-zero exit.
#[allow(clippy::too_many_arguments)]
pub fn run_typst_compile(
    command: &[String],
    timeout_seconds: f64,
    phase: &str,
    stem: &str,
    typ_path: &Path,
    pdf_path: &Path,
    work_dir: &Path,
    extra: serde_json::Map<String, serde_json::Value>,
) -> Result<(), TypstCompileError> {
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            TypstCompileError::runtime_failed(
                phase, stem, typ_path, pdf_path, command, work_dir, extra.clone(),
                format!("Typst runtime failed to start: {}", describe_io_error(&err)),
                command.first().cloned().unwrap_or_default(),
            )
        })?;
    let mut stdout_reader = child.stdout.take();
    let mut stderr_reader = child.stderr.take();
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(reader) = &mut stdout_reader {
            let _ = reader.read_to_string(&mut buf);
        }
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(reader) = &mut stderr_reader {
            let _ = reader.read_to_string(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed().as_secs_f64() > timeout_seconds {
                    let _ = child.kill();
                    let _ = child.wait();
                    let stderr = format!(
                        "Typst compile timed out after {timeout_seconds:.0}s. This can happen when Typst stalls downloading a @preview package from packages.typst.org over a slow or stuck connection."
                    );
                    let mut timeout_extra = extra.clone();
                    timeout_extra.insert(
                        "runtime_error_type".to_string(),
                        serde_json::json!("TimeoutExpired"),
                    );
                    timeout_extra.insert(
                        "timeout_seconds".to_string(),
                        serde_json::json!(timeout_seconds),
                    );
                    let stdout = stdout_thread.join().unwrap_or_default();
                    return Err(TypstCompileError::new(
                        phase, stem, typ_path, pdf_path, command, -1, stdout, stderr,
                        work_dir, timeout_extra,
                    ));
                }
            }
            Err(err) => {
                let _ = child.kill();
                let stderr = format!("Typst runtime failed to wait: {}", describe_io_error(&err));
                return Err(TypstCompileError::runtime_failed(
                    phase, stem, typ_path, pdf_path, command, work_dir, extra,
                    stderr, command.first().cloned().unwrap_or_default(),
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    let return_code = status.code().unwrap_or(-1);
    if return_code != 0 {
        return Err(TypstCompileError::new(
            phase, stem, typ_path, pdf_path, command, return_code, stdout, stderr,
            work_dir, extra,
        ));
    }
    Ok(())
}

fn describe_io_error(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => format!("FileNotFoundError: {err}"),
        std::io::ErrorKind::PermissionDenied => format!("PermissionError: {err}"),
        _ => format!("OSError: {err}"),
    }
}

/// `TypstCompileError`.
#[derive(Debug, Clone)]
pub struct TypstCompileError {
    pub phase: String,
    pub stem: String,
    pub typ_path: PathBuf,
    pub pdf_path: PathBuf,
    pub command: Vec<String>,
    pub return_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub work_dir: PathBuf,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl TypstCompileError {
    #[allow(clippy::too_many_arguments)]
    fn new(
        phase: &str,
        stem: &str,
        typ_path: &Path,
        pdf_path: &Path,
        command: &[String],
        return_code: i32,
        stdout: String,
        stderr: String,
        work_dir: &Path,
        extra: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        TypstCompileError {
            phase: phase.to_string(),
            stem: stem.to_string(),
            typ_path: typ_path.to_path_buf(),
            pdf_path: pdf_path.to_path_buf(),
            command: command.to_vec(),
            return_code,
            stdout,
            stderr,
            work_dir: if work_dir.as_os_str().is_empty() {
                typ_path.parent().map(Path::to_path_buf).unwrap_or_default()
            } else {
                work_dir.to_path_buf()
            },
            extra,
        }
    }

    fn runtime_failed(
        phase: &str,
        stem: &str,
        typ_path: &Path,
        pdf_path: &Path,
        command: &[String],
        work_dir: &Path,
        extra: serde_json::Map<String, serde_json::Value>,
        stderr: String,
        typ_bin: String,
    ) -> Self {
        let mut runtime_extra = extra;
        runtime_extra.insert("runtime_error_type".to_string(), serde_json::json!("OSError"));
        runtime_extra.insert("typst_bin".to_string(), serde_json::json!(typ_bin));
        TypstCompileError::new(
            phase, stem, typ_path, pdf_path, command, -1, String::new(), stderr,
            work_dir, runtime_extra,
        )
    }

    /// `_message`.
    pub fn message(&self) -> String {
        let detail = if self.stderr.trim().is_empty() {
            self.stdout.trim().to_string()
        } else {
            self.stderr.trim().to_string()
        };
        let prefix = format!(
            "Typst compile failed phase={} stem={} code={} typ={}",
            self.phase,
            self.stem,
            self.return_code,
            self.typ_path.display()
        );
        if detail.is_empty() {
            prefix
        } else {
            format!("{prefix}\n{detail}")
        }
    }

    /// `to_dict`.
    pub fn to_dict(&self) -> serde_json::Value {
        let mut payload = serde_json::Map::new();
        payload.insert("phase".to_string(), serde_json::json!(self.phase));
        payload.insert("stem".to_string(), serde_json::json!(self.stem));
        payload.insert(
            "typ_path".to_string(),
            serde_json::json!(self.typ_path.to_string_lossy()),
        );
        payload.insert(
            "pdf_path".to_string(),
            serde_json::json!(self.pdf_path.to_string_lossy()),
        );
        payload.insert(
            "work_dir".to_string(),
            serde_json::json!(self.work_dir.to_string_lossy()),
        );
        payload.insert("command".to_string(), serde_json::json!(self.command));
        payload.insert("return_code".to_string(), serde_json::json!(self.return_code));
        payload.insert("stdout".to_string(), serde_json::json!(self.stdout));
        payload.insert("stderr".to_string(), serde_json::json!(self.stderr));
        payload.insert("message".to_string(), serde_json::json!(self.message()));
        if !self.extra.is_empty() {
            payload.insert("extra".to_string(), serde_json::Value::Object(self.extra.clone()));
        }
        serde_json::Value::Object(payload)
    }
}

impl std::fmt::Display for TypstCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for TypstCompileError {}

/// Write the `.typ` source and compile it to `{stem}.pdf` inside `work_dir`,
/// returning the PDF path.
#[allow(clippy::too_many_arguments)]
pub fn compile_typst_source(
    source: &str,
    stem: &str,
    phase: &str,
    work_dir: &Path,
    root: Option<&Path>,
    font_paths: &[PathBuf],
    ctx: &CompileContext,
    extra: serde_json::Map<String, serde_json::Value>,
) -> Result<PathBuf, TypstCompileError> {
    std::fs::create_dir_all(work_dir)
        .map_err(|err| runtime_error(phase, stem, ctx, &err, "mkdir"))?;
    let typ_path = work_dir.join(format!("{stem}.typ"));
    let pdf_path = work_dir.join(format!("{stem}.pdf"));
    std::fs::write(&typ_path, source)
        .map_err(|err| runtime_error(phase, stem, ctx, &err, "write .typ"))?;
    let command = typst_compile_command(&typ_path, &pdf_path, ctx, root, font_paths);
    run_typst_compile(
        &command,
        ctx.timeout_seconds,
        phase,
        stem,
        &typ_path,
        &pdf_path,
        work_dir,
        extra,
    )?;
    Ok(pdf_path)
}

fn runtime_error(
    phase: &str,
    stem: &str,
    ctx: &CompileContext,
    err: &std::io::Error,
    step: &str,
) -> TypstCompileError {
    TypstCompileError::new(
        phase,
        stem,
        Path::new(""),
        Path::new(""),
        &[ctx.typ_bin.clone()],
        -1,
        String::new(),
        format!("{step} failed: {}", describe_io_error(err)),
        Path::new(""),
        serde_json::Map::new(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(typ_bin: &str) -> CompileContext {
        CompileContext {
            typ_bin: typ_bin.to_string(),
            timeout_seconds: 5.0,
            backends_fonts_dir: None,
            env_font_dirs: None,
        }
    }

    #[test]
    fn typst_bin_resolution_prefers_env() {
        assert_eq!(resolve_typst_bin(Some("  /opt/typst "), Some("/usr/bin/typst")), "/opt/typst");
        assert_eq!(resolve_typst_bin(None, Some("/usr/bin/typst")), "/usr/bin/typst");
        assert_eq!(resolve_typst_bin(None, None), DEFAULT_TYPST_BIN);
    }

    #[test]
    fn font_paths_are_deduplicated() {
        let backends = std::env::temp_dir(); // exists -> auto-prepended
        let got = resolved_font_paths(
            Some(&backends),
            Some("/env/a:/env/b:/env/a"),
            &[PathBuf::from("/env/b"), PathBuf::from("/explicit")],
        );
        let expected = vec![
            backends.clone(),
            PathBuf::from("/env/a"),
            PathBuf::from("/env/b"),
            PathBuf::from("/explicit"),
        ];
        assert_eq!(got, expected);
    }

    #[test]
    fn command_assembles_without_root() {
        let c = ctx("/opt/typst");
        let command = typst_compile_command(
            Path::new("/w/out.typ"),
            Path::new("/w/out.pdf"),
            &c,
            None,
            &[PathBuf::from("/fonts")],
        );
        assert_eq!(
            command,
            vec![
                "/opt/typst",
                "compile",
                "--font-path",
                "/fonts",
                "/w/out.typ",
                "/w/out.pdf",
            ]
        );
    }

    #[test]
    fn command_assembles_with_root_first() {
        let c = ctx("/opt/typst");
        let command = typst_compile_command(
            Path::new("/w/out.typ"),
            Path::new("/w/out.pdf"),
            &c,
            Some(Path::new("/project")),
            &[],
        );
        assert_eq!(
            command,
            vec![
                "/opt/typst",
                "compile",
                "--root",
                "/project",
                "/w/out.typ",
                "/w/out.pdf",
            ]
        );
    }

    #[test]
    fn common_root_longest_prefix() {
        let fallback = Path::new("/");
        let got = resolved_common_root(
            &[PathBuf::from("/a/b/c/x.typ"), PathBuf::from("/a/b/d/y.pdf"), PathBuf::from("/a/b/background.pdf")],
            fallback,
        );
        assert_eq!(got, PathBuf::from("/a/b"));
    }

    #[test]
    fn common_root_single_path() {
        let got = resolved_common_root(&[PathBuf::from("/a/b/c")], Path::new("/"));
        assert_eq!(got, PathBuf::from("/a/b/c"));
    }

    #[test]
    fn error_message_matches_python_shape() {
        let err = TypstCompileError::new(
            "render_pages",
            "stem_x",
            Path::new("/w/stem_x.typ"),
            Path::new("/w/stem_x.pdf"),
            &["/opt/typst".to_string(), "compile".to_string()],
            2,
            String::new(),
            "some diagnostics".to_string(),
            Path::new("/w"),
            serde_json::Map::new(),
        );
        assert_eq!(
            err.message(),
            "Typst compile failed phase=render_pages stem=stem_x code=2 typ=/w/stem_x.typ\nsome diagnostics"
        );
    }

    #[test]
    fn successful_compile_returns_ok() {
        let dir = std::env::temp_dir().join(format!("pdrf_compile_ok_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake_typst.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let c = ctx(script.to_str().unwrap());
        let pdf = compile_typst_source(
            "#set page()", "out", "test", &dir, None, &[], &c, serde_json::Map::new(),
        );
        assert!(pdf.is_ok());
        assert_eq!(pdf.unwrap(), dir.join("out.pdf"));
        assert!(dir.join("out.typ").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failing_compile_propagates_return_code() {
        let dir = std::env::temp_dir().join(format!("pdrf_compile_fail_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake_typst.sh");
        std::fs::write(&script, "#!/bin/sh\necho boom >&2\nexit 3\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let c = ctx(script.to_str().unwrap());
        let pdf = compile_typst_source(
            "#set page()", "out", "test", &dir, None, &[], &c, serde_json::Map::new(),
        );
        let err = pdf.unwrap_err();
        assert_eq!(err.return_code, 3);
        assert!(err.stderr.contains("boom"), "stderr was: {}", err.stderr);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timeout_kills_and_reports() {
        let dir = std::env::temp_dir().join(format!("pdrf_compile_timeout_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("fake_typst.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut c = ctx(script.to_str().unwrap());
        c.timeout_seconds = 0.2;
        let pdf = compile_typst_source(
            "#set page()", "out", "test", &dir, None, &[], &c, serde_json::Map::new(),
        );
        let err = pdf.unwrap_err();
        assert_eq!(err.return_code, -1);
        assert!(err.stderr.contains("timed out"), "stderr was: {}", err.stderr);
        assert_eq!(err.extra["timeout_seconds"], 0.2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
