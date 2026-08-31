mod artifact_chunks;
mod chunking;
mod config;
mod llm;
mod retrieval;

use std::fs;
use std::path::Path;

use crate::error::AppError;
use crate::models::api::{
    ReaderAiChatRequest, ReaderAiChatView, ReaderAiUsedContextView, TranslateTextRequest,
    TranslateTextView,
};
use crate::models::domain::{JobSnapshot, JobStatusKind};
use crate::storage_paths::resolve_markdown_path;
use tracing::info;

use artifact_chunks::chunks_from_translation_artifacts;
use chunking::chunk_markdown;
use config::ReaderAiConfig;
use llm::{complete_reader_answer, complete_text_translation};
use retrieval::retrieve_chunks;

/// 阅读器「选中文字翻译」:无状态,凭据经 ReaderAiChatRequest 合成复用 ReaderAiConfig 解析。
pub(crate) async fn translate_text(
    request: TranslateTextRequest,
) -> Result<TranslateTextView, AppError> {
    let text = normalize_translate_text(&request.text)?;
    let target_language = request.target_language.trim().to_string();
    let synthesized = ReaderAiChatRequest {
        message: text.clone(),
        scope: "translate".to_string(),
        provider: request.provider,
        model: request.model,
        api_key: request.api_key,
        base_url: request.base_url,
        context: None,
        history: vec![],
    };
    let config = ReaderAiConfig::from_request(Some(&synthesized))?;
    let translated_text = complete_text_translation(&config, &text, &target_language).await?;
    Ok(TranslateTextView {
        translated_text,
        target_language,
    })
}

/// 同步校验+规整:trim、非空、长度上限。失败返回 BadRequest。
fn normalize_translate_text(raw: &str) -> Result<String, AppError> {
    let text = raw.trim().to_string();
    if text.is_empty() {
        return Err(AppError::bad_request("text is required"));
    }
    if text.chars().count() > 2000 {
        return Err(AppError::bad_request("text is too long (max 2000 chars)"));
    }
    Ok(text)
}

pub(crate) async fn answer_reader_chat(
    data_root: &Path,
    job: &JobSnapshot,
    request: ReaderAiChatRequest,
) -> Result<ReaderAiChatView, AppError> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err(AppError::bad_request("message is required"));
    }
    let scope = normalized_scope(&request.scope)?;
    ensure_markdown_ready(job)?;

    let (chunks, chunk_source) = load_reader_chunks(data_root, job)?;
    if chunks.is_empty() {
        return Err(AppError::not_found(format!(
            "reader text has no readable chunks: {}",
            job.job_id
        )));
    }
    let retrieved = retrieve_chunks(&chunks, message, request.context.as_ref(), 8);
    log_retrieved_chunks(&job.job_id, &retrieved);
    let citations = retrieved
        .iter()
        .map(|item| item.chunk.citation())
        .collect::<Vec<_>>();

    let config = ReaderAiConfig::from_request(Some(&request))?;
    let answer = complete_reader_answer(&config, &request, &retrieved).await?;
    Ok(ReaderAiChatView {
        answer,
        citations,
        used_context: ReaderAiUsedContextView {
            source: chunk_source,
            scope,
        },
    })
}

fn load_reader_chunks(
    data_root: &Path,
    job: &JobSnapshot,
) -> Result<(Vec<chunking::MarkdownChunk>, String), AppError> {
    let artifact_chunks = chunks_from_translation_artifacts(data_root, job)?;
    if !artifact_chunks.is_empty() {
        return Ok((artifact_chunks, "translation_manifest".to_string()));
    }
    let markdown_path = resolve_markdown_path(job, data_root)
        .ok_or_else(|| AppError::not_found(format!("markdown not found: {}", job.job_id)))?;
    let markdown = fs::read_to_string(&markdown_path).map_err(|err| {
        AppError::internal(format!(
            "failed to read markdown {}: {err}",
            markdown_path.display()
        ))
    })?;
    if markdown.trim().is_empty() {
        return Err(AppError::not_found(format!(
            "markdown is empty: {}",
            job.job_id
        )));
    }
    Ok((chunk_markdown(&markdown), "markdown".to_string()))
}

fn log_retrieved_chunks(job_id: &str, retrieved: &[retrieval::RetrievedChunk]) {
    let chunks = retrieved
        .iter()
        .enumerate()
        .map(|(index, item)| {
            format!(
                "#{} title={:?} page={:?} score={:.2}",
                index + 1,
                item.chunk.title,
                item.chunk.page,
                item.score
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    info!(
        job_id = %job_id,
        chunks = %chunks,
        "reader ai retrieved chunks"
    );
}

fn normalized_scope(scope: &str) -> Result<String, AppError> {
    let value = scope.trim();
    if value.is_empty() || value == "document" {
        return Ok("document".to_string());
    }
    Err(AppError::bad_request(format!(
        "unsupported reader ai scope: {value}"
    )))
}

fn ensure_markdown_ready(job: &JobSnapshot) -> Result<(), AppError> {
    if matches!(job.status, JobStatusKind::Succeeded) {
        return Ok(());
    }
    Err(AppError::conflict(format!(
        "job is not complete; markdown is not ready: {}",
        job.job_id
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CreateJobInput, JobSnapshot};

    fn request_with(text: &str, target_language: Option<&str>) -> TranslateTextRequest {
        let mut value = serde_json::json!({ "text": text });
        if let Some(lang) = target_language {
            value["target_language"] = serde_json::json!(lang);
        }
        serde_json::from_value(value).expect("translate request")
    }

    #[test]
    fn translate_text_rejects_empty_text() {
        let err = normalize_translate_text("  ").expect_err("empty rejected");
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn translate_text_rejects_overlong_text() {
        let long = "a".repeat(2001);
        let err = normalize_translate_text(&long).expect_err("too long rejected");
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn translate_text_trims_input() {
        assert_eq!(
            normalize_translate_text("  hello  ").expect("trimmed"),
            "hello".to_string()
        );
    }

    #[test]
    fn translate_text_defaults_target_language_to_simplified_chinese() {
        let request = request_with("hello", None);
        assert_eq!(request.target_language, "简体中文");
    }

    #[test]
    fn translate_text_accepts_explicit_target_language() {
        let request = request_with("你好", Some("English"));
        assert_eq!(request.target_language, "English");
    }

    #[test]
    fn translate_text_maps_credentials_into_reader_ai_config() {
        let request = request_with("hello", None);
        let request = TranslateTextRequest {
            provider: Some("deepseek".to_string()),
            model: Some("deepseek-chat".to_string()),
            api_key: Some("sk-translate".to_string()),
            base_url: Some("https://translate.example/v1".to_string()),
            ..request
        };
        let synthesized = ReaderAiChatRequest {
            message: request.text.clone(),
            scope: "translate".to_string(),
            provider: request.provider,
            model: request.model,
            api_key: request.api_key,
            base_url: request.base_url,
            context: None,
            history: vec![],
        };
        let config = ReaderAiConfig::from_request(Some(&synthesized)).expect("config");
        assert_eq!(config.model, "deepseek-chat");
        assert_eq!(config.api_key, "sk-translate");
        assert_eq!(config.base_url, "https://translate.example/v1");
    }

    #[test]
    fn rejects_running_job_before_markdown_chat() {
        let job = JobSnapshot::new(
            "job-running".to_string(),
            CreateJobInput::default(),
            vec!["python".to_string()],
        );

        let err = ensure_markdown_ready(&job).expect_err("running rejected");
        assert!(matches!(err, AppError::Conflict(_)));
    }

    #[test]
    fn accepts_only_document_scope() {
        assert_eq!(
            normalized_scope("document").expect("scope"),
            "document".to_string()
        );
        assert!(matches!(
            normalized_scope("selection").expect_err("unsupported"),
            AppError::BadRequest(_)
        ));
    }
}
