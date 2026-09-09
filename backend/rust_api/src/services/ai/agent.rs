//! Agentic 检索问答的薄循环(移植自 retainpdf_ai/agent.py)。
//! 裸 function calling 循环:单 provider、单用户本地服务,轮数/超时/引用编号全自持。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use serde_json::{json, Map, Value};

use crate::error::AppError;

use super::llm::{Chat, ToolCall};
use super::memory::HistoryMessage;
use super::tools::AiTools;

pub const SYSTEM_PROMPT: &str =
    "你是 RetainPDF 图书馆的文献问答助手。用户的库里是科学文献(原文多为英文,已翻译为中文)。

工作方式:
- 先用工具找证据,再回答;不要凭空回答文献内容。可以多轮使用工具、更换关键词反复检索。
- 工具结果里每条证据有 ref 编号与 page(从 1 开始的页码)。回答里只能用方括号数字引用,例如 [1] [2]。
  正确:「该方法显著降低计算量 [2]。」
  错误:「…… [p002-b0004]」「…… (block_id=…)」「…… page_idx=3」——禁止输出任何内部 ID。
- 用 Markdown 组织回答(小标题、列表、加粗);公式用 $...$ / $$...$$。
- 工具结果可能带 image_urls。若问题涉及图/表/结构式,可用:
  ![简短说明](/api/v1/jobs/.../markdown/images/...)
  只使用工具返回的 URL,不要编造。
- 找不到证据就直说没找到,不要编造。
- 用中文回答,术语保留原文。简洁、直接,不要复述工具原始 JSON。";

/// 一条软锚点引用(done.citations 的原始数据)。
#[derive(Debug, Clone)]
pub struct Citation {
    pub ref_num: i64,
    pub document_id: String,
    pub job_id: String,
    pub page_idx: i64,
    pub block_id: String,
    pub snippet: String,
}

/// 单轮 ask 的结果(agent 内表示;序列化在 ask 层完成)。
#[derive(Debug, Clone)]
pub struct AskResult {
    pub answer: String,
    pub citations: Vec<Citation>,
    pub tool_trace: Vec<Value>,
    pub rounds: i64,
}

pub struct RetrievalAgent<'a> {
    tools: AiTools<'a>,
    max_tool_rounds: usize,
}

impl<'a> RetrievalAgent<'a> {
    pub fn new(tools: AiTools<'a>, max_tool_rounds: usize) -> Self {
        Self {
            tools,
            max_tool_rounds: max_tool_rounds.max(1),
        }
    }

    /// 运行完整工具循环。on_event 收到 tool/answer_delta 已由 llm 层处理后的过程事件。
    pub async fn ask<C, F>(
        &self,
        client: &C,
        question: &str,
        document_id: &str,
        job_id: &str,
        history: &[HistoryMessage],
        mut on_event: F,
    ) -> Result<AskResult, AppError>
    where
        C: Chat,
        F: FnMut(Value),
    {
        let scoped_document_id = document_id.trim().to_string();
        let scoped_job_id = job_id.trim().to_string();
        let mut user_content = question.trim().to_string();
        if !scoped_document_id.is_empty() {
            // 硬范围说明 + 工具层强制注入 document_id(见 scope_tool_arguments)
            let mut prefix = format!("(限定文档 document_id={scoped_document_id}");
            if !scoped_job_id.is_empty() {
                prefix.push_str(&format!(", job_id={scoped_job_id}"));
            }
            prefix.push_str(
                "。search_fulltext / search_favorites / list_documents / read_blocks / \
                 find_mentions 必须只在该文档内操作。)\n",
            );
            user_content = format!("{prefix}{user_content}");
        }
        let mut messages: Vec<Value> = vec![json!({"role": "system", "content": SYSTEM_PROMPT})];
        // 多轮对话:只回放 role/content,工具轨迹不回放
        for turn in history {
            if matches!(turn.role.as_str(), "user" | "assistant") && !turn.content.trim().is_empty()
            {
                messages.push(json!({"role": turn.role, "content": turn.content}));
            }
        }
        messages.push(json!({"role": "user", "content": user_content}));

        let mut citations: BTreeMap<i64, Citation> = BTreeMap::new();
        let mut trace: Vec<Value> = Vec::new();
        let mut next_ref: i64 = 1;
        // 整本问答:不暴露 list_documents,避免模型去「浏览图书馆」
        let tool_specs = self.tools.specs(&scoped_document_id);

        for round_index in 1..=self.max_tool_rounds {
            let message = client.chat(messages.clone(), tool_specs.clone()).await?;
            if message.tool_calls.is_empty() {
                let answer = sanitize_answer_text(&message.content, &citations);
                let cited = referenced_citations(&answer, &citations);
                return Ok(AskResult {
                    answer,
                    citations: cited,
                    tool_trace: trace,
                    rounds: round_index as i64,
                });
            }
            messages.push(assistant_tool_round(&message));
            for call in &message.tool_calls {
                if !scoped_document_id.is_empty() && call.name == "list_documents" {
                    let result = json!({
                        "error": "整本问答不允许浏览图书馆，请用 search_fulltext / read_blocks。",
                        "document_id": scoped_document_id,
                    });
                    let skipped = json!({"skipped": true});
                    on_event(json!({
                        "type": "tool",
                        "round": round_index,
                        "tool": call.name,
                        "arguments": skipped,
                    }));
                    trace.push(json!({
                        "round": round_index,
                        "tool": call.name,
                        "arguments": skipped,
                    }));
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call.id,
                        "content": result.to_string(),
                    }));
                    continue;
                }
                let arguments = parse_arguments(call);
                let arguments = scope_tool_arguments(
                    &call.name,
                    arguments,
                    &scoped_document_id,
                    &scoped_job_id,
                );
                on_event(json!({
                    "type": "tool",
                    "round": round_index,
                    "tool": call.name,
                    "arguments": arguments,
                }));
                let mut result = self.tools.invoke(&call.name, &arguments);
                next_ref = assign_refs(&mut result, &mut citations, next_ref);
                trace.push(json!({
                    "round": round_index,
                    "tool": call.name,
                    "arguments": arguments,
                }));
                let payload = public_tool_payload(&result);
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call.id,
                    "content": payload.to_string(),
                }));
            }
        }

        // 轮数耗尽:强制模型基于已有证据收尾(不给工具)
        messages.push(json!({
            "role": "user",
            "content": "请基于以上已检索到的证据直接给出最终回答,不要再调用工具。引用只用 [n]。",
        }));
        let message = client.chat(messages.clone(), Vec::new()).await?;
        let answer = sanitize_answer_text(&message.content, &citations);
        let cited = referenced_citations(&answer, &citations);
        Ok(AskResult {
            answer,
            citations: cited,
            tool_trace: trace,
            rounds: self.max_tool_rounds as i64,
        })
    }
}

fn assistant_tool_round(message: &super::llm::AssistantMessage) -> Value {
    let calls: Vec<Value> = message
        .tool_calls
        .iter()
        .map(|call| {
            json!({
                "id": call.id,
                "type": "function",
                "function": {"name": call.name, "arguments": call.arguments},
            })
        })
        .collect();
    json!({
        "role": "assistant",
        "content": message.content,
        "tool_calls": calls,
    })
}

fn parse_arguments(call: &ToolCall) -> Map<String, Value> {
    serde_json::from_str::<Map<String, Value>>(&call.arguments).unwrap_or_default()
}

/// 整本问答时强制工具落在当前文档/任务,不依赖模型自觉传参。
fn scope_tool_arguments(
    name: &str,
    mut arguments: Map<String, Value>,
    document_id: &str,
    job_id: &str,
) -> Map<String, Value> {
    if document_id.is_empty() {
        return arguments;
    }
    if matches!(
        name,
        "search_fulltext"
            | "search_favorites"
            | "list_documents"
            | "read_blocks"
            | "find_mentions"
    ) {
        arguments.insert(
            "document_id".to_string(),
            Value::String(document_id.to_string()),
        );
    }
    if name == "read_blocks" && !job_id.is_empty() {
        let has_job = arguments
            .get("job_id")
            .and_then(Value::as_str)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
        if !has_job {
            arguments.insert("job_id".to_string(), Value::String(job_id.to_string()));
        }
    }
    arguments
}

fn pick_snippet(entry: &Value) -> String {
    for key in [
        "translated_snippet",
        "translated_text",
        "translated_quote_text",
        "source_snippet",
        "source_text",
        "quote_text",
        "snippet",
    ] {
        if let Some(text) = entry.get(key).and_then(Value::as_str) {
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }
    String::new()
}

/// 给带锚点的工具结果编引用号,并把 ref 写回结果条目(内部仍保留 block_id 供 Citation)。
fn assign_refs(
    result: &mut Value,
    citations: &mut BTreeMap<i64, Citation>,
    mut next_ref: i64,
) -> i64 {
    // read_blocks: 把外层锚点写回每个 block
    let outer_doc = result.get("document_id").cloned();
    let outer_job = result.get("job_id").cloned();
    let outer_page = result.get("page_idx").cloned();
    if let Some(blocks) = result.get_mut("blocks").and_then(Value::as_array_mut) {
        for block in blocks.iter_mut() {
            if let Some(block_map) = block.as_object_mut() {
                if let Some(doc) = &outer_doc {
                    block_map
                        .entry("document_id".to_string())
                        .or_insert_with(|| doc.clone());
                }
                if let Some(job) = &outer_job {
                    block_map
                        .entry("job_id".to_string())
                        .or_insert_with(|| job.clone());
                }
                if let Some(page) = &outer_page {
                    block_map
                        .entry("page_idx".to_string())
                        .or_insert_with(|| page.clone());
                }
            }
        }
    }
    for key in ["hits", "favorites", "blocks"] {
        let Some(entries) = result.get_mut(key).and_then(Value::as_array_mut) else {
            continue;
        };
        for entry in entries.iter_mut() {
            let document_id = entry
                .get("document_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let block_id = entry
                .get("block_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if document_id.is_empty() || block_id.is_empty() {
                continue;
            }
            let snippet = pick_snippet(entry);
            let job_id = entry
                .get("job_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let page_idx = entry.get("page_idx").and_then(Value::as_i64).unwrap_or(0);
            entry["ref"] = Value::from(next_ref);
            citations.insert(
                next_ref,
                Citation {
                    ref_num: next_ref,
                    document_id,
                    job_id,
                    page_idx,
                    block_id,
                    snippet: snippet.chars().take(200).collect(),
                },
            );
            next_ref += 1;
        }
    }
    next_ref
}

/// 模型可见的锚点:只有 ref / page(1 基) / snippet,无内部 ID。
/// 用户标注额外带 note(非空才给),否则模型看不到「我为什么标它」。
fn public_anchor(entry: &Value) -> Option<Value> {
    let ref_num = entry.get("ref").and_then(Value::as_i64)?;
    let page_idx = entry.get("page_idx").and_then(Value::as_i64).unwrap_or(0);
    let snippet = pick_snippet(entry);
    let mut anchor = json!({
        "ref": ref_num,
        "page": page_idx + 1,
        "snippet": snippet.chars().take(280).collect::<String>(),
    });
    if let Some(note) = entry.get("note").and_then(Value::as_str) {
        let note = note.trim();
        if !note.is_empty() {
            anchor["note"] = Value::String(note.chars().take(200).collect());
        }
    }
    Some(anchor)
}

/// 工具原始结果 → 模型上下文。剥离 block_id/job_id 等,避免抄进回答。
fn public_tool_payload(result: &Value) -> Value {
    if let Some(error) = result.get("error") {
        return json!({"error": error.as_str().unwrap_or("invalid tool result")});
    }
    let mut public: Map<String, Value> = Map::new();
    if let Some(hint) = result.get("hint").and_then(Value::as_str) {
        public.insert("hint".to_string(), Value::String(hint.to_string()));
    }
    if result.get("document_id").is_some() {
        // 仅在需要确认范围时给文档 id,一般整本会话已锁定
        public.insert("scoped".to_string(), Value::Bool(true));
    }
    let mut public_hits: Vec<Value> = Vec::new();
    if let Some(hits) = result.get("hits").and_then(Value::as_array) {
        for hit in hits {
            if let Some(anchor) = public_anchor(hit) {
                public_hits.push(anchor);
            }
        }
        if !public_hits.is_empty() {
            public.insert("hits".to_string(), Value::Array(public_hits));
            public.insert(
                "how_to_cite".to_string(),
                Value::String("回答时用 hits[].ref 写成 [1] [2],page 是页码仅供参考。".to_string()),
            );
        }
    }
    let mut public_favs: Vec<Value> = Vec::new();
    if let Some(favorites) = result.get("favorites").and_then(Value::as_array) {
        for favorite in favorites {
            if let Some(anchor) = public_anchor(favorite) {
                public_favs.push(anchor);
            }
        }
        if !public_favs.is_empty() {
            public.insert("favorites".to_string(), Value::Array(public_favs));
        }
    }
    // search_entities:实体无 block 锚点,不编号;entity_id 必须透传,
    // 否则模型拿不到 find_mentions 的入参。
    let mut public_entities: Vec<Value> = Vec::new();
    if let Some(entities) = result.get("entities").and_then(Value::as_array) {
        for entity in entities.iter().take(30) {
            let Some(entity_id) = entity.get("entity_id").and_then(Value::as_str) else {
                continue;
            };
            let mut item = json!({
                "entity_id": entity_id,
                "name": entity.get("name").cloned().unwrap_or(Value::Null),
                "entity_type": entity.get("entity_type").cloned().unwrap_or(Value::Null),
                "aliases": entity.get("aliases").cloned().unwrap_or_else(|| json!([])),
                "mention_count": entity.get("mention_count").cloned().unwrap_or(Value::Null),
                "document_count": entity.get("document_count").cloned().unwrap_or(Value::Null),
            });
            // related_entities 额外带连边信息;search_entities 没有这些键。
            if let Some(relation_type) = entity.get("relation_type") {
                item["relation_type"] = relation_type.clone();
                item["direction"] = entity.get("direction").cloned().unwrap_or(Value::Null);
            }
            if let Some(explanation) = entity.get("explanation").and_then(Value::as_str) {
                if !explanation.is_empty() {
                    item["why"] = Value::String(explanation.to_string());
                }
            }
            public_entities.push(item);
        }
        if !public_entities.is_empty() {
            public.insert("entities".to_string(), Value::Array(public_entities));
            public.insert(
                "how_to_use_entities".to_string(),
                Value::String(
                    "取 entities[].entity_id 调 find_mentions 拿证据;回答里禁止写出 entity_id。"
                        .to_string(),
                ),
            );
        }
    }
    // get_entity_page:综述正文 + 引用证据。正文里的 [n] 已被剥掉,
    // 模型要用 blocks[].ref 重新编号引用。
    if let Some(page) = result.get("entity_page").and_then(Value::as_object) {
        let mut item = Map::new();
        for key in ["entity_id", "name", "entity_type", "stale", "edited", "body_md"] {
            if let Some(value) = page.get(key) {
                item.insert(key.to_string(), value.clone());
            }
        }
        if !item.is_empty() {
            public.insert("entity_page".to_string(), Value::Object(item));
            public.insert(
                "how_to_use_page".to_string(),
                Value::String(
                    "这是该实体已生成的概念页,可直接作为回答依据;引用请用 blocks[].ref 写成 [n]。"
                        .to_string(),
                ),
            );
        }
    }
    let mut public_blocks: Vec<Value> = Vec::new();
    if let Some(blocks) = result.get("blocks").and_then(Value::as_array) {
        for block in blocks {
            if let Some(anchor) = public_anchor(block) {
                public_blocks.push(anchor);
            }
        }
        if !public_blocks.is_empty() {
            public.insert("blocks".to_string(), Value::Array(public_blocks));
            // 只有 read_blocks 有单页语义;find_mentions 的块跨页,页码看 blocks[].page。
            if let Some(page_idx) = result.get("page_idx").and_then(Value::as_i64) {
                public.insert("page".to_string(), Value::from(page_idx + 1));
            }
            public.insert(
                "how_to_cite".to_string(),
                Value::String("回答时用 blocks[].ref 写成 [n]。".to_string()),
            );
        }
    }
    // search 命中上挂的 image_urls 已在 hits 剥离时丢掉;从原始 hits 收集
    let mut image_urls: Vec<Value> = Vec::new();
    if let Some(images) = result.get("image_urls").and_then(Value::as_array) {
        for url in images.iter().take(8) {
            image_urls.push(url.clone());
        }
    }
    if image_urls.is_empty() {
        if let Some(hits) = result.get("hits").and_then(Value::as_array) {
            for hit in hits {
                if let Some(urls) = hit.get("image_urls").and_then(Value::as_array) {
                    for url in urls {
                        image_urls.push(url.clone());
                        if image_urls.len() >= 8 {
                            break;
                        }
                    }
                }
                if image_urls.len() >= 8 {
                    break;
                }
            }
        }
    }
    if !image_urls.is_empty() {
        public.insert("image_urls".to_string(), Value::Array(image_urls));
    }
    if public.is_empty() {
        public.insert("ok".to_string(), Value::Bool(true));
    }
    Value::Object(public)
}

// 引用清洗用正则：编译一次、全局复用，避免每轮回答重复编译。
static BRACKET_BLOCK_ID_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)\[\s*(p\d+[-_]b\d+)\s*\]").expect("bracket regex"));
static BARE_BLOCK_ID_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)p\d+[-_]b\d+").expect("bare regex"));
/// 实体 id(ent-<14 位时间戳>-<6 位 hex>)是工具协议内部标识,禁止出现在回答里。
static ENTITY_ID_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)ent-\d{14}-[0-9a-f]{6}").expect("entity id regex"));
static MULTI_SPACE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[ \t]{2,}").expect("space regex"));
static TRAILING_SPACE_NEWLINE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r" *\n").expect("newline regex"));
static CITATION_REF_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\[(\d+)\]").expect("citation regex"));

/// 把正文里的 [p002-b0004] / 裸 block_id 映射成 [n] 或删掉。
fn sanitize_answer_text(answer: &str, citations: &BTreeMap<i64, Citation>) -> String {
    if answer.trim().is_empty() {
        return answer.to_string();
    }
    let by_block: HashMap<String, i64> = citations
        .values()
        .filter(|citation| !citation.block_id.is_empty())
        .map(|citation| {
            (
                citation.block_id.to_lowercase().replace('_', "-"),
                citation.ref_num,
            )
        })
        .collect();
    // 1. 方括号形式:[p002-b0004]
    let cleaned = BRACKET_BLOCK_ID_RE
        .replace_all(answer, |caps: &regex::Captures| {
            block_id_to_ref(caps, &by_block)
        })
        .to_string();
    // 2. 裸形式:p002-b0004(边界 = 前后均非 [A-Za-z0-9_/];regex 不支持 look-around,
    //    手动按字节检查边界)
    let mut out = String::with_capacity(cleaned.len());
    let mut last = 0usize;
    for matched in BARE_BLOCK_ID_RE.find_iter(&cleaned) {
        let prev_ok =
            matched.start() == 0 || !is_word_or_slash(cleaned.as_bytes()[matched.start() - 1]);
        let next_ok =
            matched.end() == cleaned.len() || !is_word_or_slash(cleaned.as_bytes()[matched.end()]);
        if !(prev_ok && next_ok) {
            continue;
        }
        out.push_str(&cleaned[last..matched.start()]);
        let key = cleaned[matched.start()..matched.end()]
            .to_lowercase()
            .replace('_', "-");
        if let Some(ref_num) = by_block.get(&key) {
            out.push_str(&format!("[{ref_num}]"));
        }
        last = matched.end();
    }
    out.push_str(&cleaned[last..]);
    // 3. 实体 id 直接删除(模型无正当理由输出它)
    let out = ENTITY_ID_RE.replace_all(&out, "").to_string();
    // 压缩因删除产生的多余空白
    let collapsed = MULTI_SPACE_RE.replace_all(&out, " ").to_string();
    TRAILING_SPACE_NEWLINE_RE
        .replace_all(&collapsed, "\n")
        .to_string()
        .trim()
        .to_string()
}

fn is_word_or_slash(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'/'
}

fn block_id_to_ref(caps: &regex::Captures, by_block: &HashMap<String, i64>) -> String {
    let key = caps
        .get(1)
        .map(|matched| matched.as_str().to_lowercase().replace('_', "-"))
        .unwrap_or_default();
    match by_block.get(&key) {
        Some(ref_num) => format!("[{ref_num}]"),
        None => String::new(),
    }
}

/// 按正文出现顺序保留 [n],避免排序打乱阅读顺序。
fn referenced_citations(answer: &str, citations: &BTreeMap<i64, Citation>) -> Vec<Citation> {
    let mut ordered_refs: Vec<i64> = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();
    for caps in CITATION_REF_RE.captures_iter(answer) {
        let ref_num = caps
            .get(1)
            .map(|matched| matched.as_str().parse::<i64>().unwrap_or(0))
            .unwrap_or(0);
        if seen.contains(&ref_num) || !citations.contains_key(&ref_num) {
            continue;
        }
        seen.insert(ref_num);
        ordered_refs.push(ref_num);
    }
    let selected: Vec<Citation> = ordered_refs
        .into_iter()
        .filter_map(|ref_num| citations.get(&ref_num).cloned())
        .collect();
    if !selected.is_empty() {
        return selected.into_iter().take(8).collect();
    }
    // 模型没标 [n] 时:按页去重,最多 3 条
    if citations.is_empty() {
        return Vec::new();
    }
    let mut picked: Vec<Citation> = Vec::new();
    let mut pages: HashSet<i64> = HashSet::new();
    for ref_num in citations.keys() {
        let item = &citations[ref_num];
        if pages.contains(&item.page_idx) {
            continue;
        }
        pages.insert(item.page_idx);
        picked.push(item.clone());
        if picked.len() >= 3 {
            break;
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(document_id: &str, block_id: &str, page_idx: i64, text: &str) -> Value {
        json!({
            "document_id": document_id,
            "job_id": "job-1",
            "page_idx": page_idx,
            "block_id": block_id,
            "source_snippet": text,
        })
    }

    #[test]
    fn assign_refs_numbered_in_order_and_persist_in_result() {
        let mut result = json!({
            "hits": [hit("doc-1", "p002-b0004", 1, "first")],
            "blocks": [
                {"block_id": "p003-b0001", "source_text": "block text"}
            ],
            "document_id": "doc-1",
            "job_id": "job-1",
            "page_idx": 2,
        });
        let mut citations: BTreeMap<i64, Citation> = BTreeMap::new();
        let next_ref = assign_refs(&mut result, &mut citations, 1);
        assert_eq!(next_ref, 3);
        assert_eq!(result["hits"][0]["ref"], json!(1));
        assert_eq!(result["blocks"][0]["ref"], json!(2));
        assert_eq!(result["blocks"][0]["document_id"], json!("doc-1"));
        assert_eq!(result["blocks"][0]["page_idx"], json!(2));
        assert_eq!(citations[&1].block_id, "p002-b0004");
        assert_eq!(citations[&2].snippet, "block text");
    }

    #[test]
    fn public_payload_strips_internal_ids() {
        let mut result = json!({
            "hits": [hit("doc-1", "p002-b0004", 1, "片段内容")],
        });
        let mut citations: BTreeMap<i64, Citation> = BTreeMap::new();
        assign_refs(&mut result, &mut citations, 1);
        let public = public_tool_payload(&result);
        assert!(public["hits"][0]["ref"].as_i64().is_some());
        assert_eq!(public["hits"][0]["page"], json!(2));
        assert!(public["hits"][0].get("block_id").is_none());
        assert!(public["hits"][0].get("document_id").is_none());
        assert!(public["hits"][0]["snippet"].as_str().is_some());
    }

    /// 标注的备注必须透给模型,否则「我在 X 上标过什么」只剩引文。
    #[test]
    fn public_payload_exposes_favorite_note() {
        let mut result = json!({
            "favorites": [{
                "favorite_id": "fav-1",
                "document_id": "doc-1",
                "job_id": "job-1",
                "page_idx": 2,
                "block_id": "p003-b0000",
                "quote_text": "GNN 的表示学习",
                "translated_quote_text": "",
                "note": "  我的备注  ",
            }],
        });
        let mut citations: BTreeMap<i64, Citation> = BTreeMap::new();
        assign_refs(&mut result, &mut citations, 1);
        let public = public_tool_payload(&result);
        assert_eq!(public["favorites"][0]["note"], json!("我的备注"));
        assert_eq!(public["favorites"][0]["snippet"], json!("GNN 的表示学习"));
        assert!(public["favorites"][0].get("favorite_id").is_none());
    }

    #[test]
    fn sanitize_maps_block_ids_to_refs_and_strips_bare() {
        let mut citations: BTreeMap<i64, Citation> = BTreeMap::new();
        citations.insert(
            1,
            Citation {
                ref_num: 1,
                document_id: "doc-1".into(),
                job_id: "job-1".into(),
                page_idx: 1,
                block_id: "p002-b0004".into(),
                snippet: "x".into(),
            },
        );
        let cleaned = sanitize_answer_text(
            "结论 [p002-b0004] 与 p002-b0004 均映射, p002-b9999 被删。",
            &citations,
        );
        assert!(cleaned.contains("结论 [1] 与 [1] 均映射"), "got: {cleaned}");
        assert!(!cleaned.contains("p002-b9999"), "got: {cleaned}");
        assert!(!cleaned.contains("b0004"), "got: {cleaned}");
    }

    #[test]
    fn public_payload_passes_entities_through() {
        let result = json!({"entities": [{
            "entity_id": "ent-20260909123456-a1b2c3",
            "name": "卤素",
            "entity_type": "term",
            "aliases": ["halogen"],
            "mention_count": 3,
            "document_count": 2,
        }]});
        let public = public_tool_payload(&result);
        assert_eq!(
            public["entities"][0]["entity_id"],
            json!("ent-20260909123456-a1b2c3")
        );
        assert_eq!(public["entities"][0]["mention_count"], json!(3));
    }

    /// related_entities 的连边字段要透给模型,否则工具只剩邻居名字。
    #[test]
    fn public_payload_carries_relation_fields() {
        let result = json!({"entities": [{
            "entity_id": "ent-20260909123456-a1b2c3",
            "name": "GNN",
            "entity_type": "method",
            "aliases": [],
            "mention_count": 1,
            "document_count": 1,
            "relation_type": "uses",
            "direction": "out",
            "explanation": "同句出现",
        }]});
        let public = public_tool_payload(&result);
        assert_eq!(public["entities"][0]["relation_type"], json!("uses"));
        assert_eq!(public["entities"][0]["direction"], json!("out"));
        assert_eq!(public["entities"][0]["why"], json!("同句出现"));
    }

    #[test]
    fn sanitize_strips_entity_ids() {
        let citations: BTreeMap<i64, Citation> = BTreeMap::new();
        let cleaned = sanitize_answer_text("实体 ent-20260909123456-a1b2c3 提及于此。", &citations);
        assert!(!cleaned.contains("ent-"), "got: {cleaned}");
        assert!(cleaned.contains("提及于此"), "got: {cleaned}");
    }

    #[test]
    fn referenced_citations_preserves_appearance_order() {
        let mut citations: BTreeMap<i64, Citation> = BTreeMap::new();
        for ref_num in 1..=3 {
            citations.insert(
                ref_num,
                Citation {
                    ref_num,
                    document_id: "doc-1".into(),
                    job_id: "job-1".into(),
                    page_idx: 1,
                    block_id: format!("p002-b{:04}", ref_num),
                    snippet: "x".into(),
                },
            );
        }
        let selected = referenced_citations("看 [3] 与 [1], 再论 [2]", &citations);
        let refs: Vec<i64> = selected.iter().map(|item| item.ref_num).collect();
        assert_eq!(refs, vec![3, 1, 2]);
    }

    #[test]
    fn referenced_citations_falls_back_by_page() {
        let mut citations: BTreeMap<i64, Citation> = BTreeMap::new();
        citations.insert(
            1,
            Citation {
                ref_num: 1,
                document_id: "doc-1".into(),
                job_id: "job-1".into(),
                page_idx: 0,
                block_id: "p001-b0001".into(),
                snippet: "x".into(),
            },
        );
        citations.insert(
            2,
            Citation {
                ref_num: 2,
                document_id: "doc-1".into(),
                job_id: "job-1".into(),
                page_idx: 0,
                block_id: "p001-b0002".into(),
                snippet: "x".into(),
            },
        );
        let selected = referenced_citations("没有编号的答案", &citations);
        let pages: Vec<i64> = selected.iter().map(|item| item.page_idx).collect();
        assert_eq!(pages.len(), 1);
        assert_eq!(selected[0].ref_num, 1);
    }

    #[test]
    fn scope_arguments_forces_document_id() {
        let mut arguments = Map::new();
        arguments.insert("query".to_string(), json!("卤素锂交换"));
        let scoped = scope_tool_arguments("search_fulltext", arguments, "doc-1", "");
        assert_eq!(scoped["document_id"], json!("doc-1"));
    }

    #[test]
    fn scope_arguments_fills_read_blocks_job_id_when_empty() {
        let mut arguments = Map::new();
        arguments.insert("page_idx".to_string(), json!(0));
        let scoped = scope_tool_arguments("read_blocks", arguments, "doc-1", "job-9");
        assert_eq!(scoped["job_id"], json!("job-9"));
        let mut preserved = Map::new();
        preserved.insert("page_idx".to_string(), json!(0));
        preserved.insert("job_id".to_string(), json!("job-8"));
        let kept = scope_tool_arguments("read_blocks", preserved, "doc-1", "job-9");
        assert_eq!(kept["job_id"], json!("job-8"));
    }

    #[test]
    fn scope_arguments_is_noop_without_document() {
        let mut arguments = Map::new();
        arguments.insert("query".to_string(), json!("x"));
        let scoped = scope_tool_arguments("search_fulltext", arguments, "", "");
        assert!(scoped.get("document_id").is_none());
    }

    // ===== agent 循环集成测试(移植自 ai_service/tests/test_agent.py)=====

    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use futures_util::future::BoxFuture;

    use super::super::llm::AssistantMessage;
    use crate::db::documents::sha256_hex;
    use crate::db::Db;
    use crate::error::AppError;
    use crate::models::api::{
        BlockEntityLink, EntityPageCitation, EntityPageRecord, FtsBlockRow, NewEntity,
    };
    use crate::models::{now_iso, UploadRecord};

    /// 脚本化 fake chat:按调用顺序返回预设 assistant 消息,并记录每次收到的上下文。
    struct ScriptedChat {
        turns: Vec<AssistantMessage>,
        calls: Mutex<Vec<(Vec<Value>, Vec<Value>)>>,
        next: AtomicUsize,
    }

    impl ScriptedChat {
        fn new(turns: Vec<AssistantMessage>) -> Self {
            Self {
                turns,
                calls: Mutex::new(Vec::new()),
                next: AtomicUsize::new(0),
            }
        }
        fn tool_call(id: &str, name: &str, arguments: &str) -> AssistantMessage {
            AssistantMessage {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments: arguments.to_string(),
                }],
            }
        }
        fn answer(content: &str) -> AssistantMessage {
            AssistantMessage {
                content: content.to_string(),
                tool_calls: Vec::new(),
            }
        }
        fn seen_tool_messages(&self) -> Vec<Value> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .flat_map(|(messages, _)| {
                    messages
                        .iter()
                        .filter(|message| message["role"] == "tool")
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .collect()
        }
        fn last_tools(&self) -> Vec<Value> {
            self.calls
                .lock()
                .unwrap()
                .last()
                .map(|(_, tools)| tools.clone())
                .unwrap_or_default()
        }
    }

    impl Chat for ScriptedChat {
        fn chat(
            &self,
            messages: Vec<Value>,
            tools: Vec<Value>,
        ) -> BoxFuture<'static, Result<AssistantMessage, AppError>> {
            let index = self.next.fetch_add(1, Ordering::SeqCst);
            let turn = self
                .turns
                .get(index)
                .cloned()
                .unwrap_or_else(|| ScriptedChat::answer("(script exhausted)"));
            self.calls.lock().unwrap().push((messages, tools));
            Box::pin(async move { Ok(turn) })
        }
    }

    struct TestDb {
        root: PathBuf,
        db_path: PathBuf,
        data_root: PathBuf,
    }

    impl TestDb {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "retainpdf-ai-agent-{tag}-{}-{}",
                std::process::id(),
                fastrand::u64(..)
            ));
            fs::create_dir_all(root.join("db")).expect("mkdir db");
            Self {
                db_path: root.join("db").join("jobs.db"),
                data_root: root.clone(),
                root,
            }
        }
        fn db(&self) -> Db {
            Db::new(self.db_path.clone(), self.data_root.clone())
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// 种子两块都含"选择性"的 FTS 行(doc id = sha256("ai-agent-{tag}")),供检索引用。
    fn seed_search_doc(db: &Db, tag: &str) {
        let document_id = sha256_hex(format!("ai-agent-{tag}").as_bytes());
        db.upsert_document_from_upload(&UploadRecord {
            upload_id: format!("up-{tag}"),
            filename: "paper.pdf".to_string(),
            stored_path: "uploads/x/paper.pdf".to_string(),
            bytes: 10,
            page_count: 12,
            uploaded_at: now_iso(),
            developer_mode: false,
            content_hash: document_id.clone(),
        })
        .expect("upsert");
        db.replace_document_fts(
            &document_id,
            "job-1",
            &[
                FtsBlockRow {
                    page_idx: 3,
                    block_id: "p004-b0002".to_string(),
                    source_text: "reaction rate increased".to_string(),
                    translated_text: "选择性来自反应速率显著提高".to_string(),
                },
                FtsBlockRow {
                    page_idx: 7,
                    block_id: "p008-b0001".to_string(),
                    source_text: "selectivity from conjugation".to_string(),
                    translated_text: "选择性来自共轭效应".to_string(),
                },
            ],
        )
        .expect("fts");
    }

    #[tokio::test]
    async fn ask_loop_runs_tools_then_answers_with_cited_anchors() {
        let fs = TestDb::new("cited");
        let db = fs.db();
        db.init().expect("init");
        seed_search_doc(&db, "cited");
        let tools = AiTools::new(&db, &fs.data_root);
        let fake = ScriptedChat::new(vec![
            ScriptedChat::tool_call("call-1", "search_fulltext", r#"{"query":"选择性"}"#),
            ScriptedChat::answer("选择性来自共轭效应 [2]。"),
        ]);
        let agent = RetrievalAgent::new(tools, 4);
        let result = agent
            .ask(&fake, "为什么有选择性?", "", "", &[], |_| {})
            .await
            .expect("ask");

        assert_eq!(result.rounds, 2);
        assert_eq!(result.answer, "选择性来自共轭效应 [2]。");
        // 只返回被引用的锚点,ref 编号写进给模型看的工具结果
        assert_eq!(result.citations.len(), 1);
        assert_eq!(result.citations[0].ref_num, 2);
        assert!(matches!(
            result.citations[0].block_id.as_str(),
            "p004-b0002" | "p008-b0001"
        ));
        let seen = fake.seen_tool_messages();
        assert_eq!(seen.len(), 1, "agent 应把工具结果回喂给模型");
        let payload: Value = serde_json::from_str(seen[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(payload["hits"][0]["ref"], json!(1));
        assert_eq!(payload["hits"][1]["ref"], json!(2));
        assert_eq!(
            json!(result.tool_trace),
            json!([{
                "round": 1,
                "tool": "search_fulltext",
                "arguments": {"query": "选择性"}
            }])
        );
    }

    #[tokio::test]
    async fn ask_loop_forces_document_id_into_scoped_search() {
        let fs = TestDb::new("scoped");
        let db = fs.db();
        db.init().expect("init");
        seed_search_doc(&db, "scoped");
        let document_id = sha256_hex("ai-agent-scoped".as_bytes());
        let tools = AiTools::new(&db, &fs.data_root);
        let fake = ScriptedChat::new(vec![
            ScriptedChat::tool_call("call-1", "search_fulltext", r#"{"query":"选择性"}"#),
            ScriptedChat::answer("答案 [1]。"),
        ]);
        let agent = RetrievalAgent::new(tools, 4);
        let result = agent
            .ask(&fake, "为什么?", &document_id, "job-1", &[], |_| {})
            .await
            .expect("ask");
        // 模型没传 document_id 也要强制注入
        assert_eq!(
            result.tool_trace[0]["arguments"]["document_id"],
            json!(document_id)
        );
    }

    #[tokio::test]
    async fn ask_loop_falls_back_to_all_citations_without_markers() {
        let fs = TestDb::new("fallback");
        let db = fs.db();
        db.init().expect("init");
        seed_search_doc(&db, "fallback");
        let tools = AiTools::new(&db, &fs.data_root);
        let fake = ScriptedChat::new(vec![
            ScriptedChat::tool_call("call-1", "search_fulltext", r#"{"query":"选择性"}"#),
            ScriptedChat::answer("选择性来自共轭效应且有反应速率。"),
        ]);
        let agent = RetrievalAgent::new(tools, 4);
        let result = agent
            .ask(&fake, "结论?", "", "", &[], |_| {})
            .await
            .expect("ask");
        assert_eq!(result.citations.len(), 2);
    }

    #[tokio::test]
    async fn ask_loop_uses_entity_tools_for_evidence() {
        let fs = TestDb::new("entity-tools");
        let db = fs.db();
        db.init().expect("init");
        seed_search_doc(&db, "entity-tools");
        let document_id = sha256_hex("ai-agent-entity-tools".as_bytes());
        let entity = db
            .upsert_entity(&NewEntity {
                name: "选择性".to_string(),
                entity_type: "term".to_string(),
                aliases: vec!["selectivity".to_string()],
                description: String::new(),
            })
            .expect("entity");
        db.link_block_entity(&BlockEntityLink {
            document_id: document_id.clone(),
            entity_id: entity.entity_id.clone(),
            page_idx: 7,
            block_id: "p008-b0001".to_string(),
            job_id: "job-1".to_string(),
            surface_form: "选择性".to_string(),
            snippet: "选择性来自共轭效应".to_string(),
            confidence: 1.0,
            source: "glossary".to_string(),
        })
        .expect("link");
        let tools = AiTools::new(&db, &fs.data_root);
        let fake = ScriptedChat::new(vec![
            ScriptedChat::tool_call("c1", "search_entities", r#"{"query":"选择性"}"#),
            ScriptedChat::tool_call(
                "c2",
                "find_mentions",
                &format!(r#"{{"entity_id":"{}"}}"#, entity.entity_id),
            ),
            ScriptedChat::answer("选择性来自共轭效应 [1]。"),
        ]);
        let agent = RetrievalAgent::new(tools, 5);
        let result = agent
            .ask(&fake, "库里关于选择性说了什么?", "", "", &[], |_| {})
            .await
            .expect("ask");
        assert_eq!(result.citations.len(), 1);
        assert_eq!(result.citations[0].block_id, "p008-b0001");
        let seen = fake.seen_tool_messages();
        let first: Value = serde_json::from_str(seen[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(first["entities"][0]["entity_id"], json!(entity.entity_id));
        let second: Value = serde_json::from_str(seen.last().unwrap()["content"].as_str().unwrap())
            .unwrap();
        assert_eq!(second["blocks"][0]["ref"], json!(1));
    }

    #[tokio::test]
    async fn ask_loop_uses_entity_page_tool() {
        let fs = TestDb::new("entity-page");
        let db = fs.db();
        db.init().expect("init");
        seed_search_doc(&db, "entity-page");
        let document_id = sha256_hex("ai-agent-entity-page".as_bytes());
        let entity = db
            .upsert_entity(&NewEntity {
                name: "选择性".to_string(),
                entity_type: "term".to_string(),
                aliases: Vec::new(),
                description: String::new(),
            })
            .expect("entity");
        db.link_block_entity(&BlockEntityLink {
            document_id: document_id.clone(),
            entity_id: entity.entity_id.clone(),
            page_idx: 7,
            block_id: "p008-b0001".to_string(),
            job_id: "job-1".to_string(),
            surface_form: "选择性".to_string(),
            snippet: "选择性来自共轭效应".to_string(),
            confidence: 1.0,
            source: "glossary".to_string(),
        })
        .expect("link");
        let sig = db
            .entity_page_evidence_sig(&entity.entity_id)
            .expect("sig");
        db.upsert_entity_page(&EntityPageRecord {
            entity_id: entity.entity_id.clone(),
            body_md: "选择性是反应倾向的度量 [1]。".to_string(),
            citations: vec![EntityPageCitation {
                ref_num: 1,
                document_id: document_id.clone(),
                document_title: "paper.pdf".to_string(),
                job_id: "job-1".to_string(),
                page_idx: 7,
                block_id: "p008-b0001".to_string(),
                snippet: "选择性来自共轭效应".to_string(),
            }],
            evidence_sig: sig,
            generated_at: now_iso(),
            edited_body_md: String::new(),
            edited_at: String::new(),
        }, true)
        .expect("page");
        let tools = AiTools::new(&db, &fs.data_root);
        let fake = ScriptedChat::new(vec![
            ScriptedChat::tool_call("c1", "search_entities", r#"{"query":"选择性"}"#),
            ScriptedChat::tool_call(
                "c2",
                "get_entity_page",
                &format!(r#"{{"entity_id":"{}"}}"#, entity.entity_id),
            ),
            ScriptedChat::answer("选择性是反应倾向的度量 [1]。"),
        ]);
        let agent = RetrievalAgent::new(tools, 5);
        let result = agent
            .ask(&fake, "选择性是什么?", "", "", &[], |_| {})
            .await
            .expect("ask");
        assert_eq!(result.citations.len(), 1);
        assert_eq!(result.citations[0].block_id, "p008-b0001");
        let seen = fake.seen_tool_messages();
        let page_payload: Value =
            serde_json::from_str(seen.last().unwrap()["content"].as_str().unwrap()).unwrap();
        // 正文里的 [1] 已剥掉,模型必须用 blocks[].ref 重新编号
        assert_eq!(
            page_payload["entity_page"]["body_md"],
            json!("选择性是反应倾向的度量 。")
        );
        assert_eq!(page_payload["entity_page"]["stale"], json!(false));
        assert_eq!(page_payload["blocks"][0]["ref"], json!(1));
    }

    #[tokio::test]
    async fn ask_loop_forces_final_answer_when_rounds_exhausted() {
        let fs = TestDb::new("exhausted");
        let db = fs.db();
        db.init().expect("init");
        seed_search_doc(&db, "exhausted");
        let tools = AiTools::new(&db, &fs.data_root);
        let fake = ScriptedChat::new(vec![
            ScriptedChat::tool_call("c1", "search_fulltext", r#"{"query":"选择性1"}"#),
            ScriptedChat::tool_call("c2", "search_fulltext", r#"{"query":"选择性2"}"#),
            ScriptedChat::tool_call("c3", "search_fulltext", r#"{"query":"选择性3"}"#),
            ScriptedChat::answer("基于已有证据的最终回答 [1]。"),
        ]);
        let agent = RetrievalAgent::new(tools, 3);
        let result = agent
            .ask(&fake, "一直想搜的问题", "", "", &[], |_| {})
            .await
            .expect("ask");
        assert_eq!(result.rounds, 3);
        assert!(result.answer.contains("最终回答"));
        assert_eq!(result.tool_trace.len(), 3);
        // 收尾轮不带工具
        assert!(fake.last_tools().is_empty());
    }

    #[tokio::test]
    async fn ask_loop_feeds_unknown_tool_error_back_to_model() {
        let fs = TestDb::new("unknown-tool");
        let db = fs.db();
        db.init().expect("init");
        seed_search_doc(&db, "unknown-tool");
        let tools = AiTools::new(&db, &fs.data_root);
        let fake = ScriptedChat::new(vec![
            AssistantMessage {
                content: String::new(),
                tool_calls: vec![
                    ToolCall {
                        id: "c1".to_string(),
                        name: "missing".to_string(),
                        arguments: "{}".to_string(),
                    },
                    ToolCall {
                        id: "c2".to_string(),
                        name: "search_fulltext".to_string(),
                        arguments: r#"{"query":"选择性"}"#.to_string(),
                    },
                ],
            },
            ScriptedChat::answer("工具都失败了,无法回答。"),
        ]);
        let agent = RetrievalAgent::new(tools, 3);
        let result = agent
            .ask(&fake, "q", "", "", &[], |_| {})
            .await
            .expect("ask");
        assert!(result.answer.starts_with("工具都失败了"));
        let contents: Vec<String> = fake
            .seen_tool_messages()
            .iter()
            .map(|message| message["content"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(
            contents
                .iter()
                .any(|content| content.contains("unknown tool: missing")),
            "未知工具错误应回喂给模型: {contents:?}"
        );
        assert!(contents.iter().any(|content| content.contains("ref")));
    }
}
