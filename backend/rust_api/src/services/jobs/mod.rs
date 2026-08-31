mod control;
mod creation;
mod debug;
mod downloads;
mod facade;
pub(super) mod live_stage;
mod presentation;
mod query;
mod reader_ai;
pub(crate) use reader_ai::translate_text;
mod reader_regions;
mod readiness;
mod stage_plan;
pub(crate) mod stage_view;
pub(super) mod summary_loaders;
mod support;

pub use control::wait_for_terminal_job;
pub(crate) use creation::context::{
    CommandJobsDeps, ControlDeps, JobSubmitDeps, QueryJobsDeps, ReplayDeps, SnapshotBuildDeps,
    UploadStoreDeps,
};
pub(crate) use creation::{store_pdf_upload, UploadedPdfInput};
pub(crate) use downloads::{DocumentDownloadKind, FileDownload, MarkdownDownload};
pub(crate) use facade::build_jobs_facade;
pub use facade::JobsFacade;
pub(crate) use readiness::job_readiness;

pub use crate::services::job_validation::{
    validate_mineru_upload_limits, validate_ocr_provider_request, validate_provider_credentials,
};
