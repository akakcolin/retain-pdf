use std::fs;
use std::io::Read;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::util::ServiceExt;

use super::jobs_common::{read_json, test_state};
use crate::app::build_app;
use crate::models::{CreateJobInput, JobArtifacts, JobSnapshot};

#[tokio::test]
async fn markdown_document_route_returns_content_and_direct_image_links() {
    let state = test_state("markdown-document");
    let job_root = state.config.output_root.join("markdown-document-job");
    let markdown_dir = job_root.join("md");
    let images_dir = markdown_dir.join("images/page-1/imgs");
    fs::create_dir_all(&images_dir).expect("create markdown images");
    fs::write(images_dir.join("chart a.png"), b"fake png").expect("write image");
    fs::write(
        markdown_dir.join("full.md"),
        "hello\n\n![Image](images/page-1/imgs/chart a.png)\n",
    )
    .expect("write markdown");

    let mut input = CreateJobInput::default();
    input.runtime.job_id = "markdown-document-job".to_string();
    let mut job = JobSnapshot::new(
        "markdown-document-job".to_string(),
        input,
        vec!["python".to_string()],
    );
    job.artifacts = Some(JobArtifacts {
        job_root: Some("jobs/markdown-document-job".to_string()),
        ..JobArtifacts::default()
    });
    state.db.save_job(&job).expect("save job");

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/jobs/markdown-document-job/markdown/document")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("markdown document request"),
        )
        .await
        .expect("markdown document response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload["data"]["job_id"], "markdown-document-job");
    assert_eq!(payload["data"]["ready"], true);
    assert_eq!(
        payload["data"]["content"],
        "hello\n\n![Image](images/page-1/imgs/chart a.png)\n"
    );
    let abs_md = payload["data"]["content_with_absolute_image_urls"]
        .as_str()
        .expect("absolute markdown");
    assert!(
        abs_md.contains(
            "http://127.0.0.1:41000/api/v1/jobs/markdown-document-job/markdown/images/page-1/imgs/chart%20a.png"
        ),
        "absolute markdown unexpected: {abs_md:?}"
    );
    // 不得出现双 images 前缀
    assert!(!abs_md.contains("/markdown/images/images/"));
    assert_eq!(
        payload["data"]["raw_path"],
        "/api/v1/jobs/markdown-document-job/markdown?raw=true"
    );
    assert_eq!(
        payload["data"]["images_base_path"],
        "/api/v1/jobs/markdown-document-job/markdown/images/"
    );
    let image = &payload["data"]["images"][0];
    assert_eq!(image["path"], "images/page-1/imgs/chart a.png");
    assert_eq!(image["content_type"], "image/png");
    assert_eq!(image["size_bytes"], 8);
    assert_eq!(
        image["url"],
        "http://127.0.0.1:41000/api/v1/jobs/markdown-document-job/markdown/images/page-1/imgs/chart%20a.png"
    );
}

#[tokio::test]
async fn markdown_document_rewrites_html_img_and_titled_markdown_links() {
    let state = test_state("markdown-html-img");
    let job_root = state.config.output_root.join("markdown-html-img-job");
    let markdown_dir = job_root.join("md");
    let images_dir = markdown_dir.join("images/page-2/imgs");
    fs::create_dir_all(&images_dir).expect("create markdown images");
    fs::write(images_dir.join("fig.png"), b"fake png").expect("write image");
    fs::write(
        markdown_dir.join("full.md"),
        concat!(
            "html\n\n",
            "<div><img src=\"images/page-2/imgs/fig.png\" alt=\"Image\" width=\"48%\" /></div>\n\n",
            "md titled\n\n",
            "![cap](images/page-2/imgs/fig.png \"figure\")\n",
        ),
    )
    .expect("write markdown");

    let mut input = CreateJobInput::default();
    input.runtime.job_id = "markdown-html-img-job".to_string();
    let mut job = JobSnapshot::new(
        "markdown-html-img-job".to_string(),
        input,
        vec!["python".to_string()],
    );
    job.artifacts = Some(JobArtifacts {
        job_root: Some("jobs/markdown-html-img-job".to_string()),
        ..JobArtifacts::default()
    });
    state.db.save_job(&job).expect("save job");

    let response = build_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/jobs/markdown-html-img-job/markdown/document")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("markdown document request"),
        )
        .await
        .expect("markdown document response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    let abs = payload["data"]["content_with_absolute_image_urls"]
        .as_str()
        .expect("absolute markdown");
    let expected = "http://127.0.0.1:41000/api/v1/jobs/markdown-html-img-job/markdown/images/page-2/imgs/fig.png";
    assert!(abs.contains(expected), "html img rewritten: {abs}");
    assert!(abs.contains(&format!("![cap]({expected})")), "titled md rewritten: {abs}");
    assert!(!abs.contains("/markdown/images/images/"));
}

#[tokio::test]
async fn translated_markdown_bundle_zip_is_registered_and_downloadable() {
    let state = test_state("translated-markdown-bundle");
    let job_root = state.config.output_root.join("translated-markdown-bundle-job");
    let markdown_dir = job_root.join("md");
    let images_dir = markdown_dir.join("images/page-1/imgs");
    fs::create_dir_all(&images_dir).expect("create markdown images");
    fs::write(images_dir.join("chart.png"), b"fake png").expect("write image");
    fs::write(
        markdown_dir.join("full.md"),
        "hello\n\n![Image](images/page-1/imgs/chart.png)\n",
    )
    .expect("write markdown");
    fs::write(
        markdown_dir.join("translated.md"),
        "你好\n\n![Image](images/page-1/imgs/chart.png)\n",
    )
    .expect("write translated markdown");

    let mut input = CreateJobInput::default();
    input.runtime.job_id = "translated-markdown-bundle-job".to_string();
    let mut job = JobSnapshot::new(
        "translated-markdown-bundle-job".to_string(),
        input,
        vec!["python".to_string()],
    );
    job.artifacts = Some(JobArtifacts {
        job_root: Some("jobs/translated-markdown-bundle-job".to_string()),
        ..JobArtifacts::default()
    });
    state.db.save_job(&job).expect("save job");

    // artifacts-manifest 同时注册原文与译文两个 bundle artifact。
    let manifest = build_app(state.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/jobs/translated-markdown-bundle-job/artifacts-manifest")
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("manifest request"),
        )
        .await
        .expect("manifest response");
    assert_eq!(manifest.status(), StatusCode::OK);
    let manifest_payload = read_json(manifest).await;
    let items = manifest_payload["data"]["items"]
        .as_array()
        .expect("manifest items");
    let original_bundle = items
        .iter()
        .find(|item| item["artifact_key"] == "markdown_bundle_zip")
        .expect("markdown_bundle_zip item");
    assert_eq!(original_bundle["ready"], true);
    let translated_bundle = items
        .iter()
        .find(|item| item["artifact_key"] == "translated_markdown_bundle_zip")
        .expect("translated_markdown_bundle_zip item");
    assert_eq!(translated_bundle["ready"], true);
    assert_eq!(
        translated_bundle["file_name"],
        "translated-markdown-bundle-job-translated-markdown.zip"
    );

    // 下载译文 markdown zip。
    let response = build_app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(
                    "/api/v1/jobs/translated-markdown-bundle-job/artifacts/translated_markdown_bundle_zip?include_job_dir=true",
                )
                .header("X-API-Key", "test-key")
                .body(Body::empty())
                .expect("bundle request"),
        )
        .await
        .expect("bundle response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    assert_eq!(&body[..2], b"PK", "zip magic");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body.to_vec())).expect("open zip");
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let full_name = names
        .iter()
        .find(|name| name.ends_with("/full.md"))
        .unwrap_or_else(|| panic!("zip contains full.md: {names:?}"));
    let mut full = archive.by_name(full_name).expect("read full.md entry");
    let mut text = String::new();
    full.read_to_string(&mut text).expect("read full.md text");
    assert!(text.contains("你好"), "translated markdown text: {text:?}");
    assert!(
        text.contains("images/page-1/imgs/chart.png"),
        "translated markdown images preserved: {text:?}"
    );
    assert!(names
        .iter()
        .any(|name| name.contains("images/page-1/imgs/chart.png")));
}
