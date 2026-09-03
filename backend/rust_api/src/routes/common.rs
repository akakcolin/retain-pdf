use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use axum::http::{header, HeaderMap};
use axum::Json;

use crate::app::{build_jobs_facade_from_state, AppState};
use crate::config::{
    AiRuntimeConfig, DeepSeekRuntimeConfig, MineruRuntimeConfig, PaddleRuntimeConfig,
};
use crate::db::Db;
use crate::metrics::MetricsRegistry;
use crate::models::api::ApiResponse;
use crate::services::jobs::JobsFacade;
use crate::services::library::LibraryDeps;

pub fn ok_json<T>(value: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse::ok(value))
}

pub struct JobsRouteDeps<'a> {
    pub jobs: JobsFacade<'a>,
    pub default_port: u16,
}

pub fn build_jobs_route_deps(state: &AppState) -> JobsRouteDeps<'_> {
    JobsRouteDeps {
        jobs: build_jobs_facade_from_state(state),
        default_port: state.config.port,
    }
}

pub fn jobs_facade(deps: JobsRouteDeps<'_>) -> JobsFacade<'_> {
    deps.jobs
}

pub fn request_base_url(headers: &HeaderMap, default_port: u16) -> String {
    let scheme = forwarded_header(headers, "x-forwarded-proto")
        .or_else(|| forwarded_header(headers, "x-scheme"))
        .unwrap_or_else(|| "http".to_string());
    let host = forwarded_header(headers, "x-forwarded-host")
        .or_else(|| forwarded_header(headers, header::HOST.as_str()))
        .unwrap_or_else(|| format!("127.0.0.1:{default_port}"));
    let forwarded_port =
        forwarded_header(headers, "x-forwarded-port").filter(|value| !value.is_empty());
    let (hostname, host_port) = split_host_port(&host);
    let candidate_port = host_port.or(forwarded_port);
    let normalized_host = match candidate_port {
        Some(port) if should_omit_port_for_scheme(&scheme, &port) => hostname,
        Some(port) => format!("{hostname}:{port}"),
        None => hostname,
    };
    format!("{scheme}://{normalized_host}")
}

fn forwarded_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').next().unwrap_or(v).trim().to_string())
        .filter(|v| !v.is_empty())
}

fn split_host_port(host: &str) -> (String, Option<String>) {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    if trimmed.starts_with('[') {
        return (trimmed.to_string(), None);
    }
    if let Some((name, port)) = trimmed.rsplit_once(':') {
        if !name.is_empty() && !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) {
            return (name.to_string(), Some(port.to_string()));
        }
    }
    (trimmed.to_string(), None)
}

fn should_omit_port_for_scheme(scheme: &str, port: &str) -> bool {
    match scheme {
        "https" => port == "443",
        "http" => port == "80",
        _ => false,
    }
}

pub struct UploadRouteDeps<'a> {
    pub db: &'a Db,
    pub uploads_dir: &'a Path,
    pub upload_max_bytes: u64,
    pub upload_max_pages: u32,
    pub upload_max_complexity: u64,
    pub python_bin: &'a str,
}

pub fn build_upload_route_deps(state: &AppState) -> UploadRouteDeps<'_> {
    UploadRouteDeps {
        db: state.db.as_ref(),
        uploads_dir: &state.config.uploads_dir,
        upload_max_bytes: state.config.upload_max_bytes,
        upload_max_pages: state.config.upload_max_pages,
        upload_max_complexity: state.config.upload_max_complexity,
        python_bin: &state.config.python_bin,
    }
}

pub struct GlossaryRouteDeps<'a> {
    pub db: &'a Db,
}

pub fn build_glossary_route_deps(state: &AppState) -> GlossaryRouteDeps<'_> {
    GlossaryRouteDeps {
        db: state.db.as_ref(),
    }
}

pub struct LibraryRouteDeps<'a> {
    pub library: LibraryDeps<'a>,
    /// Jobs creation path for document translate-from-library (and future library→job flows).
    pub jobs: JobsFacade<'a>,
    pub default_port: u16,
}

pub fn build_library_route_deps(state: &AppState) -> LibraryRouteDeps<'_> {
    LibraryRouteDeps {
        library: LibraryDeps {
            db: state.db.as_ref(),
            data_root: &state.config.data_root,
            output_root: &state.config.output_root,
            downloads_dir: &state.config.downloads_dir,
            scripts_dir: &state.config.scripts_dir,
            python_bin: &state.config.python_bin,
        },
        jobs: build_jobs_facade_from_state(state),
        default_port: state.config.port,
    }
}

pub struct HealthRouteDeps<'a> {
    pub db: &'a Db,
    pub metrics: &'a MetricsRegistry,
    pub data_root: &'a Path,
}

pub fn build_health_route_deps(state: &AppState) -> HealthRouteDeps<'_> {
    HealthRouteDeps {
        db: state.db.as_ref(),
        metrics: &state.metrics,
        data_root: &state.config.data_root,
    }
}

/// /metrics 与 /health 依赖形状一致(db + metrics + data_root),复用同一构建器。
pub fn build_metrics_route_deps(state: &AppState) -> HealthRouteDeps<'_> {
    build_health_route_deps(state)
}

pub struct AiRouteDeps<'a> {
    /// Arc 句柄:SSE 路径要把 db 克隆进后台任务,非流式路径用 as_ref() 借用。
    pub db: &'a Arc<Db>,
    pub data_root: &'a Path,
    pub ai: &'a AiRuntimeConfig,
}

pub fn build_ai_route_deps(state: &AppState) -> AiRouteDeps<'_> {
    AiRouteDeps {
        db: &state.db,
        data_root: &state.config.data_root,
        ai: &state.config.ai,
    }
}

pub struct AuthRouteDeps<'a> {
    pub api_keys: &'a HashSet<String>,
}

pub fn build_auth_route_deps(state: &AppState) -> AuthRouteDeps<'_> {
    AuthRouteDeps {
        api_keys: &state.config.api_keys,
    }
}

pub struct ProviderRouteDeps {
    pub mineru_runtime: MineruRuntimeConfig,
    pub paddle_runtime: PaddleRuntimeConfig,
    pub deepseek_runtime: DeepSeekRuntimeConfig,
}

pub fn build_provider_route_deps(state: &AppState) -> ProviderRouteDeps {
    ProviderRouteDeps {
        mineru_runtime: state.config.provider_runtime.mineru.clone(),
        paddle_runtime: state.config.provider_runtime.paddle.clone(),
        deepseek_runtime: state.config.provider_runtime.deepseek.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn request_base_url_prefers_forwarded_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("8443"));

        let base_url = request_base_url(&headers, 41000);
        assert_eq!(base_url, "https://example.com:8443");
    }

    #[test]
    fn request_base_url_prefers_port_embedded_in_forwarded_host() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("qzlab:40001"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("80"));

        let base_url = request_base_url(&headers, 41000);
        assert_eq!(base_url, "http://qzlab:40001");
    }

    #[test]
    fn request_base_url_omits_default_https_port() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("443"));

        let base_url = request_base_url(&headers, 41000);
        assert_eq!(base_url, "https://example.com");
    }

    #[test]
    fn request_base_url_omits_default_http_port() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("80"));

        let base_url = request_base_url(&headers, 41000);
        assert_eq!(base_url, "http://example.com");
    }

    #[test]
    fn request_base_url_omits_default_port_embedded_in_forwarded_host() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("example.com:443"),
        );

        let base_url = request_base_url(&headers, 41000);
        assert_eq!(base_url, "https://example.com");
    }

    #[test]
    fn request_base_url_keeps_non_default_https_port() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("80"));

        let base_url = request_base_url(&headers, 41000);
        assert_eq!(base_url, "https://example.com:80");
    }

    #[test]
    fn request_base_url_keeps_non_default_http_port() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        headers.insert("x-forwarded-host", HeaderValue::from_static("example.com"));
        headers.insert("x-forwarded-port", HeaderValue::from_static("443"));

        let base_url = request_base_url(&headers, 41000);
        assert_eq!(base_url, "http://example.com:443");
    }
}
