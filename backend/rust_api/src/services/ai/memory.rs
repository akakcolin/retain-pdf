//! 抽取式上下文压缩 extractive_v1 + 窗口组装(移植自 retainpdf_ai/memory/*)。
//! 不调用 LLM,规则折叠早期轮次,保持与旧 AI 服务一致的行为。

use serde_json::Value;

pub const SUMMARY_PREFIX: &str = "【对话摘要】";

/// transcript 消息;citations_json 是软锚点快照(JSON 数组字符串)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptMessage {
    pub role: String,
    pub content: String,
    pub citations_json: String,
}

impl TranscriptMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
            citations_json: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompressEvent {
    pub dropped_turns: usize,
    pub summary_chars: usize,
    pub kept_evidence: usize,
    pub window_turns: usize,
}

impl CompressEvent {
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "type": "compress",
            "dropped_turns": self.dropped_turns,
            "summary_chars": self.summary_chars,
            "kept_evidence": self.kept_evidence,
            "policy": "extractive_v1",
            "window_turns": self.window_turns,
        })
    }
}

#[derive(Debug, Clone)]
pub struct CompressResult {
    pub messages: Vec<TranscriptMessage>,
    pub compressed: bool,
    pub summary_message: Option<TranscriptMessage>,
    pub event: Option<CompressEvent>,
}

/// history 消息(agent 只消费 role/content)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
}

pub fn is_summary_message(message: &TranscriptMessage) -> bool {
    message.content.trim_start().starts_with(SUMMARY_PREFIX)
}

/// 返回 (最新 summary 消息或 None, 该 summary 之后的 turn 消息)。
pub fn split_transcript(
    messages: &[TranscriptMessage],
) -> (Option<TranscriptMessage>, Vec<TranscriptMessage>) {
    let mut last_summary: Option<TranscriptMessage> = None;
    let mut last_summary_idx = -1isize;
    for (index, message) in messages.iter().enumerate() {
        if is_summary_message(message) {
            last_summary = Some(message.clone());
            last_summary_idx = index as isize;
        }
    }
    let start = (last_summary_idx + 1) as usize;
    let turns = messages[start..]
        .iter()
        .filter(|message| {
            matches!(message.role.as_str(), "user" | "assistant")
                && !message.content.trim().is_empty()
                && !is_summary_message(message)
        })
        .cloned()
        .collect();
    (last_summary, turns)
}

/// 粗算「轮」:user 条数。
pub fn count_turns(messages: &[TranscriptMessage]) -> usize {
    messages
        .iter()
        .filter(|message| message.role == "user")
        .count()
}

fn clip(text: &str, max_chars: usize) -> String {
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut truncated: String = normalized
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect();
    truncated.push('…');
    truncated
}

/// 从被折叠的 turns 抽出问题 / 带引用结论 / 证据片段。
fn build_extractive_summary(turns: &[TranscriptMessage], max_chars: usize) -> String {
    let mut user_questions: Vec<String> = Vec::new();
    let mut cited_lines: Vec<String> = Vec::new();
    let mut evidence_lines: Vec<String> = Vec::new();

    for message in turns {
        let content = message.content.trim();
        if content.is_empty() {
            continue;
        }
        if message.role == "user" {
            user_questions.push(clip(content, 120));
            continue;
        }
        if message.role != "assistant" {
            continue;
        }
        for line in content.lines() {
            let line = line.trim();
            if !line.is_empty() && has_citation(line) {
                cited_lines.push(clip(line, 160));
            }
        }
        for item in parse_citations(&message.citations_json).into_iter().take(8) {
            let ref_label = item
                .get("ref")
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_string());
            let page_label = match item.get("page_idx") {
                Some(Value::Number(number)) if number.as_i64().is_some() => {
                    format!("p.{}", number.as_i64().unwrap() + 1)
                }
                _ => String::new(),
            };
            let snippet = clip(
                item.get("snippet").and_then(Value::as_str).unwrap_or(""),
                80,
            );
            let block = item
                .get("block_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let mut parts = Vec::new();
            parts.push(format!("[{ref_label}]"));
            if !page_label.is_empty() {
                parts.push(page_label);
            }
            if !block.is_empty() {
                parts.push(block);
            }
            if !snippet.is_empty() {
                parts.push(snippet);
            }
            evidence_lines.push(parts.join(" "));
        }
    }

    let mut lines: Vec<String> = vec![SUMMARY_PREFIX.to_string(), "- 用户关注：".to_string()];
    if user_questions.is_empty() {
        lines.push("  · （无）".to_string());
    } else {
        for question in user_questions.iter().rev().take(8).rev() {
            lines.push(format!("  · {question}"));
        }
    }
    lines.push("- 已确认结论（含引用）：".to_string());
    if cited_lines.is_empty() {
        lines.push("  · （早期回答未标注 [n]，仅保留主题）".to_string());
    } else {
        for line in cited_lines.iter().rev().take(10).rev() {
            lines.push(format!("  · {line}"));
        }
    }
    lines.push("- 重要证据：".to_string());
    if evidence_lines.is_empty() {
        lines.push("  · （无结构化 citations）".to_string());
    } else {
        let mut seen = std::collections::HashSet::new();
        for line in &evidence_lines {
            if !seen.insert(line.clone()) {
                continue;
            }
            lines.push(format!("  · {line}"));
            if seen.len() >= 12 {
                break;
            }
        }
    }
    let text = lines.join("\n");
    if text.chars().count() > max_chars {
        let mut truncated: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        truncated.push('…');
        truncated
    } else {
        text
    }
}

fn has_citation(line: &str) -> bool {
    line.contains('[') && line.chars().any(|c| c.is_ascii_digit()) && line.contains(']')
}

fn parse_citations(citations_json: &str) -> Vec<Value> {
    let trimmed = citations_json.trim();
    if !trimmed.starts_with('[') {
        return Vec::new();
    }
    serde_json::from_str::<Vec<Value>>(trimmed).unwrap_or_default()
}

/// 若 turn 数超过阈值或 force,把「最新 summary 之后、窗口之外」的早期轮次折叠为一条
/// assistant 摘要消息。返回的 messages 是逻辑 transcript(内存视图)。
pub fn maybe_compress_transcript(
    messages: &[TranscriptMessage],
    window_turns: usize,
    compress_after_turns: usize,
    force: bool,
    summary_max_chars: usize,
) -> CompressResult {
    let normalized: Vec<TranscriptMessage> = messages
        .iter()
        .filter(|message| {
            matches!(message.role.as_str(), "user" | "assistant")
                && !message.content.trim().is_empty()
        })
        .cloned()
        .collect();
    let (last_summary, turns) = split_transcript(&normalized);
    let turn_count = count_turns(&turns);
    let window_turns = window_turns.max(1);
    let compress_after_turns = compress_after_turns.max(window_turns + 1);

    if !force && turn_count <= compress_after_turns {
        return CompressResult {
            messages: normalized,
            compressed: false,
            summary_message: None,
            event: None,
        };
    }

    let keep_n = window_turns * 2;
    let (to_fold, kept): (&[TranscriptMessage], &[TranscriptMessage]) = if turns.len() > keep_n {
        (
            &turns[..turns.len() - keep_n],
            &turns[turns.len() - keep_n..],
        )
    } else if force {
        (&turns[..], &turns[..])
    } else {
        return CompressResult {
            messages: normalized,
            compressed: false,
            summary_message: None,
            event: None,
        };
    };

    if to_fold.is_empty() {
        return CompressResult {
            messages: normalized,
            compressed: false,
            summary_message: None,
            event: None,
        };
    }

    let mut summary_text = build_extractive_summary(to_fold, summary_max_chars);
    if let Some(summary) = last_summary.as_ref() {
        let prior = summary.content.trim();
        if !prior.is_empty() && !summary_text.contains(prior) {
            summary_text = format!("{prior}\n\n——\n\n{summary_text}");
        }
    }
    let summary_message = TranscriptMessage::new("assistant", &summary_text);
    let mut new_messages = vec![summary_message.clone()];
    new_messages.extend_from_slice(kept);

    let dropped_turns = count_turns(to_fold);
    let event = CompressEvent {
        dropped_turns,
        summary_chars: summary_text.chars().count(),
        kept_evidence: summary_text.matches('[').count(),
        window_turns,
    };
    CompressResult {
        messages: new_messages,
        compressed: true,
        summary_message: Some(summary_message),
        event: Some(event),
    }
}

pub fn estimate_tokens(text: &str) -> usize {
    let n = text.chars().count();
    if n == 0 {
        0
    } else {
        (n as f64 / 2.5) as usize
    }
}

fn clip_content(role: &str, content: &str) -> String {
    let limit = if role == "user" { 2000 } else { 3000 };
    let text = content;
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(limit.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

pub struct AssembleResult {
    pub history: Vec<HistoryMessage>,
    pub debug: serde_json::Value,
}

/// 把完整/已压缩 transcript 收成 agent history 列表。
pub fn assemble_history(
    messages: &[TranscriptMessage],
    window_turns: usize,
    max_chars: usize,
) -> AssembleResult {
    let window_turns = window_turns.max(1);
    let (last_summary, turns) = split_transcript(messages);
    let keep_n = window_turns * 2;
    let window: Vec<&TranscriptMessage> = if turns.len() > keep_n {
        turns[turns.len() - keep_n..].iter().collect()
    } else {
        turns.iter().collect()
    };

    let mut history: Vec<HistoryMessage> = Vec::new();
    let mut had_summary = false;
    if let Some(summary) = last_summary.as_ref() {
        if !summary.content.trim().is_empty() {
            had_summary = true;
            history.push(HistoryMessage {
                role: "user".to_string(),
                content: format!(
                    "以下是更早对话的摘要，请当作已知背景：\n{}",
                    summary.content
                ),
            });
            history.push(HistoryMessage {
                role: "assistant".to_string(),
                content: "好的，我将基于摘要与新问题继续。".to_string(),
            });
        }
    }

    for message in window {
        if !matches!(message.role.as_str(), "user" | "assistant") || is_summary_message(message) {
            continue;
        }
        let content = clip_content(&message.role, &message.content);
        if content.trim().is_empty() {
            continue;
        }
        history.push(HistoryMessage {
            role: message.role.clone(),
            content,
        });
    }

    // 总长护栏:从窗口头(摘要伪轮之后)开始丢,尽量成对
    let prefix_len = if had_summary { 2 } else { 0 };
    let total_chars = |items: &[HistoryMessage]| -> usize {
        items.iter().map(|item| item.content.chars().count()).sum()
    };
    while history.len() > prefix_len + 2 && total_chars(&history) > max_chars {
        history.remove(prefix_len);
        if history.len() > prefix_len && history[prefix_len].role == "assistant" {
            history.remove(prefix_len);
        }
    }

    let prompt_est = estimate_tokens(
        &history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let debug = serde_json::json!({
        "window_turns": window_turns,
        "had_summary": had_summary,
        "history_messages": history.len(),
        "prompt_tokens_est": prompt_est,
        "total_chars": total_chars(&history),
    });
    AssembleResult { history, debug }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turns(n_user: usize) -> Vec<TranscriptMessage> {
        let mut messages = Vec::new();
        for i in 0..n_user {
            messages.push(TranscriptMessage::new(
                "user",
                &format!("问题{} 关于卤素锂交换", i + 1),
            ));
            messages.push(TranscriptMessage {
                role: "assistant".to_string(),
                content: format!("回答{}：选择性显著 [1]。", i + 1),
                citations_json: "[{\"ref\":1,\"page_idx\":2,\"block_id\":\"p003-b0001\",\"snippet\":\"选择性片段\"}]".to_string(),
            });
        }
        messages
    }

    #[test]
    fn estimate_tokens_positive() {
        assert_eq!(estimate_tokens(""), 0);
        assert!(estimate_tokens("你好 world") >= 1);
    }

    #[test]
    fn build_extractive_summary_contains_questions_and_citations() {
        let text = build_extractive_summary(&turns(3), 1800);
        assert!(text.starts_with(SUMMARY_PREFIX));
        assert!(text.contains("问题1") || text.contains("问题3"));
        assert!(text.contains("[1]"));
        assert!(text.contains("选择性") || text.contains("p.3"));
    }

    #[test]
    fn maybe_compress_when_over_threshold() {
        let messages = turns(15);
        let result = maybe_compress_transcript(&messages, 6, 12, false, 1800);
        assert!(result.compressed);
        let summary = result.summary_message.expect("summary");
        assert!(summary.content.starts_with(SUMMARY_PREFIX));
        let event = result.event.expect("event");
        assert!(event.dropped_turns >= 1);
        assert!(result.messages.len() <= 1 + 12);
        assert!(result.messages[0].content.starts_with(SUMMARY_PREFIX));
    }

    #[test]
    fn maybe_compress_noop_when_short() {
        let messages = turns(3);
        let result = maybe_compress_transcript(&messages, 6, 12, false, 1800);
        assert!(!result.compressed);
        assert!(result.summary_message.is_none());
        assert_eq!(result.messages.len(), 6);
    }

    #[test]
    fn force_compress_short_history() {
        let messages = turns(4);
        let result = maybe_compress_transcript(&messages, 2, 12, true, 1800);
        assert!(result.compressed);
        assert_eq!(result.event.expect("event").dropped_turns, 2);
    }

    #[test]
    fn assemble_history_injects_summary_prefix() {
        let compressed = maybe_compress_transcript(&turns(14), 4, 8, false, 1800);
        let assembled = assemble_history(&compressed.messages, 4, 24000);
        assert_eq!(assembled.debug["had_summary"], Value::Bool(true));
        assert_eq!(assembled.history[0].role, "user");
        assert!(assembled.history[0].content.contains("摘要"));
        assert_eq!(assembled.history[1].role, "assistant");
        assert!(assembled
            .history
            .iter()
            .any(|item| item.role == "user" && item.content.contains("问题")));
    }

    #[test]
    fn assemble_clips_long_content() {
        let long = "x".repeat(5000);
        let messages = vec![
            TranscriptMessage::new("user", &long),
            TranscriptMessage::new("assistant", &long),
        ];
        let assembled = assemble_history(&messages, 6, 24000);
        assert!(assembled.history[0].content.chars().count() <= 2000);
        assert!(assembled.history[1].content.chars().count() <= 3000);
    }
}
