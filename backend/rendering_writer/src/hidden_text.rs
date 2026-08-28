//! Port of production `source/preparation/hidden_text_strip.py` (Phase 5C-7):
//! drop every BT..ET text object whose text shows all carry render mode 3
//! (invisible) text, by rewriting each page's content stream.
//!
//! Divergence from production: production pre-scans pages with fitz
//! (`page_is_pseudo_editable_scan` — a large background image AND editable
//! text) and only rewrites candidates; mupdf-rs exposes no render-mode API, so
//! this rewrites every page using in-stream `Tr` analysis. The corpus page is a
//! candidate under the production heuristic, so the differential stays
//! semantic-equivalent.

use mupdf::pdf::PdfDocument;
use mupdf::Error;
use rendering_core::source_cleanup::content_stream::serialize;
use rendering_core::source_cleanup::content_stream::tokenize;
use rendering_core::source_cleanup::content_stream::ContentToken;
use rendering_core::source_cleanup::pdf_math::to_float;
use rendering_core::source_cleanup::pdf_math::Operand;

use crate::contents::page_contents_bytes;
use crate::contents::replace_page_contents;

/// Production `TEXT_SHOW_OPERATORS`.
const TEXT_SHOW_OPERATORS: [&str; 4] = ["Tj", "TJ", "'", "\""];

/// Per-document hidden-text strip outcome, mirroring `HiddenTextStripResult`.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct HiddenTextStripResult {
    pub changed: bool,
    pub pages_changed: usize,
    pub text_objects_removed: usize,
}

/// Drop all-invisible text objects from every page's content stream. Returns
/// the outcome; the caller still decides the final file-level save.
pub fn strip_hidden_text(doc: &mut PdfDocument) -> Result<HiddenTextStripResult, Error> {
    let mut result = HiddenTextStripResult::default();
    let count = doc.page_count()?;
    for page_idx in 0..count {
        let page = doc.load_pdf_page(page_idx)?;
        let stream = page_contents_bytes(&page)?;
        if stream.is_empty() {
            continue;
        }
        let tokens = match tokenize(&stream) {
            Ok(tokens) => tokens,
            Err(_) => continue,
        };
        let (kept, removed) = strip_hidden_groups(&tokens);
        if removed == 0 {
            continue;
        }
        replace_page_contents(&page, doc, &serialize(&kept))?;
        result.pages_changed += 1;
        result.text_objects_removed += removed;
    }
    result.changed = result.pages_changed > 0;
    Ok(result)
}

/// `_strip_hidden_text_objects_from_page` — walk the instruction stream tracking
/// the render mode (`Tr` with a `q`/`Q` stack); collect each BT..ET group and
/// drop it when `_analyze_text_object_visibility` reports it all-hidden.
fn strip_hidden_groups(tokens: &[ContentToken]) -> (Vec<ContentToken>, usize) {
    let mut kept: Vec<ContentToken> = Vec::new();
    let mut removed = 0usize;
    let mut index = 0usize;
    let mut render_mode = 0i32;
    let mut render_mode_stack: Vec<i32> = Vec::new();
    while index < tokens.len() {
        let op = tokens[index].operator.as_str();
        if op == "q" {
            render_mode_stack.push(render_mode);
            kept.push(tokens[index].clone());
            index += 1;
            continue;
        }
        if op == "Q" {
            render_mode = render_mode_stack.pop().unwrap_or(0);
            kept.push(tokens[index].clone());
            index += 1;
            continue;
        }
        if op == "Tr" {
            render_mode = tr_render_mode(&tokens[index]);
            kept.push(tokens[index].clone());
            index += 1;
            continue;
        }
        if op != "BT" {
            kept.push(tokens[index].clone());
            index += 1;
            continue;
        }

        let group: Vec<ContentToken> = {
            let mut group = vec![tokens[index].clone()];
            index += 1;
            while index < tokens.len() {
                group.push(tokens[index].clone());
                if tokens[index].operator.as_str() == "ET" {
                    index += 1;
                    break;
                }
                index += 1;
            }
            group
        };

        let (hidden, final_render_mode) = analyze_group(&group, render_mode);
        render_mode = final_render_mode;
        if hidden {
            removed += 1;
        } else {
            kept.extend(group);
        }
    }
    (kept, removed)
}

/// `_analyze_text_object_visibility` — linear pass over a BT..ET group: `Tr`
/// updates the render mode (no `q`/`Q` inside the group), text shows flag the
/// object. Returns `(saw_text_show AND all_text_show_is_hidden,
/// final_render_mode)` — the final mode is returned unconditionally.
fn analyze_group(group: &[ContentToken], initial_render_mode: i32) -> (bool, i32) {
    let mut render_mode = initial_render_mode;
    let mut saw_text_show = false;
    let mut all_hidden = true;
    for token in group {
        let op = token.operator.as_str();
        if op == "Tr" {
            render_mode = tr_render_mode(token);
        }
        if TEXT_SHOW_OPERATORS.contains(&op) {
            saw_text_show = true;
            if render_mode != 3 {
                all_hidden = false;
            }
        }
    }
    (saw_text_show && all_hidden, render_mode)
}

/// `int(operands[0])` on a `Tr` operand — truncating toward zero; non-numeric
/// operands read as 0.
fn tr_render_mode(token: &ContentToken) -> i32 {
    let default = Operand::Num(0.0);
    let op = token.operands.first().unwrap_or(&default);
    to_float(op, 0.0).trunc() as i32
}
