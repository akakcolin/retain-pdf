use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::api_tests::jobs_common::{read_json, test_state};
use crate::app::build_app;

#[tokio::test]
async fn list_ocr_providers_returns_public_contract() {
    let response = build_app(test_state("providers-ocr-contract"))
        .oneshot(
            Request::builder()
                .uri("/api/v1/providers/ocr")
                .header("x-api-key", "test-key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    let providers = payload["data"].as_array().expect("providers array");
    let paddle = providers
        .iter()
        .find(|item| item["key"] == "paddle")
        .expect("paddle provider");

    assert_eq!(paddle["display_name"], "PaddleOCR");
    assert_eq!(paddle["provider_kind"], "remote");
    assert_eq!(paddle["credential"]["field"], "paddle_token");
    assert_eq!(paddle["credential"]["env"], "RETAIN_PADDLE_API_TOKEN");
    assert_eq!(paddle["options"]["paddle_model"]["type"], "string");
    assert_eq!(
        paddle["options"]["paddle_model"]["default"],
        "PaddleOCR-VL-1.6"
    );
    assert_eq!(
        paddle["options"]["paddle_model"]["aliases"]["paddleocr-vl"],
        "PaddleOCR-VL-1.6"
    );
    assert!(
        providers.iter().all(|item| item["key"] != "local"),
        "local command-provider OCR is retired and must not be listed"
    );
}
