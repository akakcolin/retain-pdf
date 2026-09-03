use axum::Json;

use crate::error::AppError;
use crate::models::api::{ApiResponse, TranslateTextRequest, TranslateTextView};
use crate::routes::common::ok_json;
use crate::services::translate_api::translate_text;

/// 阅读器「选中文字翻译」:无状态文本翻译端点,凭据由前端按请求携带或回退服务端 env。
pub async fn translate_text_route(
    Json(request): Json<TranslateTextRequest>,
) -> Result<Json<ApiResponse<TranslateTextView>>, AppError> {
    Ok(ok_json(translate_text(request).await?))
}
