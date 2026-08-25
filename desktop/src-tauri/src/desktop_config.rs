use std::fs;
use std::path::PathBuf;

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

const DEFAULT_OCR_PROVIDER: &str = "paddle";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";

pub fn create_default_config() -> Value {
    json!({
        "firstRunCompleted": false,
        "ocrProvider": DEFAULT_OCR_PROVIDER,
        "mineruToken": "",
        "paddleToken": "",
        "modelApiKey": "",
        "model": DEFAULT_MODEL,
        "baseUrl": DEFAULT_BASE_URL,
        "developerConfig": {},
        "closeToTrayHintShown": false,
    })
}

fn as_bool(value: &Value, fallback: bool) -> bool {
    value.as_bool().unwrap_or(fallback)
}

fn as_string(value: &Value) -> String {
    value.as_str().unwrap_or("").trim().to_string()
}

fn as_object(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(map.clone()),
        _ => Value::Object(Map::new()),
    }
}

fn has_own(value: &Value, key: &str) -> bool {
    value.get(key).is_some()
}

fn normalize_config(raw: &Value) -> Value {
    let map = match raw {
        Value::Object(map) => map,
        _ => return create_default_config(),
    };
    let mut result = create_default_config();
    if let Some(value) = map.get("firstRunCompleted") {
        result["firstRunCompleted"] = Value::Bool(as_bool(value, false));
    }
    if let Some(value) = map.get("ocrProvider") {
        let provider = as_string(value);
        result["ocrProvider"] = Value::String(if provider == "paddle" {
            "paddle".to_string()
        } else {
            DEFAULT_OCR_PROVIDER.to_string()
        });
    }
    for key in ["mineruToken", "paddleToken", "modelApiKey"] {
        if let Some(value) = map.get(key) {
            result[key] = Value::String(as_string(value));
        }
    }
    if let Some(value) = map.get("model") {
        let model = as_string(value);
        result["model"] = Value::String(if model.is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            model
        });
    }
    if let Some(value) = map.get("baseUrl") {
        let base_url = as_string(value);
        result["baseUrl"] = Value::String(if base_url.is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            base_url
        });
    }
    if let Some(value) = map.get("developerConfig") {
        result["developerConfig"] = as_object(value);
    }
    if let Some(value) = map.get("closeToTrayHintShown") {
        result["closeToTrayHintShown"] = Value::Bool(as_bool(value, false));
    }
    result
}

fn merge_config(current: &Value, payload: &Value) -> Value {
    let mut merged = current.clone();
    let runtime_config = match payload.get("runtimeConfig") {
        Some(value) if value.is_object() => value.clone(),
        _ => Value::Object(Map::new()),
    };
    let keys = [
        "ocrProvider",
        "mineruToken",
        "paddleToken",
        "modelApiKey",
        "model",
        "baseUrl",
        "closeToTrayHintShown",
    ];
    for key in keys {
        if has_own(payload, key) {
            if let Some(value) = payload.get(key) {
                merged[key] = value.clone();
            }
        } else if has_own(&runtime_config, key) {
            if let Some(value) = runtime_config.get(key) {
                merged[key] = value.clone();
            }
        }
    }
    if has_own(payload, "developerConfig") {
        if let Some(value) = payload.get("developerConfig") {
            merged["developerConfig"] = as_object(value);
        }
    }
    if has_own(payload, "firstRunCompleted") {
        if let Some(value) = payload.get("firstRunCompleted") {
            merged["firstRunCompleted"] = Value::Bool(as_bool(value, false));
        }
    }
    normalize_config(&merged)
}

fn build_browser_config(config: &Value) -> Value {
    json!({
        "ocrProvider": config.get("ocrProvider").and_then(Value::as_str).unwrap_or(DEFAULT_OCR_PROVIDER),
        "mineruToken": config.get("mineruToken").and_then(Value::as_str).unwrap_or(""),
        "paddleToken": config.get("paddleToken").and_then(Value::as_str).unwrap_or(""),
        "modelApiKey": config.get("modelApiKey").and_then(Value::as_str).unwrap_or(""),
    })
}

fn build_runtime_config(config: &Value, api_key: &str) -> Value {
    let browser_config = build_browser_config(config);
    let mut runtime = json!({
        "apiBase": "http://127.0.0.1:41000",
        "xApiKey": api_key,
        "model": config.get("model").and_then(Value::as_str).unwrap_or(DEFAULT_MODEL),
        "baseUrl": config.get("baseUrl").and_then(Value::as_str).unwrap_or(DEFAULT_BASE_URL),
        "developerConfig": config.get("developerConfig").cloned().unwrap_or_else(|| json!({})),
    });
    for key in ["ocrProvider", "mineruToken", "paddleToken", "modelApiKey"] {
        runtime[key] = browser_config[key].clone();
    }
    runtime
}

pub fn build_response(config: &Value, api_key: &str) -> Value {
    json!({
        "firstRunCompleted": as_bool(config.get("firstRunCompleted").unwrap_or(&Value::Bool(false)), false),
        "closeToTrayHintShown": as_bool(config.get("closeToTrayHintShown").unwrap_or(&Value::Bool(false)), false),
        "browserConfig": build_browser_config(config),
        "developerConfig": config.get("developerConfig").cloned().unwrap_or_else(|| json!({})),
        "runtimeConfig": build_runtime_config(config, api_key),
    })
}

fn config_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|root| root.join("desktop-config.json"))
        .unwrap_or_default()
}

pub fn load(app: &AppHandle) -> Value {
    let path = config_path(app);
    if !path.exists() {
        return create_default_config();
    }
    let raw = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!("[desktop] failed to load desktop config: {error}");
            return create_default_config();
        }
    };
    match serde_json::from_str(&raw) {
        Ok(value) => normalize_config(&value),
        Err(error) => {
            eprintln!("[desktop] failed to parse desktop config: {error}");
            create_default_config()
        }
    }
}

pub fn save(app: &AppHandle, payload: &Value) -> Value {
    let next = merge_config(&load(app), payload);
    let path = config_path(app);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let serialized = serde_json::to_string_pretty(&next).unwrap_or_else(|_| "{}".to_string());
    let _ = fs::write(&path, format!("{serialized}\n"));
    next
}
