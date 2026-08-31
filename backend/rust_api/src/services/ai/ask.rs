//! AI 问答编排(移植自 retainpdf_ai/app.py):文档解析、会话落库、记忆窗口、
//! agent 循环、历史回写。SSE 事件由路由层逐条 emit。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::AiRuntimeConfig;
use crate::db::Db;
use crate::error::AppError;
use crate::models::api::MessageRecord;
use crate::models::domain::build_job_id;

use super::agent::{AskResult, Citation, RetrievalAgent};
use super::llm::LlmClient;
use super::memory::{
    assemble_history, maybe_compress_transcript, HistoryMessage, TranscriptMessage,
};
use super::tools::AiTools;
use super::AiDeps;

/// POST /api/v1/ai/ask 请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct AskRequest {
    pub question: String,
    #[serde(default)]
    pub document_id: String,
    #[serde(default)]
    pub job_id: String,
    #[serde(default)]
    pub conversation_id: String,
    #[serde(default)]
    pub parent_id: String,
    #[serde(default)]
    pub regenerate: bool,
    #[serde(default)]
    pub user_message_id: String,
    #[serde(default)]
    pub assistant_message_id: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub force_compress: bool,
    #[serde(default)]
    pub llm_api_key: String,
    #[serde(default)]
    pub llm_base_url: String,
    #[serde(default)]
    pub llm_model: String,
}

/// done 事件 / JSON data 的载荷(与前端 normalizeDonePayload 对齐)。
#[derive(Debug, Clone, Serialize)]
pub struct AskPayload {
    pub answer: String,
    pub citations: Vec<Value>,
    pub tool_trace: Vec<Value>,
    pub rounds: i64,
    pub persisted: bool,
    pub conversation_id: String,
    pub memory: Value,
}

fn resolve_document_id(db: &Db, request: &AskRequest) -> String {
    let document_id = request.document_id.trim().to_string();
    if !document_id.is_empty() || request.job_id.trim().is_empty() {
        return document_id;
    }
    match db.get_document_by_job_id(request.job_id.trim()) {
        Ok(Some(document)) => document.document_id,
        _ => String::new(),
    }
}

fn ensure_conversation_id(db: &Db, request: &AskRequest, document_id: &str) -> String {
    let existing = request.conversation_id.trim().to_string();
    if !existing.is_empty() {
        return existing;
    }
    let mut title = request.question.trim().replace('\n', " ");
    if title.chars().count() > 48 {
        title = format!("{}…", title.chars().take(48).collect::<String>().trim_end());
    }
    if title.is_empty() {
        title = "阅读问答".to_string();
    }
    let document_id = if document_id.is_empty() {
        None
    } else {
        Some(document_id)
    };
    match db.create_conversation(&format!("conv-{}", build_job_id()), &title, document_id) {
        Ok(conversation) => conversation.conversation_id,
        Err(_) => String::new(),
    }
}

/// 从 head(或 stop_at)沿 parent_id 回溯,返回根→叶路径。
/// 无 parent / message_id 的旧数据按 seq 串成线性链。
fn visible_path(messages: &[MessageRecord], head_id: &str, stop_at: &str) -> Vec<MessageRecord> {
    if messages.is_empty() {
        return Vec::new();
    }
    let mut ordered: Vec<&MessageRecord> = messages.iter().collect();
    ordered.sort_by_key(|message| message.seq);
    // 合成稳定 id + 线性 parent,保证无树字段时退化为整条 transcript
    let mut by_id: HashMap<String, MessageRecord> = HashMap::new();
    let mut prev_id = String::new();
    let mut last_id = String::new();
    for (index, raw) in ordered.iter().enumerate() {
        let mid = if raw.message_id.trim().is_empty() {
            format!("__seq_{}", raw.seq)
        } else {
            raw.message_id.clone()
        };
        let pid = if raw.parent_id.trim().is_empty() {
            prev_id.clone()
        } else {
            raw.parent_id.clone()
        };
        let mut node = (*raw).clone();
        node.message_id = mid.clone();
        node.parent_id = pid;
        by_id.insert(mid.clone(), node);
        prev_id = mid.clone();
        last_id = mid;
        let _ = index;
    }
    let start_id = if !stop_at.trim().is_empty() {
        stop_at.trim().to_string()
    } else if !head_id.trim().is_empty() {
        head_id.trim().to_string()
    } else {
        last_id.clone()
    };
    let mut cur = by_id.get(&start_id);
    if cur.is_none() && !ordered.is_empty() {
        cur = by_id.get(&last_id);
    }
    let mut chain: Vec<MessageRecord> = Vec::new();
    let mut guard = 0usize;
    while let Some(node) = cur {
        chain.push(node.clone());
        guard += 1;
        if guard > messages.len() + 2 {
            break;
        }
        let pid = node.parent_id.trim().to_string();
        cur = if pid.is_empty() { None } else { by_id.get(&pid) };
    }
    chain.reverse();
    chain
}

fn load_transcript(db: &Db, conversation_id: &str, stop_at: &str) -> Vec<TranscriptMessage> {
    if conversation_id.is_empty() {
        return Vec::new();
    }
    let Ok(Some(conversation)) = db.get_conversation(conversation_id) else {
        return Vec::new();
    };
    let Ok(messages) = db.list_messages(conversation_id, 2000) else {
        return Vec::new();
    };
    let head_id = conversation.head_id;
    visible_path(&messages, &head_id, stop_at)
        .into_iter()
        .filter_map(|message| {
            if !matches!(message.role.as_str(), "user" | "assistant") || message.content.trim().is_empty() {
                return None;
            }
            let citations_json = if message.citations_json.trim().is_empty() {
                "[]".to_string()
            } else {
                message.citations_json
            };
            Some(TranscriptMessage {
                role: message.role,
                content: message.content,
                citations_json,
            })
        })
        .collect()
}

/// 压缩(可选) + 组装 history。summary_id 非空时,调用方必须把本轮 user
/// (或 regenerate 的 assistant)挂在它下面——摘要只有落在 head 路径上,
/// 下一轮 load_transcript 才读得回来(审计 A2)。
fn prepare_memory(
    db: &Db,
    config: &AiRuntimeConfig,
    conversation_id: &str,
    force_compress: bool,
    stop_at: &str,
) -> (Vec<HistoryMessage>, Option<Value>, Value, String) {
    let transcript = load_transcript(db, conversation_id, stop_at);
    let compress = maybe_compress_transcript(
        &transcript,
        config.memory_window_turns,
        config.memory_compress_after_turns,
        force_compress,
        config.memory_max_chars,
    );
    let working = compress.messages;
    let mut compress_event: Option<Value> = None;
    let mut summary_id = String::new();
    if compress.compressed && compress.summary_message.is_some() && !conversation_id.is_empty() {
        if let Some(summary) = &compress.summary_message {
            let message_id = format!("msg-{}", build_job_id());
            let appended = db.append_message(
                conversation_id,
                &message_id,
                "assistant",
                &summary.content,
                "[]",
                "[]",
                "memory/extractive_v1",
                stop_at,
                false,
            );
            if let Ok(message) = appended {
                summary_id = message.message_id;
                if let Some(event) = &compress.event {
                    compress_event = Some(event.to_value());
                }
            }
        }
    }
    let assembled = assemble_history(&working, config.memory_window_turns, config.memory_max_chars);
    let mut debug = assembled.debug;
    debug["compressed"] = Value::Bool(compress_event.is_some());
    debug["evidence_count"] = Value::from(0);
    (assembled.history, compress_event, debug, summary_id)
}

fn citation_to_json(citation: &Citation) -> Value {
    json!({
        "ref": citation.ref_num,
        "document_id": citation.document_id,
        "job_id": citation.job_id,
        "page_idx": citation.page_idx,
        "block_id": citation.block_id,
        "snippet": citation.snippet,
    })
}

/// 尽力而为的历史回写:失败返回 false(done.persisted=false 透传给前端)。
fn persist_turn(
    db: &Db,
    config: &AiRuntimeConfig,
    request: &AskRequest,
    conversation_id: &str,
    result: &AskResult,
    chain_parent_id: &str,
) -> bool {
    if conversation_id.is_empty() {
        return true;
    }
    let citations_json =
        serde_json::to_string(&result.citations.iter().map(citation_to_json).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".to_string());
    let tool_trace_json = serde_json::to_string(&result.tool_trace).unwrap_or_else(|_| "[]".to_string());
    let model = if request.llm_model.trim().is_empty() {
        config.llm_model.clone()
    } else {
        request.llm_model.trim().to_string()
    };
    let parent_hint = if chain_parent_id.trim().is_empty() {
        request.parent_id.trim().to_string()
    } else {
        chain_parent_id.trim().to_string()
    };
    if request.regenerate {
        // 重试: parent_id 必须是 user 消息
        let user_parent = parent_hint;
        let message_id = resolved_message_id(&request.assistant_message_id);
        return db
            .append_message(
                conversation_id,
                &message_id,
                "assistant",
                &result.answer,
                &citations_json,
                &tool_trace_json,
                &model,
                &user_parent,
                true,
            )
            .is_ok();
    }
    let user_message_id = resolved_message_id(&request.user_message_id);
    let user_id = match db.append_message(
        conversation_id,
        &user_message_id,
        "user",
        request.question.trim(),
        "[]",
        "[]",
        &model,
        &parent_hint,
        true,
    ) {
        Ok(message) => message.message_id,
        Err(_) => return false,
    };
    let assistant_message_id = resolved_message_id(&request.assistant_message_id);
    let assistant_parent = if user_id.is_empty() {
        parent_hint
    } else {
        user_id
    };
    db.append_message(
        conversation_id,
        &assistant_message_id,
        "assistant",
        &result.answer,
        &citations_json,
        &tool_trace_json,
        &model,
        &assistant_parent,
        true,
    )
    .is_ok()
}

fn resolved_message_id(client_id: &str) -> String {
    let trimmed = client_id.trim();
    if trimmed.is_empty() {
        format!("msg-{}", build_job_id())
    } else {
        trimmed.to_string()
    }
}

/// 核心编排:文档解析 → 会话 → 记忆 → agent → 回写。on_event 收到过程中的
/// compress / tool 事件(路由层负责把 done/error 也序列化进 SSE 或组装成 JSON)。
pub async fn run_ask<F>(
    deps: &AiDeps<'_>,
    request: &AskRequest,
    client: &LlmClient,
    mut on_event: F,
) -> Result<AskPayload, AppError>
where
    F: FnMut(Value),
{
    let document_id = resolve_document_id(deps.db, request);
    let conversation_id = ensure_conversation_id(deps.db, request, &document_id);
    let memory_stop = if request.regenerate && !request.parent_id.trim().is_empty() {
        request.parent_id.trim().to_string()
    } else {
        String::new()
    };
    let (history, compress_event, memory_debug, summary_id) = prepare_memory(
        deps.db,
        deps.config,
        &conversation_id,
        request.force_compress,
        &memory_stop,
    );
    if let Some(event) = compress_event {
        on_event(event);
    }
    let tools = AiTools::new(deps.db, deps.data_root);
    let agent = RetrievalAgent::new(tools, deps.config.max_tool_rounds);
    let result = agent
        .ask(
            client,
            &request.question,
            &document_id,
            &request.job_id,
            &history,
            on_event,
        )
        .await?;
    let persisted =
        persist_turn(deps.db, deps.config, request, &conversation_id, &result, &summary_id);
    Ok(AskPayload {
        answer: result.answer,
        citations: result.citations.iter().map(citation_to_json).collect(),
        tool_trace: result.tool_trace,
        rounds: result.rounds,
        persisted,
        conversation_id,
        memory: memory_debug,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(seq: i64, message_id: &str, parent_id: &str) -> MessageRecord {
        MessageRecord {
            message_id: message_id.to_string(),
            conversation_id: "conv-1".to_string(),
            seq,
            role: if message_id.starts_with("u") { "user" } else { "assistant" }.to_string(),
            content: format!("content-{message_id}"),
            citations_json: "[]".to_string(),
            tool_trace_json: "[]".to_string(),
            model: "deepseek".to_string(),
            created_at: "2026-04-02T00:00:00Z".to_string(),
            parent_id: parent_id.to_string(),
        }
    }

    #[test]
    fn visible_path_builds_linear_chain() {
        let messages = vec![
            record(1, "u1", ""),
            record(2, "a1", "u1"),
            record(3, "u2", "a1"),
            record(4, "a2", "u2"),
        ];
        let path = visible_path(&messages, "a2", "");
        let ids: Vec<&str> = path.iter().map(|item| item.message_id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "a1", "u2", "a2"]);
    }

    #[test]
    fn visible_path_stops_at_given_message() {
        let messages = vec![
            record(1, "u1", ""),
            record(2, "a1", "u1"),
            record(3, "u2", "a1"),
            record(4, "a2", "u2"),
            record(5, "a2b", "u2"),
        ];
        let path = visible_path(&messages, "a2b", "u2");
        let ids: Vec<&str> = path.iter().map(|item| item.message_id.as_str()).collect();
        assert_eq!(ids, vec!["u1", "a1", "u2"]);
    }

    #[test]
    fn visible_path_falls_back_by_seq_when_no_tree_fields() {
        let messages = vec![
            MessageRecord {
                message_id: String::new(),
                conversation_id: "conv-1".to_string(),
                seq: 1,
                role: "user".to_string(),
                content: "x".to_string(),
                citations_json: "[]".to_string(),
                tool_trace_json: "[]".to_string(),
                model: String::new(),
                created_at: String::new(),
                parent_id: String::new(),
            },
            MessageRecord {
                message_id: String::new(),
                conversation_id: "conv-1".to_string(),
                seq: 2,
                role: "assistant".to_string(),
                content: "y".to_string(),
                citations_json: "[]".to_string(),
                tool_trace_json: "[]".to_string(),
                model: String::new(),
                created_at: String::new(),
                parent_id: String::new(),
            },
        ];
        let path = visible_path(&messages, "", "");
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].message_id, "__seq_1");
    }

    #[test]
    fn ensure_conversation_truncates_long_title() {
        // ensure_conversation_id 需要 db;此处只验证标题截断逻辑片段
        let long = "x".repeat(80);
        let mut title = long.replace('\n', " ");
        if title.chars().count() > 48 {
            title = format!("{}…", title.chars().take(48).collect::<String>().trim_end());
        }
        assert_eq!(title.chars().count(), 49);
        assert!(title.ends_with('…'));
    }
}
