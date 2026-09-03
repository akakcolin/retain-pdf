//! 资产存储(收藏截图等二进制附件)与 AI 问答会话历史。
//!
//! 资产是内容寻址的:asset_id = sha256(文件字节),文件落
//! data_root/assets/<前2位>/<hash>,重复上传自动归并,URL 永久可缓存。
//! 会话遵循"软锚点"原则:引用只存 JSON 快照,不做 job 删除保护。
//!
//! All handlers go through library_api (PR5).

use axum::extract::{Multipart, Path as AxumPath, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::error::AppError;
use crate::models::api::{
    ApiResponse, AppendMessageInput, AssetRecord, ConversationDetailView, ConversationListView,
    ConversationMutationResult, ConversationRecord, CreateConversationInput,
    ListConversationsQuery, MessageRecord, PatchConversationInput,
};
use crate::routes::common::{build_library_route_deps, ok_json};
use crate::services::library_api::{
    append_message_view, create_conversation_view, delete_conversation_view, get_conversation_view,
    list_conversations_view, load_asset_view, patch_conversation_view, store_asset_view,
};
use crate::AppState;

pub async fn upload_asset_route(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<ApiResponse<AssetRecord>>, AppError> {
    let mut bytes: Option<(String, Vec<u8>)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| AppError::bad_request(format!("invalid multipart: {error}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let mime = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let data = field
            .bytes()
            .await
            .map_err(|error| AppError::bad_request(format!("read upload failed: {error}")))?;
        bytes = Some((mime, data.to_vec()));
        break;
    }
    let Some((mime, data)) = bytes else {
        return Err(AppError::bad_request("multipart field 'file' is required"));
    };
    let deps = build_library_route_deps(&state);
    Ok(ok_json(store_asset_view(&deps.library, &mime, &data)?))
}

pub async fn download_asset_route(
    State(state): State<AppState>,
    AxumPath(asset_id): AxumPath<String>,
) -> Result<Response, AppError> {
    let deps = build_library_route_deps(&state);
    let asset = load_asset_view(&deps.library, &asset_id)?;
    Ok((
        [
            (header::CONTENT_TYPE, asset.mime),
            // 内容寻址 → 永久缓存安全
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        asset.data,
    )
        .into_response())
}

pub async fn create_conversation_route(
    State(state): State<AppState>,
    Json(payload): Json<CreateConversationInput>,
) -> Result<Json<ApiResponse<ConversationRecord>>, AppError> {
    let deps = build_library_route_deps(&state);
    Ok(ok_json(create_conversation_view(&deps.library, &payload)?))
}

pub async fn list_conversations_route(
    State(state): State<AppState>,
    Query(query): Query<ListConversationsQuery>,
) -> Result<Json<ApiResponse<ConversationListView>>, AppError> {
    let deps = build_library_route_deps(&state);
    Ok(ok_json(list_conversations_view(&deps.library, &query)?))
}

pub async fn get_conversation_route(
    State(state): State<AppState>,
    AxumPath(conversation_id): AxumPath<String>,
) -> Result<Json<ApiResponse<ConversationDetailView>>, AppError> {
    let deps = build_library_route_deps(&state);
    Ok(ok_json(get_conversation_view(
        &deps.library,
        &conversation_id,
    )?))
}

pub async fn delete_conversation_route(
    State(state): State<AppState>,
    AxumPath(conversation_id): AxumPath<String>,
) -> Result<Json<ApiResponse<ConversationMutationResult>>, AppError> {
    let deps = build_library_route_deps(&state);
    Ok(ok_json(delete_conversation_view(
        &deps.library,
        &conversation_id,
    )?))
}

pub async fn patch_conversation_route(
    State(state): State<AppState>,
    AxumPath(conversation_id): AxumPath<String>,
    Json(payload): Json<PatchConversationInput>,
) -> Result<Json<ApiResponse<ConversationRecord>>, AppError> {
    let deps = build_library_route_deps(&state);
    Ok(ok_json(patch_conversation_view(
        &deps.library,
        &conversation_id,
        &payload,
    )?))
}

pub async fn append_message_route(
    State(state): State<AppState>,
    AxumPath(conversation_id): AxumPath<String>,
    Json(payload): Json<AppendMessageInput>,
) -> Result<Json<ApiResponse<MessageRecord>>, AppError> {
    let deps = build_library_route_deps(&state);
    Ok(ok_json(append_message_view(
        &deps.library,
        &conversation_id,
        payload,
    )?))
}
