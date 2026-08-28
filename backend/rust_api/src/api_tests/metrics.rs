use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::util::ServiceExt;

use crate::api_tests::jobs_common::{read_json, test_state};
use crate::app::build_app;
use crate::models::domain::{JobSnapshot, JobStatusKind};
use crate::models::request::CreateJobInput;

fn render_command() -> Vec<String> {
    vec![
        "/opt/bin/render_rs".to_string(),
        "--spec".to_string(),
        "/tmp/spec.json".to_string(),
    ]
}

fn save_succeeded_render_job(state: &crate::AppState, job_id: &str, renderer: &str) {
    let mut job = JobSnapshot::new(job_id.to_string(), CreateJobInput::default(), render_command());
    job.status = JobStatusKind::Succeeded;
    job.runtime.get_or_insert_with(Default::default).renderer = Some(renderer.to_string());
    job.sync_runtime_state();
    state.db.save_job(&job).expect("save render job");
}

#[tokio::test]
async fn metrics_endpoint_emits_prometheus_text_without_api_key() {
    let state = test_state("metrics-endpoint");
    save_succeeded_render_job(&state, "job-render-1", "render_rs");

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("metrics request"),
        )
        .await
        .expect("metrics response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/plain; version=0.0.4; charset=utf-8")
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("metrics body");
    let text = String::from_utf8(body.to_vec()).expect("utf8 body");
    assert!(text.contains("retainpdf_jobs_total{status=\"succeeded\"} 1"));
    assert!(text.contains(
        "retainpdf_render_jobs_total{renderer=\"render_rs\",status=\"succeeded\"} 1"
    ));
    assert!(text.contains("retainpdf_render_elapsed_seconds_count 0"));
}

#[tokio::test]
async fn health_aggregates_renderer_and_native_hit_ratio() {
    let state = test_state("health-metrics");
    save_succeeded_render_job(&state, "job-render-2", "render_rs");

    let artifacts_dir = state
        .config
        .data_root
        .join("jobs")
        .join("job-render-2")
        .join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("artifacts dir");
    std::fs::write(
        artifacts_dir.join("native_stats.json"),
        r#"{"hits": {"source": 5}, "fallbacks": {"source": {"in_memory_page": 5}}}"#,
    )
    .expect("write native stats");

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("health request"),
        )
        .await
        .expect("health response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = read_json(response).await;
    assert_eq!(body["data"]["render_jobs_by_renderer"]["render_rs"], 1);
    assert!(
        (body["data"]["native_hit_ratio"]["source"]
            .as_f64()
            .expect("ratio")
            - 0.5)
            .abs()
            < 1e-9
    );
}
