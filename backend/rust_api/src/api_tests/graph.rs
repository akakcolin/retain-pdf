use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::util::ServiceExt;

use super::jobs_common::{read_json, test_state};
use crate::app::build_app;
use crate::models::api::{
    BlockEntityLink, EntityPageRecord, FavoriteRecord, NewEntity, NewEntityRelation,
};
use crate::models::{now_iso, UploadRecord};

fn entity(name: &str, entity_type: &str) -> NewEntity {
    NewEntity {
        name: name.to_string(),
        entity_type: entity_type.to_string(),
        aliases: Vec::new(),
        description: String::new(),
    }
}

fn seed_document(state: &crate::AppState) {
    state
        .db
        .upsert_document_from_upload(&UploadRecord {
            upload_id: "up-1".to_string(),
            filename: "化学.pdf".to_string(),
            stored_path: "uploads/x/chem.pdf".to_string(),
            bytes: 10,
            page_count: 1,
            uploaded_at: now_iso(),
            developer_mode: false,
            content_hash: "doc-1".to_string(),
        })
        .expect("document");
}

fn link_block(state: &crate::AppState, entity_id: &str, block: &str) {
    state
        .db
        .link_block_entity(&BlockEntityLink {
            document_id: "doc-1".to_string(),
            entity_id: entity_id.to_string(),
            page_idx: 0,
            block_id: block.to_string(),
            job_id: "job-1".to_string(),
            surface_form: "x".to_string(),
            snippet: "x 片段".to_string(),
            confidence: 1.0,
            source: "extraction".to_string(),
        })
        .expect("link");
}

fn favorite(id: &str, quote: &str, note: &str) -> FavoriteRecord {
    FavoriteRecord {
        favorite_id: id.to_string(),
        document_id: "doc-1".to_string(),
        job_id: "job-1".to_string(),
        page_idx: 2,
        block_id: format!("p003-b{id}"),
        char_start: None,
        char_end: None,
        kind: "sentence".to_string(),
        quote_text: quote.to_string(),
        translated_quote_text: String::new(),
        note: note.to_string(),
        asset_id: String::new(),
        rect_json: String::new(),
        created_at: now_iso(),
        updated_at: now_iso(),
    }
}

#[tokio::test]
async fn entity_relations_route_returns_neighbors() {
    let state = test_state("graph-relations");
    let app = build_app(state.clone());
    let method = state.db.upsert_entity(&entity("GNN", "method")).expect("a");
    let dataset = state.db.upsert_entity(&entity("QM9", "dataset")).expect("b");
    state
        .db
        .add_entity_relation(&NewEntityRelation {
            from_entity_id: method.entity_id.clone(),
            to_entity_id: dataset.entity_id.clone(),
            relation_type: "evaluates".to_string(),
            confidence: 0.8,
            explanation: "同一句".to_string(),
            source_document_id: "doc-1".to_string(),
            source_block_id: String::new(),
        })
        .expect("relation");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/entities/{}/relations", method.entity_id))
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    let item = &payload["data"]["items"][0];
    assert_eq!(item["name"], serde_json::json!("QM9"));
    assert_eq!(item["relation_type"], serde_json::json!("evaluates"));
    assert_eq!(item["direction"], serde_json::json!("out"));
}

#[tokio::test]
async fn entity_relations_route_404s_for_unknown_entity() {
    let state = test_state("graph-relations-404");
    let app = build_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-20260909123456-abcdef/relations")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

fn relate(state: &crate::AppState, from: &str, to: &str, kind: &str) {
    state
        .db
        .add_entity_relation(&NewEntityRelation {
            from_entity_id: from.to_string(),
            to_entity_id: to.to_string(),
            relation_type: kind.to_string(),
            confidence: 0.8,
            explanation: "同一句".to_string(),
            source_document_id: "doc-1".to_string(),
            source_block_id: String::new(),
        })
        .expect("relation");
}

#[tokio::test]
async fn entity_neighborhood_route_returns_subgraph() {
    let state = test_state("graph-neighborhood");
    let app = build_app(state.clone());
    let root = state.db.upsert_entity(&entity("GNN", "method")).expect("root");
    let neighbor = state.db.upsert_entity(&entity("QM9", "dataset")).expect("n");
    relate(&state, &root.entity_id, &neighbor.entity_id, "evaluates");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/entities/{}/neighborhood", root.entity_id))
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["root"], serde_json::json!(root.entity_id));
    assert_eq!(
        payload["data"]["nodes"].as_array().expect("nodes").len(),
        2
    );
    let edge = &payload["data"]["edges"][0];
    assert_eq!(
        edge["from_entity_id"],
        serde_json::json!(root.entity_id)
    );
    assert_eq!(
        edge["to_entity_id"],
        serde_json::json!(neighbor.entity_id)
    );
    assert_eq!(edge["relation_type"], serde_json::json!("evaluates"));
}

#[tokio::test]
async fn entity_neighborhood_route_depth_controls_node_count() {
    let state = test_state("graph-neighborhood-depth");
    let app = build_app(state.clone());
    let a = state.db.upsert_entity(&entity("A", "concept")).expect("a");
    let b = state.db.upsert_entity(&entity("B", "concept")).expect("b");
    let c = state.db.upsert_entity(&entity("C", "concept")).expect("c");
    relate(&state, &a.entity_id, &b.entity_id, "uses");
    relate(&state, &b.entity_id, &c.entity_id, "uses");

    let fetch = |depth: u32| {
        let app = app.clone();
        let id = a.entity_id.clone();
        async move {
            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!(
                            "/api/v1/entities/{id}/neighborhood?depth={depth}"
                        ))
                        .header("X-API-Key", "test-key")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            read_json(response).await
        }
    };

    let one = fetch(1).await;
    assert_eq!(one["data"]["nodes"].as_array().expect("nodes").len(), 2);
    let two = fetch(2).await;
    assert_eq!(two["data"]["nodes"].as_array().expect("nodes").len(), 3);
}

#[tokio::test]
async fn entity_neighborhood_route_404s_for_unknown_entity() {
    let state = test_state("graph-neighborhood-404");
    let app = build_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-20260909123456-abcdef/neighborhood")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn extract_document_graph_requires_llm_key() {
    // 抽取要真调模型:缺 key 时干净地 400,不能打到上游才 401。
    let state = test_state("graph-extract-no-key");
    let app = build_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/documents/doc-1/graph/extract")
                .header("X-API-Key", "test-key")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = read_json(response).await;
    assert!(
        payload["message"]
            .as_str()
            .unwrap_or("")
            .contains("LLM API Key"),
        "unexpected payload: {payload}"
    );
}

#[tokio::test]
async fn entity_page_route_reports_no_page_without_404() {
    // 实体存在但没生成过页 → 200 + has_page:false,前端据此显示「生成」按钮。
    let state = test_state("graph-page-missing");
    let app = build_app(state.clone());
    let entity = state.db.upsert_entity(&entity("GNN", "method")).expect("entity");
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/entities/{}/page", entity.entity_id))
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["has_page"], serde_json::json!(false));
    assert_eq!(payload["data"]["body_md"], serde_json::json!(""));
    assert_eq!(payload["data"]["links"], serde_json::json!([]));
}

#[tokio::test]
async fn entity_page_route_404s_for_unknown_entity() {
    let state = test_state("graph-page-404");
    let app = build_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-20260909123456-abcdef/page")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn generate_entity_page_requires_llm_key() {
    let state = test_state("graph-page-no-key");
    let app = build_app(state.clone());
    let entity = state.db.upsert_entity(&entity("GNN", "method")).expect("entity");
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/entities/{}/page", entity.entity_id))
                .header("X-API-Key", "test-key")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = read_json(response).await;
    assert!(
        payload["message"]
            .as_str()
            .unwrap_or("")
            .contains("LLM API Key"),
        "unexpected payload: {payload}"
    );
}

#[tokio::test]
async fn entity_backlinks_route_returns_linking_pages() {
    let state = test_state("graph-backlinks");
    let app = build_app(state.clone());
    let target = state.db.upsert_entity(&entity("GNN", "method")).expect("a");
    let source = state.db.upsert_entity(&entity("QM9", "dataset")).expect("b");
    state
        .db
        .upsert_entity_page(
            &EntityPageRecord {
                entity_id: source.entity_id.clone(),
                body_md: "QM9 常用于评测 [[GNN]] 与 [[不存在]] [1]。".to_string(),
                citations: Vec::new(),
                evidence_sig: String::new(),
                generated_at: now_iso(),
                edited_body_md: String::new(),
                edited_at: String::new(),
            },
            true,
        )
        .expect("page");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/entities/{}/backlinks", target.entity_id))
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    let item = &payload["data"]["items"][0];
    assert_eq!(item["name"], serde_json::json!("QM9"));
    assert_eq!(item["entity_type"], serde_json::json!("dataset"));
    assert!(
        item["snippet"].as_str().unwrap_or("").contains("GNN"),
        "unexpected payload: {payload}"
    );
}

#[tokio::test]
async fn entity_backlinks_route_404s_for_unknown_entity() {
    let state = test_state("graph-backlinks-404");
    let app = build_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-nope/backlinks")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn entity_favorites_route_returns_matching_annotations() {
    let state = test_state("graph-favorites");
    let app = build_app(state.clone());
    seed_document(&state);
    let target = state.db.upsert_entity(&entity("GNN", "method")).expect("a");
    state
        .db
        .save_favorite(&favorite("fav-1", "GNN 的表示学习", ""))
        .expect("f1");
    state
        .db
        .save_favorite(&favorite("fav-2", "无关句子", "提到 GNN 一次"))
        .expect("f2");
    state
        .db
        .save_favorite(&favorite("fav-3", "完全无关", ""))
        .expect("f3");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/v1/entities/{}/favorites", target.entity_id))
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    let items = payload["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "unexpected payload: {payload}");
    let quotes: Vec<&str> = items
        .iter()
        .filter_map(|item| item["quote_text"].as_str())
        .collect();
    assert!(quotes.contains(&"GNN 的表示学习"));
    assert!(quotes.contains(&"无关句子"));
    assert_eq!(items[0]["document_title"], serde_json::json!("化学"));
    assert_eq!(items[0]["page_idx"], serde_json::json!(2));
}

#[tokio::test]
async fn entity_favorites_route_404s_for_unknown_entity() {
    let state = test_state("graph-favorites-404");
    let app = build_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-nope/favorites")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn pending_pages_route_returns_missing_and_stale() {
    let state = test_state("graph-pending");
    let app = build_app(state.clone());
    seed_document(&state);
    let missing = state.db.upsert_entity(&entity("GNN", "method")).expect("a");
    link_block(&state, &missing.entity_id, "p001-b0000");
    let stale = state.db.upsert_entity(&entity("QM9", "dataset")).expect("b");
    link_block(&state, &stale.entity_id, "p001-b0001");
    state
        .db
        .upsert_entity_page(
            &EntityPageRecord {
                entity_id: stale.entity_id.clone(),
                body_md: "旧正文".to_string(),
                citations: Vec::new(),
                evidence_sig: "old".to_string(),
                generated_at: now_iso(),
                edited_body_md: String::new(),
                edited_at: String::new(),
            },
            true,
        )
        .expect("page");

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/documents/doc-1/graph/pending-pages")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    let items = payload["data"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "unexpected payload: {payload}");
    // 陈旧的排前,缺页的在后
    assert_eq!(items[0]["name"], serde_json::json!("QM9"));
    assert_eq!(items[0]["has_page"], serde_json::json!(true));
    assert_eq!(items[0]["stale"], serde_json::json!(true));
    assert_eq!(items[1]["name"], serde_json::json!("GNN"));
    assert_eq!(items[1]["has_page"], serde_json::json!(false));
    assert_eq!(items[1]["stale"], serde_json::json!(false));
}

#[tokio::test]
async fn pending_pages_route_404s_for_unknown_document() {
    let state = test_state("graph-pending-404");
    let app = build_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/documents/doc-nope/graph/pending-pages")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// 给实体写一张模型原文概念页(人工编辑路由测试用)。
fn seed_page(state: &crate::AppState, entity_id: &str) {
    state
        .db
        .upsert_entity_page(
            &EntityPageRecord {
                entity_id: entity_id.to_string(),
                body_md: "模型原文 [1]。".to_string(),
                citations: Vec::new(),
                evidence_sig: "m0:0:r0:0".to_string(),
                generated_at: now_iso(),
                edited_body_md: String::new(),
                edited_at: String::new(),
            },
            true,
        )
        .expect("page");
}

#[tokio::test]
async fn save_entity_page_route_persists_manual_edit() {
    let state = test_state("graph-page-save");
    let app = build_app(state.clone());
    let entity = state.db.upsert_entity(&entity("GNN", "method")).expect("entity");
    seed_page(&state, &entity.entity_id);
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/entities/{}/page", entity.entity_id))
                .header("X-API-Key", "test-key")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"body_md":"人工修订 [[GNN]]"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["edited"], serde_json::json!(true));
    assert_eq!(payload["data"]["body_md"], serde_json::json!("人工修订 [[GNN]]"));
    assert_eq!(payload["data"]["links"][0]["surface"], serde_json::json!("GNN"));
    assert!(
        !payload["data"]["edited_at"].as_str().unwrap_or("").is_empty(),
        "unexpected payload: {payload}"
    );
    // 模型原文仍保留
    let page = state
        .db
        .get_entity_page(&entity.entity_id)
        .expect("load")
        .expect("some");
    assert_eq!(page.body_md, "模型原文 [1]。");
    assert_eq!(page.effective_body(), "人工修订 [[GNN]]");
}

#[tokio::test]
async fn save_entity_page_route_revert_restores_model_body() {
    let state = test_state("graph-page-revert");
    let app = build_app(state.clone());
    let entity = state.db.upsert_entity(&entity("GNN", "method")).expect("entity");
    seed_page(&state, &entity.entity_id);
    state
        .db
        .set_entity_page_edit(&entity.entity_id, "人工修订", "t")
        .expect("edit");
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/entities/{}/page", entity.entity_id))
                .header("X-API-Key", "test-key")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"revert":true}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["edited"], serde_json::json!(false));
    assert_eq!(payload["data"]["body_md"], serde_json::json!("模型原文 [1]。"));
    assert_eq!(payload["data"]["edited_at"], serde_json::json!(""));
}

#[tokio::test]
async fn save_entity_page_route_rejects_empty_body() {
    let state = test_state("graph-page-empty");
    let app = build_app(state.clone());
    let entity = state.db.upsert_entity(&entity("GNN", "method")).expect("entity");
    seed_page(&state, &entity.entity_id);
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/entities/{}/page", entity.entity_id))
                .header("X-API-Key", "test-key")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"body_md":"   "}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = read_json(response).await;
    assert!(
        payload["message"].as_str().unwrap_or("").contains("不能为空"),
        "unexpected payload: {payload}"
    );
}

#[tokio::test]
async fn save_entity_page_route_400s_without_page() {
    let state = test_state("graph-page-no-page");
    let app = build_app(state.clone());
    let entity = state.db.upsert_entity(&entity("GNN", "method")).expect("entity");
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/entities/{}/page", entity.entity_id))
                .header("X-API-Key", "test-key")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"body_md":"x"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn save_entity_page_route_404s_for_unknown_entity() {
    let state = test_state("graph-page-save-404");
    let app = build_app(state);
    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/entities/ent-nope/page")
                .header("X-API-Key", "test-key")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"body_md":"x"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn generate_entity_page_409s_on_manual_edit_without_overwrite() {
    let state = test_state("graph-page-conflict");
    let app = build_app(state.clone());
    let entity = state.db.upsert_entity(&entity("GNN", "method")).expect("entity");
    seed_page(&state, &entity.entity_id);
    state
        .db
        .set_entity_page_edit(&entity.entity_id, "人工修订", "t")
        .expect("edit");
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/entities/{}/page", entity.entity_id))
                .header("X-API-Key", "test-key")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"llm_api_key":"sk-test"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn graph_routes_require_api_key() {
    let state = test_state("graph-auth");
    let app = build_app(state);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-x/page")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-x/backlinks")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-x/favorites")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/entities/ent-x/neighborhood")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/entities/ent-x/page")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"body_md":"x"}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/documents/doc-1/graph/pending-pages")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
