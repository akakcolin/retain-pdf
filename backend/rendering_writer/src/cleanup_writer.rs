//! Port of the production bbox-text-strip write path:
//! `source_cleanup/pdf/{stream_engine.py,document.py,xobject_ops.py}`.
//!
//! The page content stream is tokenized into `rendering_core`'s
//! `ContentToken`s, driven through the ported decision state machine, and
//! re-serialized. Form XObjects named by `Do` operators are cloned per-site
//! (`clone_form_xobject`), rewritten recursively with the strip rects in the
//! form's user space (`ctm * form /Matrix`), and installed under a fresh
//! `/<name>_sc<N>` key so shared forms are not mutated.

use std::collections::HashMap;
use std::collections::HashSet;

use mupdf::pdf::PdfDocument;
use mupdf::pdf::PdfObject;
use mupdf::pdf::PdfPage;
use mupdf::Buffer;
use mupdf::Error;

use rendering_core::source_cleanup::content_stream::{serialize, tokenize, ContentToken};
use rendering_core::source_cleanup::hit_test::{RectIndex, RectTuple};
use rendering_core::source_cleanup::path_removal::{
    decide_path_paint_rewrite, PathTracker, PATH_CONSTRUCTION_OPERATORS, PATH_PAINT_OPERATORS,
};
use rendering_core::source_cleanup::pdf_math::{mul_matrix, Operand, PdfMatrix, IDENTITY_MATRIX};
use rendering_core::source_cleanup::stream_state::ContentStreamState;
use rendering_core::source_cleanup::text_ops::TEXT_SHOW_OPERATORS;
use rendering_core::source_cleanup::text_removal::decide_text_show_rewrite;

use crate::contents::{page_contents_bytes, replace_page_contents, resolve};

/// Result of rewriting a single content stream.
#[derive(Debug, Clone, PartialEq)]
pub struct StripStreamResult {
    /// Rewritten stream bytes when text was removed, else `None`.
    pub content: Option<Vec<u8>>,
    pub removed: usize,
    pub forms_changed: usize,
}

/// Result of stripping rects from a whole PDF.
#[derive(Debug, Clone, PartialEq)]
pub struct StripPdfResult {
    pub pages_changed: usize,
    pub text_show_ops_removed: usize,
    pub forms_changed: usize,
    pub changed_page_indices: Vec<i32>,
}

/// Port of `document.py::strip_bbox_text_rects_from_pdf_copy` minus the
/// parallel page workers and file I/O: run the strip on each targeted page in
/// the already-open `doc` and replace `/Contents` where text was removed.
pub fn strip_bbox_text_rects_from_pdf(
    doc: &mut PdfDocument,
    page_rects: &HashMap<i32, Vec<RectTuple>>,
    page_protected_rects: &HashMap<i32, Vec<RectTuple>>,
    recurse_forms: bool,
) -> Result<StripPdfResult, Error> {
    let mut pages_changed = 0;
    let mut removed_total = 0;
    let mut forms_changed_total = 0;
    let mut changed_page_indices = Vec::new();

    for (&page_idx, rects) in page_rects {
        let page = doc.load_pdf_page(page_idx)?;
        let protected = page_protected_rects.get(&page_idx).cloned().unwrap_or_default();
        let result = strip_bbox_text_from_page(doc, &page, rects, &protected, recurse_forms)?;

        forms_changed_total += result.forms_changed;
        if result.content.is_none() || result.removed == 0 {
            if result.forms_changed > 0 {
                pages_changed += 1;
                changed_page_indices.push(page_idx);
                removed_total += result.removed;
            }
            continue;
        }
        let content = result.content.unwrap();
        replace_page_contents(&page, doc, &content)?;
        pages_changed += 1;
        changed_page_indices.push(page_idx);
        removed_total += result.removed;
    }

    Ok(StripPdfResult {
        pages_changed,
        text_show_ops_removed: removed_total,
        forms_changed: forms_changed_total,
        changed_page_indices,
    })
}

/// Port of `stream_engine.py::strip_bbox_text_from_page`.
pub fn strip_bbox_text_from_page(
    doc: &mut PdfDocument,
    page: &PdfPage,
    rects: &[RectTuple],
    protected_rects: &[RectTuple],
    recurse_forms: bool,
) -> Result<StripStreamResult, Error> {
    let stream = page_contents_bytes(page)?;
    let page_obj = page.object();
    let mut xobjects = page_xobjects(&page_obj)?;
    let mut visited_forms = HashSet::new();
    strip_bbox_text_from_stream(
        doc,
        &stream,
        rects,
        protected_rects,
        recurse_forms,
        &IDENTITY_MATRIX,
        &mut visited_forms,
        xobjects.as_mut(),
    )
}

/// The page's `/Resources`/`/XObject` dictionary, or `None`.
fn page_xobjects(page_obj: &PdfObject) -> Result<Option<PdfObject>, Error> {
    let Some(resources) = page_obj.get_dict("Resources")? else {
        return Ok(None);
    };
    let resources = resolve(&resources)?;
    match resources.get_dict("XObject")? {
        Some(x) => Ok(Some(resolve(&x)?)),
        None => Ok(None),
    }
}

/// Port of `stream_engine.py::strip_bbox_text_from_stream` plus the
/// `xobject_ops.py` Form recursion. `xobjects` is the stream's
/// `/Resources`/`/XObject` dict; Form clones are installed into it.
fn strip_bbox_text_from_stream(
    doc: &mut PdfDocument,
    stream: &[u8],
    rects: &[RectTuple],
    protected_rects: &[RectTuple],
    recurse_forms: bool,
    initial_ctm: &PdfMatrix,
    visited_forms: &mut HashSet<i32>,
    mut xobjects: Option<&mut PdfObject>,
) -> Result<StripStreamResult, Error> {
    if stream.is_empty() || rects.is_empty() {
        return Ok(StripStreamResult { content: None, removed: 0, forms_changed: 0 });
    }
    let tokens = match tokenize(stream) {
        Ok(tokens) if !tokens.is_empty() => tokens,
        _ => return Ok(StripStreamResult { content: None, removed: 0, forms_changed: 0 }),
    };

    let strip_index = RectIndex::build(rects.iter().copied());
    let protected_index = RectIndex::build(protected_rects.iter().copied());
    let mut state = ContentStreamState::default();
    state.ctm = *initial_ctm;
    let mut path_tracker = PathTracker::empty();
    let mut pending_path_ops: Vec<ContentToken> = Vec::new();
    let mut output: Vec<ContentToken> = Vec::new();
    let mut removed = 0usize;
    let mut path_removed = 0usize;
    let mut forms_changed = 0usize;

    for token in &tokens {
        let op = token.operator.as_str();
        if state.apply_state_operator(op, &token.operands) {
            output.push(token.clone());
            continue;
        }
        if op == "Do" && !token.operands.is_empty() {
            let (new_operands, x_removed, x_forms) = rewrite_xobject_do(
                doc,
                xobjects.as_deref_mut(),
                &token.operands,
                rects,
                protected_rects,
                recurse_forms,
                &state.ctm,
                visited_forms,
            )?;
            removed += x_removed;
            forms_changed += x_forms;
            output.push(ContentToken {
                operands: new_operands,
                operator: "Do".to_string(),
            });
            continue;
        }
        if op == "'" || op == "\"" {
            state.prepare_quote_text_show(op, &token.operands);
        }
        if TEXT_SHOW_OPERATORS.contains(&op) {
            let decision = decide_text_show_rewrite(
                &token.operands,
                &state.ctm,
                &state.text_matrix,
                &state.text_state,
                &strip_index,
                &protected_index,
            );
            state.advance_text(&token.operands, Some(&decision.text_metrics));
            if decision.remove {
                removed += 1;
                continue;
            }
        }
        if PATH_CONSTRUCTION_OPERATORS.contains(&op) {
            path_tracker.record(op, &token.operands, &state.ctm);
            pending_path_ops.push(token.clone());
            continue;
        }
        if PATH_PAINT_OPERATORS.contains(&op) && !pending_path_ops.is_empty() {
            let path_decision = decide_path_paint_rewrite(
                op,
                path_tracker.rect(),
                &strip_index,
                &protected_index,
            );
            path_tracker.clear();
            if path_decision.remove {
                pending_path_ops.clear();
                path_removed += 1;
                continue;
            }
            output.extend(pending_path_ops.drain(..));
        }
        output.push(token.clone());
    }
    output.extend(pending_path_ops.drain(..));
    removed += path_removed;
    if removed == 0 {
        return Ok(StripStreamResult { content: None, removed: 0, forms_changed });
    }
    Ok(StripStreamResult {
        content: Some(serialize(&output)),
        removed,
        forms_changed,
    })
}

/// Port of `xobject_ops.py::rewrite_xobject_do`.
#[allow(clippy::too_many_arguments)]
fn rewrite_xobject_do(
    doc: &mut PdfDocument,
    xobjects: Option<&mut PdfObject>,
    operands: &[Operand],
    rects: &[RectTuple],
    protected_rects: &[RectTuple],
    recurse_forms: bool,
    ctm: &PdfMatrix,
    visited_forms: &mut HashSet<i32>,
) -> Result<(Vec<Operand>, usize, usize), Error> {
    let Some(xobjects) = xobjects else {
        return Ok((operands.to_vec(), 0, 0));
    };
    if !recurse_forms || operands.is_empty() {
        return Ok((operands.to_vec(), 0, 0));
    }
    let Operand::Name(name) = &operands[0] else {
        return Ok((operands.to_vec(), 0, 0));
    };
    let Some(xobject_raw) = xobjects.get_dict(name.as_str())? else {
        return Ok((operands.to_vec(), 0, 0));
    };
    let xobject = resolve(&xobject_raw)?;
    if !is_form_xobject(&xobject)? {
        return Ok((operands.to_vec(), 0, 0));
    }
    // The raw (reference) object number keys cycle protection; a resolved
    // object has no number.
    let form_key = xobject_raw.as_indirect().unwrap_or(-1);
    if !visited_forms.insert(form_key) {
        return Ok((operands.to_vec(), 0, 0));
    }
    let result = rewrite_form_context(
        doc,
        xobjects,
        operands,
        name,
        &xobject_raw,
        &xobject,
        rects,
        protected_rects,
        recurse_forms,
        ctm,
        visited_forms,
    );
    visited_forms.remove(&form_key);
    result
}

/// Port of `xobject_ops.py::_rewrite_form_context`.
#[allow(clippy::too_many_arguments)]
fn rewrite_form_context(
    doc: &mut PdfDocument,
    xobjects: &mut PdfObject,
    operands: &[Operand],
    xobject_name: &str,
    xobject_raw: &PdfObject,
    xobject: &PdfObject,
    rects: &[RectTuple],
    protected_rects: &[RectTuple],
    recurse_forms: bool,
    ctm: &PdfMatrix,
    visited_forms: &mut HashSet<i32>,
) -> Result<(Vec<Operand>, usize, usize), Error> {
    let mut clone = clone_form_xobject(doc, xobject_raw, xobject)?;
    let form_matrix = form_matrix(doc, xobject)?;
    let form_ctm = mul_matrix(ctm, &form_matrix);

    // The recursion reads the clone's own resources so nested Form clones are
    // installed into the clone's dictionary, not the shared original.
    let mut clone_xobjects = clone_xobject_dict(&clone)?;
    let clone_bytes = clone.read_stream()?;
    let nested = strip_bbox_text_from_stream(
        doc,
        &clone_bytes,
        rects,
        protected_rects,
        recurse_forms,
        &form_ctm,
        visited_forms,
        clone_xobjects.as_mut(),
    )?;

    if nested.content.is_none() || nested.removed == 0 {
        return Ok((operands.to_vec(), 0, nested.forms_changed));
    }
    let content = nested.content.unwrap();
    let buf = Buffer::from_bytes(&content)?;
    clone.write_stream_buffer(&buf)?;
    let cloned_name = install_cloned_xobject(xobjects, xobject_name, &clone)?;
    Ok((
        vec![Operand::Name(cloned_name)],
        nested.removed,
        nested.forms_changed + 1,
    ))
}

/// The clone's `/Resources`/`/XObject` dict, or `None` when absent.
fn clone_xobject_dict(clone: &PdfObject) -> Result<Option<PdfObject>, Error> {
    let Some(resources) = clone.get_dict("Resources")? else {
        return Ok(None);
    };
    let resources = resolve(&resources)?;
    match resources.get_dict("XObject")? {
        Some(x) => Ok(Some(resolve(&x)?)),
        None => Ok(None),
    }
}

/// Port of `xobject_ops.py::_clone_form_xobject`. The stream is read from the
/// indirect reference (`xobject_raw`); the resolved `xobject` supplies the dict.
fn clone_form_xobject(
    doc: &mut PdfDocument,
    xobject_raw: &PdfObject,
    xobject: &PdfObject,
) -> Result<PdfObject, Error> {
    let raw = xobject_raw.read_stream()?;
    let buf = Buffer::from_bytes(&raw)?;
    let mut clone = doc.add_stream(&buf, None, false)?;
    for pair in xobject.dict_iter()? {
        let (key, value) = pair?;
        let name_bytes = key.as_name()?;
        let key_name = String::from_utf8_lossy(&name_bytes);
        if key_name == "Length" || key_name == "Filter" || key_name == "DecodeParms" {
            continue;
        }
        if resolve(&value)?.is_null()? {
            continue;
        }
        if key_name == "Resources" {
            let resources = resolve(&value)?;
            let cloned = clone_resources(doc, &resources)?;
            clone.dict_put("Resources", cloned)?;
        } else {
            clone.dict_put(key_name.as_ref(), value)?;
        }
    }
    Ok(clone)
}

/// Port of `xobject_ops.py::_clone_resources` — shallow dict copy; `/XObject`
/// gets its own dictionary so nested clones do not leak into the shared dict.
fn clone_resources(doc: &mut PdfDocument, resources: &PdfObject) -> Result<PdfObject, Error> {
    let mut cloned = doc.new_dict()?;
    for pair in resources.dict_iter()? {
        let (key, value) = pair?;
        let name_bytes = key.as_name()?;
        let key_name = String::from_utf8_lossy(&name_bytes);
        if resolve(&value)?.is_null()? {
            continue;
        }
        if key_name == "XObject" {
            let xobjects = resolve(&value)?;
            let copied = xobjects.copy_dict()?;
            cloned.dict_put("XObject", copied)?;
        } else {
            cloned.dict_put(key_name.as_ref(), value)?;
        }
    }
    Ok(cloned)
}

/// Port of `xobject_ops.py::_install_cloned_xobject` + `_unique_xobject_name`.
fn install_cloned_xobject(
    xobjects: &mut PdfObject,
    original_name: &str,
    cloned_xobject: &PdfObject,
) -> Result<String, Error> {
    let base = format!("{}_sc", original_name);
    let mut index = 1;
    loop {
        let candidate = format!("{base}{index}");
        if xobjects.get_dict(candidate.as_str())?.is_none() {
            xobjects.dict_put(candidate.as_str(), cloned_xobject.clone())?;
            return Ok(candidate);
        }
        index += 1;
    }
}

/// `xobject_ops.py::_is_form_xobject`.
fn is_form_xobject(xobject: &PdfObject) -> Result<bool, Error> {
    let Some(subtype) = xobject.get_dict("Subtype")? else {
        return Ok(false);
    };
    let subtype = resolve(&subtype)?;
    Ok(subtype.is_name()? && subtype.as_name()? == b"Form")
}

/// `pdf_math.py::matrix_from_object` — the form `/Matrix`, identity by default.
fn form_matrix(doc: &PdfDocument, xobject: &PdfObject) -> Result<PdfMatrix, Error> {
    let _ = doc;
    let Some(matrix_obj) = xobject.get_dict("Matrix")? else {
        return Ok(IDENTITY_MATRIX);
    };
    let matrix_obj = resolve(&matrix_obj)?;
    if !matrix_obj.is_array()? || matrix_obj.len()? < 6 {
        return Ok(IDENTITY_MATRIX);
    }
    let mut out = [0.0; 6];
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some(item) = matrix_obj.get_array(i as i32)? {
            *slot = pdf_to_float(&item);
        }
    }
    Ok(PdfMatrix(out))
}

/// `pdf_math.py::to_float` over a mupdf object.
fn pdf_to_float(value: &PdfObject) -> f64 {
    let value = match value.resolve() {
        Ok(Some(v)) => v,
        _ => value.clone(),
    };
    match value.as_float() {
        Ok(v) => v as f64,
        Err(_) => match value.as_string() {
            Ok(s) => s.trim().parse().unwrap_or(0.0),
            Err(_) => 0.0,
        },
    }
}
