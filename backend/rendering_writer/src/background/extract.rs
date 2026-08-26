//! Port of `source/background/extract.py`: read raw FlateDecode image streams
//! from a PDF and classify them for solid-fill replacement. mupdf-rs substitutes
//! fitz `xref_get_key`/`extract_image` with `PdfDocument::xref_object` +
//! `PdfObject::get_dict` (both resolve indirect references, matching fitz), and
//! `xref_stream` for `fitz.Document.xref_stream` (filter-decoded bytes).

use mupdf::pdf::{PdfDocument, PdfObject};
use mupdf::Error;

/// Solid value that can replace a classified image stream. Mirrors the Python
/// `fill` key: `1` (mode "1"), `255` (mode "L"), or `(255, 255, 255)` (RGB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidFill {
    One,
    Byte(u8),
    Rgb([u8; 3]),
}

/// Classified raw stream: mode plus the solid fill that can replace it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawStreamImageMeta {
    pub mode: &'static str,
    pub width: u32,
    pub height: u32,
    pub fill: SolidFill,
}

/// `extract.py::_pdf_int` — resolved int, default 0 on any mismatch.
fn pdf_int(obj: Option<&PdfObject>) -> Result<i32, Error> {
    match resolve(obj)? {
        Some(o) if o.is_int()? => o.as_int(),
        _ => Ok(0),
    }
}

/// `extract.py::_pdf_bool` — true only for a resolved bool with value true.
fn pdf_bool(obj: Option<&PdfObject>) -> Result<bool, Error> {
    match resolve(obj)? {
        Some(o) if o.is_bool()? => o.as_bool(),
        _ => Ok(false),
    }
}

/// `extract.py::_pdf_name` — resolved name as `/FlateDecode`-style string,
/// empty string for any non-name object.
fn pdf_name(obj: Option<&PdfObject>) -> Result<String, Error> {
    match resolve(obj)? {
        Some(o) if o.is_name()? => Ok(o.to_string()),
        _ => Ok(String::new()),
    }
}

/// `extract.py::_pdf_names` — a single name yields itself; an array yields the
/// name strings of its items (separator-insensitive).
fn pdf_filter_names(obj: Option<&PdfObject>) -> Result<Vec<String>, Error> {
    match resolve(obj)? {
        Some(o) if o.is_name()? => Ok(vec![o.to_string()]),
        Some(o) if o.is_array()? => {
            let mut out = Vec::new();
            for item in o.array_iter()? {
                let item = item?;
                match resolve(Some(&item))? {
                    Some(n) if n.is_name()? => out.push(n.to_string()),
                    _ => {}
                }
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

fn resolve(obj: Option<&PdfObject>) -> Result<Option<PdfObject>, Error> {
    match obj {
        Some(o) => Ok(Some(o.resolve()?.unwrap_or_else(|| o.clone()))),
        None => Ok(None),
    }
}

/// `extract.py::raw_stream_image_meta` — classify the image XObject at `xref`:
/// `None` unless it is an 8-bit FlateDecode Gray/RGB stream or a 1-bit image
/// mask. Any non-`name` Filter (e.g. an array) is rejected exactly like Python.
pub fn raw_stream_image_meta(
    doc: &PdfDocument,
    xref: i32,
) -> Result<Option<RawStreamImageMeta>, Error> {
    if xref < 0 || xref as usize >= doc.xref_len()? {
        return Ok(None);
    }
    let Some(obj) = doc.xref_object(xref)? else {
        return Ok(None);
    };
    let filter_name = match resolve(obj.get_dict("Filter")?.as_ref())? {
        Some(f) if f.is_name()? => f.to_string(),
        _ => return Ok(None),
    };
    if filter_name != "/FlateDecode" {
        return Ok(None);
    }
    let width = pdf_int(obj.get_dict("Width")?.as_ref())?;
    let height = pdf_int(obj.get_dict("Height")?.as_ref())?;
    let bits_per_component = pdf_int(obj.get_dict("BitsPerComponent")?.as_ref())?;
    let image_mask = pdf_bool(obj.get_dict("ImageMask")?.as_ref())?;
    let color_space = pdf_name(obj.get_dict("ColorSpace")?.as_ref())?;

    if width <= 0 || height <= 0 {
        return Ok(None);
    }
    let (width, height) = (width as u32, height as u32);
    let meta = if image_mask && bits_per_component == 1 {
        RawStreamImageMeta {
            mode: "1",
            width,
            height,
            fill: SolidFill::One,
        }
    } else if bits_per_component == 8 && color_space == "/DeviceGray" {
        RawStreamImageMeta {
            mode: "L",
            width,
            height,
            fill: SolidFill::Byte(255),
        }
    } else if bits_per_component == 8 && color_space == "/DeviceRGB" {
        RawStreamImageMeta {
            mode: "RGB",
            width,
            height,
            fill: SolidFill::Rgb([255, 255, 255]),
        }
    } else {
        return Ok(None);
    };
    Ok(Some(meta))
}

/// `extract.py::extract_raw_stream_image` — the filter-decoded stream bytes,
/// `None` unless the byte count matches the mode (Python `Image.frombytes`
/// raises on mismatch). `mode "1"` expects `ceil(width*height/8)` bytes.
pub fn extract_raw_stream_image(
    doc: &PdfDocument,
    xref: i32,
    meta: &RawStreamImageMeta,
) -> Result<Option<Vec<u8>>, Error> {
    let bytes = doc.xref_stream(xref)?;
    let expected = match meta.mode {
        // PIL mode "1" packs bits MSB-first with each row padded to a byte
        // boundary: `ceil(width / 8)` bytes per row.
        "1" => ((meta.width as usize + 7) / 8) * meta.height as usize,
        "L" => meta.width as usize * meta.height as usize,
        "RGB" => meta.width as usize * meta.height as usize * 3,
        _ => return Ok(None),
    };
    if bytes.len() != expected {
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// `extract.py::image_prefers_solid_fill` — image masks, 1-bit stencils, and
/// JBIG2/CCITT gray halftones are better replaced by a solid cover than kept.
pub fn image_prefers_solid_fill(doc: &PdfDocument, xref: i32) -> Result<bool, Error> {
    if xref < 0 || xref as usize >= doc.xref_len()? {
        return Ok(false);
    }
    let Some(obj) = doc.xref_object(xref)? else {
        return Ok(false);
    };
    let filters = pdf_filter_names(obj.get_dict("Filter")?.as_ref())?;
    let bits_per_component = pdf_int(obj.get_dict("BitsPerComponent")?.as_ref())?;
    let image_mask = pdf_bool(obj.get_dict("ImageMask")?.as_ref())?;
    let color_space = pdf_name(obj.get_dict("ColorSpace")?.as_ref())?;

    if image_mask || bits_per_component == 1 {
        return Ok(true);
    }
    if color_space == "/DeviceGray"
        && filters.iter().any(|f| f == "/JBIG2Decode" || f == "/CCITTFaxDecode")
    {
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupdf::pdf::PdfDocument;
    use mupdf::Buffer;

    /// Build an image XObject. `colorspace` is the bare name (e.g.
    /// `"DeviceRGB"`); `filter == "FlateDecode"` emits a real FlateDecode stream
    /// over `raw`; any other `filter` emits a dict-only object (no stream) so
    /// filter-rejection paths are exercised without valid stream bytes.
    fn make_image_object(
        doc: &mut PdfDocument,
        width: i32,
        height: i32,
        bpc: i32,
        colorspace: Option<&str>,
        image_mask: bool,
        filter: &str,
        raw: &[u8],
    ) -> PdfObject {
        let mut dict = doc.new_dict().expect("dict");
        dict.dict_put("Type", PdfObject::new_name("XObject").unwrap()).expect("Type");
        dict.dict_put("Subtype", PdfObject::new_name("Image").unwrap()).expect("Subtype");
        dict.dict_put("Width", PdfObject::new_int(width).unwrap()).expect("Width");
        dict.dict_put("Height", PdfObject::new_int(height).unwrap()).expect("Height");
        dict.dict_put("BitsPerComponent", PdfObject::new_int(bpc).unwrap()).expect("BPC");
        if let Some(cs) = colorspace {
            dict.dict_put("ColorSpace", PdfObject::new_name(cs).unwrap()).expect("CS");
        }
        if image_mask {
            dict.dict_put("ImageMask", PdfObject::new_bool(true)).expect("ImageMask");
        }
        if !filter.is_empty() {
            dict.dict_put("Filter", PdfObject::new_name(filter).unwrap()).expect("Filter");
        }
        // add_object registers the dict in the doc and returns an indirect ref.
        let mut obj = doc.add_object(&dict).expect("register");
        if filter == "FlateDecode" {
            // This MuPDF's pdf_update_stream stores the buffer verbatim and
            // never adds a FlateDecode filter, so compress with flate2 and set
            // /Filter ourselves; compressed=1 keeps the filter key.
            let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            std::io::Write::write_all(&mut enc, raw).expect("write");
            let compressed = enc.finish().expect("finish");
            let buf = Buffer::from_bytes(&compressed).expect("buffer");
            obj.write_raw_stream_buffer(&buf).expect("stream");
        }
        obj
    }

    fn xref(obj: &PdfObject) -> i32 {
        obj.as_indirect().expect("indirect")
    }

    #[test]
    fn rgb_stream_classifies_and_extracts() {
        let mut doc = PdfDocument::new();
        let raw: Vec<u8> = (0..4 * 3 * 3).map(|_| 0xE0).collect();
        let obj = make_image_object(&mut doc, 4, 3, 8, Some("DeviceRGB"), false, "FlateDecode", &raw);
        let xref_id = xref(&obj);
        let meta = raw_stream_image_meta(&doc, xref_id)
            .expect("meta")
            .expect("classified");
        assert_eq!(meta.mode, "RGB");
        assert_eq!((meta.width, meta.height), (4, 3));
        assert_eq!(meta.fill, SolidFill::Rgb([255, 255, 255]));
        let bytes = extract_raw_stream_image(&doc, xref_id, &meta)
            .expect("extract")
            .expect("bytes");
        assert_eq!(bytes, raw);
        assert!(!image_prefers_solid_fill(&doc, xref_id).expect("prefers"));
    }

    #[test]
    fn gray_stream_classifies_as_L() {
        let mut doc = PdfDocument::new();
        let raw: Vec<u8> = (0..4).map(|_| 0xE0).collect();
        let obj = make_image_object(&mut doc, 2, 2, 8, Some("DeviceGray"), false, "FlateDecode", &raw);
        let xref_id = xref(&obj);
        let meta = raw_stream_image_meta(&doc, xref_id)
            .expect("meta")
            .expect("classified");
        assert_eq!(meta.mode, "L");
        assert_eq!(meta.fill, SolidFill::Byte(255));
        let bytes = extract_raw_stream_image(&doc, xref_id, &meta)
            .expect("extract")
            .expect("bytes");
        assert_eq!(bytes, raw);
    }

    #[test]
    fn image_mask_classifies_as_1bit_and_prefers_solid() {
        let mut doc = PdfDocument::new();
        // 4x3 mask, row-padded to 1 byte per row (PIL mode "1" layout).
        let raw = [0b1010_1010u8, 0b1111_0000u8, 0b1100_0011u8];
        let obj = make_image_object(&mut doc, 4, 3, 1, None, true, "FlateDecode", &raw);
        let xref_id = xref(&obj);
        let meta = raw_stream_image_meta(&doc, xref_id)
            .expect("meta")
            .expect("classified");
        assert_eq!(meta.mode, "1");
        assert_eq!(meta.fill, SolidFill::One);
        let bytes = extract_raw_stream_image(&doc, xref_id, &meta)
            .expect("extract")
            .expect("bytes");
        assert_eq!(bytes, raw);
        assert!(image_prefers_solid_fill(&doc, xref_id).expect("prefers"));
    }

    #[test]
    fn onebit_extract_requires_row_padded_len() {
        // 9-wide mask: PIL mode "1" pads each row to ceil(9/8) = 2 bytes, so a
        // 9x4 mask needs 8 bytes, not ceil(9*4/8) = 5.
        let mut doc = PdfDocument::new();
        let raw = [0u8; 8];
        let obj = make_image_object(&mut doc, 9, 4, 1, None, true, "FlateDecode", &raw);
        let xref_id = xref(&obj);
        let meta = raw_stream_image_meta(&doc, xref_id)
            .expect("meta")
            .expect("classified");
        let bytes = extract_raw_stream_image(&doc, xref_id, &meta)
            .expect("extract")
            .expect("bytes");
        assert_eq!(bytes, raw);
        // ceil(9*4/8) = 5 bytes is rejected (PIL would raise).
        let short = [0u8; 5];
        let obj = make_image_object(&mut doc, 9, 4, 1, None, true, "FlateDecode", &short);
        assert!(extract_raw_stream_image(&doc, xref(&obj), &meta).expect("short").is_none());
    }

    #[test]
    fn non_flate_or_missing_filter_rejected() {
        let mut doc = PdfDocument::new();
        // Non-Flate named filter (e.g. DCTDecode) → no classification.
        let dct = make_image_object(&mut doc, 2, 2, 8, Some("DeviceRGB"), false, "DCTDecode", &[]);
        assert!(raw_stream_image_meta(&doc, xref(&dct)).expect("dct").is_none());
        // No filter key at all → no classification.
        let none = make_image_object(&mut doc, 2, 2, 8, Some("DeviceRGB"), false, "", &[]);
        assert!(raw_stream_image_meta(&doc, xref(&none)).expect("none").is_none());
        // Out-of-range xref → None, not an error.
        assert!(raw_stream_image_meta(&doc, 999_999).expect("oob").is_none());
        // A non-image object (the catalog) → None.
        let catalog = doc.catalog().expect("catalog");
        assert!(raw_stream_image_meta(&doc, xref(&catalog)).expect("cat").is_none());
    }

    #[test]
    fn non_flate_image_does_not_prefer_solid() {
        let mut doc = PdfDocument::new();
        let dct = make_image_object(&mut doc, 2, 2, 8, Some("DeviceGray"), false, "DCTDecode", &[]);
        let xref_id = xref(&dct);
        // DCTDecode is not JBIG2/CCITT, so gray DCT images do not prefer solid.
        assert!(!image_prefers_solid_fill(&doc, xref_id).expect("prefers"));
        // 1-bit is rejected by meta but still prefers solid.
        let bpc1 = make_image_object(&mut doc, 2, 2, 1, Some("DeviceGray"), false, "DCTDecode", &[]);
        assert!(image_prefers_solid_fill(&doc, xref(&bpc1)).expect("bpc1"));
    }
}
