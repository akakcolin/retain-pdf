use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Response};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::LocalPaddlexRuntimeConfig;
use crate::ocr_provider::local::errors::LocalPaddlexProviderError;

/// PaddleX `PP-StructureV3` request options, mirrored verbatim from
/// `paddle_api.py::build_optional_payload("PP-StructureV3")` so the Rust client
/// sends byte-identical payloads to the reference Python wrapper.
pub fn build_paddlex_optional_payload() -> Value {
    json!({
        "max_num_input_imgs": 999,
        "markdownIgnoreLabels": [
            "header",
            "header_image",
            "footer",
            "footer_image",
            "number",
            "footnote",
            "aside_text",
        ],
        "useChartRecognition": false,
        "useRegionDetection": true,
        "useDocOrientationClassify": false,
        "useDocUnwarping": false,
        "useTextlineOrientation": false,
        "useSealRecognition": true,
        "useFormulaRecognition": true,
        "useTableRecognition": true,
        "layoutThreshold": 0.5,
        "layoutNms": true,
        "layoutUnclipRatio": 1,
        "textDetLimitType": "min",
        "textDetLimitSideLen": 64,
        "textDetThresh": 0.3,
        "textDetBoxThresh": 0.6,
        "textDetUnclipRatio": 1.5,
        "textRecScoreThresh": 0,
        "sealDetLimitType": "min",
        "sealDetLimitSideLen": 736,
        "sealDetThresh": 0.2,
        "sealDetBoxThresh": 0.6,
        "sealDetUnclipRatio": 0.5,
        "sealRecScoreThresh": 0,
        "useTableOrientationClassify": true,
        "useOcrResultsWithTableCells": true,
        "useE2eWiredTableRecModel": false,
        "useE2eWirelessTableRecModel": false,
        "useWiredTableCellsTransToHtml": false,
        "useWirelessTableCellsTransToHtml": false,
        "parseLanguage": "default",
        "visualize": false,
    })
}

#[derive(Debug, Clone)]
pub struct LocalPaddlexClient {
    pub base_url: String,
    http: Client,
}

#[derive(Debug, Clone)]
pub struct LocalPaddlexResultPayload {
    pub payload: Value,
    pub log_id: Option<String>,
}

impl LocalPaddlexClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_runtime(base_url, LocalPaddlexRuntimeConfig::from_env())
    }

    pub fn with_runtime(
        base_url: impl Into<String>,
        runtime: LocalPaddlexRuntimeConfig,
    ) -> Self {
        let base_url = {
            let raw = base_url.into();
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                runtime.default_base_url.trim_end_matches('/').to_string()
            } else {
                trimmed.trim_end_matches('/').to_string()
            }
        };
        let http = build_http_client(&runtime);
        Self { base_url, http }
    }

    /// `local_paddlex_wrapper.run` — POST the base64-embedded PDF to
    /// `{base}/layout-parsing` and return the PP-StructureV3 `result` object.
    pub async fn layout_parse(&self, source_pdf: &Path) -> Result<LocalPaddlexResultPayload> {
        let file_bytes = tokio::fs::read(source_pdf)
            .await
            .with_context(|| format!("failed to read source pdf {}", source_pdf.display()))?;
        let request_payload = build_request_payload(&file_bytes);
        let url = format!("{}/layout-parsing", self.base_url);
        let response = self
            .http
            .post(&url)
            .header(CONTENT_TYPE, "application/json")
            .json(&request_payload)
            .send()
            .await
            .map_err(|err| {
                anyhow::Error::new(LocalPaddlexProviderError::request_failed(
                    "layout_parse",
                    &err,
                    None,
                ))
            })?;
        let envelope = parse_json_response("layout_parse", response).await?;
        if envelope.error_code != 0 {
            return Err(anyhow::Error::new(LocalPaddlexProviderError::provider_error(
                "layout_parse",
                envelope.error_code,
                &envelope.error_msg,
                normalize_trace_id(&envelope.log_id).as_deref(),
            )));
        }
        let log_id = normalize_trace_id(&envelope.log_id);
        let result = envelope
            .result
            .filter(|value| {
                value
                    .get("layoutParsingResults")
                    .and_then(Value::as_array)
                    .is_some()
            })
            .ok_or_else(|| {
                anyhow::Error::new(LocalPaddlexProviderError::invalid_response(
                    "layout_parse",
                    "PaddleX layout-parsing response missing result.layoutParsingResults",
                    log_id.as_deref(),
                ))
            })?;
        Ok(LocalPaddlexResultPayload {
            payload: result,
            log_id,
        })
    }
}

fn build_request_payload(file_bytes: &[u8]) -> Value {
    let base64_file = base64::engine::general_purpose::STANDARD.encode(file_bytes);
    // Mirrors `local_paddlex_wrapper.build_request_payload`: `payload.update(...)`.
    let mut payload = json!({
        "file": base64_file,
        "fileType": 0,
    });
    if let Some(optional_obj) = build_paddlex_optional_payload().as_object() {
        if let Some(payload_obj) = payload.as_object_mut() {
            for (key, value) in optional_obj {
                payload_obj.insert(key.clone(), value.clone());
            }
        }
    }
    payload
}

fn build_http_client(runtime: &LocalPaddlexRuntimeConfig) -> Client {
    Client::builder()
        .connect_timeout(Duration::from_secs(runtime.request_timeout_secs))
        .timeout(Duration::from_secs(runtime.request_timeout_secs))
        .no_proxy()
        .build()
        .expect("reqwest client")
}

fn normalize_trace_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct LocalPaddlexEnvelope {
    #[serde(default, rename = "logId")]
    log_id: String,
    #[serde(default, rename = "errorCode")]
    error_code: i64,
    #[serde(default, rename = "errorMsg")]
    error_msg: String,
    result: Option<Value>,
}

async fn parse_json_response(
    stage: &'static str,
    response: Response,
) -> Result<LocalPaddlexEnvelope> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(|err| {
        anyhow::Error::new(LocalPaddlexProviderError::request_failed(stage, &err, None))
    })?;
    if !status.is_success() {
        return Err(anyhow::Error::new(LocalPaddlexProviderError::http_status(
            stage,
            status,
            &String::from_utf8_lossy(&bytes),
            None,
        )));
    }
    let envelope = serde_json::from_slice::<LocalPaddlexEnvelope>(&bytes).with_context(|| {
        format!(
            "failed to parse PaddleX JSON: {}",
            String::from_utf8_lossy(&bytes)
        )
    })?;
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::build_paddlex_optional_payload;
    use super::{LocalPaddlexClient, LocalPaddlexEnvelope};
    use crate::config::LocalPaddlexRuntimeConfig;
    use serde_json::json;

    #[test]
    fn paddlex_optional_payload_has_expected_keys() {
        let payload = build_paddlex_optional_payload();
        let object = payload.as_object().expect("payload is an object");
        for key in [
            "max_num_input_imgs",
            "markdownIgnoreLabels",
            "useChartRecognition",
            "useRegionDetection",
            "useDocOrientationClassify",
            "useDocUnwarping",
            "useTextlineOrientation",
            "useSealRecognition",
            "useFormulaRecognition",
            "useTableRecognition",
            "layoutThreshold",
            "layoutNms",
            "layoutUnclipRatio",
            "textDetLimitType",
            "textDetLimitSideLen",
            "textDetThresh",
            "textDetBoxThresh",
            "textDetUnclipRatio",
            "textRecScoreThresh",
            "sealDetLimitType",
            "sealDetLimitSideLen",
            "sealDetThresh",
            "sealDetBoxThresh",
            "sealDetUnclipRatio",
            "sealRecScoreThresh",
            "useTableOrientationClassify",
            "useOcrResultsWithTableCells",
            "useE2eWiredTableRecModel",
            "useE2eWirelessTableRecModel",
            "useWiredTableCellsTransToHtml",
            "useWirelessTableCellsTransToHtml",
            "parseLanguage",
            "visualize",
        ] {
            assert!(object.contains_key(key), "missing key {key}");
        }
        assert_eq!(object["max_num_input_imgs"], 999);
        assert_eq!(object["visualize"], false);
    }

    #[test]
    fn envelope_parses_paddlex_layout_success_response() {
        let envelope: LocalPaddlexEnvelope = serde_json::from_value(json!({
            "errorCode": 0,
            "errorMsg": "",
            "logId": "log-123",
            "result": {
                "layoutParsingResults": [{"page": 1}],
                "dataInfo": {}
            }
        }))
        .expect("parse envelope");

        assert_eq!(envelope.error_code, 0);
        assert_eq!(envelope.log_id, "log-123");
        let result = envelope.result.expect("result present");
        assert!(result.get("layoutParsingResults").and_then(|v| v.as_array()).is_some());
    }

    #[test]
    fn envelope_defaults_missing_error_fields() {
        let envelope: LocalPaddlexEnvelope = serde_json::from_value(json!({
            "result": {"layoutParsingResults": []}
        }))
        .expect("parse envelope");

        assert_eq!(envelope.error_code, 0);
        assert_eq!(envelope.error_msg, "");
        assert_eq!(envelope.log_id, "");
        assert!(envelope.result.is_some());
    }

    #[test]
    fn envelope_reports_non_zero_error_code() {
        let envelope: LocalPaddlexEnvelope = serde_json::from_value(json!({
            "errorCode": 400,
            "errorMsg": "bad pdf",
            "logId": "log-bad",
            "result": null
        }))
        .expect("parse envelope");

        assert_eq!(envelope.error_code, 400);
        assert_eq!(envelope.error_msg, "bad pdf");
        assert!(envelope.result.is_none());
    }

    #[tokio::test]
    async fn layout_parse_round_trips_against_local_stub_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let canned_body = br#"{"errorCode":0,"errorMsg":"","logId":"log-e2e","result":{"layoutParsingResults":[{"page":1}],"dataInfo":{}}}"#;
        let canned_headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            canned_body.len()
        );

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).await.expect("read");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n");
                if let Some(header_end) = header_end {
                    let body_start = header_end + 4;
                    let cl = String::from_utf8_lossy(&buf[..header_end])
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if buf.len() >= body_start + cl {
                        break;
                    }
                }
            }
            let received = String::from_utf8_lossy(&buf);
            assert!(received.contains("\"fileType\":0"), "must send fileType=0");
            assert!(
                received.contains("\"max_num_input_imgs\""),
                "must send PP-StructureV3 options"
            );
            assert!(
                received.contains("\"parseLanguage\":\"default\""),
                "must send parseLanguage default"
            );
            socket
                .write_all(canned_headers.as_bytes())
                .await
                .expect("write headers");
            socket.write_all(canned_body).await.expect("write body");
            socket.flush().await.expect("flush");
        });

        let runtime = LocalPaddlexRuntimeConfig {
            default_base_url: format!("http://{addr}"),
            request_timeout_secs: 30,
            allow_private_urls: true,
        };
        let client = LocalPaddlexClient::with_runtime("", runtime);
        let tmp = std::env::temp_dir().join(format!("local-paddlex-e2e-{}.pdf", fastrand::u64(..)));
        std::fs::write(&tmp, b"%PDF-1.4 test").expect("write temp pdf");
        let result = client.layout_parse(&tmp).await.expect("layout_parse");
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(result.log_id.as_deref(), Some("log-e2e"));
        assert_eq!(result.payload["layoutParsingResults"][0]["page"], 1);
        server.await.expect("stub server");
    }

    #[tokio::test]
    async fn layout_parse_surfaces_provider_error_from_stub_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let canned_body =
            br#"{"errorCode":400,"errorMsg":"bad pdf","logId":"log-bad","result":null}"#;
        let canned_headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            canned_body.len()
        );

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut chunk = [0u8; 4096];
            let mut total = 0usize;
            while total < 1024 {
                let n = socket.read(&mut chunk).await.expect("read");
                if n == 0 {
                    break;
                }
                total += n;
                if chunk.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            socket
                .write_all(canned_headers.as_bytes())
                .await
                .expect("write headers");
            socket.write_all(canned_body).await.expect("write body");
            socket.flush().await.expect("flush");
        });

        let runtime = LocalPaddlexRuntimeConfig {
            default_base_url: format!("http://{addr}"),
            request_timeout_secs: 30,
            allow_private_urls: true,
        };
        let client = LocalPaddlexClient::with_runtime("", runtime);
        let tmp = std::env::temp_dir().join(format!("local-paddlex-e2e-{}.pdf", fastrand::u64(..)));
        std::fs::write(&tmp, b"%PDF-1.4 test").expect("write temp pdf");
        let err = client.layout_parse(&tmp).await.expect_err("provider error");
        let _ = std::fs::remove_file(&tmp);
        assert!(err
            .to_string()
            .contains("PaddleX layout-parsing 返回 errorCode=400"));
        server.await.expect("stub server");
    }
}
