//! 直连 LLM(DeepSeek 兼容端点)的流式/非流式 chat/completions 客户端。
//! 移植自 retainpdf_ai/agent.py 的 build_deepseek_chat_fn + assemble_streaming_message
//! + _friendly_llm_error:同一个 message dict(role/content/tool_calls)喂给 agent 循环,
//! 上层无需感知流式与否。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::error::AppError;

/// 模型发起的工具调用(与 OpenAI 工具调用同构)。
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 一轮 model 输出:纯回答(content)或带工具调用。
#[derive(Debug, Clone)]
pub struct AssistantMessage {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

/// 可注入的 chat 抽象:生产用 [`LlmClient`] 直连 LLM,测试用脚本化 fake 驱动
/// agent 循环(与 Python `chat_fn` 注入等价)。参数持有时避免生命周期纠缠。
pub trait Chat: Send + Sync {
    fn chat(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
    ) -> BoxFuture<'static, Result<AssistantMessage, AppError>>;
}

impl Chat for LlmClient {
    fn chat(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
    ) -> BoxFuture<'static, Result<AssistantMessage, AppError>> {
        let client = self.clone();
        Box::pin(async move { LlmClient::chat(&client, &messages, &tools).await })
    }
}

/// 可克隆的聊天客户端;on_delta 非空时按流式 SSE 请求并逐 token 回调。
#[derive(Clone)]
pub struct LlmClient {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub timeout_s: u64,
    pub on_delta: Option<Arc<dyn Fn(String) + Send + Sync>>,
}

impl LlmClient {
    pub fn new(base_url: String, model: String, api_key: String, timeout_s: u64) -> Self {
        Self {
            base_url,
            model,
            api_key,
            timeout_s,
            on_delta: None,
        }
    }

    pub fn with_on_delta(mut self, on_delta: Arc<dyn Fn(String) + Send + Sync>) -> Self {
        self.on_delta = Some(on_delta);
        self
    }

    /// 发送一轮 chat。tools 为空数组 = 纯回答(轮数耗尽收尾)。
    pub async fn chat(&self, messages: &[Value], tools: &[Value]) -> Result<AssistantMessage, AppError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
            "temperature": 0.2,
        });
        if self.on_delta.is_some() {
            body["stream"] = Value::Bool(true);
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(self.timeout_s.max(1)))
            .build()
            .map_err(|err| AppError::internal(format!("failed to build AI HTTP client: {err}")))?;
        let response = client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|err| AppError::bad_gateway(format!("AI model request failed: {err}")))?;
        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            let detail = response.text().await.unwrap_or_default();
            return Err(friendly_llm_error(status, &detail));
        }
        if self.on_delta.is_some() {
            self.consume_stream(response).await
        } else {
            let parsed: Value = response
                .json()
                .await
                .map_err(|err| AppError::bad_gateway(format!("AI model returned invalid JSON: {err}")))?;
            parse_non_stream_message(&parsed)
        }
    }

    async fn consume_stream(&self, response: reqwest::Response) -> Result<AssistantMessage, AppError> {
        let Some(on_delta) = self.on_delta.as_ref() else {
            return Ok(AssistantMessage {
                content: String::new(),
                tool_calls: Vec::new(),
            });
        };
        let mut lines = SseLineBuffer::new(Box::pin(response.bytes_stream()));
        let mut content_parts: Vec<String> = Vec::new();
        let mut tool_calls: BTreeMap<usize, ToolCall> = BTreeMap::new();
        let mut saw_tool_calls = false;
        // 审计 A3:模型可能在同一轮先吐 content 前言再吐 tool_calls——立即 emit 会把
        // "让我搜索…"这类脏前言当答案流给前端(done 时又被覆盖,闪烁)。前 64 个字符
        // 先缓冲定性:出现 tool_calls → 静默丢弃;攒满仍无 → 判纯回答轮,flush 后直通。
        let holdback_chars = 64usize;
        let mut pending: Vec<String> = Vec::new();
        let mut pending_flushed = false;

        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if !line.starts_with("data:") {
                continue;
            }
            let data = line["data:".len()..].trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            let Some(delta) = chunk
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("delta"))
            else {
                continue;
            };
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                if !saw_tool_calls {
                    pending.clear();
                }
                saw_tool_calls = true;
                for call in calls {
                    let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let slot = tool_calls.entry(index).or_insert_with(|| ToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                    if let Some(id) = call.get("id").and_then(Value::as_str) {
                        if !id.is_empty() {
                            slot.id = id.to_string();
                        }
                    }
                    if let Some(function) = call.get("function") {
                        if let Some(name) = function.get("name").and_then(Value::as_str) {
                            slot.name.push_str(name);
                        }
                        if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                            slot.arguments.push_str(arguments);
                        }
                    }
                }
            }
            if let Some(piece) = delta.get("content").and_then(Value::as_str) {
                if !piece.is_empty() {
                    content_parts.push(piece.to_string());
                    if !saw_tool_calls {
                        if pending_flushed {
                            on_delta(piece.to_string());
                        } else {
                            pending.push(piece.to_string());
                            let held: usize = pending.iter().map(|part| part.len()).sum();
                            if held >= holdback_chars {
                                flush_pending(on_delta, &mut pending, &mut pending_flushed);
                            }
                        }
                    }
                }
            }
        }
        // 短纯回答(不足缓冲阈值)在流结束时补发
        if !saw_tool_calls && !pending_flushed {
            flush_pending(on_delta, &mut pending, &mut pending_flushed);
        }
        let tool_calls = tool_calls.into_values().collect::<Vec<_>>();
        Ok(AssistantMessage {
            content: content_parts.concat(),
            tool_calls,
        })
    }
}

fn flush_pending(
    on_delta: &Arc<dyn Fn(String) + Send + Sync>,
    pending: &mut Vec<String>,
    pending_flushed: &mut bool,
) {
    if !pending.is_empty() {
        on_delta(pending.concat());
    }
    pending.clear();
    *pending_flushed = true;
}

/// 把 reqwest bytes 流按 `\n` 切成完整行(SSE 事件以空行分隔)。
struct SseLineBuffer<S> {
    stream: S,
    buf: Vec<u8>,
}

impl<S, B> SseLineBuffer<S>
where
    S: futures_util::Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
{
    fn new(stream: S) -> Self {
        Self {
            stream,
            buf: Vec::new(),
        }
    }

    async fn next_line(&mut self) -> Result<Option<String>, AppError> {
        loop {
            if let Some(pos) = self.buf.iter().position(|byte| *byte == b'\n') {
                let line = self.buf.drain(..=pos).collect::<Vec<u8>>();
                let line = String::from_utf8_lossy(&line);
                return Ok(Some(
                    line.trim_end_matches(['\r', '\n']).to_string(),
                ));
            }
            match self.stream.next().await {
                Some(Ok(chunk)) => self.buf.extend_from_slice(chunk.as_ref()),
                Some(Err(err)) => {
                    return Err(AppError::bad_gateway(format!("AI stream read failed: {err}")))
                }
                None => {
                    if self.buf.is_empty() {
                        return Ok(None);
                    }
                    let line = std::mem::take(&mut self.buf);
                    let line = String::from_utf8_lossy(&line);
                    return Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()));
                }
            }
        }
    }
}

/// 把上游 HTTP 错误翻译成用户能行动的中文(审计 C1)。
fn friendly_llm_error(status: StatusCode, detail: &str) -> AppError {
    let hint = match status {
        StatusCode::BAD_REQUEST => "请求被模型服务拒绝（参数或上下文过长）".to_string(),
        StatusCode::UNAUTHORIZED => {
            "模型 API Key 无效或未授权：请到 设置 → API 设置 检查 Key".to_string()
        }
        StatusCode::PAYMENT_REQUIRED => "模型账户余额不足：请前往服务商充值后重试".to_string(),
        StatusCode::FORBIDDEN => "模型服务拒绝访问：请检查 Key 权限或所选模型".to_string(),
        StatusCode::NOT_FOUND => "模型或接口地址不存在：请检查模型名称与 Base URL".to_string(),
        StatusCode::TOO_MANY_REQUESTS => {
            "模型请求过于频繁（限流）：请稍候几秒再试".to_string()
        }
        _ if status.is_server_error() => {
            "模型服务暂时不可用（上游故障）：请稍后重试".to_string()
        }
        _ => format!("模型服务返回错误（HTTP {}）", status.as_u16()),
    };
    let mut snippet = detail.trim().replace('\n', " ");
    if snippet.chars().count() > 200 {
        snippet = format!("{}…", snippet.chars().take(200).collect::<String>());
    }
    let mut message = hint;
    if !snippet.is_empty() {
        message.push_str(&format!("（上游信息：{snippet}）"));
    }
    AppError::bad_gateway(message)
}

fn parse_non_stream_message(parsed: &Value) -> Result<AssistantMessage, AppError> {
    let message = parsed
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| AppError::bad_gateway("AI model returned invalid response shape"))?;
    let content = message.get("content").and_then(Value::as_str).unwrap_or("").to_string();
    let mut tool_calls = Vec::new();
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let id = call.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let function = call.get("function").unwrap_or(&Value::Null);
            let name = function.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let arguments = function.get("arguments").and_then(Value::as_str).unwrap_or("").to_string();
            tool_calls.push(ToolCall { id, name, arguments });
        }
    }
    Ok(AssistantMessage { content, tool_calls })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_non_stream_extracts_tool_calls() {
        let body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "search_fulltext", "arguments": "{\"query\":\"卤素\"}"}
                    }]
                }
            }]
        });
        let message = parse_non_stream_message(&body).expect("parse");
        assert!(message.content.is_empty());
        assert_eq!(message.tool_calls.len(), 1);
        assert_eq!(message.tool_calls[0].name, "search_fulltext");
        assert!(message.tool_calls[0].arguments.contains("卤素"));
    }

    #[test]
    fn parse_non_stream_rejects_shapeless_body() {
        assert!(parse_non_stream_message(&json!({"foo": 1})).is_err());
    }

    #[test]
    fn friendly_error_has_actionable_hint() {
        let error = friendly_llm_error(StatusCode::PAYMENT_REQUIRED, "insufficient balance");
        assert!(error.to_string().contains("余额不足"));
        assert!(error.to_string().contains("insufficient balance"));
    }
}
