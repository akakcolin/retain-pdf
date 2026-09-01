pub mod client;
pub mod errors;

#[allow(unused_imports)]
pub use client::{build_paddlex_optional_payload, LocalPaddlexClient, LocalPaddlexResultPayload};
pub use errors::LocalPaddlexProviderError;

use crate::ocr_provider::types::OcrProviderCapabilities;

pub fn capabilities() -> OcrProviderCapabilities {
    OcrProviderCapabilities {
        supports_remote_url_submit: false,
        supports_local_file_upload: true,
        supports_polling: false,
        supports_download_bundle: false,
        supports_extra_formats: false,
        supports_formula_toggle: false,
        supports_table_toggle: false,
    }
}
