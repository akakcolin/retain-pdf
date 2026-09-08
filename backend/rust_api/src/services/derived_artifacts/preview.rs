use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::models::domain::JobSnapshot;

use super::{job_artifacts_dir, DerivedArtifactDeps};

/// fitz `pix.save(output)` default JPEG quality (cover/thumbnail).
const JPEG_QUALITY_DEFAULT: u8 = 95;
/// fitz `pix.save(output, jpg_quality=82)` (page preview).
const JPEG_QUALITY_PREVIEW: u8 = 82;

#[derive(Clone, Copy)]
pub(crate) enum BookImageKind {
    Cover,
    Thumbnail,
}

impl BookImageKind {
    pub(crate) fn file_name(self) -> &'static str {
        match self {
            Self::Cover => "cover.jpg",
            Self::Thumbnail => "thumbnail.jpg",
        }
    }

    pub(crate) fn width_px(self) -> u32 {
        match self {
            Self::Cover => 900,
            Self::Thumbnail => 360,
        }
    }
}

pub(crate) fn ensure_book_image(
    deps: DerivedArtifactDeps<'_>,
    data_root: &Path,
    job: &JobSnapshot,
    source_pdf: &Path,
    kind: BookImageKind,
) -> Result<PathBuf, AppError> {
    let output_dir = job_artifacts_dir(data_root, job)?;
    let output_path = output_dir.join(kind.file_name());
    ensure_book_image_at_path(deps, source_pdf, &output_path, kind)
}

/// 文档级封面/缩略图：从源 PDF 首页渲染，缓存到 documents/<id>/。
pub(crate) fn ensure_document_book_image(
    deps: DerivedArtifactDeps<'_>,
    data_root: &Path,
    document_id: &str,
    source_pdf: &Path,
    kind: BookImageKind,
) -> Result<PathBuf, AppError> {
    let output_dir = super::document_artifacts_dir(data_root, document_id)?;
    let output_path = output_dir.join(kind.file_name());
    ensure_book_image_at_path(deps, source_pdf, &output_path, kind)
}

fn ensure_book_image_at_path(
    deps: DerivedArtifactDeps<'_>,
    source_pdf: &Path,
    output_path: &Path,
    kind: BookImageKind,
) -> Result<PathBuf, AppError> {
    if output_path.exists() && output_path.is_file() {
        return Ok(output_path.to_path_buf());
    }
    render_book_image(deps.render_rs_bin, source_pdf, output_path, kind.width_px())?;
    Ok(output_path.to_path_buf())
}

pub(crate) fn ensure_page_preview(
    deps: DerivedArtifactDeps<'_>,
    output_path: &Path,
    source_pdf: &Path,
    page_index: u32,
    width_px: u32,
    dpi: u32,
) -> Result<PathBuf, AppError> {
    if output_path.exists() && output_path.is_file() {
        return Ok(output_path.to_path_buf());
    }
    render_pdf_page_preview(
        deps.render_rs_bin,
        source_pdf,
        output_path,
        page_index,
        width_px,
        dpi,
    )?;
    Ok(output_path.to_path_buf())
}

fn render_book_image(
    render_rs_bin: &Path,
    source_pdf: &Path,
    output_path: &Path,
    width_px: u32,
) -> Result<(), AppError> {
    // First page, scaled to width_px; fitz's default JPEG quality.
    run_render_page_jpeg(
        render_rs_bin,
        source_pdf,
        output_path,
        0,
        width_px,
        0,
        JPEG_QUALITY_DEFAULT,
        "failed to render book image",
    )
}

fn render_pdf_page_preview(
    render_rs_bin: &Path,
    source_pdf: &Path,
    output_path: &Path,
    page_index: u32,
    width_px: u32,
    dpi: u32,
) -> Result<(), AppError> {
    run_render_page_jpeg(
        render_rs_bin,
        source_pdf,
        output_path,
        page_index,
        width_px,
        dpi,
        JPEG_QUALITY_PREVIEW,
        "failed to render page preview",
    )
}

#[allow(clippy::too_many_arguments)]
fn run_render_page_jpeg(
    render_rs_bin: &Path,
    source_pdf: &Path,
    output_path: &Path,
    page_index: u32,
    width_px: u32,
    dpi: u32,
    quality: u8,
    failure: &str,
) -> Result<(), AppError> {
    let status = std::process::Command::new(render_rs_bin)
        .arg("--render-page-jpeg")
        .arg(source_pdf)
        .arg(output_path)
        .arg(page_index.to_string())
        .arg(width_px.to_string())
        .arg(dpi.to_string())
        .arg(quality.to_string())
        .status()
        .map_err(|error| AppError::internal(format!("{failure}: {error}")))?;
    if !status.success() || !output_path.exists() {
        return Err(AppError::internal(failure));
    }
    Ok(())
}
