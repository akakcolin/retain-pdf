use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager, RunEvent};

use crate::backend_startup::{self, BackendState};

pub fn on_event(handle: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::Exit => kill_backend(handle),
        RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Destroyed,
            ..
        } => {
            if label == "main" && !cfg!(target_os = "macos") {
                handle.exit(0);
            }
        }
        RunEvent::Reopen { .. } => {
            if cfg!(target_os = "macos") && handle.get_webview_window("main").is_none() {
                let _ = backend_startup::create_main_window(handle);
            }
        }
        _ => {}
    }
}

fn kill_backend(handle: &AppHandle) {
    let state = handle.state::<BackendState>();
    state.stopping.store(true, Ordering::SeqCst);
    let mut ai_guard = state.ai_child.lock().unwrap();
    if let Some(child) = ai_guard.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
    if state.using_external.load(Ordering::SeqCst) {
        return;
    }
    let mut guard = state.child.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
}
