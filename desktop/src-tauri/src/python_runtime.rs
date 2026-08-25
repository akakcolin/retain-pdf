use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct PythonRuntime {
    pub command: String,
    pub bundled_home: Option<PathBuf>,
}

pub struct ProbeResult {
    pub ok: bool,
    pub timed_out: bool,
    pub reason: String,
    #[allow(dead_code)]
    pub stdout: String,
    pub stderr: String,
}

pub fn system_python_runtime() -> PythonRuntime {
    PythonRuntime {
        command: if cfg!(windows) {
            "python".to_string()
        } else {
            "python3".to_string()
        },
        bundled_home: None,
    }
}

pub fn resolve_python_runtime(backend_root: &Path, packaged: bool) -> PythonRuntime {
    let bundled_root = backend_root.join("python");
    let bundled_candidates = if cfg!(windows) {
        vec![bundled_root.join("python.exe")]
    } else {
        vec![
            bundled_root.join("bin").join("python3"),
            bundled_root.join("bin").join("python"),
        ]
    };
    for candidate in bundled_candidates {
        if candidate.exists() {
            return PythonRuntime {
                command: candidate.to_string_lossy().into_owned(),
                bundled_home: Some(bundled_root.clone()),
            };
        }
    }
    if packaged {
        return PythonRuntime {
            command: String::new(),
            bundled_home: None,
        };
    }
    system_python_runtime()
}

pub fn resolve_bundled_python_home(bundled_home: &Path) -> Option<PathBuf> {
    if !bundled_home.exists() {
        return None;
    }
    if cfg!(target_os = "macos") {
        let framework_home = bundled_home
            .join("Frameworks")
            .join("Python.framework")
            .join("Versions")
            .join("Current");
        if framework_home.exists() {
            return Some(framework_home);
        }
    }
    if !bundled_home.join("pyvenv.cfg").exists() {
        return Some(bundled_home.to_path_buf());
    }
    None
}

fn bundled_site_packages(home: &Path) -> Vec<PathBuf> {
    if !home.exists() {
        return Vec::new();
    }
    if cfg!(windows) {
        let site_packages = home.join("Lib").join("site-packages");
        return if site_packages.exists() {
            vec![site_packages]
        } else {
            Vec::new()
        };
    }
    let lib_root = home.join("lib");
    if !lib_root.exists() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&lib_root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy().into_owned();
            if !name.starts_with("python") || !name[6..].chars().all(|c| c.is_ascii_digit() || c == '.') {
                continue;
            }
            let site_packages = entry.path().join("site-packages");
            if site_packages.exists() {
                matches.push(site_packages);
            }
        }
    }
    matches
}

fn bundled_lib_dynload(home: &Path) -> Vec<PathBuf> {
    if !home.exists() || !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let python_home = resolve_bundled_python_home(home);
    let lib_root = python_home
        .as_ref()
        .map(|root| root.join("lib"))
        .unwrap_or_else(|| home.join("lib"));
    if !lib_root.exists() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&lib_root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy().into_owned();
            if !name.starts_with("python") {
                continue;
            }
            let lib_dynload = entry.path().join("lib-dynload");
            if lib_dynload.exists() {
                matches.push(lib_dynload);
            }
        }
    }
    matches
}

pub fn bundled_import_paths(home: &Path) -> Vec<PathBuf> {
    let mut paths = bundled_site_packages(home);
    paths.extend(bundled_lib_dynload(home));
    paths
}

pub fn build_probe_script(include_dependency_imports: bool) -> String {
    if !include_dependency_imports {
        return [
            "import sys",
            "print(sys.executable, flush=True)",
            "print(f'prefix={sys.prefix} exec_prefix={sys.exec_prefix} base_exec_prefix={sys.base_exec_prefix}', flush=True)",
            "print('python_runtime_startup_check=ok', flush=True)",
        ]
        .join("\n");
    }
    [
        "import importlib",
        "import sys",
        "print(sys.executable, flush=True)",
        "print(f'prefix={sys.prefix} exec_prefix={sys.exec_prefix} base_exec_prefix={sys.base_exec_prefix}', flush=True)",
        "for module_name in ['_socket', 'socket', 'ssl', 'requests', 'fitz', 'pikepdf', 'PIL', 'urllib3']:",
        "    print(f'importing:{module_name}', flush=True)",
        "    importlib.import_module(module_name)",
        "    print(f'imported:{module_name}', flush=True)",
        "print('python_runtime_import_check=ok', flush=True)",
    ]
    .join("\n")
}

fn probe_env(runtime: &PythonRuntime, inherit_host_pythonpath: bool) -> Vec<(String, String)> {
    let mut env = vec![
        ("PYTHONUNBUFFERED".to_string(), "1".to_string()),
        ("PYTHONUTF8".to_string(), "1".to_string()),
    ];
    let import_paths = runtime
        .bundled_home
        .as_ref()
        .map(|home| bundled_import_paths(home))
        .unwrap_or_default();
    if !import_paths.is_empty() {
        let mut parts: Vec<String> = import_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        if inherit_host_pythonpath {
            let inherited = std::env::var("PYTHONPATH").unwrap_or_default();
            if !inherited.is_empty() {
                parts.push(inherited);
            }
        }
        env.push((
            "PYTHONPATH".to_string(),
            parts.join(if cfg!(windows) { ";" } else { ":" }),
        ));
    }
    if let Some(home) = runtime.bundled_home.as_ref() {
        if let Some(python_home) = resolve_bundled_python_home(home) {
            env.push((
                "PYTHONHOME".to_string(),
                python_home.to_string_lossy().into_owned(),
            ));
        }
    }
    env
}

pub fn probe_python(
    runtime: &PythonRuntime,
    include_dependency_imports: bool,
    timeout_ms: u64,
    inherit_host_pythonpath: bool,
) -> ProbeResult {
    if runtime.command.is_empty() {
        return ProbeResult {
            ok: false,
            timed_out: false,
            reason: "missing_python_command".to_string(),
            stdout: String::new(),
            stderr: String::new(),
        };
    }
    run_probe(
        &runtime.command,
        &["-c".to_string(), build_probe_script(include_dependency_imports)],
        &probe_env(runtime, inherit_host_pythonpath),
        timeout_ms,
    )
}

fn run_probe(command: &str, args: &[String], env: &[(String, String)], timeout_ms: u64) -> ProbeResult {
    let mut child = match Command::new(command)
        .args(args)
        .envs(env.iter().cloned())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return ProbeResult {
                ok: false,
                timed_out: false,
                reason: format!("spawn error: {error}"),
                stdout: String::new(),
                stderr: String::new(),
            };
        }
    };
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel();
    let (stderr_tx, stderr_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = Read::read_to_string(&mut stdout, &mut buffer);
        let _ = stdout_tx.send(buffer);
    });
    std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = Read::read_to_string(&mut stderr, &mut buffer);
        let _ = stderr_tx.send(buffer);
    });

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().ok().flatten() {
            break Some(status);
        }
        if started.elapsed().as_millis() as u64 >= timeout_ms {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let stdout = stdout_rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let stderr = stderr_rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let code = status.and_then(|status| status.code());
    let ok = !timed_out && code == Some(0);
    let reason = if timed_out {
        format!("timeout_after_ms={timeout_ms}")
    } else if let Some(code) = code {
        format!("exit_code={code}")
    } else {
        "exit_code=null".to_string()
    };
    ProbeResult {
        ok,
        timed_out,
        reason,
        stdout: stdout.trim().to_string(),
        stderr: stderr.trim().to_string(),
    }
}

fn current_platform_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    }
}

pub struct PrepareOptions {
    pub packaged: bool,
    pub on_progress: Option<Box<dyn Fn(u8, &str, &str) + Send + Sync>>,
}

pub fn prepare_python_runtime(runtime: PythonRuntime, options: &PrepareOptions) -> Result<PythonRuntime, String> {
    let platform_label = current_platform_label();
    let startup_probe =
        probe_python(&runtime, false, if options.packaged { 10000 } else { 8000 }, !options.packaged);
    let mut selected = runtime;
    if !startup_probe.ok && selected.bundled_home.is_some() {
        if options.packaged {
            return Err(format!(
                "packaged {platform_label} bundled python startup probe failed: {}\n{}",
                startup_probe.reason,
                startup_probe
                    .stderr
                    .lines()
                    .next()
                    .map(|line| line.to_string())
                    .unwrap_or_default(),
            ));
        }
        eprintln!(
            "[desktop] bundled {platform_label} python startup probe failed, fallback to system python: {}\n{}",
            startup_probe.reason,
            startup_probe.stderr.lines().next().unwrap_or(""),
        );
        if let Some(callback) = &options.on_progress {
            callback(26, "正在检查 Python 运行时", "内置 Python 不可用，正在回退系统 Python");
        }
        let fallback = system_python_runtime();
        let fallback_probe = probe_python(&fallback, false, 10000, true);
        if fallback_probe.ok {
            selected = fallback;
        } else {
            return Err(format!(
                "{platform_label} Python runtime startup probe failed; bundled={}; fallback={}",
                startup_probe.reason, fallback_probe.reason
            ));
        }
    }

    let dependency_probe = probe_python(
        &selected,
        true,
        if options.packaged { 30000 } else { 8000 },
        !options.packaged,
    );
    if !dependency_probe.ok {
        if options.packaged && selected.bundled_home.is_some() && dependency_probe.timed_out {
            eprintln!(
                "[desktop] packaged {platform_label} python dependency probe timed out; continuing to backend startup: {}",
                dependency_probe.reason
            );
            if let Some(callback) = &options.on_progress {
                callback(26, "正在检查 Python 运行时", "内置 Python 启动较慢，继续启动本地服务");
            }
        } else if selected.bundled_home.is_some() && !options.packaged {
            eprintln!(
                "[desktop] bundled {platform_label} python dependency probe failed, fallback to system python: {}",
                dependency_probe.reason
            );
            if let Some(callback) = &options.on_progress {
                callback(26, "正在检查 Python 运行时", "内置 Python 不可用，正在回退系统 Python");
            }
            let fallback = system_python_runtime();
            let fallback_probe = probe_python(&fallback, true, 10000, true);
            if fallback_probe.ok {
                selected = fallback;
            } else {
                return Err(format!(
                    "{platform_label} Python runtime import probe failed; bundled={}; fallback={}",
                    dependency_probe.reason, fallback_probe.reason
                ));
            }
        } else {
            return Err(format!(
                "packaged {platform_label} bundled python probe failed: {}",
                dependency_probe.reason
            ));
        }
    }
    Ok(selected)
}
