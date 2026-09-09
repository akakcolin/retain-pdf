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

/// 新建有向关系入参(id / 时间戳由 Db 生成)。
#[derive(Debug, Clone)]
pub struct NewEntityRelation {
    pub from_entity_id: String,
    pub to_entity_id: String,
    pub relation_type: String,
    pub confidence: f64,
    pub explanation: String,
    pub source_document_id: String,
    pub source_block_id: String,
}

/// 邻居实体 + 连边信息(direction 以被查询实体为参照)。
#[derive(Debug, Clone, Serialize)]
pub struct RelatedEntity {
    pub entity_id: String,
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
    pub mention_count: i64,
    pub document_count: i64,
    pub relation_type: String,
    /// out = 被查询实体 → 邻居;in = 邻居 → 被查询实体
    pub direction: String,
    pub confidence: f64,
    pub explanation: String,
    pub source_document_id: String,
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

/// GET /api/v1/entities/:id/relations 响应。
#[derive(Debug, Serialize)]
pub struct EntityRelationListView {
    pub items: Vec<RelatedEntity>,
}

/// GET /api/v1/entities/:id/relations 查询参数。
#[derive(Debug, Deserialize)]
pub struct ListRelationsQuery {
    #[serde(default)]
    pub relation_type: Option<String>,
    #[serde(default = "default_graph_limit")]
    pub limit: u32,
}

/// POST /api/v1/documents/:id/graph/extract 请求体:按请求携带 LLM 凭据,
/// 留空回落启动期配置(与 /ai/ask 一致,前端凭据存浏览器侧)。
#[derive(Debug, Default, Deserialize)]
pub struct ExtractGraphRequest {
    #[serde(default)]
    pub llm_api_key: String,
    #[serde(default)]
    pub llm_base_url: String,
    #[serde(default)]
    pub llm_model: String,
}

/// POST /api/v1/documents/:id/graph/link 的结果。
#[derive(Debug, Serialize)]
pub struct LinkDocumentGraphView {
    pub document_id: String,
    pub entities: usize,
    pub mentions: usize,
}

/// POST /api/v1/documents/:id/graph/extract 的结果。
#[derive(Debug, Serialize)]
pub struct ExtractDocumentGraphView {
    pub document_id: String,
    /// 本次抽取落到的实体数
    pub entities: usize,
    /// 新挂的证据条数
    pub mentions: usize,
    /// 新写入的关系条数
    pub relations: usize,
}

/// 概念页引用:正文 [n] 对应的证据锚点。ref 与正文编号一一对应(1 基)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityPageCitation {
    #[serde(rename = "ref")]
    pub ref_num: i64,
    pub document_id: String,
    pub document_title: String,
    pub job_id: String,
    pub page_idx: i64,
    pub block_id: String,
    pub snippet: String,
}

/// 概念页的证据片段(block_entities + 文档标题)。
#[derive(Debug, Clone)]
pub struct EntityPageEvidence {
    pub document_id: String,
    pub document_title: String,
    pub job_id: String,
    pub page_idx: i64,
    pub block_id: String,
    pub snippet: String,
}

/// 概念页记录(一实体一页,整体覆盖写)。
#[derive(Debug, Clone)]
pub struct EntityPageRecord {
    pub entity_id: String,
    pub body_md: String,
    pub citations: Vec<EntityPageCitation>,
    /// 生成时的证据签名,用于读取时判断 stale。
    pub evidence_sig: String,
    pub generated_at: String,
}

/// 概念页正文里的 [[实体名]] 解析结果。surface = 正文原样文本,前端按它建索引。
#[derive(Debug, Clone, Serialize)]
pub struct EntityPageLink {
    pub surface: String,
    pub entity_id: String,
    pub name: String,
    pub entity_type: String,
    pub aliases: Vec<String>,
}

/// GET/POST /api/v1/entities/:id/page 响应。无页时 has_page=false,其余为空。
#[derive(Debug, Serialize)]
pub struct EntityPageView {
    pub entity_id: String,
    pub name: String,
    pub entity_type: String,
    pub has_page: bool,
    /// 证据签名与生成时不一致 = 页内容可能过时。
    pub stale: bool,
    pub generated_at: String,
    pub body_md: String,
    pub citations: Vec<EntityPageCitation>,
    /// 正文 [[实体名]] 里能解析到实体的那些(读取时现算)。
    pub links: Vec<EntityPageLink>,
}

/// POST /api/v1/entities/:id/page 请求体:与抽取同形,按请求携带 LLM 凭据。
pub type GenerateEntityPageRequest = ExtractGraphRequest;
