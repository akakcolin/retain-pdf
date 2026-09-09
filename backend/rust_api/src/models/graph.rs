//! 概念图谱实体层:实体 / block 证据挂载 / 有向关系。
//! 证据单位是 block(自带 page_idx + block_id),可直接跳阅读器原位。

use serde::{Deserialize, Serialize};

fn default_graph_limit() -> u32 {
    50
}

/// 可复用实体。name_norm 是归一化名(小写 + 折叠空白),用于词面消歧。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRecord {
    pub entity_id: String,
    pub name: String,
    pub name_norm: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

/// 新建实体入参(id / 时间戳由 Db 生成)。
#[derive(Debug, Clone)]
pub struct NewEntity {
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub description: String,
}

/// 实体→block 证据挂载。snippet 在建链时写入,查询免回查 blocks_fts。
#[derive(Debug, Clone)]
pub struct BlockEntityLink {
    pub document_id: String,
    pub entity_id: String,
    pub page_idx: i64,
    pub block_id: String,
    pub job_id: String,
    pub surface_form: String,
    pub snippet: String,
    pub confidence: f64,
    /// glossary | extraction | manual
    pub source: String,
}

/// 实体摘要:带提及次数与覆盖文档数。
#[derive(Debug, Clone, Serialize)]
pub struct EntitySummary {
    pub entity_id: String,
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub mention_count: i64,
    pub document_count: i64,
}

/// 实体提及:完整锚点,可跳转阅读器。
#[derive(Debug, Clone, Serialize)]
pub struct EntityMention {
    pub document_id: String,
    pub job_id: String,
    pub page_idx: i64,
    pub block_id: String,
    pub snippet: String,
}

/// GET /api/v1/entities 响应。
#[derive(Debug, Serialize)]
pub struct EntityListView {
    pub items: Vec<EntitySummary>,
}

/// GET /api/v1/entities/:id/mentions 响应。
#[derive(Debug, Serialize)]
pub struct EntityMentionListView {
    pub items: Vec<EntityMention>,
}

/// GET /api/v1/entities 查询参数。
#[derive(Debug, Deserialize)]
pub struct ListEntitiesQuery {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub entity_type: Option<String>,
    /// 非空时改查该文档的实体概览。
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default = "default_graph_limit")]
    pub limit: u32,
}

/// GET /api/v1/entities/:id/mentions 查询参数。
#[derive(Debug, Deserialize)]
pub struct ListMentionsQuery {
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default = "default_graph_limit")]
    pub limit: u32,
}

/// POST /api/v1/documents/:id/graph/link 的结果。
#[derive(Debug, Serialize)]
pub struct LinkDocumentGraphView {
    pub document_id: String,
    pub entities: usize,
    pub mentions: usize,
}
