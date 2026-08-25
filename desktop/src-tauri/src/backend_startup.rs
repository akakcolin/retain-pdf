use std::fs;
use std::io::{BufRead, Read};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, EventTarget, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::backend_env::{build_backend_env, BackendEnvInput};
use crate::logging;
use crate::constants::{AI_SERVICE_PORT, API_PORT, DESKTOP_API_KEY, SIMPLE_PORT};
use crate::python_runtime::{
    bundled_import_paths, prepare_python_runtime, resolve_bundled_python_home,
    resolve_python_runtime, PrepareOptions,
};

pub struct BackendState {
    pub child: Mutex<Option<Child>>,
    pub ai_child: Mutex<Option<Child>>,
    pub recent_stdout: Mutex<Vec<String>>,
    pub recent_stderr: Mutex<Vec<String>>,
    pub command: Mutex<String>,
    pub cwd: Mutex<String>,
    pub exit_detail: Mutex<String>,
    pub ready: AtomicBool,
    pub stopping: AtomicBool,
    pub using_external: AtomicBool,
    pub crash_reported: AtomicBool,
}

impl Default for BackendState {
    fn default() -> Self {
        BackendState {
            child: Mutex::new(None),
            ai_child: Mutex::new(None),
            recent_stdout: Mutex::new(Vec::new()),
            recent_stderr: Mutex::new(Vec::new()),
            command: Mutex::new(String::new()),
            cwd: Mutex::new(String::new()),
            exit_detail: Mutex::new(String::new()),
            ready: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            using_external: AtomicBool::new(false),
            crash_reported: AtomicBool::new(false),
        }
    }
}

pub fn run_startup(handle: AppHandle) {
    emit_progress(&handle, 6, "正在准备运行环境", "正在检查桌面组件与本地资源");
    match start_bundled_backend(&handle) {
        Ok(()) => {
            if let Err(error) = create_main_window(&handle) {
                fail_startup(&handle, &format!("window creation failed: {error}"));
            }
        }
        Err(error) => fail_startup(&handle, &error),
    }
}

fn fail_startup(handle: &AppHandle, error: &str) {
    let log_path = logging::log_path(handle);
    let detail = format!("{error}\n完整日志: {}", log_path.display());
    logging::log_error(handle, &format!("[desktop] startup failed: {detail}"));
    show_error_dialog(handle, "RetainPDF startup failed", &detail);
    handle.exit(1);
}

fn start_bundled_backend(handle: &AppHandle) -> Result<(), String> {
    emit_progress(handle, 18, "正在检查运行文件", "正在校验后端、Python 和脚本资源");
    let packaged = is_packaged(handle);
    let backend_root = resolve_backend_root(handle);
    let backend_bin = resolve_backend_binary(&backend_root);
    let mut python_runtime = resolve_python_runtime(&backend_root, packaged);
    let scripts_dir = backend_root.join("scripts");
    let typst_bin = resolve_typst_binary(&backend_root, packaged);
    let bundled_font_path = backend_root.join("fonts").join("SourceHanSerifSC-Regular.otf");
    let bundled_title_bold_font_path = backend_root.join("fonts").join("SourceHanSerifSC-Bold.otf");
    let bundled_typst_font_dir = backend_root.join("fonts");
    let data_root = handle
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("data");
    let rust_api_root = data_root.join("rust_api");
    let typst_package_path = backend_root.join("typst-packages");
    let typst_package_cache_path = data_root.join("typst-package-cache");
    let api_port = API_PORT;
    let simple_port = SIMPLE_PORT;
    let desktop_api_key = DESKTOP_API_KEY.to_string();

    logging::log(
        handle,
        &format!(
            "[desktop] starting bundled backend platform={} packaged={} backendRoot={} backendBin={} python={} pythonHome={} scriptsDir={} typst={} log={}",
            std::env::consts::OS,
            packaged,
            backend_root.display(),
            backend_bin.display(),
            if python_runtime.command.is_empty() {
                "<missing>".to_string()
            } else {
                python_runtime.command.clone()
            },
            python_runtime
                .bundled_home
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<system>".to_string()),
            scripts_dir.display(),
            typst_bin
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<missing>".to_string()),
            logging::log_path(handle).display(),
        ),
    );

    // In dev, `tauri dev` runs beforeDevCommand (prepare-app) concurrently with the
    // cargo build, so prepare-app artifacts may not exist yet when the app launches.
    // Wait briefly for them instead of failing immediately. Packaged builds are
    // sequential (beforeBuildCommand runs first) so a missing file there is a real error.
    if !backend_bin.exists() {
        if packaged {
            return Err(format!("missing bundled backend binary: {}", backend_bin.display()));
        }
        emit_progress(handle, 24, "正在准备运行文件", "正在等待后端资源就绪");
        if !wait_for_file(&backend_bin, 20_000) {
            return Err(format!("missing bundled backend binary: {}", backend_bin.display()));
        }
    }
    if python_runtime.command.is_empty() {
        return Err("missing python runtime".to_string());
    }
    if !scripts_dir.exists() {
        if packaged {
            return Err(format!("missing bundled scripts directory: {}", scripts_dir.display()));
        }
        if !wait_for_file(&scripts_dir, 20_000) {
            return Err(format!("missing bundled scripts directory: {}", scripts_dir.display()));
        }
    }
    if packaged && typst_bin.is_none() {
        return Err(format!(
            "missing bundled typst runtime under {}",
            backend_root.join("typst").display()
        ));
    }

    let progress_handle = handle.clone();
    python_runtime = prepare_python_runtime(python_runtime, &PrepareOptions {
        packaged,
        on_progress: Some(Box::new(move |progress, title, detail| {
            emit_progress(&progress_handle, progress, title, detail);
        })),
    })
    .map_err(|error| error.to_string())?;
    if packaged && python_runtime.bundled_home.is_none() {
        return Err(format!(
            "missing bundled python runtime under {}",
            backend_root.join("python").display()
        ));
    }

    fs::create_dir_all(&data_root).map_err(|error| error.to_string())?;
    fs::create_dir_all(&rust_api_root).map_err(|error| error.to_string())?;
    fs::create_dir_all(&typst_package_cache_path).map_err(|error| error.to_string())?;
    emit_progress(handle, 34, "正在准备工作目录", "正在初始化本地数据目录");

    let ai_service_root = resolve_ai_service_root(&backend_root, packaged);
    let bundled_python_home = python_runtime
        .bundled_home
        .as_ref()
        .and_then(|home| resolve_bundled_python_home(home))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bundled_python_import_paths = python_runtime
        .bundled_home
        .as_ref()
        .map(|home| bundled_import_paths(home))
        .unwrap_or_default();

    let env = build_backend_env(BackendEnvInput {
        ai_service_root: ai_service_root.clone(),
        api_port,
        backend_root: backend_root.clone(),
        bundled_font_path,
        bundled_python_home,
        bundled_python_import_paths,
        bundled_title_bold_font_path,
        bundled_typst_font_dir,
        data_root,
        desktop_api_key: desktop_api_key.clone(),
        inherit_host_pythonpath: !packaged,
        python_command: python_runtime.command.clone(),
        rust_api_root,
        scripts_dir: scripts_dir.clone(),
        simple_port,
        typst_bin: typst_bin.clone(),
        typst_package_cache_path,
        typst_package_path,
    });

    let api_port_busy = can_connect_to_port("127.0.0.1", api_port, 800);
    logging::log(handle, &format!("[desktop] port {api_port} busy={api_port_busy}"));
    if api_port_busy {
        let allow_external = std::env::var("RETAINPDF_DESKTOP_ALLOW_EXTERNAL_BACKEND")
            .map(|value| value == "1")
            .unwrap_or(false);
        if packaged && !allow_external {
            return Err(format!(
                "端口 {api_port} 已被占用。正式桌面端不会复用已有后端，避免连接到旧版本或开发版后端导致渲染错误。请关闭其他 RetainPDF、旧版桌面端、Docker/系统服务后再启动。"
            ));
        }
        if crate::backend_http::can_reuse_existing_backend(api_port, &desktop_api_key) {
            handle
                .state::<BackendState>()
                .using_external
                .store(true, Ordering::SeqCst);
            logging::log(handle, &format!("[desktop] reusing existing backend on port {api_port}"));
            emit_progress(handle, 52, "检测到已有本地服务", "桌面端将直接复用当前后端");
            launch_ai_service(handle, &ai_service_root, &python_runtime.command, &env);
            wait_for_port_only("127.0.0.1", api_port, 5000)?;
            emit_progress(handle, 92, "本地服务已就绪", "正在加载主界面");
            return Ok(());
        }
        return Err(format!(
            "端口 {api_port} 已被其他进程占用，且不是可复用的 RetainPDF 后端。请先关闭占用进程后再启动桌面端。"
        ));
    }

    let simple_port_busy = can_connect_to_port("127.0.0.1", simple_port, 800);
    logging::log(handle, &format!("[desktop] port {simple_port} busy={simple_port_busy}"));
    if simple_port_busy {
        return Err(format!("端口 {simple_port} 已被其他进程占用，请先释放后再启动桌面端。"));
    }

    emit_progress(handle, 52, "正在启动本地服务", "Rust API 与 Python worker 正在启动");
    logging::log(handle, &format!("[desktop] spawning backend: {}", backend_bin.display()));
    let mut child = Command::new(&backend_bin)
        .current_dir(&backend_root)
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn backend: {error}"))?;

    let stdout = child.stdout.take().ok_or("missing piped stdout")?;
    let stderr = child.stderr.take().ok_or("missing piped stderr")?;
    {
        let state = handle.state::<BackendState>();
        *state.command.lock().unwrap() = backend_bin.to_string_lossy().into_owned();
        *state.cwd.lock().unwrap() = backend_root.to_string_lossy().into_owned();
        *state.child.lock().unwrap() = Some(child);
    }

    let stdout_handle = handle.clone();
    std::thread::spawn(move || stream_output(stdout, stdout_handle, false));
    let stderr_handle = handle.clone();
    std::thread::spawn(move || stream_output(stderr, stderr_handle, true));

    launch_ai_service(handle, &ai_service_root, &python_runtime.command, &env);

    let timeout_ms = if packaged { 90_000 } else { 30_000 };
    logging::log(handle, &format!("[desktop] waiting for backend port {api_port} timeoutMs={timeout_ms}"));
    wait_for_backend_ready(handle, "127.0.0.1", api_port, timeout_ms)?;
    handle.state::<BackendState>().ready.store(true, Ordering::SeqCst);
    logging::log(handle, &format!("[desktop] backend ready on port {api_port}"));
    emit_progress(handle, 92, "本地服务已就绪", "正在加载主界面");
    Ok(())
}

pub(crate) fn create_main_window(handle: &AppHandle) -> Result<(), String> {
    let splash_handle = handle.clone();
    let _window = WebviewWindowBuilder::new(handle, "main", WebviewUrl::App("index.html".into()))
        .title("RetainPDF")
        .inner_size(1480.0, 960.0)
        .min_inner_size(1200.0, 760.0)
        .visible(false)
        .on_page_load(move |window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = window.show();
                let _ = window.set_focus();
                if let Some(splash) = splash_handle.get_webview_window("splash") {
                    let _ = splash.close();
                }
            }
        })
        .build()
        .map_err(|error| error.to_string())?;
    logging::log(handle, "[desktop] main window created");
    Ok(())
}

fn stream_output(stream: impl Read + Send, handle: AppHandle, is_stderr: bool) {
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim_end().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                logging::log(&handle, &format!("[rust_api] {trimmed}"));
                remember_output(&handle, is_stderr, trimmed);
            }
            Err(_) => break,
        }
    }
    handle_crash_if_needed(&handle);
}

fn resolve_ai_service_root(backend_root: &Path, packaged: bool) -> PathBuf {
    let bundled = backend_root.join("ai_service");
    if !packaged && !bundled.join("retainpdf_ai").join("__main__.py").exists() {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("backend")
            .join("ai_service");
        if repo.join("retainpdf_ai").join("__main__.py").exists() {
            return repo;
        }
    }
    bundled
}

fn launch_ai_service(
    handle: &AppHandle,
    ai_service_root: &Path,
    python_command: &str,
    env: &[(String, String)],
) {
    if !ai_service_root.join("retainpdf_ai").join("__main__.py").exists() {
        logging::log_error(
            handle,
            &format!(
                "[desktop] retainpdf-ai package missing under {}; AI ask will return 502",
                ai_service_root.display()
            ),
        );
        return;
    }
    if can_connect_to_port("127.0.0.1", AI_SERVICE_PORT, 800) {
        logging::log(
            handle,
            &format!(
                "[desktop] AI service port {} already in use; reusing",
                AI_SERVICE_PORT
            ),
        );
        return;
    }
    logging::log(
        handle,
        &format!(
            "[desktop] spawning retainpdf-ai: {} -m retainpdf_ai (port {})",
            python_command, AI_SERVICE_PORT
        ),
    );
    let mut child = match Command::new(python_command)
        .args(["-m", "retainpdf_ai"])
        .current_dir(ai_service_root)
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            logging::log_error(handle, &format!("[desktop] failed to spawn retainpdf-ai: {error}"));
            return;
        }
    };
    let stdout = match child.stdout.take() {
        Some(stream) => stream,
        None => return,
    };
    let stderr = match child.stderr.take() {
        Some(stream) => stream,
        None => return,
    };
    {
        let state = handle.state::<BackendState>();
        *state.ai_child.lock().unwrap() = Some(child);
    }
    let stdout_handle = handle.clone();
    std::thread::spawn(move || stream_ai_output(stdout, stdout_handle, true));
    let stderr_handle = handle.clone();
    std::thread::spawn(move || stream_ai_output(stderr, stderr_handle, false));

    let wait_handle = handle.clone();
    std::thread::spawn(move || {
        let timeout_ms = if is_packaged(&wait_handle) { 60_000 } else { 20_000 };
        let started = Instant::now();
        while !can_connect_to_port("127.0.0.1", AI_SERVICE_PORT, 800) {
            if started.elapsed().as_millis() as u64 >= timeout_ms {
                logging::log_error(
                    &wait_handle,
                    &format!(
                        "[desktop] retainpdf-ai failed to become ready on port {}",
                        AI_SERVICE_PORT
                    ),
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        logging::log(
            &wait_handle,
            &format!("[desktop] retainpdf-ai ready on port {}", AI_SERVICE_PORT),
        );
    });
}

fn stream_ai_output(stream: impl Read + Send, handle: AppHandle, report_exit: bool) {
    let mut reader = std::io::BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim_end().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                logging::log(&handle, &format!("[retainpdf_ai] {trimmed}"));
            }
            Err(_) => break,
        }
    }
    if report_exit {
        let state = handle.state::<BackendState>();
        if !state.stopping.load(Ordering::SeqCst) {
            logging::log_error(&handle, "[desktop] retainpdf-ai exited unexpectedly; AI ask will return 502");
        }
    }
}

fn remember_output(handle: &AppHandle, is_stderr: bool, line: String) {
    let state = handle.state::<BackendState>();
    let mut buffer = if is_stderr {
        state.recent_stderr.lock().unwrap()
    } else {
        state.recent_stdout.lock().unwrap()
    };
    buffer.push(line);
    if buffer.len() > 20 {
        let excess = buffer.len() - 20;
        buffer.drain(..excess);
    }
}

fn handle_crash_if_needed(handle: &AppHandle) {
    let state = handle.state::<BackendState>();
    if state.stopping.load(Ordering::SeqCst) || state.using_external.load(Ordering::SeqCst) {
        return;
    }
    if !state.ready.load(Ordering::SeqCst) {
        return;
    }
    // Both the stdout and stderr reader threads detect the process death; only one
    // should report the crash to avoid stacking duplicate dialogs.
    if state.crash_reported.swap(true, Ordering::SeqCst) {
        return;
    }
    let detail = {
        let mut guard = state.child.lock().unwrap();
        guard
            .as_mut()
            .and_then(|child| child.try_wait().ok().flatten())
            .map(|status| format!("code={} signal={}", status_code(&status), status_signal(&status)))
            .unwrap_or_else(|| "code=null signal=null".to_string())
    };
    logging::log_error(handle, &format!("[desktop] Rust API worker crashed: {detail}"));
    show_error_dialog(handle, "Rust API worker crashed", &detail);
}

fn status_code(status: &std::process::ExitStatus) -> String {
    status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| "null".to_string())
}

#[cfg(unix)]
fn status_signal(status: &std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    status
        .signal()
        .map(|signal| signal.to_string())
        .unwrap_or_else(|| "null".to_string())
}

#[cfg(not(unix))]
fn status_signal(_status: &std::process::ExitStatus) -> String {
    "null".to_string()
}

fn wait_for_backend_ready(handle: &AppHandle, host: &str, port: u16, timeout_ms: u64) -> Result<(), String> {
    let started = Instant::now();
    let mut progress = 58;
    loop {
        if can_connect_to_port(host, port, 500) {
            return Ok(());
        }
        let state = handle.state::<BackendState>();
        let exited = state
            .child
            .lock()
            .unwrap()
            .as_mut()
            .map_or(false, |child| child.try_wait().ok().flatten().is_some());
        if exited {
            return Err(build_diagnostic(handle, host, port, timeout_ms));
        }
        if started.elapsed().as_millis() as u64 >= timeout_ms {
            return Err(build_diagnostic(handle, host, port, timeout_ms));
        }
        progress = std::cmp::min(progress + 3, 88);
        emit_progress(handle, progress, "正在连接本地服务", "首次启动可能稍慢，请稍候");
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn wait_for_file(path: &Path, timeout_ms: u64) -> bool {
    let started = Instant::now();
    while !path.exists() {
        if started.elapsed().as_millis() as u64 >= timeout_ms {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    true
}

fn wait_for_port_only(host: &str, port: u16, timeout_ms: u64) -> Result<(), String> {
    let started = Instant::now();
    loop {
        if can_connect_to_port(host, port, 800) {
            return Ok(());
        }
        if started.elapsed().as_millis() as u64 >= timeout_ms {
            return Err(format!("backend did not become ready on {host}:{port}"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn build_diagnostic(handle: &AppHandle, host: &str, port: u16, timeout_ms: u64) -> String {
    let state = handle.state::<BackendState>();
    let command = state.command.lock().unwrap().clone();
    let cwd = state.cwd.lock().unwrap().clone();
    let exit_detail = state.exit_detail.lock().unwrap().clone();
    let recent_stdout = state.recent_stdout.lock().unwrap().clone();
    let recent_stderr = state.recent_stderr.lock().unwrap().clone();
    let log_path = logging::log_path(handle);
    let mut lines = vec![
        format!("backend did not become ready on {host}:{port}"),
        format!("timeout_ms={timeout_ms}"),
        format!("command={command}"),
        format!("cwd={cwd}"),
        if exit_detail.is_empty() {
            "backend_exit=<still-running-or-unknown>".to_string()
        } else {
            format!("backend_exit={exit_detail}")
        },
        format!("desktop_log={}", log_path.display()),
    ];
    if !recent_stdout.is_empty() {
        lines.push("recent_stdout:".to_string());
        lines.extend(recent_stdout);
    }
    if !recent_stderr.is_empty() {
        lines.push("recent_stderr:".to_string());
        lines.extend(recent_stderr);
    }
    lines.join("\n")
}

fn emit_progress(handle: &AppHandle, progress: u8, title: &str, detail: &str) {
    let _ = handle.emit_to(
        EventTarget::window("splash"),
        "startup-progress",
        serde_json::json!({
            "progress": progress,
            "title": title,
            "detail": detail,
        }),
    );
}

fn show_error_dialog(handle: &AppHandle, title: &str, detail: &str) {
    let _ = handle
        .dialog()
        .message(detail.to_string())
        .title(title.to_string())
        .kind(MessageDialogKind::Error)
        .blocking_show();
}

fn is_packaged(_handle: &AppHandle) -> bool {
    !cfg!(debug_assertions)
}

fn resolve_backend_root(handle: &AppHandle) -> PathBuf {
    if is_packaged(handle) {
        if let Ok(resource) = handle.path().resource_dir() {
            let resource_backend = resource.join("backend");
            if resource_backend.exists() {
                return resource_backend;
            }
        }
    }
    let dev_backend = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../app/backend");
    if dev_backend.exists() {
        return dev_backend;
    }
    if let Ok(resource) = handle.path().resource_dir() {
        resource.join("backend")
    } else {
        dev_backend
    }
}

fn resolve_backend_binary(backend_root: &Path) -> PathBuf {
    if cfg!(windows) {
        backend_root.join("bin").join("rust_api.exe")
    } else {
        backend_root.join("bin").join("rust_api")
    }
}

fn resolve_typst_binary(backend_root: &Path, packaged: bool) -> Option<PathBuf> {
    let mut candidates = if cfg!(windows) {
        vec![backend_root.join("typst").join("bin").join("typst.exe")]
    } else {
        vec![backend_root.join("typst").join("bin").join("typst")]
    };
    if !packaged {
        candidates.push(PathBuf::from("/usr/local/bin/typst"));
        candidates.push(PathBuf::from("/opt/homebrew/bin/typst"));
    }
    candidates.into_iter().find(|candidate| candidate.exists())
}

fn can_connect_to_port(host: &str, port: u16, timeout_ms: u64) -> bool {
    match (host, port).to_socket_addrs() {
        Ok(mut addrs) => addrs.next().map_or(false, |addr| {
            TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)).is_ok()
        }),
        Err(_) => false,
    }
}
