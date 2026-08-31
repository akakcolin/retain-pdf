//! Overlay port of `document/pikepdf_overlay.py` (Phase 5C-5).
//!
//! Production overlays overlay-page `j` onto source-page `i` via
//! `source_page.add_overlay(overlay_page, rect=cropbox, push_stack=True,
//! shrink=False, expand=False)`. pikepdf's `_over_underlay` decomposes that into:
//!   1. convert the overlay page to a Form XObject (`as_form_xobject`): `/BBox` =
//!      trimbox-or-cropbox-or-mediabox, `/Matrix` = `getMatrixForTransformations()`
//!      (non-inverted, from the overlay page's `/Rotate` + `/UserUnit`), `/Resources`
//!      copied;
//!   2. `calc_form_xobject_placement` = qpdf `placeFormXObject`: a placement matrix
//!      `cm` computed from the destination page's trimbox-or-cropbox-or-mediabox,
//!      `/Rotate` + `/UserUnit` (inverted) and the form's `/BBox` + `/Matrix`,
//!      yielding the content `q\n<cm> cm\n<name> Do\nQ\n`;
//!   3. `contents_add(b"q\n", prepend=True)` + `contents_add(b"Q\n")` +
//!      `contents_add(cs)` + `contents_coalesce()` → final content
//!      `q\n` + old + `Q\n` + cs.
//!
//! The placement math is a faithful port of qpdf's
//! `QPDFPageObjectHelper::getMatrixForFormXObjectPlacement` (fetched at
//! /tmp/qpdf_pagehelper.cc). Note qpdf's `concat` composes as "apply this first,
//! then other", so every matrix product below is ordered accordingly.

use mupdf::pdf::{PdfDocument, PdfObject, PdfPage};
use mupdf::{Buffer, Error, Matrix, Rect};

use rendering_core::source_cleanup::pdf_math::{
    invert_matrix, mul_matrix, transform_rect, PdfMatrix, IDENTITY_MATRIX,
};

use crate::contents::{page_contents_bytes, replace_page_contents, resolve};

/// A Form XObject built from an overlay page, plus the geometry qpdf's
/// placement math consumes.
struct FormXObject {
    /// The form stream (an indirect reference in the destination document).
    obj: PdfObject,
    /// `/BBox` of the form.
    bbox: [f64; 4],
    /// `/Matrix` of the form (identity when the overlay page is unrotated).
    matrix: PdfMatrix,
}

/// Overlay page `overlay_page_idx` of `overlay` onto page `source_page_idx` of
/// `source`, mirroring production
/// `source_page.add_overlay(overlay_page, rect=cropbox, push_stack=True,
/// shrink=False, expand=False)`.
pub fn overlay_page(
    source: &mut PdfDocument,
    source_page_idx: i32,
    overlay: &PdfDocument,
    overlay_page_idx: i32,
) -> Result<(), Error> {
    let form = page_to_form_xobject(source, overlay, overlay_page_idx)?;
    let source_page = source.load_pdf_page(source_page_idx)?;

    // qpdf `getMatrixForTransformations(true)` uses the destination page's
    // trimbox-or-cropbox-or-mediabox plus /Rotate + /UserUnit.
    let trim_rect = get_trim_box(&source_page)?;
    let dest_rotate = page_int(&source_page, "Rotate")?;
    let dest_user_unit = page_float(&source_page, "UserUnit", 1.0)?;
    let tmatrix = get_matrix_for_transformations(trim_rect, dest_rotate, dest_user_unit, true);

    // Production rect = `_page_crop_rect(source_page)` (cropbox, else mediabox).
    let rect = crop_box(&source_page)?.unwrap_or([0.0, 0.0, 612.0, 792.0]);

    let cm = place_form_cm(&tmatrix, &form.matrix, &form.bbox, &rect)
        .ok_or_else(|| Error::InvalidArgument("overlay placement: degenerate form bbox".into()))?;

    let name = next_free_form_name(&source_page)?;
    let mut resources = resolve(&source_page.resources()?)?;
    let xobjects = match resources.get_dict("XObject")? {
        Some(x) => resolve(&x)?,
        None => {
            let x = source.add_object(&source.new_dict()?)?;
            resources.dict_put("XObject", x.clone())?;
            x
        }
    };
    if !xobjects.is_dict()? {
        return Err(Error::InvalidArgument("page /Resources/XObject is not a dict".into()));
    }
    let mut xobjects = xobjects;
    xobjects.dict_put(name.as_str(), form.obj)?;

    // push_stack: `q\n` + old + `Q\n` + `q\n<cm> cm\n/<name> Do\nQ\n`.
    let old = page_contents_bytes(&source_page)?;
    let cs = format!("q\n{} cm\n/{} Do\nQ\n", format_matrix(&cm), name);
    let mut new = Vec::with_capacity(old.len() + cs.len() + 4);
    new.extend_from_slice(b"q\n");
    new.extend_from_slice(&old);
    new.extend_from_slice(b"Q\n");
    new.extend_from_slice(cs.as_bytes());
    replace_page_contents(&source_page, source, &new)?;
    Ok(())
}

/// Place `source_page_idx` of `source` onto `target_page_idx` of `target` at
/// `rect` (fitz display coordinates), mirroring PyMuPDF
/// `target_page.show_pdf_page(rect, source, source_page_idx, overlay=True)`.
///
/// Uses MuPDF's two-form mechanism (pixel-verified against fitz 1.26.5): a
/// "fullpage" Form XObject carrying the source content + grafted resources,
/// wrapped by a second Form XObject whose `/Matrix` is `calc_matrix`. The
/// wrapper `/Matrix` must be written with fitz's "%.5f-strip" number format,
/// otherwise anti-aliased edges shift by one pixel.
pub fn show_pdf_page(
    target: &mut PdfDocument,
    target_page_idx: i32,
    source: &PdfDocument,
    source_page_idx: i32,
    rect: [f64; 4],
) -> Result<(), Error> {
    let target_page = target.load_pdf_page(target_page_idx)?;
    let source_page = source.load_pdf_page(source_page_idx)?;

    // Geometry in PDF user space (undo each page's fitz transform). Mirrors
    // PyMuPDF `Page.transformation_matrix`: for rotated pages PyMuPDF overrides
    // the ctm with a flip-only matrix `Matrix(1,0,0,-1,0,cropbox.height)`.
    let src_ctm = page_transformation_matrix(&source_page)?;
    let src_bounds = rect_to_f64(source_page.bounds()?);
    let src_rect = transform_rect(
        &invert_matrix(&src_ctm).unwrap_or(IDENTITY_MATRIX),
        &src_bounds,
    );
    let tgt_ctm = page_transformation_matrix(&target_page)?;
    let tar_rect = transform_rect(&invert_matrix(&tgt_ctm).unwrap_or(IDENTITY_MATRIX), &rect);
    let cm = calc_matrix(&src_rect, &tar_rect);

    // fullpage form: raw source content, BBox = MediaBox, identity matrix,
    // grafted source resources.
    let content = page_contents_bytes(&source_page)?;
    let media = match source_page.media_box() {
        Ok(mb) if !mb.is_empty() => rect_to_f64(mb),
        _ => crop_box(&source_page)?.unwrap_or([0.0, 0.0, 612.0, 792.0]),
    };
    let mut full = target.add_stream(&Buffer::from_bytes(&content)?, None, false)?;
    full.dict_put("Type", target.new_name("XObject")?)?;
    full.dict_put("Subtype", target.new_name("Form")?)?;
    full.dict_put("BBox", rect_array5(target, &media)?)?;
    full.dict_put("Matrix", matrix_array5(target, &IDENTITY_MATRIX)?)?;
    let resources = match source_page.object().get_dict("Resources")? {
        Some(r) => r,
        None => source_page.resources()?,
    };
    full.dict_put("Resources", target.graft_object(&resources)?)?;

    // wrapper form: ` /fullpage Do ` clipped to src_rect, mapped by calc_matrix.
    let mut wrapper = target.add_stream(&Buffer::from_bytes(b"/fullpage Do")?, None, false)?;
    wrapper.dict_put("Type", target.new_name("XObject")?)?;
    wrapper.dict_put("Subtype", target.new_name("Form")?)?;
    wrapper.dict_put("BBox", rect_array5(target, &src_rect)?)?;
    wrapper.dict_put("Matrix", matrix_array5(target, &cm)?)?;
    let mut inner = target.new_dict()?;
    inner.dict_put("fullpage", full)?;
    let mut wrapper_res = target.new_dict()?;
    wrapper_res.dict_put("XObject", inner)?;
    wrapper.dict_put("Resources", wrapper_res)?;

    // Register the wrapper in the page /Resources/XObject.
    let name = next_free_form_name(&target_page)?;
    let mut resources = resolve(&target_page.resources()?)?;
    let xobjects = match resources.get_dict("XObject")? {
        Some(x) => resolve(&x)?,
        None => {
            let x = target.add_object(&target.new_dict()?)?;
            resources.dict_put("XObject", x.clone())?;
            x
        }
    };
    if !xobjects.is_dict()? {
        return Err(Error::InvalidArgument(
            "page /Resources/XObject is not a dict".into(),
        ));
    }
    let mut xobjects = xobjects;
    xobjects.dict_put(name.as_str(), wrapper)?;

    // Wrap existing content, then append the placement (overlay semantics).
    let old = page_contents_bytes(&target_page)?;
    let mut new = Vec::with_capacity(old.len() + name.len() + 16);
    new.extend_from_slice(b"q\n");
    new.extend_from_slice(&old);
    new.extend_from_slice(b"\nQ\n q /");
    new.extend_from_slice(name.as_bytes());
    new.extend_from_slice(b" Do Q ");
    replace_page_contents(&target_page, target, &new)?;
    Ok(())
}

/// Build the dual-book doc: each page = source page (left) + translated page
/// (right), mirroring `book_support.build_dual_doc_pages`. Used by the render
/// orchestrator's dual stage.
pub fn build_dual_doc_pages(
    source: &PdfDocument,
    translated: &PdfDocument,
    start_page: i32,
    end_page: i32,
) -> Result<PdfDocument, Error> {
    let mut dual = PdfDocument::new();
    let last_page = source.page_count()? - 1;
    let start = start_page.max(0);
    let end = if end_page < 0 {
        last_page
    } else {
        end_page.min(last_page)
    };
    for page_idx in start..=end {
        let src_page = source.load_pdf_page(page_idx)?;
        let trl_page = translated.load_pdf_page(page_idx)?;
        let src_bounds = src_page.bounds()?;
        let trl_bounds = trl_page.bounds()?;
        let src_w = src_bounds.width();
        let src_h = src_bounds.height();
        let trl_w = trl_bounds.width();
        let trl_h = trl_bounds.height();
        let page_w = src_w + trl_w;
        let page_h = src_h.max(trl_h);
        dual.new_page(mupdf::Size::new(page_w, page_h))?;
        let page_no = dual.page_count()? - 1;
        show_pdf_page(
            &mut dual,
            page_no,
            source,
            page_idx,
            [0.0, 0.0, src_w as f64, src_h as f64],
        )?;
        show_pdf_page(
            &mut dual,
            page_no,
            translated,
            page_idx,
            [src_w as f64, 0.0, (src_w + trl_w) as f64, trl_h as f64],
        )?;
    }
    Ok(dual)
}

/// Convert overlay page `overlay_page_idx` of `overlay` into a Form XObject
/// created in `source`, mirroring pikepdf `Page.as_form_xobject()` +
/// `add_resource(..., Name.XObject)`.
fn page_to_form_xobject(
    source: &mut PdfDocument,
    overlay: &PdfDocument,
    overlay_page_idx: i32,
) -> Result<FormXObject, Error> {
    let overlay_page = overlay.load_pdf_page(overlay_page_idx)?;
    let content = page_contents_bytes(&overlay_page)?;
    let bbox = get_trim_box(&overlay_page)?.unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let rotate = page_int(&overlay_page, "Rotate")?;
    let user_unit = page_float(&overlay_page, "UserUnit", 1.0)?;
    let matrix = get_matrix_for_transformations(Some(bbox), rotate, user_unit, false);

    let mut stream = source.add_stream(&Buffer::from_bytes(&content)?, None, false)?;
    stream.dict_put("Type", source.new_name("XObject")?)?;
    stream.dict_put("Subtype", source.new_name("Form")?)?;
    stream.dict_put("BBox", rect_array(source, &bbox)?)?;
    if matrix != IDENTITY_MATRIX {
        stream.dict_put("Matrix", matrix_array(source, &matrix)?)?;
    }
    let resources = match overlay_page.object().get_dict("Resources")? {
        Some(r) => r,
        None => overlay_page.resources()?,
    };
    let resources = source.graft_object(&resources)?;
    stream.dict_put("Resources", resources)?;

    Ok(FormXObject { obj: stream, bbox, matrix })
}

/// Port of qpdf `getMatrixForTransformations(invert)`.
///
/// `trim_rect` is the page trimbox-or-cropbox-or-mediabox; returns identity when
/// the page has neither `/Rotate` nor a non-default `/UserUnit` (qpdf's
/// `rotate_obj.null() && scale_obj.null()` guard). `invert` is true when the
/// matrix is for the destination page (undoing its rotations/scale).
fn get_matrix_for_transformations(
    trim_rect: Option<[f64; 4]>,
    rotate: i32,
    user_unit: f64,
    invert: bool,
) -> PdfMatrix {
    let Some(rect) = trim_rect else {
        return IDENTITY_MATRIX;
    };
    if rotate == 0 && user_unit == 1.0 {
        return IDENTITY_MATRIX;
    }
    let width = rect[2] - rect[0];
    let height = rect[3] - rect[1];
    let mut scale = if user_unit != 0.0 { user_unit } else { 1.0 };
    let mut rot = rotate;
    if invert {
        if scale == 0.0 {
            return IDENTITY_MATRIX;
        }
        scale = 1.0 / scale;
        rot = 360 - rot;
    }
    rot = rot.rem_euclid(360);
    PdfMatrix(match rot {
        90 => [0.0, -scale, scale, 0.0, 0.0, width * scale],
        180 => [-scale, 0.0, 0.0, -scale, width * scale, height * scale],
        270 => [0.0, scale, -scale, 0.0, height * scale, 0.0],
        _ => [scale, 0.0, 0.0, scale, 0.0, 0.0],
    })
}

/// Port of qpdf `getMatrixForFormXObjectPlacement` with
/// `invert_transformations=true, allow_shrink=false, allow_expand=false`.
///
/// `rect` is the destination rectangle (source cropbox); `form_bbox` the form
/// `/BBox`; `tmatrix` the inverted destination transformations; `fmatrix` the
/// form `/Matrix`. Returns `None` for a degenerate bounding box (qpdf returns an
/// empty matrix there).
fn place_form_cm(
    tmatrix: &PdfMatrix,
    fmatrix: &PdfMatrix,
    form_bbox: &[f64; 4],
    rect: &[f64; 4],
) -> Option<PdfMatrix> {
    // Step 1: wmatrix = fmatrix · tmatrix; scale to fit, clamped to 1.0 because
    // shrink and expand are both disabled.
    let wmatrix = mul_matrix(fmatrix, tmatrix);
    let t_rect = transform_rect(&wmatrix, form_bbox);
    if t_rect[2] == t_rect[0] || t_rect[3] == t_rect[1] {
        return None;
    }
    // scale = min(rect_w/t_w, rect_h/t_h), but allow_expand=false and
    // allow_shrink=false both clamp it to 1.0.
    let scale = 1.0;

    // Step 2: center the scaled form in the destination rectangle.
    // wmatrix2 = fmatrix · tmatrix · scale.
    let wmatrix2 = mul_matrix(fmatrix, &mul_matrix(tmatrix, &scale_matrix(scale)));
    let t2 = transform_rect(&wmatrix2, form_bbox);
    let t_cx = (t2[0] + t2[2]) / 2.0;
    let t_cy = (t2[1] + t2[3]) / 2.0;
    let r_cx = (rect[0] + rect[2]) / 2.0;
    let r_cy = (rect[1] + rect[3]) / 2.0;
    let tx = r_cx - t_cx;
    let ty = r_cy - t_cy;

    // cm = tmatrix · scale · translate(tx, ty).
    let cm = mul_matrix(
        tmatrix,
        &mul_matrix(&scale_matrix(scale), &translate_matrix(tx, ty)),
    );
    Some(cm)
}

fn scale_matrix(s: f64) -> PdfMatrix {
    PdfMatrix([s, 0.0, 0.0, s, 0.0, 0.0])
}

fn translate_matrix(tx: f64, ty: f64) -> PdfMatrix {
    PdfMatrix([1.0, 0.0, 0.0, 1.0, tx, ty])
}

fn format_matrix(m: &PdfMatrix) -> String {
    let [a, b, c, d, e, f] = m.0;
    format!(
        "{} {} {} {} {} {}",
        fmt_num(a),
        fmt_num(b),
        fmt_num(c),
        fmt_num(d),
        fmt_num(e),
        fmt_num(f)
    )
}

fn fmt_num(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if x == x.trunc() && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        let s = format!("{x:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

fn rect_array(doc: &PdfDocument, rect: &[f64; 4]) -> Result<PdfObject, Error> {
    doc.new_object_from_str(&format!(
        "[{} {} {} {}]",
        fmt_num(rect[0]),
        fmt_num(rect[1]),
        fmt_num(rect[2]),
        fmt_num(rect[3])
    ))
}

fn matrix_array(doc: &PdfDocument, m: &PdfMatrix) -> Result<PdfObject, Error> {
    doc.new_object_from_str(&format!(
        "[{} {} {} {} {} {}]",
        fmt_num(m.0[0]),
        fmt_num(m.0[1]),
        fmt_num(m.0[2]),
        fmt_num(m.0[3]),
        fmt_num(m.0[4]),
        fmt_num(m.0[5]),
    ))
}

/// First free `/<name>` in the page's `/Resources/XObject` (production uses a
/// random name; any unused name is semantically equivalent).
fn next_free_form_name(page: &PdfPage) -> Result<String, Error> {
    let mut existing = Vec::new();
    if let Some(r) = page.object().get_dict("Resources")? {
        if r.is_dict()? {
            if let Some(x) = r.get_dict("XObject")? {
                if x.is_dict()? {
                    for pair in x.dict_iter()? {
                        let (key, _value) = pair?;
                        existing.push(String::from_utf8_lossy(&key.as_name()?).into_owned());
                    }
                }
            }
        }
    }
    for i in 1..1000 {
        let name = format!("Rp{i}");
        if !existing.iter().any(|k| k == &name) {
            return Ok(name);
        }
    }
    Err(Error::InvalidArgument("no free form xobject name".into()))
}

/// trimbox-or-cropbox-or-mediabox, mirroring qpdf `getTrimBox(false)`.
fn get_trim_box(page: &PdfPage) -> Result<Option<[f64; 4]>, Error> {
    for key in ["TrimBox", "CropBox", "MediaBox"] {
        if let Some(r) = page_box(page, key)? {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

/// PyMuPDF `Page.transformation_matrix`: the page ctm, but for rotated pages
/// (rotation not a multiple of 360) overridden with `Matrix(1,0,0,-1,0,cropbox
/// .height)` in PDF coords. Mirrors PyMuPDF's `%pythonappend` on the property.
fn page_transformation_matrix(page: &PdfPage) -> Result<PdfMatrix, Error> {
    let rotation = page.rotation()?;
    if rotation % 360 == 0 {
        return Ok(matrix_to_pdf(&page.ctm()?));
    }
    let crop = crop_box(page)?.unwrap_or([0.0, 0.0, 612.0, 792.0]);
    Ok(PdfMatrix([1.0, 0.0, 0.0, -1.0, 0.0, crop[3] - crop[1]]))
}

/// Production `_page_crop_rect`: cropbox, else mediabox.
fn crop_box(page: &PdfPage) -> Result<Option<[f64; 4]>, Error> {
    for key in ["CropBox", "MediaBox"] {
        if let Some(r) = page_box(page, key)? {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

fn page_box(page: &PdfPage, key: &str) -> Result<Option<[f64; 4]>, Error> {
    match page.object().get_dict(key)? {
        Some(obj) => rect_from_obj(&obj),
        None => Ok(None),
    }
}

fn rect_from_obj(obj: &PdfObject) -> Result<Option<[f64; 4]>, Error> {
    let obj = resolve(obj)?;
    if !obj.is_array()? || obj.len()? < 4 {
        return Ok(None);
    }
    let mut out = [0.0; 4];
    for (i, slot) in out.iter_mut().enumerate() {
        match obj.get_array(i as i32)? {
            Some(v) if v.is_number()? => *slot = v.as_float()? as f64,
            _ => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn page_int(page: &PdfPage, key: &str) -> Result<i32, Error> {
    match page.object().get_dict(key)? {
        Some(obj) if obj.is_number()? => Ok(obj.as_int()?),
        _ => Ok(0),
    }
}

fn page_float(page: &PdfPage, key: &str, default: f64) -> Result<f64, Error> {
    match page.object().get_dict(key)? {
        Some(obj) if obj.is_number()? => Ok(obj.as_float()? as f64),
        _ => Ok(default),
    }
}

fn matrix_to_pdf(m: &Matrix) -> PdfMatrix {
    PdfMatrix([m.a as f64, m.b as f64, m.c as f64, m.d as f64, m.e as f64, m.f as f64])
}

fn rect_to_f64(r: Rect) -> [f64; 4] {
    [r.x0 as f64, r.y0 as f64, r.x1 as f64, r.y1 as f64]
}

/// PyMuPDF `show_pdf_page.calc_matrix(keep=True, rotate=0)`, in f64.
///
/// `pdf_math::mul_matrix` composes "left applied first", the inverse of fitz's
/// `fz_concat`, so each composition step is written right-to-left relative to
/// the PyMuPDF reference (verified: `mul_qpdf(X, m) == probe_mul(m, X)`).
fn calc_matrix(src: &[f64; 4], tar: &[f64; 4]) -> PdfMatrix {
    let smp = ((src[0] + src[2]) / 2.0, (src[1] + src[3]) / 2.0);
    let tmp = ((tar[0] + tar[2]) / 2.0, (tar[1] + tar[3]) / 2.0);
    let mut m = PdfMatrix([1.0, 0.0, 0.0, 1.0, -smp.0, -smp.1]);
    let sr1 = transform_rect(&m, src);
    let fw = (tar[2] - tar[0]) / (sr1[2] - sr1[0]);
    let fh = (tar[3] - tar[1]) / (sr1[3] - sr1[1]);
    let f = fw.min(fh);
    m = mul_matrix(&PdfMatrix([f, 0.0, 0.0, f, 0.0, 0.0]), &m);
    m = mul_matrix(&PdfMatrix([1.0, 0.0, 0.0, 1.0, tmp.0, tmp.1]), &m);
    m
}

/// fitz number writer for form matrices: "%.5f" with trailing zeros stripped.
/// Pixel parity with PyMuPDF's output depends on this exact format.
fn fmt_num5(x: f64) -> String {
    let mut s = format!("{x:.5}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s.is_empty() {
        "0".into()
    } else {
        s
    }
}

fn matrix_array5(doc: &PdfDocument, m: &PdfMatrix) -> Result<PdfObject, Error> {
    doc.new_object_from_str(&format!(
        "[{} {} {} {} {} {}]",
        fmt_num5(m.0[0]),
        fmt_num5(m.0[1]),
        fmt_num5(m.0[2]),
        fmt_num5(m.0[3]),
        fmt_num5(m.0[4]),
        fmt_num5(m.0[5]),
    ))
}

fn rect_array5(doc: &PdfDocument, rect: &[f64; 4]) -> Result<PdfObject, Error> {
    doc.new_object_from_str(&format!(
        "[{} {} {} {}]",
        fmt_num5(rect[0]),
        fmt_num5(rect[1]),
        fmt_num5(rect[2]),
        fmt_num5(rect[3]),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupdf::Size;

    fn blank_pdf(page_sizes: &[(f32, f32)]) -> PdfDocument {
        let mut doc = PdfDocument::new();
        for (w, h) in page_sizes {
            doc.new_page(Size::new(*w, *h)).expect("new page");
        }
        doc
    }

    #[test]
    fn dual_compose_sums_widths_keeps_page_count() {
        let source = blank_pdf(&[(200.0, 300.0), (200.0, 300.0)]);
        let translated = blank_pdf(&[(200.0, 300.0), (200.0, 300.0)]);
        let dual = build_dual_doc_pages(&source, &translated, 0, -1).expect("compose");
        assert_eq!(dual.page_count().expect("count"), 2);
        for idx in 0..2 {
            let b = dual.load_pdf_page(idx).expect("load").bounds().expect("bounds");
            assert!((b.width() - 400.0).abs() < 0.01, "page {idx} width");
            assert!((b.height() - 300.0).abs() < 0.01, "page {idx} height");
        }
    }

    #[test]
    fn dual_compose_honors_page_range() {
        let source = blank_pdf(&[(200.0, 300.0); 3]);
        let translated = blank_pdf(&[(200.0, 300.0); 3]);
        let dual = build_dual_doc_pages(&source, &translated, 1, 1).expect("compose");
        assert_eq!(dual.page_count().expect("count"), 1);
        let b = dual.load_pdf_page(0).expect("load").bounds().expect("bounds");
        assert!((b.width() - 400.0).abs() < 0.01);
    }

    #[test]
    fn dual_compose_uses_max_height() {
        let source = blank_pdf(&[(200.0, 300.0)]);
        let translated = blank_pdf(&[(200.0, 400.0)]);
        let dual = build_dual_doc_pages(&source, &translated, 0, -1).expect("compose");
        let b = dual.load_pdf_page(0).expect("load").bounds().expect("bounds");
        assert!((b.height() - 400.0).abs() < 0.01, "max height");
        assert!((b.width() - 400.0).abs() < 0.01, "sum width");
    }
}
