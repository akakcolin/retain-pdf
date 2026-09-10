use axum::extract::{Path as AxumPath, Query, State};
use axum::Json;

use crate::error::AppError;
use crate::models::api::{
    ApiResponse, EntityBacklinkListView, EntityFavoriteListView, EntityListView,
    EntityMentionListView, EntityNeighborhoodQuery, EntityNeighborhoodView, EntityPageView,
    EntityRecord, EntityRelationListView, ExtractDocumentGraphView, ExtractGraphRequest,
    GenerateEntityPageRequest, LinkDocumentGraphView, ListBacklinksQuery, ListEntitiesQuery,
    ListEntityFavoritesQuery, ListMentionsQuery, ListPendingPagesQuery, ListRelationsQuery,
    PendingEntityPageListView, RelinkDocumentGraphView,
    SaveEntityPageRequest,
};
use crate::routes::common::{build_graph_route_deps, ok_json};
use crate::services::ai_api::{resolve_llm_credentials, LlmClient};
use crate::services::graph::page::{
    generate_entity_page, get_entity_page, revert_entity_page_edit, save_entity_page_edit,
};
use crate::services::graph_api::{
    extract_document_graph_view, get_entity_view, link_document_graph_view, list_backlinks_view,
    list_entities_view, list_entity_favorites_view, list_mentions_view, list_pending_pages_view,
    list_relations_view, neighborhood_view, relink_document_graph_view,
};
use crate::AppState;

pub async fn list_entities_route(
    State(state): State<AppState>,
    Query(query): Query<ListEntitiesQuery>,
) -> Result<Json<ApiResponse<EntityListView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(list_entities_view(deps.graph.db, &query)?))
}

pub async fn get_entity_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
) -> Result<Json<ApiResponse<EntityRecord>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(get_entity_view(deps.graph.db, &entity_id)?))
}

pub async fn list_entity_mentions_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
    Query(query): Query<ListMentionsQuery>,
) -> Result<Json<ApiResponse<EntityMentionListView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(list_mentions_view(
        deps.graph.db,
        &entity_id,
        &query,
    )?))
}

pub async fn list_entity_relations_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
    Query(query): Query<ListRelationsQuery>,
) -> Result<Json<ApiResponse<EntityRelationListView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(list_relations_view(
        deps.graph.db,
        &entity_id,
        &query,
    )?))
}

/// 实体 N 跳子图(节点 + 有向边),供概念面板图谱视图。
pub async fn entity_neighborhood_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
    Query(query): Query<EntityNeighborhoodQuery>,
) -> Result<Json<ApiResponse<EntityNeighborhoodView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(neighborhood_view(
        deps.graph.db,
        &entity_id,
        &query,
    )?))
}

/// 反链:哪些概念页提到了本实体(读取时现算)。
pub async fn list_entity_backlinks_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
    Query(query): Query<ListBacklinksQuery>,
) -> Result<Json<ApiResponse<EntityBacklinkListView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(list_backlinks_view(
        deps.graph.db,
        &entity_id,
        &query,
    )?))
}

/// 标注反查:收藏的引文/译文/备注里提到本实体的那些(读取时现算)。
pub async fn list_entity_favorites_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
    Query(query): Query<ListEntityFavoritesQuery>,
) -> Result<Json<ApiResponse<EntityFavoriteListView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(list_entity_favorites_view(
        deps.graph.db,
        &entity_id,
        &query,
    )?))
}

/// 读概念页。未生成时 has_page=false(实体存在,不是 404)。
pub async fn get_entity_page_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
) -> Result<Json<ApiResponse<EntityPageView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(get_entity_page(&deps.graph, &entity_id)?))
}

/// 生成/刷新概念页(按请求携带的凭据构建 client)。
pub async fn generate_entity_page_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
    Json(request): Json<GenerateEntityPageRequest>,
) -> Result<Json<ApiResponse<EntityPageView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    let (api_key, base_url, model) = resolve_llm_credentials(
        deps.ai,
        &request.llm_api_key,
        &request.llm_base_url,
        &request.llm_model,
    )?;
    let client = LlmClient::new(base_url, model, api_key, deps.ai.llm_timeout_s);
    Ok(ok_json(
        generate_entity_page(&deps.graph, &client, &entity_id, request.overwrite_manual).await?,
    ))
}

/// 保存/撤销概念页人工修订。`revert=true` 撤销,否则保存 body_md。
pub async fn save_entity_page_route(
    State(state): State<AppState>,
    AxumPath(entity_id): AxumPath<String>,
    Json(request): Json<SaveEntityPageRequest>,
) -> Result<Json<ApiResponse<EntityPageView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    let view = if request.revert {
        revert_entity_page_edit(&deps.graph, &entity_id)?
    } else {
        save_entity_page_edit(&deps.graph, &entity_id, &request.body_md)?
    };
    Ok(ok_json(view))
}

/// 待维护的概念页清单:该文档缺页或页已陈旧的实体(读取时现算)。
pub async fn list_pending_pages_route(
    State(state): State<AppState>,
    AxumPath(document_id): AxumPath<String>,
    Query(query): Query<ListPendingPagesQuery>,
) -> Result<Json<ApiResponse<PendingEntityPageListView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(list_pending_pages_view(
        deps.graph.db,
        &document_id,
        &query,
    )?))
}

pub async fn link_document_graph_route(
    State(state): State<AppState>,
    AxumPath(document_id): AxumPath<String>,
) -> Result<Json<ApiResponse<LinkDocumentGraphView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(link_document_graph_view(&deps.graph, &document_id)?))
}

/// 零 token 重新关联:用边界感知匹配器重扫本文档,差量修正误挂/漏挂。
pub async fn relink_document_graph_route(
    State(state): State<AppState>,
    AxumPath(document_id): AxumPath<String>,
) -> Result<Json<ApiResponse<RelinkDocumentGraphView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(relink_document_graph_view(
        &deps.graph,
        &document_id,
    )?))
}

pub async fn extract_document_graph_route(
    State(state): State<AppState>,
    AxumPath(document_id): AxumPath<String>,
    Json(request): Json<ExtractGraphRequest>,
) -> Result<Json<ApiResponse<ExtractDocumentGraphView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    let (api_key, base_url, model) = resolve_llm_credentials(
        deps.ai,
        &request.llm_api_key,
        &request.llm_base_url,
        &request.llm_model,
    )?;
    let client = LlmClient::new(base_url, model, api_key, deps.ai.llm_timeout_s);
    Ok(ok_json(
        extract_document_graph_view(&deps.graph, &client, &document_id).await?,
    ))
}
