use std::fs;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::constants::DESKTOP_API_KEY;
use crate::desktop_config;

#[tauri::command]
pub fn load_desktop_config(app: AppHandle) -> Value {
    let config = desktop_config::load(&app);
    desktop_config::build_response(&config, DESKTOP_API_KEY)
}

#[tauri::command]
pub fn save_desktop_config(app: AppHandle, payload: Value) -> Value {
    let config = desktop_config::save(&app, &payload);
    desktop_config::build_response(&config, DESKTOP_API_KEY)
}

#[tauri::command]
pub fn open_output_directory(app: AppHandle) -> Result<Value, String> {
    let output_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("data")
        .join("jobs");
    fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    open::that(&output_dir).map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "outputDir": output_dir.to_string_lossy(),
    }))
}
