//! /api/v1/ai/ask 路由门面:凭据合并与 agentic ask 编排入口。
//! 路由只允许经本模块触达 AI 能力,不直接 import services::ai 内部。

use serde_json::Value;

use crate::config::AiRuntimeConfig;
use crate::error::AppError;

use super::ai::run_ask;

pub use super::ai::{AiDeps, AskPayload, AskRequest, LlmClient};

/// 合并启动期 env 配置与按请求携带的 LLM 凭据;缺 key 直接 400(避免打到上游才 401)。
pub fn resolve_llm_settings(
    config: &AiRuntimeConfig,
    request: &AskRequest,
) -> Result<(String, String, String), AppError> {
    resolve_llm_credentials(
        config,
        &request.llm_api_key,
        &request.llm_base_url,
        &request.llm_model,
    )
}

/// 凭据合并的通用形式:抽取等非问答入口按请求携带同名三字段。
pub fn resolve_llm_credentials(
    config: &AiRuntimeConfig,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<(String, String, String), AppError> {
    let api_key = if api_key.trim().is_empty() {
        config.llm_api_key.trim().to_string()
    } else {
        api_key.trim().to_string()
    };
    if api_key.is_empty() {
        return Err(AppError::bad_request(
            "缺少 LLM API Key:请在前端凭据设置中填写模型 API Key。",
        ));
    }
    let base_url = if base_url.trim().is_empty() {
        config.llm_base_url.trim_end_matches('/').to_string()
    } else {
        base_url.trim().trim_end_matches('/').to_string()
    };
    let model = if model.trim().is_empty() {
        config.llm_model.clone()
    } else {
        model.trim().to_string()
    };
    Ok((api_key, base_url, model))
}

/// agentic ask 编排:等价 services::ai::run_ask,经门面供路由调用。
/// on_event 收到过程中的 compress / tool 事件(路由层负责把 done/error 也序列化进 SSE 或组装成 JSON)。
pub async fn ask<F>(
    deps: &AiDeps<'_>,
    request: &AskRequest,
    client: &LlmClient,
    on_event: F,
) -> Result<AskPayload, AppError>
where
    F: FnMut(Value),
{
    run_ask(deps, request, client, on_event).await
}
