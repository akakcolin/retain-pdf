//! 概念图谱 HTTP 视图:实体检索 / 提及 / 关系 / 文档建链 / LLM 抽取。

use crate::db::Db;
use crate::error::AppError;
use crate::models::api::{
    EntityBacklinkListView, EntityListView, EntityMentionListView, EntityRecord,
    EntityRelationListView, ExtractDocumentGraphView, LinkDocumentGraphView, ListBacklinksQuery,
    ListEntitiesQuery, ListMentionsQuery, ListRelationsQuery,
};
use crate::services::ai::llm::Chat;
use crate::services::graph::extract::extract_document_graph;
use crate::services::graph::mentions::link_document_mentions;
use crate::services::graph::page::list_entity_backlinks;
use crate::services::graph::seed::seed_entities_from_glossaries;
use crate::services::graph::GraphDeps;

const MAX_LIMIT: u32 = 200;

/// document_id 非空时返回该文档的实体概览;否则按 query 词面检索
/// (query 为空 = 全库实体按提及数排序)。
pub fn list_entities_view(db: &Db, query: &ListEntitiesQuery) -> Result<EntityListView, AppError> {
    let limit = query.limit.clamp(1, MAX_LIMIT);
    let document_id = query
        .document_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let items = match document_id {
        Some(document_id) => db.entities_for_document(document_id, limit)?,
        None => db.search_entities(
            query.query.trim(),
            query.entity_type.as_deref(),
            limit,
        )?,
    };
    Ok(EntityListView { items })
}

pub fn get_entity_view(db: &Db, entity_id: &str) -> Result<EntityRecord, AppError> {
    db.get_entity(entity_id)
        .map_err(|_| AppError::not_found(format!("entity not found: {entity_id}")))
}

pub fn list_mentions_view(
    db: &Db,
    entity_id: &str,
    query: &ListMentionsQuery,
) -> Result<EntityMentionListView, AppError> {
    db.get_entity(entity_id)
        .map_err(|_| AppError::not_found(format!("entity not found: {entity_id}")))?;
    let items = db.list_entity_mentions(
        entity_id,
        query.document_id.as_deref(),
        query.limit.clamp(1, MAX_LIMIT),
    )?;
    Ok(EntityMentionListView { items })
}

/// 实体关系(出边 + 入边),按关系类型过滤可选。
pub fn list_relations_view(
    db: &Db,
    entity_id: &str,
    query: &ListRelationsQuery,
) -> Result<EntityRelationListView, AppError> {
    db.get_entity(entity_id)
        .map_err(|_| AppError::not_found(format!("entity not found: {entity_id}")))?;
    let items = db.related_entities(
        entity_id,
        query.relation_type.as_deref(),
        query.limit.clamp(1, MAX_LIMIT),
    )?;
    Ok(EntityRelationListView { items })
}

/// 反链:哪些已生成的概念页提到了本实体(读取时现算)。
pub fn list_backlinks_view(
    db: &Db,
    entity_id: &str,
    query: &ListBacklinksQuery,
) -> Result<EntityBacklinkListView, AppError> {
    let items = list_entity_backlinks(db, entity_id, query.limit.clamp(1, MAX_LIMIT))?;
    Ok(EntityBacklinkListView { items })
}

/// 手动触发:术语表灌实体 + 该文档全块字面扫描挂证据。零 LLM 成本,可重复调用。
/// 只重建 glossary 来源的证据,抽取来源的证据与关系不动(实体本身也不删,
/// 所以删词条只影响新扫描,不会回收已建的实体行)。
pub fn link_document_graph_view(
    deps: &GraphDeps<'_>,
    document_id: &str,
) -> Result<LinkDocumentGraphView, AppError> {
    deps.db
        .get_document(document_id)
        .map_err(|_| AppError::not_found(format!("document not found: {document_id}")))?;
    seed_entities_from_glossaries(deps)?;
    deps.db.clear_document_graph(document_id)?;
    let mentions = link_document_mentions(deps, document_id, "glossary")?;
    let entities = deps.db.entities_for_document(document_id, u32::MAX)?.len();
    Ok(LinkDocumentGraphView {
        document_id: document_id.to_string(),
        entities,
        mentions,
    })
}

/// LLM 抽取实体 + 关系(按请求携带的凭据构建 client)。
pub async fn extract_document_graph_view<C: Chat>(
    deps: &GraphDeps<'_>,
    client: &C,
    document_id: &str,
) -> Result<ExtractDocumentGraphView, AppError> {
    deps.db
        .get_document(document_id)
        .map_err(|_| AppError::not_found(format!("document not found: {document_id}")))?;
    let outcome = extract_document_graph(deps, client, document_id).await?;
    Ok(ExtractDocumentGraphView {
        document_id: document_id.to_string(),
        entities: outcome.entities,
        mentions: outcome.mentions,
        relations: outcome.relations,
    })
}
