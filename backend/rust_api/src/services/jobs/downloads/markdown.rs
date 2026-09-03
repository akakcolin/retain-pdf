use std::sync::LazyLock;

use crate::error::AppError;
use crate::models::api::{to_absolute_url, MarkdownDocumentView, MarkdownImageView};
use crate::models::domain::JobSnapshot;
use crate::storage_paths::{resolve_markdown_images_dir, resolve_markdown_path};

use super::super::query::load_supported_job;
use super::paths::safe_markdown_image_path;
use super::{FileDownload, MarkdownDownload, QueryJobsDeps};

// 匹配 ![alt](images/...)，路径内可含空格；可选 "title"/'title'、尖括号、./ 前缀
// path 用贪婪 [^)>\n]+；title 必须带引号，避免把 `chart a.png` 的空格误当 title 分隔
const MARKDOWN_IMAGE_LINK_RE: &str =
    r#"!\[([^\]]*)\]\(\s*<?((?:\./)?images/[^)>\n]+)>?(?:[ \t]+(?:"[^"]*"|'[^']*'))?\s*\)"#;
// HTML: 分单/双引号（regex crate 不支持 backref）
const HTML_IMAGE_SRC_DQ_RE: &str = r#"(?i)(<img\b[^>]*?\bsrc\s*=\s*")((?:\./)?images/[^"]+)(")"#;
const HTML_IMAGE_SRC_SQ_RE: &str = r#"(?i)(<img\b[^>]*?\bsrc\s*=\s*')((?:\./)?images/[^']+)(')"#;

// 编译一次、全局复用，避免每次请求重复编译三条正则。
static MD_IMAGE_LINK: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(MARKDOWN_IMAGE_LINK_RE).expect("valid markdown image regex")
});
static HTML_IMAGE_SRC_DQ: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(HTML_IMAGE_SRC_DQ_RE).expect("valid html img dq regex"));
static HTML_IMAGE_SRC_SQ: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(HTML_IMAGE_SRC_SQ_RE).expect("valid html img sq regex"));

pub(crate) async fn markdown_download(
    deps: &QueryJobsDeps<'_>,
    job_id: String,
) -> Result<MarkdownDownload, AppError> {
    let job = load_supported_job(deps.db, deps.data_root, &job_id)?;
    let markdown_path = resolve_markdown_path(&job, deps.data_root)
        .ok_or_else(|| AppError::not_found(format!("markdown not found: {job_id}")))?;
    let content = tokio::fs::read_to_string(&markdown_path).await?;
    Ok(MarkdownDownload {
        job_id: job.job_id.clone(),
        content,
    })
}

pub(crate) async fn markdown_document_view(
    deps: &QueryJobsDeps<'_>,
    job_id: &str,
    base_url: &str,
) -> Result<MarkdownDocumentView, AppError> {
    let job = load_supported_job(deps.db, deps.data_root, job_id)?;
    let markdown_path = resolve_markdown_path(&job, deps.data_root)
        .ok_or_else(|| AppError::not_found(format!("markdown not found: {job_id}")))?;
    let content = tokio::fs::read_to_string(&markdown_path).await?;
    let raw_path = format!("/api/v1/jobs/{}/markdown?raw=true", job.job_id);
    let markdown_path_url = format!("/api/v1/jobs/{}/markdown/document", job.job_id);
    let images_base_path = format!("/api/v1/jobs/{}/markdown/images/", job.job_id);
    let images = markdown_images_view(deps, &job, base_url)?;
    let content_with_absolute_image_urls =
        rewrite_markdown_image_links_to_absolute_urls(&content, &job.job_id, base_url);
    Ok(MarkdownDocumentView {
        job_id: job.job_id.clone(),
        ready: true,
        content,
        content_with_absolute_image_urls,
        markdown_path: markdown_path_url.clone(),
        markdown_url: to_absolute_url(base_url, &markdown_path_url),
        raw_path: raw_path.clone(),
        raw_url: to_absolute_url(base_url, &raw_path),
        images_base_path: images_base_path.clone(),
        images_base_url: to_absolute_url(base_url, &images_base_path),
        images,
    })
}

pub(crate) fn markdown_image_download(
    deps: &QueryJobsDeps<'_>,
    job_id: &str,
    path: &str,
) -> Result<FileDownload, AppError> {
    let job = load_supported_job(deps.db, deps.data_root, job_id)?;
    let images_dir = resolve_markdown_images_dir(&job, deps.data_root)
        .ok_or_else(|| AppError::not_found(format!("markdown images not found: {job_id}")))?;
    let relative_path = safe_markdown_image_path(path)?;
    let file_path = images_dir.join(relative_path);
    if !file_path.exists() || !file_path.is_file() {
        return Err(AppError::not_found(format!(
            "markdown image not found: {path}"
        )));
    }
    let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
    Ok(FileDownload::new(file_path, mime.as_ref(), None))
}

fn markdown_images_view(
    deps: &QueryJobsDeps<'_>,
    job: &JobSnapshot,
    base_url: &str,
) -> Result<Vec<MarkdownImageView>, AppError> {
    let Some(images_dir) = resolve_markdown_images_dir(job, deps.data_root) else {
        return Ok(Vec::new());
    };
    let mut images = Vec::new();
    for entry in walkdir::WalkDir::new(&images_dir)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(&images_dir) else {
            continue;
        };
        let relative_path = relative.to_string_lossy().replace('\\', "/");
        let resource_path = format!(
            "/api/v1/jobs/{}/markdown/images/{}",
            job.job_id,
            url_path_escape(&relative_path)
        );
        let metadata = path.metadata().ok();
        // path 与 markdown 原文引用一致：images/<rel>
        // 注意：拼 images_base_url 时前端必须去掉 images/ 前缀，见 normalize_markdown_image_rel
        images.push(MarkdownImageView {
            path: format!("images/{relative_path}"),
            url: to_absolute_url(base_url, &resource_path),
            content_type: mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string(),
            size_bytes: metadata.map(|item| item.len()),
        });
    }
    images.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(images)
}

/// 把 markdown/html 里的相对图片引用改成可直连的 API 绝对 URL。
/// 同时修掉「images_base + images/...」双重 images 的前端拼法隐患：
/// 绝对 URL 直接指向 /markdown/images/<rel>，不再依赖 base 拼接。
pub(crate) fn rewrite_markdown_image_links_to_absolute_urls(
    content: &str,
    job_id: &str,
    base_url: &str,
) -> String {
    let rewritten_md = MD_IMAGE_LINK.replace_all(content, |captures: &regex::Captures<'_>| {
        let alt = &captures[1];
        let raw_path = &captures[2];
        let absolute = absolute_markdown_image_url(raw_path, job_id, base_url);
        format!("![{alt}]({absolute})")
    });

    let rewritten_html_dq =
        HTML_IMAGE_SRC_DQ.replace_all(&rewritten_md, |captures: &regex::Captures<'_>| {
            let prefix = &captures[1];
            let raw_path = &captures[2];
            let suffix = &captures[3];
            let absolute = absolute_markdown_image_url(raw_path, job_id, base_url);
            format!("{prefix}{absolute}{suffix}")
        });

    HTML_IMAGE_SRC_SQ
        .replace_all(&rewritten_html_dq, |captures: &regex::Captures<'_>| {
            let prefix = &captures[1];
            let raw_path = &captures[2];
            let suffix = &captures[3];
            let absolute = absolute_markdown_image_url(raw_path, job_id, base_url);
            format!("{prefix}{absolute}{suffix}")
        })
        .into_owned()
}

fn absolute_markdown_image_url(raw_path: &str, job_id: &str, base_url: &str) -> String {
    let relative = normalize_markdown_image_rel(raw_path);
    if relative.is_empty() {
        return raw_path.to_string();
    }
    let resource_path = format!(
        "/api/v1/jobs/{job_id}/markdown/images/{}",
        url_path_escape(&relative)
    );
    to_absolute_url(base_url, &resource_path)
}

/// 归一化 markdown 图片相对路径 → 相对 images 目录的路径（不含 images/ 前缀）
fn normalize_markdown_image_rel(raw: &str) -> String {
    let mut path = raw
        .trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_string();
    // 去掉可选 title：仅当 `path "title"` / `path 'title'` 时截断
    // 文件名本身可含空格（chart a.png），不能见空白就截
    if let Some(idx) = path.find(" \"") {
        path = path[..idx].to_string();
    } else if let Some(idx) = path.find(" '") {
        path = path[..idx].to_string();
    }
    path = path.replace('\\', "/");
    while path.starts_with("./") {
        path = path[2..].to_string();
    }
    // 剥掉一层或多层 images/ 前缀，避免 images/images/...
    while path.starts_with("images/") {
        path = path["images/".len()..].to_string();
    }
    path.trim().to_string()
}

fn url_path_escape(path: &str) -> String {
    path.split('/')
        .map(percent_encode_path_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode_path_segment(segment: &str) -> String {
    let mut encoded = String::new();
    for byte in segment.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            encoded.push(ch);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
