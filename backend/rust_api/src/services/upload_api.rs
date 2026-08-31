use std::path::Path;

use crate::db::Db;
use crate::error::AppError;
use crate::models::api::{upload_to_response, UploadView};
use crate::models::domain::UploadRecord;
use crate::services::jobs::{store_pdf_upload, UploadedPdfInput};

pub async fn store_upload(
    db: &Db,
    uploads_dir: &Path,
    upload_max_bytes: u64,
    upload_max_pages: u32,
    upload_max_complexity: u64,
    python_bin: &str,
    filename: String,
    bytes: Vec<u8>,
    developer_mode: bool,
) -> Result<UploadRecord, AppError> {
    store_pdf_upload(
        db,
        uploads_dir,
        upload_max_bytes,
        upload_max_pages,
        upload_max_complexity,
        python_bin,
        UploadedPdfInput {
            filename,
            bytes,
            developer_mode,
        },
    )
    .await
}

pub async fn store_upload_view(
    db: &Db,
    uploads_dir: &Path,
    upload_max_bytes: u64,
    upload_max_pages: u32,
    upload_max_complexity: u64,
    python_bin: &str,
    filename: String,
    bytes: Vec<u8>,
    developer_mode: bool,
) -> Result<UploadView, AppError> {
    let upload = store_upload(
        db,
        uploads_dir,
        upload_max_bytes,
        upload_max_pages,
        upload_max_complexity,
        python_bin,
        filename,
        bytes,
        developer_mode,
    )
    .await?;
    Ok(upload_to_response(&upload))
}
