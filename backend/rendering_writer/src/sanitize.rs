//! Port of production `source/preparation/xobject_sanitize.py` (Phase 5C-7):
//! replace every `/Image` XObject with a missing or non-positive `/Width` or
//! `/Height` by a single shared empty Form XObject, recursing into nested Forms
//! with objgen identity cycle protection.
//!
//! Divergence from production: pikepdf assigns direct objects the identity
//! `(0, 0)` (so a second direct XObject in a dict is skipped by the cycle
//! guard); mupdf-rs has no objgen for direct objects, so direct objects are
//! never seen-skipped here. The corpus uses only indirect streams, so the
//! differential never observes the difference.

use std::collections::HashSet;

use mupdf::pdf::PdfDocument;
use mupdf::pdf::PdfObject;
use mupdf::Buffer;
use mupdf::Error;

use crate::contents::resolve;

/// Per-document sanitize outcome, mirroring `XObjectSanitizeResult`.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct SanitizeResult {
    pub changed: bool,
    pub invalid_image_xobjects: usize,
    pub pages_changed: usize,
}

/// Replace invalid `/Image` XObjects in every page's resource tree in place.
/// Returns the outcome; the caller still decides the final file-level save.
pub fn sanitize_invalid_xobjects(doc: &mut PdfDocument) -> Result<SanitizeResult, Error> {
    let mut result = SanitizeResult::default();
    let mut seen: HashSet<i32> = HashSet::new();
    let empty_form = make_empty_form(doc)?;
    let count = doc.page_count()?;
    for page_idx in 0..count {
        let page = doc.load_pdf_page(page_idx)?;
        let container = page.object();
        let before = result.invalid_image_xobjects;
        sanitize_container(doc, &container, &empty_form, &mut result, &mut seen)?;
        if result.invalid_image_xobjects > before {
            result.pages_changed += 1;
        }
    }
    result.changed = result.invalid_image_xobjects > 0;
    Ok(result)
}

/// `_sanitize_container_xobjects` — walk `/Resources/XObject` of `container`
/// (page or Form), replacing invalid images and recursing into Forms. `seen`
/// holds objnums of every resolved XObject (indirect only) so cyclic resource
/// graphs terminate.
fn sanitize_container(
    doc: &mut PdfDocument,
    container: &PdfObject,
    empty_form: &PdfObject,
    result: &mut SanitizeResult,
    seen: &mut HashSet<i32>,
) -> Result<(), Error> {
    let Some(resources_raw) = container.get_dict("Resources")? else {
        return Ok(());
    };
    let resources = resolve(&resources_raw)?;
    if !resources.is_dict()? {
        return Ok(());
    }
    let Some(xobjects_raw) = resources.get_dict("XObject")? else {
        return Ok(());
    };
    let mut xobjects = resolve(&xobjects_raw)?;
    if !xobjects.is_dict()? {
        return Ok(());
    }
    let items: Vec<(PdfObject, PdfObject)> =
        xobjects.dict_iter()?.collect::<Result<Vec<_>, _>>()?;
    for (key, value) in items {
        let identity = if value.is_indirect()? {
            Some(value.as_indirect()?)
        } else {
            None
        };
        let resolved = resolve(&value)?;
        if let Some(id) = identity {
            if !seen.insert(id) {
                continue;
            }
        }
        let Some(subtype) = resolved.get_dict("Subtype")? else {
            continue;
        };
        if subtype_name_is(&subtype, "Image")? && invalid_image_xobject(&resolved)? {
            xobjects.dict_put(key, empty_form.clone())?;
            result.invalid_image_xobjects += 1;
            continue;
        }
        if subtype_name_is(&subtype, "Form")? {
            sanitize_container(doc, &resolved, empty_form, result, seen)?;
        }
    }
    Ok(())
}

/// `(total_images, invalid_images)` across every page's resource tree — the
/// structural fact the sanitize differential asserts on the output.
pub fn count_invalid_images(doc: &PdfDocument) -> Result<(usize, usize), Error> {
    let mut total = 0usize;
    let mut invalid = 0usize;
    let mut seen: HashSet<i32> = HashSet::new();
    let count = doc.page_count()?;
    for page_idx in 0..count {
        let page = doc.load_pdf_page(page_idx)?;
        let container = page.object();
        walk_xobjects(&container, &mut seen, &mut |_key, resolved| {
            let is_image = match resolved.get_dict("Subtype")? {
                Some(sub) => subtype_name_is(&sub, "Image")?,
                None => false,
            };
            if is_image {
                total += 1;
                if invalid_image_xobject(resolved)? {
                    invalid += 1;
                }
            }
            Ok(())
        })?;
    }
    Ok((total, invalid))
}

/// Sorted `(resource name, subtype)` pairs of every XObject across the doc's
/// resource trees — the structural inventory that pins the sanitize replacement
/// to a retained empty Form (mirrors the generator's `_scan_xobject_subtypes`).
pub fn xobject_subtypes(doc: &PdfDocument) -> Result<Vec<(String, String)>, Error> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: HashSet<i32> = HashSet::new();
    let count = doc.page_count()?;
    for page_idx in 0..count {
        let page = doc.load_pdf_page(page_idx)?;
        let container = page.object();
        walk_xobjects(&container, &mut seen, &mut |key, resolved| {
            let key_name = key
                .as_name()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|_| "?".into());
            let subtype = match resolved.get_dict("Subtype")? {
                Some(s) => match s.as_name() {
                    Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                    Err(_) => "?".into(),
                },
                None => "-".into(),
            };
            out.push((key_name, subtype));
            Ok(())
        })?;
    }
    out.sort();
    Ok(out)
}

/// Walk `/Resources/XObject` of `container` (recursing into Forms), calling
/// `visit` with each `(resource name, resolved XObject)` pair. Shared by
/// `count_invalid_images` and `xobject_subtypes`.
fn walk_xobjects(
    container: &PdfObject,
    seen: &mut HashSet<i32>,
    visit: &mut dyn FnMut(&PdfObject, &PdfObject) -> Result<(), Error>,
) -> Result<(), Error> {
    let Some(resources_raw) = container.get_dict("Resources")? else {
        return Ok(());
    };
    let resources = resolve(&resources_raw)?;
    if !resources.is_dict()? {
        return Ok(());
    }
    let Some(xobjects_raw) = resources.get_dict("XObject")? else {
        return Ok(());
    };
    let xobjects = resolve(&xobjects_raw)?;
    if !xobjects.is_dict()? {
        return Ok(());
    }
    let items: Vec<(PdfObject, PdfObject)> =
        xobjects.dict_iter()?.collect::<Result<Vec<_>, _>>()?;
    for (key, value) in items {
        let identity = if value.is_indirect()? {
            Some(value.as_indirect()?)
        } else {
            None
        };
        let resolved = resolve(&value)?;
        if let Some(id) = identity {
            if !seen.insert(id) {
                continue;
            }
        }
        let is_form = match resolved.get_dict("Subtype")? {
            Some(sub) => subtype_name_is(&sub, "Form")?,
            None => false,
        };
        visit(&key, &resolved)?;
        if is_form {
            walk_xobjects(&resolved, seen, visit)?;
        }
    }
    Ok(())
}

/// `_invalid_image_xobject` — `/Width` or `/Height` missing, non-numeric, or
/// non-positive means the image cannot be displayed.
fn invalid_image_xobject(xobject: &PdfObject) -> Result<bool, Error> {
    let width = number_or_none(xobject.get_dict("Width")?)?;
    let height = number_or_none(xobject.get_dict("Height")?)?;
    Ok(width <= 0 || height <= 0)
}

fn number_or_none(obj: Option<PdfObject>) -> Result<i32, Error> {
    match obj {
        Some(o) if o.is_number()? => Ok(o.as_int().unwrap_or(-1)),
        _ => Ok(-1),
    }
}

/// Compare a resolved `/Subtype` value against a name (without the leading
/// slash), tolerating non-name values.
fn subtype_name_is(subtype: &PdfObject, name: &str) -> Result<bool, Error> {
    match subtype.as_name() {
        Ok(bytes) => Ok(bytes == name.as_bytes()),
        Err(_) => Ok(false),
    }
}

/// `_make_empty_form_xobject` — one shared `/Type /XObject /Subtype /Form
/// /BBox [0 0 0 0]` stream reused for every replacement.
fn make_empty_form(doc: &mut PdfDocument) -> Result<PdfObject, Error> {
    let buf = Buffer::from_bytes(b"")?;
    let mut form = doc.add_stream(&buf, None, false)?;
    form.dict_put("Type", doc.new_name("XObject")?)?;
    form.dict_put("Subtype", doc.new_name("Form")?)?;
    let mut bbox = doc.new_array()?;
    for _ in 0..4 {
        bbox.array_push(doc.new_int(0)?)?;
    }
    form.dict_put("BBox", bbox)?;
    Ok(form)
}
