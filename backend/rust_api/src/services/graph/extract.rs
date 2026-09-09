//! 文档级 LLM 抽取:一次调用产出实体 + 关系。
//! 模型只给名字与类型,不给 block_id(会幻觉);证据归属由 mentions.rs 字面扫描完成。

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::graph::normalize_entity_name;
use crate::error::AppError;
use crate::models::api::{EntityRecord, NewEntity, NewEntityRelation};
use crate::services::ai::blocks::{load_job_blocks, Block};
use crate::services::ai::llm::Chat;
use crate::services::ai::tools::safe_job_root;

use super::mentions::link_entities_mentions;
use super::GraphDeps;

/// 单次抽取送进模型的正文上限(原文 + 译文)。
const EXTRACT_MAX_CHARS: usize = 24_000;
const MAX_ENTITIES: usize = 60;
const MAX_RELATIONS: usize = 120;

pub const ENTITY_TYPES: [&str; 9] = [
    "concept", "method", "material", "dataset", "person", "org", "metric", "formula", "term",
];
pub const RELATION_TYPES: [&str; 8] = [
    "uses",
    "improves_on",
    "contradicts",
    "part_of",
    "related_to",
    "defines",
    "evaluates",
    "produces",
];

const EXTRACTION_SYSTEM_PROMPT: &str = "\
你是文献知识图谱抽取器。阅读用户给出的文献正文(英文原文与中文译文混排),抽取可复用实体及其关系。

实体类型只能是:concept / method / material / dataset / person / org / metric / formula / term
关系类型只能是:uses / improves_on / contradicts / part_of / related_to / defines / evaluates / produces

只输出一个 JSON 对象,不要 Markdown 代码块、不要任何解释:
{\"entities\":[{\"name\":\"规范名\",\"type\":\"method\",\"aliases\":[\"别名或缩写\"],\"description\":\"一句话\"}],\
\"relations\":[{\"from\":\"实体name\",\"to\":\"实体name\",\"type\":\"uses\",\"confidence\":0.8,\"explanation\":\"依据\"}]}

要求:
- name 用文献中的规范写法(英文优先),aliases 放同义写法/缩写/中文译名。
- 只抽正文明确出现的实体;最多 60 个实体、120 条关系。
- relations 的 from/to 必须与上面 entities 里的某个 name 完全一致。
- 不确定的关系用 related_to,不要臆造。";

/// 抽取结果统计。
#[derive(Debug, Default)]
pub struct ExtractOutcome {
    pub entities: usize,
    pub mentions: usize,
    pub relations: usize,
}

/// 抽某文档的实体与关系:读块 → 一次 LLM 调用 → 消歧落库 → 字面扫描挂证据 → 写关系。
/// 可重复调用(先清本文档抽取产物再重写)。
pub async fn extract_document_graph<C: Chat>(
    deps: &GraphDeps<'_>,
    client: &C,
    document_id: &str,
) -> Result<ExtractOutcome, AppError> {
    let document = deps.db.get_document(document_id)?;
    let job_id = document
        .active_job_id
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::bad_request("文档尚无翻译产物，无法抽取实体"))?;
    let job_root = safe_job_root(deps.data_root, &job_id)
        .ok_or_else(|| AppError::internal("invalid active job id"))?;
    let blocks = load_job_blocks(&job_root)?;
    let corpus = build_corpus(&blocks, EXTRACT_MAX_CHARS);
    if corpus.trim().is_empty() {
        return Err(AppError::bad_request("文档没有可抽取的文本"));
    }
    let messages = vec![
        json!({"role": "system", "content": EXTRACTION_SYSTEM_PROMPT}),
        json!({"role": "user", "content": corpus}),
    ];
    // 空 tools = 纯回答,不触发 function calling。
    let message = client.chat(messages, Vec::new()).await?;
    let payload = parse_extraction(&message.content)?;

    // 模型调用成功后再清旧产物:失败时保留上一次结果。
    deps.db.clear_document_extraction(document_id)?;

    let mut by_name: HashMap<String, String> = HashMap::new();
    let mut resolved: Vec<EntityRecord> = Vec::new();
    for raw in payload.entities.iter().take(MAX_ENTITIES) {
        let name = raw.name.trim();
        let name_norm = normalize_entity_name(name);
        if name_norm.is_empty() || by_name.contains_key(&name_norm) {
            continue;
        }
        let entity_type = normalize_entity_type(&raw.entity_type);
        let record = resolve_or_create(
            deps,
            name,
            &name_norm,
            &entity_type,
            &as_str_list(Some(&raw.aliases)),
            raw.description.trim(),
        )?;
        by_name.insert(name_norm, record.entity_id.clone());
        for alias in &record.aliases {
            by_name
                .entry(normalize_entity_name(alias))
                .or_insert_with(|| record.entity_id.clone());
        }
        if !resolved
            .iter()
            .any(|existing| existing.entity_id == record.entity_id)
        {
            resolved.push(record);
        }
    }

    let mentions = link_entities_mentions(deps, document_id, &resolved, "extraction")?;

    let mut relations = 0usize;
    for raw in payload.relations.iter().take(MAX_RELATIONS) {
        let Some(from_id) = resolve_endpoint(deps, &by_name, &raw.from)? else {
            continue;
        };
        let Some(to_id) = resolve_endpoint(deps, &by_name, &raw.to)? else {
            continue;
        };
        if from_id == to_id {
            continue;
        }
        let (relation_type, explanation) =
            normalize_relation_type(&raw.relation_type, &raw.explanation);
        if deps.db.add_entity_relation(&NewEntityRelation {
            from_entity_id: from_id,
            to_entity_id: to_id,
            relation_type,
            confidence: as_confidence(&raw.confidence),
            explanation,
            source_document_id: document_id.to_string(),
            source_block_id: String::new(),
        })? {
            relations += 1;
        }
    }

    deps.db.mark_document_graph_extracted(document_id)?;
    Ok(ExtractOutcome {
        entities: resolved.len(),
        mentions,
        relations,
    })
}

/// 消歧级联:同类型规范名精确 → 名字/别名命中已有实体(并入新写法)→ 新建。
fn resolve_or_create(
    deps: &GraphDeps<'_>,
    name: &str,
    name_norm: &str,
    entity_type: &str,
    aliases: &[String],
    description: &str,
) -> Result<EntityRecord, AppError> {
    if let Some(existing) = deps.db.find_entity_exact(name_norm, entity_type)? {
        return Ok(deps.db.upsert_entity(&NewEntity {
            name: existing.name,
            entity_type: existing.entity_type,
            aliases: aliases.to_vec(),
            description: description.to_string(),
        })?);
    }
    if let Some(existing) = deps.db.resolve_entity(name_norm)? {
        // 命中别名/跨类型:把新写法并进已有实体,否则这个写法的提及扫不到。
        let mut merged = aliases.to_vec();
        merged.push(name.to_string());
        return Ok(deps.db.upsert_entity(&NewEntity {
            name: existing.name,
            entity_type: existing.entity_type,
            aliases: merged,
            description: description.to_string(),
        })?);
    }
    Ok(deps.db.upsert_entity(&NewEntity {
        name: name.to_string(),
        entity_type: entity_type.to_string(),
        aliases: aliases.to_vec(),
        description: description.to_string(),
    })?)
}

/// 关系端点:先查本次抽出的实体表,再回查库(术语表种子等已存在实体)。
fn resolve_endpoint(
    deps: &GraphDeps<'_>,
    by_name: &HashMap<String, String>,
    name: &str,
) -> Result<Option<String>, AppError> {
    let name_norm = normalize_entity_name(name);
    if let Some(entity_id) = by_name.get(&name_norm) {
        return Ok(Some(entity_id.clone()));
    }
    Ok(deps
        .db
        .resolve_entity(&name_norm)?
        .map(|record| record.entity_id))
}

fn build_corpus(blocks: &[Block], max_chars: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for block in blocks {
        for text in [&block.source_text, &block.translated_text] {
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let count = text.chars().count();
            if used + count > max_chars {
                return out;
            }
            used += count;
            out.push_str(text);
            out.push('\n');
        }
    }
    out
}

#[derive(Debug, Default, Deserialize)]
struct ExtractionPayload {
    #[serde(default)]
    entities: Vec<ExtractedEntity>,
    #[serde(default)]
    relations: Vec<ExtractedRelation>,
}

#[derive(Debug, Default, Deserialize)]
struct ExtractedEntity {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    entity_type: String,
    /// 模型可能给数组或单个字符串,统一用 Value 接再归一。
    #[serde(default)]
    aliases: Value,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Default, Deserialize)]
struct ExtractedRelation {
    #[serde(default)]
    from: String,
    #[serde(default)]
    to: String,
    #[serde(default, rename = "type")]
    relation_type: String,
    #[serde(default)]
    confidence: Value,
    #[serde(default)]
    explanation: String,
}

/// 容忍 Markdown 代码块与前后杂音,抠出第一个 `{`..最后一个 `}` 解析。
fn parse_extraction(content: &str) -> Result<ExtractionPayload, AppError> {
    let body = strip_code_fence(content.trim());
    let Some(start) = body.find('{') else {
        return Err(AppError::bad_gateway("抽取结果不是 JSON"));
    };
    let Some(end) = body.rfind('}') else {
        return Err(AppError::bad_gateway("抽取结果不是 JSON"));
    };
    if end <= start {
        return Err(AppError::bad_gateway("抽取结果不是 JSON"));
    }
    serde_json::from_str(&body[start..=end])
        .map_err(|err| AppError::bad_gateway(format!("解析抽取结果失败: {err}")))
}

pub(super) fn strip_code_fence(text: &str) -> &str {
    if !text.starts_with("```") {
        return text;
    }
    let rest = text.split_once('\n').map(|parts| parts.1).unwrap_or("");
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

fn normalize_entity_type(raw: &str) -> String {
    let value = raw.trim().to_lowercase();
    if ENTITY_TYPES.contains(&value.as_str()) {
        value
    } else {
        "concept".to_string()
    }
}

/// 未知关系类型落 related_to,原词留在 explanation 里(不静默丢)。
fn normalize_relation_type(raw: &str, explanation: &str) -> (String, String) {
    let value = raw.trim().to_lowercase().replace(' ', "_");
    if RELATION_TYPES.contains(&value.as_str()) {
        return (value, explanation.trim().to_string());
    }
    let note = if raw.trim().is_empty() {
        String::new()
    } else {
        format!("原词: {}", raw.trim())
    };
    let explanation = match (explanation.trim().is_empty(), note.is_empty()) {
        (true, true) => String::new(),
        (true, false) => note,
        (false, true) => explanation.trim().to_string(),
        (false, false) => format!("{}｜{}", explanation.trim(), note),
    };
    ("related_to".to_string(), explanation)
}

fn as_str_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
        Some(Value::String(text)) if !text.trim().is_empty() => vec![text.trim().to_string()],
        _ => Vec::new(),
    }
}

fn as_confidence(value: &Value) -> f64 {
    let parsed = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    parsed.unwrap_or(1.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use futures_util::future::BoxFuture;
    use serde_json::Value;

    use crate::db::Db;
    use crate::models::{now_iso, UploadRecord};
    use crate::services::ai::llm::{AssistantMessage, Chat};

    use super::*;

    struct ScriptedChat {
        reply: String,
        seen: Mutex<Vec<Value>>,
    }

    impl Chat for ScriptedChat {
        fn chat(
            &self,
            messages: Vec<Value>,
            tools: Vec<Value>,
        ) -> BoxFuture<'static, Result<AssistantMessage, AppError>> {
            self.seen.lock().expect("lock").push(serde_json::json!({
                "messages": messages,
                "tools": tools,
            }));
            let reply = self.reply.clone();
            Box::pin(async move {
                Ok(AssistantMessage {
                    content: reply,
                    tool_calls: Vec::new(),
                })
            })
        }
    }

    fn test_db(name: &str) -> (PathBuf, PathBuf, Db) {
        let root = std::env::temp_dir().join(format!(
            "graph-extract-{name}-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        let data_root: PathBuf = root.join("data");
        fs::create_dir_all(&data_root).expect("data root");
        fs::create_dir_all(root.join("db")).expect("db dir");
        let db = Db::new(root.join("db").join("jobs.db"), data_root.clone());
        db.init().expect("init");
        (root, data_root, db)
    }

    fn seed_document_with_blocks(db: &Db, data_root: &std::path::Path) {
        db.upsert_document_from_upload(&UploadRecord {
            upload_id: "up-1".to_string(),
            filename: "paper.pdf".to_string(),
            stored_path: "uploads/x/paper.pdf".to_string(),
            bytes: 10,
            page_count: 1,
            uploaded_at: now_iso(),
            developer_mode: false,
            content_hash: "doc-1".to_string(),
        })
        .expect("insert document");
        db.set_document_active_job("doc-1", "job-1", None)
            .expect("active job");
        let normalized = data_root.join("jobs/job-1/ocr/normalized");
        fs::create_dir_all(&normalized).expect("mkdir normalized");
        fs::write(
            normalized.join("document.v1.json"),
            serde_json::json!({
                "pages": [{"page_index": 0, "blocks": [
                    {"block_id": "p001-b0000", "text": "GNN uses message passing over aryl lithium"},
                    {"block_id": "p001-b0001", "text": "accuracy reaches 0.91 on QM9"},
                ]}]
            })
            .to_string(),
        )
        .expect("write document");
    }

    #[test]
    fn parse_tolerates_fence_and_garbage() {
        let payload = parse_extraction(
            "```json\n{\"entities\":[{\"name\":\"GNN\",\"type\":\"method\",\"aliases\":[\"图神经网络\"]}],\
             \"relations\":[{\"from\":\"GNN\",\"to\":\"QM9\",\"type\":\"evaluates\"}]}\n```",
        )
        .expect("parse");
        assert_eq!(payload.entities.len(), 1);
        assert_eq!(payload.entities[0].name, "GNN");
        assert_eq!(
            as_str_list(Some(&payload.entities[0].aliases)),
            vec!["图神经网络"]
        );
        assert_eq!(payload.relations[0].relation_type, "evaluates");

        assert!(parse_extraction("抱歉，我无法完成。").is_err());
    }

    #[test]
    fn normalize_falls_back_on_unknown_vocab() {
        assert_eq!(normalize_entity_type("Method"), "method");
        assert_eq!(normalize_entity_type("widget"), "concept");
        assert_eq!(normalize_relation_type("uses", "x"), ("uses".to_string(), "x".to_string()));
        let (kind, explanation) = normalize_relation_type("invented_by", "文中提到");
        assert_eq!(kind, "related_to");
        assert!(explanation.contains("原词: invented_by"));
        assert!(explanation.contains("文中提到"));
        // 别名字符串形式也能接住
        assert_eq!(as_str_list(Some(&json!("HLE"))), vec!["HLE".to_string()]);
        assert_eq!(as_confidence(&json!("0.5")), 0.5);
        assert_eq!(as_confidence(&json!(2)), 1.0);
        assert_eq!(as_confidence(&Value::Null), 1.0);
    }

    #[test]
    fn build_corpus_stops_at_char_budget() {
        let blocks = vec![
            Block {
                page_idx: 0,
                block_id: "p001-b0000".to_string(),
                source_text: "abcd".to_string(),
                translated_text: String::new(),
            },
            Block {
                page_idx: 0,
                block_id: "p001-b0001".to_string(),
                source_text: "efgh".to_string(),
                translated_text: String::new(),
            },
        ];
        assert_eq!(build_corpus(&blocks, 6), "abcd\n");
        assert_eq!(build_corpus(&blocks, 100), "abcd\nefgh\n");
    }

    /// 端到端:模型给名字 → 实体落库 + 字面扫描挂证据 + 关系落库。
    #[tokio::test]
    async fn extraction_links_evidence_and_relations() {
        let (root, data_root, db) = test_db("e2e");
        seed_document_with_blocks(&db, &data_root);
        let chat = ScriptedChat {
            reply: json!({
                "entities": [
                    {"name": "GNN", "type": "method", "aliases": ["图神经网络"], "description": "图神经网络"},
                    {"name": "aryl lithium", "type": "material", "aliases": ["芳基锂"]},
                    {"name": "QM9", "type": "dataset", "aliases": []}
                ],
                "relations": [
                    {"from": "GNN", "to": "aryl lithium", "type": "uses", "confidence": 0.7, "explanation": "原文同句"},
                    {"from": "GNN", "to": "QM9", "type": "invented", "explanation": "未知关系类型"},
                    {"from": "GNN", "to": "GNN", "type": "uses"}
                ]
            })
            .to_string(),
            seen: Mutex::new(Vec::new()),
        };

        let deps = GraphDeps {
            db: &db,
            data_root: &data_root,
        };
        let outcome = extract_document_graph(&deps, &chat, "doc-1")
            .await
            .expect("extract");
        assert_eq!(outcome.entities, 3);
        assert_eq!(outcome.mentions, 3, "三个实体各命中一个块");
        assert_eq!(outcome.relations, 2, "自环被丢弃");

        let gnn = db.search_entities("GNN", None, 5).expect("search");
        assert_eq!(gnn.len(), 1);
        let mentions = db
            .list_entity_mentions(&gnn[0].entity_id, Some("doc-1"), 10)
            .expect("mentions");
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].block_id, "p001-b0000");

        let related = db.related_entities(&gnn[0].entity_id, None, 10).expect("related");
        assert_eq!(related.len(), 2);
        let unknown = related
            .iter()
            .find(|item| item.name == "QM9")
            .expect("qm9 edge");
        assert_eq!(unknown.relation_type, "related_to");
        assert!(unknown.explanation.contains("原词: invented"));

        // 第二次抽取:先清本文件抽取产物再重写,结果不翻倍
        let again = extract_document_graph(&deps, &chat, "doc-1").await.expect("again");
        assert_eq!(again.mentions, 3);
        assert_eq!(again.relations, 2);
        assert_eq!(
            db.list_entity_mentions(&gnn[0].entity_id, Some("doc-1"), 10)
                .expect("after")
                .len(),
            1
        );
        assert_eq!(
            db.related_entities(&gnn[0].entity_id, None, 10)
                .expect("related after")
                .len(),
            2,
            "关系不累积"
        );
        fs::remove_dir_all(&root).ok();
    }
}
