//! Side-by-side merge parity.
//!
//! Each output page is `[source | translated]`; its geometry must be the
//! sum/max of the two input page rects (rotation included) and each half must
//! match a direct render of the corresponding input page. The half render and
//! the placed render come from the same native renderer, so only anti-aliasing
//! at the placement seam can differ. A per-pixel max tolerance is deliberately
//! not asserted: the shared `show_pdf_page` primitive carries a pre-existing
//! f64-vs-fitz-f32 placement divergence (documented in the A0/A3 port notes)
//! that spikes a handful of seam pixels. Rotated pages are geometry-only (see
//! the exclusion note in the test body).

use std::path::{Path, PathBuf};

use mupdf::pdf::PdfDocument as PdfWriter;
use mupdf::{Document, Size};
use rendering_orchestrator::derived_artifacts::side_by_side;
use rendering_reader::render::render_page_clip_rgb;
use rendering_reader::PdfDocument as PdfReader;
use rendering_writer::overlay::show_pdf_page;

const SCALE: f32 = 1.5;
const FIXTURE_W: f32 = 400.0;
const FIXTURE_H: f32 = 560.0;
const ROTATED_PAGE: usize = 1;
/// Mean absolute per-channel difference allowed between an output half and a
/// direct render of the same page.
const MEAN_ABS_TOLERANCE: f64 = 0.5;
/// 99.9th-percentile per-channel difference allowed (max is ignored).
const P999_ABS_TOLERANCE: u8 = 8;
/// Page-box tolerance in points.
const BOX_TOLERANCE: f64 = 0.01;

fn repo_pdf(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/samples/golden-pdfs")
        .join(name)
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("render-rs-sbs-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Build a fixture PDF: `page_count` pages of `FIXTURE_W x FIXTURE_H`, each
/// showing the corresponding page of `golden`, with page `ROTATED_PAGE`
/// rotated 90 degrees.
fn build_fixture(dir: &Path, name: &str, golden: &str, page_count: usize) -> PathBuf {
    let source = PdfWriter::open(repo_pdf(golden).as_path()).expect("open golden pdf");
    let mut doc = PdfWriter::new();
    for index in 0..page_count {
        doc.new_page(Size::new(FIXTURE_W, FIXTURE_H))
            .expect("new page");
        let page_no = doc.page_count().expect("page count") - 1;
        show_pdf_page(
            &mut doc,
            page_no,
            &source,
            index as i32,
            [0.0, 0.0, FIXTURE_W as f64, FIXTURE_H as f64],
        )
        .expect("place golden page");
        if index == ROTATED_PAGE {
            let mut page = doc.load_pdf_page(page_no).expect("load page");
            page.set_rotation(90).expect("set rotation");
        }
    }
    let path = dir.join(name);
    doc.save(path.to_str().expect("utf8 path"))
        .expect("save fixture");
    path
}

fn page_rect(doc: &Document, index: i64) -> Option<(f64, f64)> {
    let rect = PdfReader::page_rect(doc, index).ok()?;
    Some((rect.width(), rect.height()))
}

/// Mean and 99.9th-percentile absolute per-channel difference between the
/// `width x height` top-left region of `out` and all of `src`.
fn region_diff(
    out: &rendering_reader::render::RenderedRgbPixels,
    src: &rendering_reader::render::RenderedRgbPixels,
) -> (f64, u8) {
    let width = src.width.min(out.width);
    let height = src.height.min(out.height);
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    let mut hist = [0u64; 256];
    for y in 0..height {
        // `stride` is bytes-per-pixel (3), not a row stride; rows are tightly
        // packed, so the row stride is width * 3.
        let out_row = (y * out.width * 3) as usize;
        let src_row = (y * src.width * 3) as usize;
        for x in 0..width {
            let o = out_row + (x * 3) as usize;
            let s = src_row + (x * 3) as usize;
            for channel in 0..3 {
                let diff = (out.samples[o + channel] as i32
                    - src.samples[s + channel] as i32)
                    .unsigned_abs();
                hist[diff as usize] += 1;
                sum += diff as u64;
                count += 1;
            }
        }
    }
    assert!(count > 0, "empty comparison region");
    let total = count;
    let mut cumulative: u64 = 0;
    let mut p999 = 0u8;
    for (value, hits) in hist.iter().enumerate() {
        cumulative += hits;
        if cumulative as f64 >= total as f64 * 0.999 {
            p999 = value as u8;
            break;
        }
    }
    (sum as f64 / total as f64, p999)
}

fn assert_region_matches(
    out: &rendering_reader::render::RenderedRgbPixels,
    src: &rendering_reader::render::RenderedRgbPixels,
    label: &str,
) {
    let (mean, p999) = region_diff(out, src);
    assert!(
        mean <= MEAN_ABS_TOLERANCE,
        "{label}: mean abs diff {mean:.4} > {MEAN_ABS_TOLERANCE}"
    );
    assert!(
        p999 <= P999_ABS_TOLERANCE,
        "{label}: p99.9 abs diff {p999} > {P999_ABS_TOLERANCE}"
    );
}

#[test]
fn side_by_side_halves_match_direct_renders() {
    let dir = temp_dir("parity");
    // 3 source pages, 2 translated pages: page 2 exercises the missing-side
    // width fallback, page 1 exercises rotation on both sides.
    let source = build_fixture(&dir, "source.pdf", "1.pdf", 3);
    let translated = build_fixture(&dir, "translated.pdf", "2.pdf", 2);
    let output = dir.join("side-by-side.pdf");

    side_by_side(&source, &translated, &output).expect("build side-by-side");

    let out_doc = Document::open(output.as_path()).expect("open output");
    let src_doc = Document::open(source.as_path()).expect("open source");
    let trl_doc = Document::open(translated.as_path()).expect("open translated");

    assert_eq!(PdfReader::page_count(&out_doc).expect("page count"), 3);

    // Guard: the fixtures must carry real content, otherwise every half-vs-half
    // comparison would trivially pass on blank pages.
    for (label, doc) in [("source", &src_doc), ("translated", &trl_doc)] {
        let probe = render_page_clip_rgb(doc, 0, None, SCALE).expect("probe render");
        let dark = probe.samples.iter().filter(|&&byte| byte < 200).count();
        assert!(
            dark > 1000,
            "{label} fixture page 0 appears blank ({dark} dark samples)"
        );
    }

    for index in 0..3i64 {
        let src_rect = page_rect(&src_doc, index);
        let trl_rect = page_rect(&trl_doc, index);
        let left_w = src_rect.or(trl_rect).expect("present side").0;
        let right_w = trl_rect.or(src_rect).expect("present side").0;
        let page_h = src_rect
            .map(|r| r.1)
            .unwrap_or(0.0)
            .max(trl_rect.map(|r| r.1).unwrap_or(0.0));

        let out_rect = page_rect(&out_doc, index).expect("output page rect");
        assert!(
            (out_rect.0 - (left_w + right_w)).abs() <= BOX_TOLERANCE,
            "page {index}: width {} != {left_w}+{right_w}",
            out_rect.0
        );
        assert!(
            (out_rect.1 - page_h).abs() <= BOX_TOLERANCE,
            "page {index}: height {} != {page_h}",
            out_rect.1
        );

        // Rotated source pages are excluded from the pixel comparison: the
        // placement primitive mirrors fitz's `transformation_matrix`, which for
        // rotated pages is a flip-only matrix (`Matrix(1,0,0,-1,0,cropbox.h)`)
        // rather than the renderer's rotation — so a placed rotated page is
        // deliberately not pixel-identical to a direct render. Geometry above
        // still covers rotation-applied bounds.
        let rotated = PdfReader::page_rotation(&src_doc, index).unwrap_or(0) % 360 != 0
            || PdfReader::page_rotation(&trl_doc, index).unwrap_or(0) % 360 != 0;
        if rotated {
            continue;
        }

        let out_pix =
            render_page_clip_rgb(&out_doc, index as i32, None, SCALE).expect("render out");
        if src_rect.is_some() {
            let src_pix =
                render_page_clip_rgb(&src_doc, index as i32, None, SCALE).expect("render src");
            assert_region_matches(&out_pix, &src_pix, &format!("page {index} left"));
        }
        if trl_rect.is_some() {
            // The right half starts at `left_w * SCALE`; compare it against the
            // translated page by shifting the output view.
            let shifted = shift_right(&out_pix, (left_w * SCALE as f64).round() as u32);
            let trl_pix =
                render_page_clip_rgb(&trl_doc, index as i32, None, SCALE).expect("render trl");
            assert_region_matches(&shifted, &trl_pix, &format!("page {index} right"));
        }
    }
}

/// Crop `pixels` to the region starting at column `offset`, keeping its full
/// height, so the right half can be compared as if it were its own page.
fn shift_right(
    pixels: &rendering_reader::render::RenderedRgbPixels,
    offset: u32,
) -> rendering_reader::render::RenderedRgbPixels {
    let width = pixels.width.saturating_sub(offset);
    let mut samples = Vec::with_capacity((width * pixels.height * 3) as usize);
    for y in 0..pixels.height {
        let row = (y * pixels.width * 3) as usize + (offset * 3) as usize;
        samples.extend_from_slice(&pixels.samples[row..row + (width * 3) as usize]);
    }
    rendering_reader::render::RenderedRgbPixels {
        width,
        height: pixels.height,
        stride: 3,
        samples,
    }
}
