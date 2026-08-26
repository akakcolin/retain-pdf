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
use mupdf::{Buffer, Error};

use rendering_core::source_cleanup::pdf_math::{
    mul_matrix, transform_rect, PdfMatrix, IDENTITY_MATRIX,
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
