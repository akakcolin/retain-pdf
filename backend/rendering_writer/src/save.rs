//! Deterministic PDF saving for `rendering_writer`.
//!
//! Python (production) saves with `object_stream_mode=generate,
//! compress_streams=True, recompress_flate=False`. mupdf-rs `PdfWriteOptions`
//! has no object-stream setter and defaults to objstms off, so `generate` is
//! approximated by the default; `set_compress` covers `compress_streams`.
//!
//! Determinism: mupdf-rs does not inject `/Producer` or dates, but it does
//! overwrite the trailer `/ID` second half with a time-seeded PRNG, so
//! `delete_trailer_id` must run before saving to make repeated writes
//! byte-identical.

use std::path::Path;

use mupdf::pdf::PdfDocument;
use mupdf::pdf::PdfWriteOptions;
use mupdf::Error;

/// `PdfWriteOptions` mirroring the production `compress_streams=True` save.
pub fn save_options(compress: bool) -> PdfWriteOptions {
    let mut options = PdfWriteOptions::default();
    options.set_compress(compress);
    options.set_garbage_level(0);
    options
}

/// Drop the trailer `/ID` so a save is byte-deterministic across runs.
pub fn delete_trailer_id(pdf: &PdfDocument) -> Result<(), Error> {
    let mut trailer = pdf.trailer()?;
    trailer.dict_delete("ID")?;
    Ok(())
}

/// Save to `path` via a short-lived sibling temp file, then rename.
pub fn save_atomic(pdf: &PdfDocument, path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Io(e))?;
    }
    let tmp = sibling_tmp_path(path);
    let result = pdf.save_with_options(
        tmp.to_str().ok_or(Error::InvalidUtf8)?,
        save_options(true),
    );
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, path).map_err(|e| Error::Io(e))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn sibling_tmp_path(path: &Path) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.chars().take(12).collect::<String>())
        .unwrap_or_else(|| "rpsave".to_string());
    let pid = std::process::id();
    match path.parent() {
        Some(parent) => parent.join(format!(".{pid}-{stem}.tmp.pdf")),
        None => std::path::PathBuf::from(format!(".{pid}-{stem}.tmp.pdf")),
    }
}
