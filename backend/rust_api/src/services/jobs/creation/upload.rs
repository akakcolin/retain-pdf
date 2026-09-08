use std::path::{Path, PathBuf};

use lopdf::Document;
use tokio::io::AsyncWriteExt;

use crate::db::Db;
use crate::error::AppError;
use crate::models::domain::{build_job_id, now_iso, UploadRecord};

/// Hard cap on the native repair subprocess (a hostile PDF must not pin the
/// worker indefinitely).
const PDF_REPAIR_TIMEOUT_SECS: u64 = 30;

#[derive(Debug)]
pub struct UploadedPdfInput {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub developer_mode: bool,
}

pub(super) fn load_upload_or_404(db: &Db, upload_id: &str) -> Result<UploadRecord, AppError> {
    db.get_upload(upload_id)
        .map_err(|_| AppError::not_found(format!("upload not found: {upload_id}")))
}

/// Reduces a client-supplied multipart filename to a bare file-name component
/// so it can never be used to escape the per-upload directory (e.g. via
/// `../../etc/x.pdf` or an absolute path like `/etc/x.pdf`).
fn sanitize_upload_filename(filename: &str) -> Result<PathBuf, AppError> {
    if filename.contains('\0') {
        return Err(AppError::bad_request("uploaded filename is invalid"));
    }
    let candidate = Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::bad_request("uploaded filename is invalid"))?;
    if candidate.is_empty()
        || candidate == "."
        || candidate == ".."
        || candidate.contains('/')
        || candidate.contains('\\')
    {
        return Err(AppError::bad_request("uploaded filename is invalid"));
    }
    Ok(PathBuf::from(candidate))
}

pub async fn store_pdf_upload(
    db: &Db,
    uploads_dir: &Path,
    upload_max_bytes: u64,
    upload_max_pages: u32,
    upload_max_complexity: u64,
    render_rs_bin: &Path,
    upload: UploadedPdfInput,
) -> Result<UploadRecord, AppError> {
    if !upload.filename.to_lowercase().ends_with(".pdf") {
        return Err(AppError::bad_request("uploaded file must be a PDF"));
    }
    let byte_count = upload.bytes.len() as u64;
    // Size gate runs before the repair subprocess: a hostile oversized upload
    // must not buy 30s of worker time.
    if upload_max_bytes > 0 && byte_count > upload_max_bytes {
        return Err(AppError::bad_request(format!(
            "当前服务限制：PDF 文件大小必须不超过 {:.2}MB",
            upload_max_bytes as f64 / 1024.0 / 1024.0
        )));
    }
    let upload_id = build_job_id();
    let upload_dir = uploads_dir.join(&upload_id);
    tokio::fs::create_dir_all(&upload_dir).await?;
    let safe_filename = sanitize_upload_filename(&upload.filename)?;
    let upload_path: PathBuf = upload_dir.join(&safe_filename);
    let mut f = tokio::fs::File::create(&upload_path).await?;
    f.write_all(&upload.bytes).await?;
    f.flush().await?;

    let page_count = load_pdf_page_count_or_repair(
        &upload_path,
        render_rs_bin,
        upload_max_bytes,
        upload_max_pages,
    )
    .await?;

    if upload_max_pages > 0 && page_count > upload_max_pages {
        return Err(AppError::bad_request(format!(
            "当前服务限制：PDF 页数必须不超过 {} 页",
            upload_max_pages
        )));
    }
    if upload_max_complexity > 0 {
        let object_count = load_pdf_object_count(&upload_path)
            .map_err(|e| AppError::bad_request(format!("invalid pdf: {e}")))?;
        let complexity = page_count as u128 * object_count as u128;
        if complexity > upload_max_complexity as u128 {
            return Err(AppError::bad_request(format!(
                "当前服务限制：PDF 复杂度（页数 {} × 对象数 {}）必须不超过 {}",
                page_count, object_count, upload_max_complexity
            )));
        }
    }

    let content_hash = crate::db::documents::sha256_hex(&upload.bytes);
    let record = UploadRecord {
        upload_id,
        filename: upload.filename,
        stored_path: upload_path.to_string_lossy().to_string(),
        bytes: byte_count,
        page_count,
        uploaded_at: now_iso(),
        developer_mode: upload.developer_mode,
        content_hash,
    };
    db.save_upload(&record)?;
    // 内容哈希即文档身份:同一 PDF 重复上传归并到同一 document
    db.upsert_document_from_upload(&record)?;
    Ok(record)
}

async fn load_pdf_page_count_or_repair(
    path: &Path,
    render_rs_bin: &Path,
    max_output_bytes: u64,
    max_pages: u32,
) -> Result<u32, AppError> {
    match load_pdf_page_count(path) {
        Ok(page_count) => Ok(page_count),
        Err(original_error) => {
            repair_pdf_via_render_rs(path, render_rs_bin, max_output_bytes, max_pages)
                .await
                .map_err(|repair_error| {
                    AppError::bad_request(format!(
                        "invalid pdf: {original_error}; repair failed: {repair_error}"
                    ))
                })?;
            load_pdf_page_count(path)
                .map_err(|e| AppError::bad_request(format!("invalid pdf after repair: {e}")))
        }
    }
}

fn load_pdf_page_count(path: &Path) -> Result<u32, lopdf::Error> {
    Document::load(path).map(|doc| doc.get_pages().len() as u32)
}

fn load_pdf_object_count(path: &Path) -> Result<u64, lopdf::Error> {
    Document::load(path).map(|doc| doc.objects.len() as u64)
}

async fn repair_pdf_via_render_rs(
    path: &Path,
    render_rs_bin: &Path,
    max_output_bytes: u64,
    max_pages: u32,
) -> Result<(), String> {
    let repaired_path = path.with_extension("repairing.pdf");
    let _ = tokio::fs::remove_file(&repaired_path).await;
    let mut command = tokio::process::Command::new(render_rs_bin);
    command
        .arg("--repair-pdf")
        .arg(path)
        .arg(&repaired_path)
        .arg(max_output_bytes.to_string())
        .arg(max_pages.to_string())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(
        std::time::Duration::from_secs(PDF_REPAIR_TIMEOUT_SECS),
        command.output(),
    )
    .await
    {
        Ok(result) => result.map_err(|e| e.to_string())?,
        Err(_) => {
            let _ = tokio::fs::remove_file(&repaired_path).await;
            return Err(format!(
                "render_rs repair timed out after {PDF_REPAIR_TIMEOUT_SECS}s"
            ));
        }
    };
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = [stdout, stderr]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let _ = tokio::fs::remove_file(&repaired_path).await;
        return Err(if detail.is_empty() {
            format!("render_rs repair exited with {}", output.status)
        } else {
            detail
        });
    }
    tokio::fs::rename(&repaired_path, path)
        .await
        .map_err(|e| e.to_string())
}
