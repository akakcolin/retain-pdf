use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::auth;
use crate::routes::ai;
use crate::routes::collections;
use crate::routes::glossaries;
use crate::routes::graph;
use crate::routes::health;
use crate::routes::jobs;
use crate::routes::library;
use crate::routes::library_data;
use crate::routes::library_extras;
use crate::routes::metrics;
use crate::routes::providers;
use crate::routes::translate;
use crate::routes::uploads;

pub fn build_app(state: AppState) -> Router {
    let api_routes = Router::new()
        .route(
            "/api/v1/ocr/jobs",
            post(jobs::create_ocr_job)
                .get(jobs::list_ocr_jobs)
                .layer(DefaultBodyLimit::disable()),
        )
        .route("/api/v1/ocr/jobs/:job_id", get(jobs::get_ocr_job))
        .route(
            "/api/v1/ocr/jobs/:job_id/events",
            get(jobs::get_ocr_job_events),
        )
        .route(
            "/api/v1/ocr/jobs/:job_id/artifacts",
            get(jobs::get_ocr_job_artifacts),
        )
        .route(
            "/api/v1/ocr/jobs/:job_id/artifacts-manifest",
            get(jobs::get_ocr_job_artifacts_manifest),
        )
        .route(
            "/api/v1/ocr/jobs/:job_id/artifacts/:artifact_key",
            get(jobs::download_ocr_artifact_by_key),
        )
        .route(
            "/api/v1/ocr/jobs/:job_id/normalized-document",
            get(jobs::download_ocr_normalized_document),
        )
        .route(
            "/api/v1/ocr/jobs/:job_id/normalization-report",
            get(jobs::download_ocr_normalization_report),
        )
        .route(
            "/api/v1/ocr/jobs/:job_id/cancel",
            post(jobs::cancel_ocr_job),
        )
        .route(
            "/api/v1/uploads",
            post(uploads::upload_pdf).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/v1/glossaries/parse-csv",
            post(glossaries::parse_glossary_csv_route),
        )
        .route(
            "/api/v1/glossaries/import",
            post(glossaries::import_glossary_route),
        )
        .route(
            "/api/v1/glossaries",
            post(glossaries::create_glossary_route).get(glossaries::list_glossaries_route),
        )
        .route(
            "/api/v1/glossaries/:glossary_id",
            get(glossaries::get_glossary_route)
                .put(glossaries::update_glossary_route)
                .delete(glossaries::delete_glossary_route),
        )
        .route(
            "/api/v1/glossaries/:glossary_id/export.csv",
            get(glossaries::export_glossary_csv_route),
        )
        .route("/api/v1/documents", get(library_data::list_documents_route))
        .route(
            "/api/v1/documents/:document_id",
            get(library_data::get_document_route)
                .patch(library_data::patch_document_route)
                .delete(library_data::delete_document_route),
        )
        .route(
            "/api/v1/documents/:document_id/source.pdf",
            get(library_data::download_document_source_pdf_route),
        )
        .route(
            "/api/v1/documents/:document_id/cover",
            get(library_data::download_document_cover_route),
        )
        .route(
            "/api/v1/documents/:document_id/thumbnail",
            get(library_data::download_document_thumbnail_route),
        )
        .route(
            "/api/v1/documents/:document_id/translate",
            post(library_data::translate_document_route),
        )
        .route(
            "/api/v1/favorites",
            post(library_data::create_favorite_route).get(library_data::list_favorites_route),
        )
        .route(
            "/api/v1/favorites/:favorite_id",
            axum::routing::patch(library_data::patch_favorite_route)
                .delete(library_data::delete_favorite_route),
        )
        .route("/api/v1/search", get(library_data::search_blocks_route))
        .route("/api/v1/entities", get(graph::list_entities_route))
        .route("/api/v1/entities/:entity_id", get(graph::get_entity_route))
        .route(
            "/api/v1/entities/:entity_id/mentions",
            get(graph::list_entity_mentions_route),
        )
        .route(
            "/api/v1/entities/:entity_id/relations",
            get(graph::list_entity_relations_route),
        )
        .route(
            "/api/v1/entities/:entity_id/backlinks",
            get(graph::list_entity_backlinks_route),
        )
        .route(
            "/api/v1/entities/:entity_id/favorites",
            get(graph::list_entity_favorites_route),
        )
        .route(
            "/api/v1/entities/:entity_id/page",
            get(graph::get_entity_page_route)
                .post(graph::generate_entity_page_route)
                .patch(graph::save_entity_page_route),
        )
        .route(
            "/api/v1/documents/:document_id/graph/link",
            post(graph::link_document_graph_route),
        )
        .route(
            "/api/v1/documents/:document_id/graph/extract",
            post(graph::extract_document_graph_route),
        )
        .route(
            "/api/v1/documents/:document_id/graph/pending-pages",
            get(graph::list_pending_pages_route),
        )
        .route("/api/v1/ai/ask", post(ai::ask_route))
        .route(
            "/api/v1/translate/text",
            post(translate::translate_text_route),
        )
        .route(
            "/api/v1/assets",
            post(library_extras::upload_asset_route).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/v1/assets/:asset_id",
            get(library_extras::download_asset_route),
        )
        .route(
            "/api/v1/ai/conversations",
            post(library_extras::create_conversation_route)
                .get(library_extras::list_conversations_route),
        )
        .route(
            "/api/v1/ai/conversations/:conversation_id",
            get(library_extras::get_conversation_route)
                .patch(library_extras::patch_conversation_route)
                .delete(library_extras::delete_conversation_route),
        )
        .route(
            "/api/v1/ai/conversations/:conversation_id/messages",
            post(library_extras::append_message_route),
        )
        .route(
            "/api/v1/collections",
            post(collections::create_collection_route).get(collections::list_collections_route),
        )
        .route(
            "/api/v1/collections/:collection_id",
            axum::routing::patch(collections::patch_collection_route)
                .delete(collections::delete_collection_route),
        )
        .route(
            "/api/v1/collections/:collection_id/documents",
            post(collections::add_collection_documents_route),
        )
        .route(
            "/api/v1/collections/:collection_id/documents/:document_id",
            axum::routing::delete(collections::remove_collection_document_route),
        )
        .route("/api/v1/library/books", get(library::list_books))
        .route("/api/v1/library/books/delete", post(library::delete_books))
        .route(
            "/api/v1/library/books/:job_id",
            get(library::get_book).delete(library::delete_book),
        )
        .route(
            "/api/v1/library/books/:job_id/cover",
            get(library::download_book_cover),
        )
        .route(
            "/api/v1/library/books/:job_id/thumbnail",
            get(library::download_book_thumbnail),
        )
        .route("/api/v1/jobs", post(jobs::create_job).get(jobs::list_jobs))
        .route("/api/v1/jobs/:job_id", get(jobs::get_job))
        .route("/api/v1/jobs/:job_id/events", get(jobs::get_job_events))
        .route(
            "/api/v1/jobs/:job_id/reader/regions",
            get(jobs::get_reader_regions),
        )
        .route(
            "/api/v1/jobs/:job_id/reader/metadata",
            get(jobs::get_reader_metadata),
        )
        .route(
            "/api/v1/jobs/:job_id/reader/ai/chat",
            post(jobs::reader_ai_chat),
        )
        .route(
            "/api/v1/jobs/:job_id/diagnostics",
            get(jobs::get_job_diagnostics),
        )
        .route(
            "/api/v1/jobs/:job_id/resume-plan",
            get(jobs::get_resume_plan),
        )
        .route(
            "/api/v1/jobs/:job_id/stage-actions",
            get(jobs::get_stage_actions),
        )
        .route("/api/v1/jobs/:job_id/resume", post(jobs::resume_job))
        .route("/api/v1/jobs/:job_id/retry-stage", post(jobs::retry_stage))
        .route(
            "/api/v1/jobs/:job_id/translation/diagnostics",
            get(jobs::get_translation_diagnostics),
        )
        .route(
            "/api/v1/jobs/:job_id/translation/items",
            get(jobs::list_translation_items),
        )
        .route(
            "/api/v1/jobs/:job_id/translation/items/:item_id",
            get(jobs::get_translation_item),
        )
        .route(
            "/api/v1/jobs/:job_id/translation/items/:item_id/replay",
            post(jobs::replay_translation_item_route),
        )
        .route(
            "/api/v1/jobs/:job_id/artifacts",
            get(jobs::get_job_artifacts),
        )
        .route(
            "/api/v1/jobs/:job_id/artifacts-manifest",
            get(jobs::get_job_artifacts_manifest),
        )
        .route(
            "/api/v1/jobs/:job_id/artifacts/:artifact_key",
            get(jobs::download_artifact_by_key),
        )
        .route("/api/v1/jobs/:job_id/pdf", get(jobs::download_pdf))
        .route(
            "/api/v1/jobs/:job_id/pdf/side-by-side",
            get(jobs::download_side_by_side_pdf),
        )
        .route("/api/v1/jobs/:job_id/cover", get(jobs::download_cover))
        .route(
            "/api/v1/jobs/:job_id/thumbnail",
            get(jobs::download_thumbnail),
        )
        .route(
            "/api/v1/jobs/:job_id/preview/pages/:page",
            get(jobs::download_page_preview),
        )
        .route(
            "/api/v1/jobs/:job_id/normalized-document",
            get(jobs::download_normalized_document),
        )
        .route(
            "/api/v1/jobs/:job_id/normalization-report",
            get(jobs::download_normalization_report),
        )
        .route(
            "/api/v1/jobs/:job_id/markdown",
            get(jobs::download_markdown),
        )
        .route(
            "/api/v1/jobs/:job_id/markdown/document",
            get(jobs::get_markdown_document),
        )
        .route(
            "/api/v1/jobs/:job_id/markdown/images/*path",
            get(jobs::download_markdown_image),
        )
        .route("/api/v1/jobs/:job_id/download", get(jobs::download_bundle))
        .route("/api/v1/jobs/:job_id/cancel", post(jobs::cancel_job))
        .route("/api/v1/jobs/:job_id/rerun", post(jobs::rerun_job))
        .route("/api/v1/providers/ocr", get(providers::list_ocr_providers))
        .route(
            "/api/v1/providers/mineru/validate-token",
            post(providers::validate_mineru_token),
        )
        .route(
            "/api/v1/providers/paddle/validate-token",
            post(providers::validate_paddle_token),
        )
        .route(
            "/api/v1/providers/deepseek/validate-token",
            post(providers::validate_deepseek_token),
        )
        .route(
            "/api/v1/providers/deepseek/balance",
            post(providers::query_deepseek_balance),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_api_key,
        ));

    Router::new()
        .route("/health", get(health::health))
        .route("/metrics", get(metrics::metrics))
        .merge(api_routes)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub fn build_simple_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/metrics", get(metrics::metrics))
        .route(
            "/api/v1/translate/bundle",
            post(jobs::translate_bundle).layer(DefaultBodyLimit::disable()),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_api_key,
        ))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
