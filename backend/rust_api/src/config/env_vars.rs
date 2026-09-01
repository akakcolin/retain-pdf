use std::env;
use std::path::PathBuf;

pub(super) fn env_u64(name: &str, fallback: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

pub(super) fn env_u32(name: &str, fallback: u32) -> u32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

pub(super) fn env_u16(name: &str, fallback: u16) -> u16 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

pub(super) fn env_usize(name: &str, fallback: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

pub(super) fn env_bool(name: &str, fallback: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(fallback)
}

pub(super) fn env_string(name: &str, fallback: &str) -> String {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

pub(super) fn env_optional_string(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn env_path(name: &str) -> Option<PathBuf> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// RETAIN_OFFLINE=1/true/yes 开启离线偏好:配置了本地组件就偏好本地,否则回退云端。
pub(crate) fn offline_mode() -> bool {
    env_bool("RETAIN_OFFLINE", false)
}

pub(crate) fn local_llm_default_model() -> String {
    env_optional_string("RUST_API_LOCAL_LLM_MODEL")
        .or_else(|| env_optional_string("RETAIN_LOCAL_LLM_MODEL"))
        .unwrap_or_else(|| "qwen2.5:7b".to_string())
}

pub(crate) fn local_llm_default_base_url() -> String {
    env_optional_string("RUST_API_LOCAL_LLM_BASE_URL")
        .or_else(|| env_optional_string("RETAIN_LOCAL_LLM_BASE_URL"))
        .unwrap_or_else(|| "http://localhost:11434/v1".to_string())
}
