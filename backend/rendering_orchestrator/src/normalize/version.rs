// Mirror of `services/document_schema/version.py`.

pub const DOCUMENT_SCHEMA_NAME: &str = "normalized_document_v1";
pub const DOCUMENT_SCHEMA_VERSION: &str = "1.1";
pub const DOCUMENT_SCHEMA_FILE_NAME: &str = "document.v1.json";
pub const DOCUMENT_SCHEMA_REPORT_FILE_NAME: &str = "document.v1.report.json";

/// Forward-reader: future schema versions are admitted by extending this set
/// (mirrors `services/document_schema/version.py::SUPPORTED_DOCUMENT_SCHEMA_VERSIONS`).
pub const SUPPORTED_DOCUMENT_SCHEMA_VERSIONS: &[&str] = &[DOCUMENT_SCHEMA_VERSION];
