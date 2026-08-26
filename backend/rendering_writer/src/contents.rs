//! Page `/Contents` read/rewrite helpers.

use mupdf::pdf::PdfDocument;
use mupdf::pdf::PdfObject;
use mupdf::pdf::PdfPage;
use mupdf::Buffer;
use mupdf::Error;

/// Resolve an indirect reference to its direct value (keeps direct values).
pub fn resolve(obj: &PdfObject) -> Result<PdfObject, Error> {
    Ok(obj.resolve()?.unwrap_or_else(|| obj.clone()))
}

/// Concatenated decoded bytes of a page's `/Contents` (single stream or
/// array), mirroring `pikepdf.parse_content_stream(page)`. Empty when absent.
///
/// `read_stream` requires an indirect reference (mupdf's `pdf_load_stream`
/// resolves only references); do NOT `resolve()` stream objects before reading.
pub fn page_contents_bytes(page: &PdfPage) -> Result<Vec<u8>, Error> {
    let Some(contents) = page.contents()? else {
        return Ok(Vec::new());
    };
    if contents.is_array()? {
        let n = contents.len()?;
        let mut out = Vec::new();
        for i in 0..n {
            if let Some(stream) = contents.get_array(i as i32)? {
                out.extend_from_slice(&stream.read_stream()?);
            }
        }
        Ok(out)
    } else {
        contents.read_stream()
    }
}

/// Replace a page's `/Contents` with a single new uncompressed stream.
pub fn replace_page_contents(
    page: &PdfPage,
    doc: &mut PdfDocument,
    content: &[u8],
) -> Result<(), Error> {
    let buf = Buffer::from_bytes(content)?;
    let stream = doc.add_stream(&buf, None, false)?;
    let mut page_obj = page.object();
    page_obj.dict_put("Contents", stream)?;
    Ok(())
}

/// Coalesce an array-valued `/Contents` into a single stream (no-op for a
/// single stream or no contents). Mirrors `pikepdf.Page.contents_coalesce`.
pub fn coalesce_contents(page: &PdfPage, doc: &mut PdfDocument) -> Result<(), Error> {
    let Some(contents) = page.contents()? else {
        return Ok(());
    };
    let contents = resolve(&contents)?;
    if contents.is_array()? && contents.len()? > 1 {
        let joined = page_contents_bytes(page)?;
        replace_page_contents(page, doc, &joined)?;
    }
    Ok(())
}
