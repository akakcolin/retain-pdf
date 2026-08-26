//! Port of `source/background/image_route.py`: replace a large background image
//! by painting the item rects (projected into image space) with the image's
//! solid fill. Only the raw-stream (FlateDecode) branch is ported; the
//! full-decode `replace_image` + PIL re-encode branch is deferred (documented
//! divergence — such images leave the image untouched and return `false`).

use mupdf::pdf::{PdfDocument, PdfPage};
use mupdf::{Buffer, Error};

use rendering_core::source_cleanup::hit_test::RectTuple;

use super::config::DETECT_PRIMARY_COVERAGE_RATIO;
use super::detect::{page_has_large_background_image, pick_primary_background_image};
use super::extract::{extract_raw_stream_image, raw_stream_image_meta, SolidFill};
use super::patch::{map_rect_to_image, merge_close_vertical_rects};

/// `image_route.py::rewrite_raw_stream_image` — paint the item rects (projected
/// into image space) with `fill`, on a copy of the raw stream bytes. Mirrors
/// PIL `ImageDraw.rectangle` including 1-bit MSB-first packing (pixel x maps to
/// bit `7 - (x % 8)`).
pub fn rewrite_raw_stream_image(
    image: &[u8],
    image_rect: &RectTuple,
    image_size: (u32, u32),
    rects: &[RectTuple],
    fill: SolidFill,
) -> Vec<u8> {
    let (width, height) = (image_size.0 as usize, image_size.1 as usize);
    let mut updated = image.to_vec();
    for rect in rects {
        let Some((mapped_x0, mapped_y0, mapped_x1, mapped_y1)) =
            map_rect_to_image(image_rect, (width as i64, height as i64), rect)
        else {
            continue;
        };
        let (x0, y0, x1, y1) = (
            mapped_x0 as usize,
            mapped_y0 as usize,
            mapped_x1 as usize,
            mapped_y1 as usize,
        );
        match fill {
            SolidFill::One => {
                // PIL mode "1" packs bits MSB-first with each row padded to a
                // byte boundary: `ceil(width / 8)` bytes per row.
                let row_bytes = (width + 7) / 8;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let bit = 7 - (x % 8);
                        updated[y * row_bytes + x / 8] |= 1 << bit;
                    }
                }
            }
            SolidFill::Byte(value) => {
                for y in y0..y1 {
                    let row = y * width;
                    for x in x0..x1 {
                        updated[row + x] = value;
                    }
                }
            }
            SolidFill::Rgb(rgb) => {
                for y in y0..y1 {
                    let row = y * width * 3;
                    for x in x0..x1 {
                        let base = row + x * 3;
                        updated[base] = rgb[0];
                        updated[base + 1] = rgb[1];
                        updated[base + 2] = rgb[2];
                    }
                }
            }
        }
    }
    updated
}

/// Deflate-compress raw stream bytes (RFC 1950 zlib), the same encoding fitz
/// `Document.update_stream(..., compress=1)` produces for a FlateDecode stream.
fn flate_compress(raw: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, raw).expect("flate write");
    encoder.finish().expect("flate finish")
}

/// `image_route.py::replace_background_image_page` — raw-stream branch. Returns
/// `true` when the primary background image was rewritten (its stream replaced
/// with the solid-fill-painted, recompressed bytes). `item_bboxes` are the
/// translated-item bboxes in page space.
pub fn replace_background_image_page(
    page: &PdfPage,
    doc: &mut PdfDocument,
    page_rect: &RectTuple,
    item_bboxes: &[RectTuple],
) -> Result<bool, Error> {
    if !page_has_large_background_image(page, page_rect)? {
        return Ok(false);
    }
    let Some((xref, image_rect)) =
        pick_primary_background_image(page, page_rect, DETECT_PRIMARY_COVERAGE_RATIO)?
    else {
        return Ok(false);
    };
    let rects = merge_close_vertical_rects(item_bboxes);
    if rects.is_empty() {
        return Ok(false);
    }
    let Some(meta) = raw_stream_image_meta(doc, xref)? else {
        return Ok(false);
    };
    let Some(raw_image) = extract_raw_stream_image(doc, xref, &meta)? else {
        return Ok(false);
    };
    let rebuilt = rewrite_raw_stream_image(
        &raw_image,
        &image_rect,
        (meta.width, meta.height),
        &rects,
        meta.fill,
    );
    let encoded = flate_compress(&rebuilt);
    let mut obj = doc.new_indirect(xref, 0)?;
    obj.write_raw_stream_buffer(&Buffer::from_bytes(&encoded)?)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> RectTuple {
        [x0, y0, x1, y1]
    }

    #[test]
    fn rgb_rewrite_fills_pixels_white() {
        // 4x3 RGB image all 200; cover rect maps to pixels x=1..2 across all rows
        let image = vec![200u8; 4 * 3 * 3];
        let out = rewrite_raw_stream_image(
            &image,
            &rect(0.0, 0.0, 100.0, 100.0),
            (4, 3),
            &[rect(25.0, 33.0, 75.0, 100.0)],
            SolidFill::Rgb([255, 255, 255]),
        );
        // mapped = (int(25*0.04), int(33*0.03), int(75*0.04), int(100*0.03))
        //         = (1, 0, 3, 3)
        for y in 0..3 {
            for x in 0..4 {
                let base = (y * 4 + x) * 3;
                let inside = x >= 1 && x < 3;
                let expected = if inside { 255u8 } else { 200u8 };
                assert_eq!(out[base], expected, "r({x},{y})");
                assert_eq!(out[base + 1], expected, "g({x},{y})");
                assert_eq!(out[base + 2], expected, "b({x},{y})");
            }
        }
    }

    #[test]
    fn gray_rewrite_fills_255() {
        // 8x2 gray; cover rect covering all pixels
        let image = vec![10u8; 8 * 2];
        let out = rewrite_raw_stream_image(
            &image,
            &rect(0.0, 0.0, 80.0, 20.0),
            (8, 2),
            &[rect(0.0, 0.0, 80.0, 20.0)],
            SolidFill::Byte(255),
        );
        assert!(out.iter().all(|&b| b == 255));
    }

    #[test]
    fn onebit_rewrite_sets_bits_msb_first() {
        // 16x4 mask all zero. Covering pixels x=0..1 sets bits 7..6 (0xC0);
        // covering pixels x=14..15 sets bits 1..0 (0x03).
        let mut image = vec![0u8; 16 * 4 / 8];
        let out = rewrite_raw_stream_image(
            &image,
            &rect(0.0, 0.0, 16.0, 4.0),
            (16, 4),
            &[rect(0.0, 0.0, 2.0, 2.0)],
            SolidFill::One,
        );
        // first byte = 0xC0; second byte untouched (first row pixels 8..15)
        assert_eq!(out[0], 0xC0);
        assert_eq!(out[1], 0x00);
        image = out;
        let out = rewrite_raw_stream_image(
            &image,
            &rect(0.0, 0.0, 16.0, 4.0),
            (16, 4),
            &[rect(14.0, 2.0, 16.0, 4.0)],
            SolidFill::One,
        );
        // row y=2..3, pixels 14..15 -> byte index 5 (row2*2+1) and 7 (row3*2+1)
        assert_eq!(out[5], 0x03);
        assert_eq!(out[7], 0x03);
    }

    #[test]
    fn onebit_rewrite_row_pads_odd_width() {
        // 9x4 mask, all zero. Each row is ceil(9/8) = 2 bytes. Covering all 9
        // columns sets byte0 = 0xFF (bits 7..0) and byte1 = 0x80 (x=8 -> bit 0).
        let image = vec![0u8; 8];
        let out = rewrite_raw_stream_image(
            &image,
            &rect(0.0, 0.0, 9.0, 4.0),
            (9, 4),
            &[rect(0.0, 0.0, 9.0, 4.0)],
            SolidFill::One,
        );
        assert_eq!(out.len(), 8, "row-padded byte count");
        for y in 0..4 {
            assert_eq!(out[y * 2], 0xFF, "row {y} first byte (bits 7..0)");
            assert_eq!(out[y * 2 + 1], 0x80, "row {y} second byte (x=8 -> bit 0)");
        }
    }

    #[test]
    fn mapped_rect_off_image_ignored() {
        let image = vec![200u8; 4 * 3 * 3];
        let out = rewrite_raw_stream_image(
            &image,
            &rect(0.0, 0.0, 100.0, 100.0),
            (4, 3),
            &[rect(200.0, 200.0, 300.0, 300.0)],
            SolidFill::Rgb([255, 255, 255]),
        );
        assert_eq!(out, image);
    }
}
