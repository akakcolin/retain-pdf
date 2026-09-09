use axum::extract::{Path as AxumPath, Query, State};
use axum::Json;

use crate::error::AppError;
use crate::models::api::{
    ApiResponse, EntityListView, EntityMentionListView, EntityRecord, LinkDocumentGraphView,
    ListEntitiesQuery, ListMentionsQuery,
};
use crate::routes::common::{build_graph_route_deps, ok_json};
use crate::services::graph_api::{
    get_entity_view, link_document_graph_view, list_entities_view, list_mentions_view,
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

pub async fn link_document_graph_route(
    State(state): State<AppState>,
    AxumPath(document_id): AxumPath<String>,
) -> Result<Json<ApiResponse<LinkDocumentGraphView>>, AppError> {
    let deps = build_graph_route_deps(&state);
    Ok(ok_json(link_document_graph_view(&deps.graph, &document_id)?))
}
