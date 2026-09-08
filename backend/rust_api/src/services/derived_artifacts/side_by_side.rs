use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::models::domain::JobSnapshot;

use super::{cached_output_is_fresh, job_artifacts_dir, DerivedArtifactDeps};

pub(crate) fn ensure_side_by_side_pdf(
    deps: DerivedArtifactDeps<'_>,
    data_root: &Path,
    job: &JobSnapshot,
    source_pdf: &Path,
    translated_pdf: &Path,
) -> Result<PathBuf, AppError> {
    let output_dir = job_artifacts_dir(data_root, job)?;
    let output_pdf = output_dir.join(format!("{}-side-by-side.pdf", job.job_id));
    if !cached_output_is_fresh(&output_pdf, &[source_pdf, translated_pdf])? {
        build_side_by_side_pdf(deps, source_pdf, translated_pdf, &output_pdf)?;
    }
    Ok(output_pdf)
}

fn build_side_by_side_pdf(
    deps: DerivedArtifactDeps<'_>,
    source_pdf: &Path,
    translated_pdf: &Path,
    output_pdf: &Path,
) -> Result<(), AppError> {
    let tmp_pdf = output_pdf.with_extension("pdf.tmp");
    let _ = std::fs::remove_file(&tmp_pdf);
    let status = std::process::Command::new(deps.render_rs_bin)
        .arg("--side-by-side")
        .arg(source_pdf)
        .arg(translated_pdf)
        .arg(&tmp_pdf)
        .status()
        .map_err(|error| {
            AppError::internal(format!("failed to build side-by-side pdf: {error}"))
        })?;
    if !status.success() || !tmp_pdf.exists() {
        let _ = std::fs::remove_file(&tmp_pdf);
        return Err(AppError::internal(
            "failed to build side-by-side pdf from source and translated pdf",
        ));
    }
    std::fs::rename(&tmp_pdf, output_pdf)?;
    Ok(())
}
