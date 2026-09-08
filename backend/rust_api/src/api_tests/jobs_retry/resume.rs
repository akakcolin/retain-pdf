use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::util::ServiceExt;

use crate::api_tests::jobs_common::{read_json, test_state};
use crate::app::build_app;
use crate::models::{JobArtifacts, JobStatusKind};

use super::common::{
    seed_ocr_checkpoint_files, seed_translation_checkpoint_files, source_job_with_artifacts,
};

#[tokio::test]
async fn resume_plan_route_reports_render_checkpoint() {
    let state = test_state("resume-plan-render");
    let source_job = source_job_with_artifacts(
        "job-resume-plan-render",
        JobArtifacts {
            source_pdf: Some("jobs/source/source/input.pdf".to_string()),
            normalized_document_json: Some("jobs/source/ocr/document.v1.json".to_string()),
            translations_dir: Some("jobs/source/translated".to_string()),
            ..JobArtifacts::default()
        },
    );
    seed_ocr_checkpoint_files(&state, &source_job);
    seed_translation_checkpoint_files(&state, &source_job);
    state.db.save_job(&source_job).expect("save source job");

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/jobs/job-resume-plan-render/resume-plan")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("resume plan request"),
        )
        .await
        .expect("resume plan response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["can_resume"], true);
    assert_eq!(payload["data"]["from_stage"], "render");
    assert_eq!(payload["data"]["resume_workflow"], "render");
    assert_eq!(payload["data"]["reruns_stages"], json!(["rendering"]));
}

#[tokio::test]
async fn resume_route_reuses_rerun_submission_contract() {
    let state = test_state("resume-render");
    let mut source_job = source_job_with_artifacts(
        "job-resume-render-source",
        JobArtifacts {
            source_pdf: Some("jobs/source/source/input.pdf".to_string()),
            normalized_document_json: Some("jobs/source/ocr/document.v1.json".to_string()),
            translations_dir: Some("jobs/source/translated".to_string()),
            ..JobArtifacts::default()
        },
    );
    source_job.status = JobStatusKind::Succeeded;
    seed_ocr_checkpoint_files(&state, &source_job);
    seed_translation_checkpoint_files(&state, &source_job);
    state.db.save_job(&source_job).expect("save source job");

    let response = build_app(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/jobs/job-resume-render-source/resume")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("resume request"),
        )
        .await
        .expect("resume response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["job_id"], "job-resume-render-source");
    assert_eq!(payload["data"]["workflow"], "render");
    let resumed_job = state.db.get_job("job-resume-render-source").expect("job");
    assert_eq!(resumed_job.workflow, crate::models::WorkflowKind::Render);
    assert_eq!(resumed_job.status, JobStatusKind::Queued);
}

#[tokio::test]
async fn resume_plan_degrades_when_manifest_page_missing() {
    let state = test_state("resume-plan-missing-page");
    let source_job = source_job_with_artifacts(
        "job-resume-plan-missing-page",
        JobArtifacts {
            source_pdf: Some("jobs/source/source/input.pdf".to_string()),
            normalized_document_json: Some("jobs/source/ocr/document.v1.json".to_string()),
            translations_dir: Some("jobs/source/translated".to_string()),
            ..JobArtifacts::default()
        },
    );
    seed_ocr_checkpoint_files(&state, &source_job);
    // manifest 存在但声明的页文件缺失：门禁应降级到 translate，而不是信任残缺产物。
    let dir = state.config.data_root.join("jobs/source/translated");
    std::fs::create_dir_all(&dir).expect("translations dir");
    std::fs::write(
        dir.join(crate::storage_paths::TRANSLATION_MANIFEST_FILE_NAME),
        br#"{"schema":"translation_manifest_v1","schema_version":1,"pages":[{"page_index":0,"page_number":1,"path":"page-001.json"}]}"#,
    )
    .expect("translation manifest");
    state.db.save_job(&source_job).expect("save source job");

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/jobs/job-resume-plan-missing-page/resume-plan")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("resume plan request"),
        )
        .await
        .expect("resume plan response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["can_resume"], true);
    assert_eq!(payload["data"]["from_stage"], "translate");
    assert_eq!(payload["data"]["resume_workflow"], "book");
    assert_eq!(
        payload["data"]["reruns_stages"],
        json!(["translation", "rendering"])
    );
}

#[tokio::test]
async fn resume_plan_uses_render_when_translation_pages_intact() {
    let state = test_state("resume-plan-intact");
    let source_job = source_job_with_artifacts(
        "job-resume-plan-intact",
        JobArtifacts {
            source_pdf: Some("jobs/source/source/input.pdf".to_string()),
            normalized_document_json: Some("jobs/source/ocr/document.v1.json".to_string()),
            translations_dir: Some("jobs/source/translated".to_string()),
            ..JobArtifacts::default()
        },
    );
    seed_ocr_checkpoint_files(&state, &source_job);
    seed_translation_checkpoint_files(&state, &source_job);
    state.db.save_job(&source_job).expect("save source job");

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/jobs/job-resume-plan-intact/resume-plan")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("resume plan request"),
        )
        .await
        .expect("resume plan response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["can_resume"], true);
    assert_eq!(payload["data"]["from_stage"], "render");
    assert_eq!(payload["data"]["resume_workflow"], "render");
    assert_eq!(payload["data"]["reruns_stages"], json!(["rendering"]));
}

#[tokio::test]
async fn resume_plan_degrades_when_translation_checksum_mismatches() {
    let state = test_state("resume-plan-checksum-mismatch");
    let source_job = source_job_with_artifacts(
        "job-resume-plan-checksum-mismatch",
        JobArtifacts {
            source_pdf: Some("jobs/source/source/input.pdf".to_string()),
            normalized_document_json: Some("jobs/source/ocr/document.v1.json".to_string()),
            translations_dir: Some("jobs/source/translated".to_string()),
            ..JobArtifacts::default()
        },
    );
    seed_ocr_checkpoint_files(&state, &source_job);
    seed_translation_checkpoint_files(&state, &source_job);
    state.db.save_job(&source_job).expect("save source job");
    // 存盘后页文件被改写（内容与长度都变，绕开 (path,size,mtime) 摘要缓存）：目录摘要
    // 与基线不匹配，门禁应降级到 translate 而不是信任。
    std::fs::write(
        state
            .config
            .data_root
            .join("jobs/source/translated/page-001.json"),
        br#"{"raw_text":"hello world, rewritten after the checkpoint baseline"}"#,
    )
    .expect("mutated translation page");

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/jobs/job-resume-plan-checksum-mismatch/resume-plan")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("resume plan request"),
        )
        .await
        .expect("resume plan response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["can_resume"], true);
    assert_eq!(payload["data"]["from_stage"], "translate");
    assert_eq!(payload["data"]["resume_workflow"], "book");
}

#[tokio::test]
async fn resume_plan_degrades_when_translation_checksum_missing() {
    let state = test_state("resume-plan-checksum-missing");
    let source_job = source_job_with_artifacts(
        "job-resume-plan-checksum-missing",
        JobArtifacts {
            source_pdf: Some("jobs/source/source/input.pdf".to_string()),
            normalized_document_json: Some("jobs/source/ocr/document.v1.json".to_string()),
            translations_dir: Some("jobs/source/translated".to_string()),
            ..JobArtifacts::default()
        },
    );
    seed_ocr_checkpoint_files(&state, &source_job);
    // 存盘时 translations_dir 尚不存在 → 基线为 NULL；之后补齐文件也不能被信任。
    state.db.save_job(&source_job).expect("save source job");
    seed_translation_checkpoint_files(&state, &source_job);

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/jobs/job-resume-plan-checksum-missing/resume-plan")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("resume plan request"),
        )
        .await
        .expect("resume plan response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["can_resume"], true);
    assert_eq!(payload["data"]["from_stage"], "translate");
    assert_eq!(payload["data"]["resume_workflow"], "book");
}
