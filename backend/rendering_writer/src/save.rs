//! Deterministic PDF saving for `rendering_writer`.
//!
//! Two save flavours:
//!   * `save_atomic` / `save_optimized` — mupdf-rs `PdfWriteOptions` on an
//!     open `PdfDocument`. Python (production) saves with
//!     `object_stream_mode=generate, compress_streams=True,
//!     recompress_flate=False`; mupdf-rs has no object-stream setter (objstms
//!     default off) and no subset API, so the compress flags + garbage 4
//!     approximate it.
//!   * `subset_and_clean` — full font-subset + clean write in one pass through
//!     the C shim (`c/save_clean.c`, compiled by `build.rs`), used by the
//!     production `save_optimized_pdf` so fitz `subset_fonts()` is no longer
//!     needed. mupdf exceptions are caught inside the shim and returned as
//!     `Err` (a raw call without a try boundary would `exit()` the process).
//!
//! Determinism: mupdf-rs does not inject `/Producer` or dates, but it does
//! overwrite the trailer `/ID` second half with a time-seeded PRNG, so
//! `delete_trailer_id` must run before saving to make repeated writes
//! byte-identical.

use std::path::Path;

use mupdf::pdf::PdfDocument;
use mupdf::pdf::PdfWriteOptions;
use mupdf::Error;

// C shim compiled by `build.rs` from `c/save_clean.c`; see that file for the
// exception-safety rationale.
extern "C" {
    fn mupdf_clean_new_context() -> *mut mupdf_sys::fz_context;
    fn mupdf_clean_free(ptr: *mut core::ffi::c_void);
    fn mupdf_subset_and_write_bytes(
        ctx: *mut mupdf_sys::fz_context,
        data: *const u8,
        size: usize,
        pwo: *const mupdf_sys::pdf_write_options,
        out: *mut *mut u8,
        out_size: *mut usize,
        errptr: *mut *mut mupdf_sys::mupdf_error_t,
    ) -> i32;
}

/// mupdf exceptions escape through longjmp; the shim guarantees a context is
/// always dropped exactly once, so this guard cannot be replaced with `?`.
struct CleanContext(*mut mupdf_sys::fz_context);
impl Drop for CleanContext {
    fn drop(&mut self) {
        unsafe { mupdf_sys::fz_drop_context(self.0) };
    }
}

/// `PdfWriteOptions` mirroring the production `compress_streams=True` save.
pub fn save_options(compress: bool) -> PdfWriteOptions {
    let mut options = PdfWriteOptions::default();
    options.set_compress(compress);
    options.set_garbage_level(0);
    options
}

/// `save_optimized` (mupdf-rs on-disk) options — production's reference save
/// uses `garbage=4, deflate/deflate_images/deflate_fonts, use_objstms=1`.
/// mupdf-rs has no object-stream setter and no `subset_fonts`, so the
/// compress-image/font flags + garbage 4 approximate it; the production
/// subset+compaction entry point is `subset_and_clean`, not this.
pub fn save_optimized_options() -> PdfWriteOptions {
    let mut options = PdfWriteOptions::default();
    options.set_compress(true);
    options.set_compress_images(true);
    options.set_compress_fonts(true);
    options.set_garbage_level(4);
    options
}

/// Drop the trailer `/ID` so a save is byte-deterministic across runs.
pub fn delete_trailer_id(pdf: &PdfDocument) -> Result<(), Error> {
    let mut trailer = pdf.trailer()?;
    trailer.dict_delete("ID")?;
    Ok(())
}

fn save_with_options_atomic(pdf: &PdfDocument, path: &Path, options: PdfWriteOptions) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::Io)?;
    }
    let tmp = sibling_tmp_path(path);
    let result = pdf.save_with_options(
        tmp.to_str().ok_or(Error::InvalidUtf8)?,
        options,
    );
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, path).map_err(Error::Io)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Save to `path` via a short-lived sibling temp file, then rename.
pub fn save_atomic(pdf: &PdfDocument, path: &Path) -> Result<(), Error> {
    save_with_options_atomic(pdf, path, save_options(true))
}

/// `save_optimized_pdf` — same atomic pattern with the optimized options.
pub fn save_optimized(pdf: &PdfDocument, path: &Path) -> Result<(), Error> {
    save_with_options_atomic(pdf, path, save_optimized_options())
}

/// Font-subset + clean-write in one mupdf pass, pure bytes in/out.
///
/// Replaces the fitz half of `save_optimized_pdf` (`doc.subset_fonts()` +
/// `tobytes()`): the shim runs `pdf_subset_fonts` then writes with the
/// optimized options (`garbage=4`, compress images/fonts, objstms) on an
/// isolated context, so mupdf-raised errors return here as `Err` instead of
/// exiting the process.
pub fn subset_and_clean(pdf_bytes: &[u8]) -> Result<Vec<u8>, String> {
    unsafe {
        let ctx = mupdf_clean_new_context();
        if ctx.is_null() {
            return Err("mupdf: failed to create clean context (version mismatch)".to_string());
        }
        let _ctx_guard = CleanContext(ctx);

        let mut pwo = mupdf_sys::pdf_default_write_options;
        pwo.do_garbage = 4;
        pwo.do_compress = 1;
        pwo.do_compress_images = 1;
        pwo.do_compress_fonts = 1;
        pwo.do_use_objstms = 1;

        let mut out: *mut u8 = std::ptr::null_mut();
        let mut out_size: usize = 0;
        let mut err: *mut mupdf_sys::mupdf_error_t = std::ptr::null_mut();
        let rc = mupdf_subset_and_write_bytes(
            ctx,
            pdf_bytes.as_ptr(),
            pdf_bytes.len(),
            &pwo,
            &mut out,
            &mut out_size,
            &mut err,
        );
        let msg = if !err.is_null() {
            let msg = std::ffi::CStr::from_ptr((*err).message)
                .to_string_lossy()
                .into_owned();
            mupdf_sys::mupdf_drop_error(err);
            msg
        } else {
            String::new()
        };
        if rc != 0 || out.is_null() || out_size == 0 {
            if !out.is_null() {
                mupdf_clean_free(out as *mut core::ffi::c_void);
            }
            return Err(if msg.is_empty() {
                "mupdf: subset/write produced no output".to_string()
            } else {
                format!("mupdf: {msg}")
            });
        }
        let bytes = std::slice::from_raw_parts(out, out_size).to_vec();
        mupdf_clean_free(out as *mut core::ffi::c_void);
        Ok(bytes)
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
