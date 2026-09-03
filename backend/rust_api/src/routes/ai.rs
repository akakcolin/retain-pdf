//! /api/v1/ai/ask 原生实现:直调 Rust agentic 循环,取代 retainpdf-ai 反代。
//! SSE 流式(前端经 fetch 携带 X-API-Key)与一次性 JSON envelope 双路径。

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::app::AppState;
use crate::error::AppError;
use crate::models::api::ApiResponse;
use crate::routes::common::build_ai_route_deps;
use crate::services::ai_api::{
    ask, resolve_llm_settings, AiDeps, AskPayload, AskRequest, LlmClient,
};

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
    let deps = build_ai_route_deps(&state);
    let (api_key, base_url, model) = resolve_llm_settings(deps.ai, &request)?;
    let timeout_s = deps.ai.llm_timeout_s;

    if !request.stream {
        let client = LlmClient::new(base_url, model, api_key, timeout_s);
        let ai_deps = AiDeps {
            db: deps.db.as_ref(),
            data_root: deps.data_root,
            config: deps.ai,
        };
        let result = ask(&ai_deps, &request, &client, |_| {}).await?;
        return Ok(Json(ApiResponse::ok(payload_to_json(&result))).into_response());
    }

    // SSE:agent 循环放后台任务,经队列逐事件推给前端
    let (tx, rx) = tokio::sync::mpsc::channel::<Value>(256);
    let on_delta_tx = tx.clone();
    let on_delta = Arc::new(move |text: String| {
        let _ = on_delta_tx.try_send(json!({"type": "answer_delta", "text": text}));
    });
    let client = LlmClient::new(base_url, model, api_key, timeout_s).with_on_delta(on_delta);
    // 后台任务只持有克隆出的 owned 句柄,不克隆整个 AppState。
    let db = Arc::clone(deps.db);
    let data_root = deps.data_root.to_path_buf();
    let ai_config = deps.ai.clone();
    let request = Arc::new(request);
    let tool_tx = tx.clone();
    let final_tx = tx;
    tokio::spawn(async move {
        let ai_deps = AiDeps {
            db: db.as_ref(),
            data_root: &data_root,
            config: &ai_config,
        };
        let result = ask(&ai_deps, &request, &client, move |event| {
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
