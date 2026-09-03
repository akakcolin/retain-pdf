//! /api/v1/ai/ask 原生实现:直调 Rust agentic 循环,取代 retainpdf-ai 反代。
//! SSE 流式(前端经 fetch 携带 X-API-Key)与一次性 JSON envelope 双路径。

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::app::AppState;
use crate::config::AiRuntimeConfig;
use crate::error::AppError;
use crate::models::api::ApiResponse;
use crate::services::ai::AiDeps;
use crate::services::ai::{run_ask, AskPayload, AskRequest, LlmClient};

/// 合并启动期 env 配置与按请求携带的 LLM 凭据;缺 key 直接 400(避免打到上游才 401)。
fn resolve_llm_settings(
    config: &AiRuntimeConfig,
    request: &AskRequest,
) -> Result<(String, String, String), AppError> {
    let api_key = if request.llm_api_key.trim().is_empty() {
        config.llm_api_key.trim().to_string()
    } else {
        request.llm_api_key.trim().to_string()
    };
    if api_key.is_empty() {
        return Err(AppError::bad_request(
            "缺少 LLM API Key:请在前端凭据设置中填写模型 API Key。",
        ));
    }
    let base_url = if request.llm_base_url.trim().is_empty() {
        config.llm_base_url.trim_end_matches('/').to_string()
    } else {
        request
            .llm_base_url
            .trim()
            .trim_end_matches('/')
            .to_string()
    };
    let model = if request.llm_model.trim().is_empty() {
        config.llm_model.clone()
    } else {
        request.llm_model.trim().to_string()
    };
    Ok((api_key, base_url, model))
}

fn payload_to_json(payload: &AskPayload) -> Value {
    serde_json::to_value(payload).expect("serialize AskPayload")
}

pub async fn ask_route(
    State(state): State<AppState>,
    Json(request): Json<AskRequest>,
) -> Result<Response, AppError> {
    let question = request.question.trim();
    if question.is_empty() {
        return Err(AppError::bad_request("question must not be empty"));
    }
    if question.chars().count() > 4000 {
        return Err(AppError::bad_request(
            "question too long (max 4000 characters)",
        ));
    }
    let (api_key, base_url, model) = resolve_llm_settings(&state.config.ai, &request)?;
    let timeout_s = state.config.ai.llm_timeout_s;

    if !request.stream {
        let client = LlmClient::new(base_url, model, api_key, timeout_s);
        let deps = AiDeps {
            db: state.db.as_ref(),
            data_root: &state.config.data_root,
            config: &state.config.ai,
        };
        let result = run_ask(&deps, &request, &client, |_| {}).await?;
        return Ok(Json(ApiResponse::ok(payload_to_json(&result))).into_response());
    }

    // SSE:agent 循环放后台任务,经队列逐事件推给前端
    let (tx, rx) = tokio::sync::mpsc::channel::<Value>(256);
    let on_delta_tx = tx.clone();
    let on_delta = Arc::new(move |text: String| {
        let _ = on_delta_tx.try_send(json!({"type": "answer_delta", "text": text}));
    });
    let client = LlmClient::new(base_url, model, api_key, timeout_s).with_on_delta(on_delta);
    let state = state.clone();
    let request = Arc::new(request);
    let tool_tx = tx.clone();
    let final_tx = tx;
    tokio::spawn(async move {
        let deps = AiDeps {
            db: state.db.as_ref(),
            data_root: &state.config.data_root,
            config: &state.config.ai,
        };
        let result = run_ask(&deps, &request, &client, move |event| {
            let _ = tool_tx.try_send(event);
        })
        .await;
        let event = match result {
            Ok(payload) => {
                let mut done = payload_to_json(&payload);
                done["type"] = json!("done");
                done
            }
            Err(err) => json!({"type": "error", "message": err.to_string()}),
        };
        // 最终事件给背压,避免满队列丢 done
        let _ = final_tx.send(event).await;
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|value| {
            (
                Ok::<String, std::convert::Infallible>(format!("data: {value}\n\n")),
                rx,
            )
        })
    });
    let body = axum::body::Body::from_stream(stream);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body)
        .map_err(|err| AppError::internal(format!("build SSE response: {err}")))
}
