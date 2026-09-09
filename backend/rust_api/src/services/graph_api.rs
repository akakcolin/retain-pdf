//! 概念图谱 HTTP 视图:实体检索 / 提及 / 文档建链。

use crate::db::Db;
use crate::error::AppError;
use crate::models::api::{
    EntityListView, EntityMentionListView, EntityRecord, LinkDocumentGraphView, ListEntitiesQuery,
    ListMentionsQuery,
};
use crate::services::graph::mentions::link_document_mentions;
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

/// 手动触发:术语表灌实体 + 该文档全块字面扫描挂证据。零 LLM 成本,可重复调用。
/// 先清空本文档旧证据再按当前全部实体重挂(实体本身不删,所以删词条只影响
/// 新扫描,不会回收已建的实体行)。
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
