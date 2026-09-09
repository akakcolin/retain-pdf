use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::util::ServiceExt;

use super::jobs_common::{read_json, test_state};
use crate::app::build_app;
use crate::models::api::{NewEntity, NewEntityRelation};

fn entity(name: &str, entity_type: &str) -> NewEntity {
    NewEntity {
        name: name.to_string(),
        entity_type: entity_type.to_string(),
        aliases: Vec::new(),
        description: String::new(),
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
}
