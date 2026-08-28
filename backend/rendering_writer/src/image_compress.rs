//! Port of the production DCTDecode image recompression pipeline
//! (Phase 5C-6): `source/compression/{image_ops.py,image_pipeline.py,analysis.py}`.
//!
//! Production scans every displayed image (`max_display_rect_by_xref`), skips
//! ineligible images (`should_skip_recompress_image`), decodes each DCTDecode
//! stream, resizes to the display target size (LANCZOS), re-encodes JPEG
//! (RGB/Gray at q78; CMYK has no `image`-crate encoder, so those are skipped —
//! a documented divergence, never exercised by the corpus), recompresses the
//! `/SMask` to FlateDecode zlib level 9, and commits only when the encoded
//! output is strictly smaller. The `/Filter` is preserved because the encoded
//! stream is written with `write_raw_stream_buffer`.

use std::collections::HashMap;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageBuffer};
use mupdf::pdf::{ExtractedImage, PdfDocument, PdfObject};
use mupdf::{Buffer, Error, Image, Pixmap};

use rendering_core::source_cleanup::content_stream::tokenize;
use rendering_core::source_cleanup::pdf_math::{transform_rect, Operand};
use rendering_core::source_cleanup::stream_state::ContentStreamState;

use crate::contents::{page_contents_bytes, resolve};

pub const IMAGE_RECOMPRESS_MIN_BYTES: usize = 20_000;
pub const IMAGE_JPEG_QUALITY: u8 = 78;

/// Per-image recompression outcome, mirroring production's `changed` flag and
/// `skipped_*` counters.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct ImageCompressResult {
    pub changed: bool,
    pub recompressed_xrefs: Vec<i32>,
    pub skipped_small: usize,
    pub skipped_special: usize,
    pub skipped_alpha: usize,
    pub skipped_not_better: usize,
    pub skipped_missing: usize,
    pub skipped_broken: usize,
}

/// Recompress every displayed DCTDecode image in `doc` to its display target
/// size at `dpi` (production `compress_pdf_images_only_impl`). Each image is
/// committed only when its encoded output is strictly smaller than the
/// original. Returns the recompression outcome; the caller still decides the
/// final file-level save.
pub fn compress_images(doc: &mut PdfDocument, dpi: i32) -> Result<ImageCompressResult, Error> {
    let mut result = ImageCompressResult::default();
    if dpi <= 0 {
        return Ok(result);
    }
    let display_rects = max_display_rect_by_xref(doc)?;
    if display_rects.is_empty() {
        return Ok(result);
    }
    let mut xrefs: Vec<i32> = display_rects.keys().copied().collect();
    xrefs.sort_unstable();
    for xref in xrefs {
        let (w_pt, h_pt) = display_rects[&xref];
        let (target_w, target_h) = target_pixel_size(w_pt, h_pt, dpi);
        if target_w <= 0 || target_h <= 0 {
            continue;
        }
        let extracted = match doc.extract_image(xref) {
            Ok(extracted) => extracted,
            Err(_) => {
                result.skipped_missing += 1;
                continue;
            }
        };
        if extracted.encoded.is_empty() || extracted.encoded.len() < IMAGE_RECOMPRESS_MIN_BYTES {
            result.skipped_small += 1;
            continue;
        }
        let mut obj = doc.new_indirect(xref, 0)?;
        let original_encoded_len =
            obj.read_raw_stream().map(|b| b.len()).unwrap_or(extracted.encoded.len());
        let mut smask = match obj.get_dict("SMask")? {
            Some(s) if !resolve(&s)?.is_null()? => Some(resolve(&s)?),
            _ => None,
        };
        let original_smask_len = match &smask {
            Some(s) => s.read_raw_stream().map(|b| b.len()).unwrap_or(0),
            None => 0,
        };

        let (skip, _reason) = should_skip_recompress_image(&obj, &extracted)?;
        if skip {
            result.skipped_special += 1;
            continue;
        }

        // Decode + resize + encode (mirrors load_pdf_image -> resize_to_target
        // -> encode_image). Decode failures count as skipped_missing, encode
        // failures as skipped_broken.
        let (resized, new_w, new_h) = match decode_resize_encode(&extracted, target_w, target_h) {
            Ok(out) => out,
            Err(DecodeEncodeError::Missing) => {
                result.skipped_missing += 1;
                continue;
            }
            Err(DecodeEncodeError::Broken) => {
                result.skipped_broken += 1;
                continue;
            }
            Err(DecodeEncodeError::Unsupported) => {
                result.skipped_alpha += 1;
                continue;
            }
        };

        let mut encoded_smask = Vec::new();
        if let Some(smask_obj) = &smask {
            match encode_soft_mask(smask_obj, new_w, new_h) {
                Ok(bytes) => encoded_smask = bytes,
                Err(DecodeEncodeError::Broken) | Err(DecodeEncodeError::Missing) => {
                    result.skipped_broken += 1;
                    continue;
                }
                Err(DecodeEncodeError::Unsupported) => {
                    result.skipped_alpha += 1;
                    continue;
                }
            }
        }

        let original_total_len = original_encoded_len + original_smask_len;
        let encoded_total_len = resized.0.len() + encoded_smask.len();
        if encoded_total_len >= original_total_len {
            result.skipped_not_better += 1;
            continue;
        }

        commit_image(
            doc,
            &mut obj,
            smask.as_mut(),
            &resized,
            new_w,
            new_h,
            &encoded_smask,
        )?;
        result.recompressed_xrefs.push(xref);
        result.changed = true;
    }
    Ok(result)
}

enum DecodeEncodeError {
    Missing,
    Broken,
    Unsupported,
}

/// Decode the DCTDecode stream, resize to the display target (LANCZOS, no-op
/// when already at/below target), and JPEG-encode. Returns the encoded bytes,
/// the resized dimensions, and the target `/ColorSpace` name.
fn decode_resize_encode(
    extracted: &ExtractedImage,
    target_w: i64,
    target_h: i64,
) -> Result<((Vec<u8>, String), i64, i64), DecodeEncodeError> {
    let image = Image::from_bytes(&extracted.encoded).map_err(|_| DecodeEncodeError::Missing)?;
    let pixmap = image.to_pixmap().map_err(|_| DecodeEncodeError::Missing)?;
    let (dynamic, new_w, new_h) = resize_to_target(&pixmap, target_w, target_h)
        .map_err(|_| DecodeEncodeError::Broken)?;
    let encoded = encode_image(&dynamic).map_err(|_| DecodeEncodeError::Broken)?;
    encoded
        .map(|(bytes, cs)| ((bytes, cs), new_w, new_h))
        .ok_or(DecodeEncodeError::Unsupported)
}

/// `resize_to_target` — LANCZOS resize, unchanged when the image already fits.
/// `new_w`/`new_h` use Python `round` (round-half-even).
fn resize_to_target(
    pixmap: &Pixmap,
    target_w: i64,
    target_h: i64,
) -> Result<(DynamicImage, i64, i64), Error> {
    let cw = pixmap.width() as i64;
    let ch = pixmap.height() as i64;
    let (new_w, new_h) = if cw <= target_w && ch <= target_h {
        (cw, ch)
    } else {
        let scale = (target_w as f64 / (cw.max(1) as f64))
            .min(target_h as f64 / (ch.max(1) as f64));
        (py_round(cw as f64 * scale).max(1), py_round(ch as f64 * scale).max(1))
    };
    let dynamic = pixmap_to_dynamic(pixmap)?;
    if new_w == cw && new_h == ch {
        Ok((dynamic, new_w, new_h))
    } else {
        Ok((
            dynamic.resize(new_w as u32, new_h as u32, FilterType::Lanczos3),
            new_w,
            new_h,
        ))
    }
}

/// Convert a mupdf pixmap (n=1 gray, n=3 RGB) to an `image` DynamicImage.
fn pixmap_to_dynamic(pixmap: &Pixmap) -> Result<DynamicImage, Error> {
    let w = pixmap.width();
    let h = pixmap.height();
    let samples = pixmap.samples().to_vec();
    match pixmap.n() {
        1 => Ok(DynamicImage::ImageLuma8(
            ImageBuffer::from_raw(w, h, samples)
                .ok_or_else(|| Error::InvalidArgument("pixmap gray dims".into()))?,
        )),
        3 => Ok(DynamicImage::ImageRgb8(
            ImageBuffer::from_raw(w, h, samples)
                .ok_or_else(|| Error::InvalidArgument("pixmap rgb dims".into()))?,
        )),
        n => Err(Error::InvalidArgument(format!("unsupported pixmap components {n}"))),
    }
}

/// `encode_image` — JPEG q78 for RGB/Gray; returns `None` for anything the
/// `image` crate cannot encode (CMYK, alpha-bearing).
fn encode_image(dynamic: &DynamicImage) -> Result<Option<(Vec<u8>, String)>, Error> {
    let (w, h) = (dynamic.width(), dynamic.height());
    let (bytes, color_type, cs) = match dynamic {
        DynamicImage::ImageLuma8(_) => {
            (dynamic.as_bytes(), ExtendedColorType::L8, "DeviceGray".to_string())
        }
        DynamicImage::ImageRgb8(_) => {
            (dynamic.as_bytes(), ExtendedColorType::Rgb8, "DeviceRGB".to_string())
        }
        _ => return Ok(None),
    };
    let mut out = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut out, IMAGE_JPEG_QUALITY);
    encoder
        .encode(bytes, w, h, color_type)
        .map_err(|e| Error::InvalidArgument(format!("jpeg encode: {e}")))?;
    Ok(Some((out, cs)))
}

/// `encode_soft_mask` — decode the SMask stream, resize to the main image's
/// target dims, flatten to gray, and zlib-compress at level 9 (FlateDecode).
fn encode_soft_mask(
    smask_obj: &PdfObject,
    new_w: i64,
    new_h: i64,
) -> Result<Vec<u8>, DecodeEncodeError> {
    let raw = smask_obj
        .read_raw_stream()
        .map_err(|_| DecodeEncodeError::Missing)?;
    let image = Image::from_bytes(&raw).map_err(|_| DecodeEncodeError::Missing)?;
    let pixmap = image.to_pixmap().map_err(|_| DecodeEncodeError::Missing)?;
    let dynamic = pixmap_to_dynamic(&pixmap).map_err(|_| DecodeEncodeError::Broken)?;
    let gray = dynamic.to_luma8();
    let resized = DynamicImage::ImageLuma8(gray)
        .resize(new_w as u32, new_h as u32, FilterType::Lanczos3);
    let mut out = Vec::new();
    let mut compressor = flate2::write::ZlibEncoder::new(&mut out, flate2::Compression::new(9));
    std::io::Write::write_all(&mut compressor, resized.as_bytes())
        .map_err(|_| DecodeEncodeError::Broken)?;
    compressor.finish().map_err(|_| DecodeEncodeError::Broken)?;
    Ok(out)
}

/// Commit a recompressed image in place (production's `obj.write(...)` +
/// dict updates + `/SMask` rewrite + `/Mask`/`/Decode`/`/DecodeParms` removal).
fn commit_image(
    doc: &mut PdfDocument,
    obj: &mut PdfObject,
    smask: Option<&mut PdfObject>,
    encoded: &(Vec<u8>, String),
    new_w: i64,
    new_h: i64,
    encoded_smask: &[u8],
) -> Result<(), Error> {
    let (encoded_bytes, colorspace) = encoded;
    obj.write_raw_stream_buffer(&Buffer::from_bytes(encoded_bytes)?)?;
    obj.dict_put("Filter", doc.new_name("DCTDecode")?)?;
    obj.dict_put("Width", doc.new_int(new_w as i32)?)?;
    obj.dict_put("Height", doc.new_int(new_h as i32)?)?;
    obj.dict_put("BitsPerComponent", doc.new_int(8)?)?;
    obj.dict_put("ColorSpace", doc.new_name(colorspace)?)?;

    if let Some(smask_obj) = smask {
        smask_obj.write_raw_stream_buffer(&Buffer::from_bytes(encoded_smask)?)?;
        smask_obj.dict_put("Filter", doc.new_name("FlateDecode")?)?;
        smask_obj.dict_put("Width", doc.new_int(new_w as i32)?)?;
        smask_obj.dict_put("Height", doc.new_int(new_h as i32)?)?;
        smask_obj.dict_put("BitsPerComponent", doc.new_int(8)?)?;
        smask_obj.dict_put("ColorSpace", doc.new_name("DeviceGray")?)?;
        for key in ["Decode", "DecodeParms"] {
            smask_obj.dict_delete(key)?;
        }
    }
    for key in ["Mask", "Decode", "DecodeParms"] {
        obj.dict_delete(key)?;
    }
    Ok(())
}

/// `analysis.py::max_display_rect_by_xref` — max displayed width/height per
/// image xref. Display rects come from the content-stream CTM at each `Do`
/// (the unit-square transform), matching fitz `get_image_rects` dimensions.
fn max_display_rect_by_xref(doc: &PdfDocument) -> Result<HashMap<i32, (f64, f64)>, Error> {
    let mut max_rects: HashMap<i32, (f64, f64)> = HashMap::new();
    let count = doc.page_count()?;
    for page_idx in 0..count {
        let page = doc.load_pdf_page(page_idx)?;
        let images = page.images()?;
        if images.is_empty() {
            continue;
        }
        let name_to_xref: HashMap<String, i32> =
            images.iter().map(|i| (i.name.clone(), i.xref)).collect();
        let stream = page_contents_bytes(&page)?;
        let tokens = match tokenize(&stream) {
            Ok(tokens) => tokens,
            Err(_) => continue,
        };
        let mut state = ContentStreamState::default();
        for token in &tokens {
            let op = token.operator.as_str();
            if state.apply_state_operator(op, &token.operands) {
                continue;
            }
            if op == "Do" && !token.operands.is_empty() {
                let Operand::Name(name) = &token.operands[0] else {
                    continue;
                };
                let Some(&xref) = name_to_xref.get(name) else {
                    continue;
                };
                let rect = transform_rect(&state.ctm, &[0.0, 0.0, 1.0, 1.0]);
                let width = (rect[2] - rect[0]).max(0.0);
                let height = (rect[3] - rect[1]).max(0.0);
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                let entry = max_rects.entry(xref).or_insert((0.0, 0.0));
                entry.0 = entry.0.max(width);
                entry.1 = entry.1.max(height);
            }
        }
    }
    Ok(max_rects)
}

/// `image_ops.py::should_skip_recompress_image` — the six skip rules plus the
/// JPEG/JPX/decode guards. `info.color_space` is the fallback when the object
/// carries no `/ColorSpace`.
fn should_skip_recompress_image(
    obj: &PdfObject,
    info: &ExtractedImage,
) -> Result<(bool, &'static str), Error> {
    let bits_per_component = info.bits_per_component.unwrap_or(0);
    let obj_colorspace = obj.get_dict("ColorSpace")?;
    let colorspace = match obj_colorspace {
        Some(cs) => cs.to_string(),
        None => info.color_space.clone().unwrap_or_default(),
    };
    let filters = info.filter.clone().unwrap_or_default();
    let decode_present = obj.get_dict("Decode")?.is_some();

    if pdf_bool(&obj.get_dict("ImageMask")?) {
        return Ok((true, "image-mask"));
    }
    if obj.get_dict("Mask")?.is_some() {
        return Ok((true, "mask"));
    }
    if bits_per_component == 1 {
        return Ok((true, "bitonal"));
    }
    if colorspace.is_empty() {
        return Ok((true, "missing-colorspace"));
    }
    if colorspace.starts_with('/') && !matches!(colorspace.as_str(), "/DeviceRGB" | "/DeviceCMYK" | "/DeviceGray") {
        return Ok((true, "non-device-colorspace"));
    }
    if colorspace.contains("Separation") || colorspace.contains("DeviceN") {
        return Ok((true, "complex-colorspace"));
    }
    if filters.contains("/JPXDecode") {
        return Ok((true, "jpxdecode"));
    }
    if decode_present {
        return Ok((true, "decode-array"));
    }
    Ok((false, ""))
}

/// `image_ops.py::pdf_bool` — PDF booleans are names "true"/"false".
fn pdf_bool(value: &Option<PdfObject>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let value = match resolve(value) {
        Ok(v) => v,
        Err(_) => value.clone(),
    };
    match value.as_name() {
        Ok(name) => name == b"true",
        Err(_) => false,
    }
}

/// `analysis.py::target_pixel_size` — `max(1, round(pt/72*dpi))` with Python
/// round (round-half-even).
fn target_pixel_size(width_pt: f64, height_pt: f64, dpi: i32) -> (i64, i64) {
    (
        py_round(width_pt / 72.0 * dpi as f64).max(1),
        py_round(height_pt / 72.0 * dpi as f64).max(1),
    )
}

/// Python `round(x)` — round-half-even (also `resize_to_target`'s size math).
fn py_round(x: f64) -> i64 {
    x.round_ties_even() as i64
}
