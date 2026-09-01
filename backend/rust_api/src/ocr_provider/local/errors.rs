use std::error::Error;
use std::fmt;

use reqwest::StatusCode;

use crate::ocr_provider::types::{OcrErrorCategory, OcrProviderErrorInfo};

/// Error type for the local PaddleX layout-parsing transport, carrying the
/// standard `OcrProviderErrorInfo` so job diagnostics stay uniform across
/// providers.
#[derive(Debug, Clone)]
pub struct LocalPaddlexProviderError {
    stage: &'static str,
    detail: String,
    info: OcrProviderErrorInfo,
}

impl LocalPaddlexProviderError {
    pub fn request_failed(
        stage: &'static str,
        err: &reqwest::Error,
        trace_id: Option<&str>,
    ) -> Self {
        if let Some(status) = err.status() {
            return Self::http_status(stage, status, &err.to_string(), trace_id);
        }
        let category = if err.is_timeout() {
            OcrErrorCategory::RemoteReadTimeout
        } else {
            OcrErrorCategory::ServiceUnavailable
        };
        Self::new(
            stage,
            category,
            err.to_string(),
            trace_id,
            None,
            None,
            Some("请检查本地 PaddleX 服务可达性、网络连通性和超时配置"),
        )
    }

    pub fn http_status(
        stage: &'static str,
        status: StatusCode,
        body_excerpt: &str,
        trace_id: Option<&str>,
    ) -> Self {
        let message = format!(
            "HTTP {}{}",
            status.as_u16(),
            sanitize_body_excerpt(body_excerpt)
                .map(|text| format!(": {text}"))
                .unwrap_or_default()
        );
        let category = match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => OcrErrorCategory::Unauthorized,
            StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => {
                OcrErrorCategory::RemoteReadTimeout
            }
            StatusCode::BAD_REQUEST
            | StatusCode::UNPROCESSABLE_ENTITY
            | StatusCode::METHOD_NOT_ALLOWED => OcrErrorCategory::InvalidRequest,
            StatusCode::TOO_MANY_REQUESTS => OcrErrorCategory::QueueFull,
            _ if status.is_server_error() => OcrErrorCategory::ServiceUnavailable,
            _ => OcrErrorCategory::HttpStatus,
        };
        Self::new(
            stage,
            category,
            "本地 PaddleX HTTP 请求失败".to_string(),
            trace_id,
            None,
            Some(message),
            Some("请检查本地 PaddleX 服务地址和服务状态"),
        )
        .with_http_status(status.as_u16())
    }

    pub fn provider_error(
        stage: &'static str,
        provider_code: i64,
        provider_message: &str,
        trace_id: Option<&str>,
    ) -> Self {
        let category = match provider_code {
            401 | 403 => OcrErrorCategory::Unauthorized,
            429 => OcrErrorCategory::QueueFull,
            code if code >= 500 => OcrErrorCategory::ServiceUnavailable,
            _ => OcrErrorCategory::ProviderFailed,
        };
        Self::new(
            stage,
            category,
            format!("PaddleX layout-parsing 返回 errorCode={provider_code}"),
            trace_id,
            Some(provider_code.to_string()),
            Some(provider_message.trim().to_string()),
            Some("请结合 PaddleX errorMsg 和 logId 排查"),
        )
    }

    pub fn invalid_response(
        stage: &'static str,
        detail: impl Into<String>,
        trace_id: Option<&str>,
    ) -> Self {
        Self::new(
            stage,
            OcrErrorCategory::InvalidProviderResponse,
            detail.into(),
            trace_id,
            None,
            None,
            Some("请检查本地 PaddleX 返回结构是否完整，重点确认 result.layoutParsingResults"),
        )
    }

    pub fn info(&self) -> &OcrProviderErrorInfo {
        &self.info
    }

    pub fn stage_detail(&self) -> String {
        let prefix = match self.stage {
            "layout_parse" => "本地 PaddleX 解析失败",
            _ => "本地 PaddleX provider 失败",
        };
        let message = self
            .info
            .provider_message
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(self.detail.as_str());
        let trace_suffix = self
            .info
            .trace_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!(" trace_id={value}"))
            .unwrap_or_default();
        format!("{prefix}: {message}{trace_suffix}")
    }

    fn new(
        stage: &'static str,
        category: OcrErrorCategory,
        detail: String,
        trace_id: Option<&str>,
        provider_code: Option<String>,
        provider_message: Option<String>,
        operator_hint: Option<&str>,
    ) -> Self {
        Self {
            stage,
            detail,
            info: OcrProviderErrorInfo {
                category,
                provider_code,
                provider_message,
                operator_hint: operator_hint.map(str::to_string),
                trace_id: trace_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                http_status: None,
            },
        }
    }

    fn with_http_status(mut self, http_status: u16) -> Self {
        self.info.http_status = Some(http_status);
        self
    }
}

impl fmt::Display for LocalPaddlexProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl Error for LocalPaddlexProviderError {}

fn sanitize_body_excerpt(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let single_line = trimmed.replace('\n', " ");
    Some(single_line.chars().take(180).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_maps_unauthorized() {
        let err = LocalPaddlexProviderError::http_status(
            "layout_parse",
            StatusCode::UNAUTHORIZED,
            r#"{"errorCode":401,"errorMsg":"bad token"}"#,
            Some("log-1"),
        );

        assert_eq!(err.info().category, OcrErrorCategory::Unauthorized);
        assert_eq!(err.info().http_status, Some(401));
        assert_eq!(err.info().trace_id.as_deref(), Some("log-1"));
    }

    #[test]
    fn provider_error_preserves_code_and_message() {
        let err =
            LocalPaddlexProviderError::provider_error("layout_parse", 400, "bad pdf", Some("l2"));

        assert_eq!(err.info().category, OcrErrorCategory::ProviderFailed);
        assert_eq!(err.info().provider_code.as_deref(), Some("400"));
        assert_eq!(err.info().provider_message.as_deref(), Some("bad pdf"));
    }

    #[test]
    fn provider_error_maps_429_to_queue_full() {
        let err = LocalPaddlexProviderError::provider_error("layout_parse", 429, "busy", None);

        assert_eq!(err.info().category, OcrErrorCategory::QueueFull);
    }
}
