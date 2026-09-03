//! /api/v1/translate 路由门面：阅读器「选中文字翻译」的无状态入口。
//! 路由只允许经本模块触达翻译能力，不直接 import services::jobs 内部。

use crate::error::AppError;
use crate::models::api::{TranslateTextRequest, TranslateTextView};

/// 无状态文本翻译：凭据由请求携带或回退服务端 env，实现在 services::jobs::reader_ai。
pub async fn translate_text(request: TranslateTextRequest) -> Result<TranslateTextView, AppError> {
    crate::services::jobs::translate_text(request).await
}
