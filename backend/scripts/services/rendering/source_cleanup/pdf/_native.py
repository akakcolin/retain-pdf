"""Optional native (Rust) backend for the bbox-text-strip executor.

Routes `source_cleanup/pdf/document.py::strip_bbox_text_rects_from_pdf_copy`
through the bridge (`rendering_bridge.strip_bbox_text_rects`, which replays the
port in `rendering_writer::cleanup_writer` over a mupdf-rs `PdfDocument` and
returns the saved PDF bytes plus `StripPdfResult` metadata). Without the native
module every call returns `None` so the caller falls back to the pure-Python
reference in `document.py` (pikepdf).

The native path handles the default production semantics (`recurse_forms` on,
form XObjects recursed in-doc). Two modes stay Python by design because the
native engine has no equivalent: `skip_form_xobject_pages=True` (Python's
per-page form detection / skip accounting) and a `max_elapsed_seconds` deadline
budget (Python's sequential form-page deadline check). The planning-level
skip/candidate metadata (complex / no-text-overlap / visual-background /
pre-skipped form pages) is carried through unchanged.
"""

from __future__ import annotations

import json
import time
from pathlib import Path

import fitz

from services.rendering import _routing
from services.rendering.source_cleanup.types import BBoxTextStripResult

try:
    from rendering_bridge import strip_bbox_text_rects as _native_strip_bbox_text_rects

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def _rect_map_json(page_rects: dict[int, list[fitz.Rect]]) -> str:
    """`{page_idx: [[x0, y0, x1, y1], ...]}` with string keys for the bridge."""
    return json.dumps(
        {
            str(page_idx): [
                [float(rect.x0), float(rect.y0), float(rect.x1), float(rect.y1)]
                for rect in rects
            ]
            for page_idx, rects in page_rects.items()
        }
    )


def strip_bbox_text_rects_from_pdf_copy(
    *,
    source_pdf_path: Path,
    output_pdf_path: Path,
    page_rects: dict[int, list[fitz.Rect]],
    page_protected_rects: dict[int, list[fitz.Rect]],
    recurse_forms: bool,
    skip_form_xobject_pages: bool,
    max_elapsed_seconds: float | None,
    pre_skipped_form_xobject_page_indices: frozenset[int],
    pre_strip_no_effect_page_indices: frozenset[int],
    skipped_complex: int,
    skipped_no_text_overlap: int,
    skipped_visual_background: int,
    skipped_complex_page_indices: frozenset[int],
    skipped_no_text_overlap_page_indices: frozenset[int],
    skipped_visual_background_page_indices: frozenset[int],
) -> BBoxTextStripResult | None:
    """Native strip when eligible, else `None` (caller uses the Python reference).

    Returns a fully-built `BBoxTextStripResult` on the native path, mirroring
    the reference result contract (counts, page-index sets, strip-no-effect).
    """
    if skip_form_xobject_pages or max_elapsed_seconds is not None:
        return None
    if not _routing.routed("source_cleanup", "strip_bbox_text_rects_from_pdf_copy", NATIVE):
        return None

    started = time.perf_counter()
    out_bytes, meta_raw = _native_strip_bbox_text_rects(
        source_pdf_path.read_bytes(),
        _rect_map_json(page_rects),
        _rect_map_json(page_protected_rects),
        recurse_forms,
    )
    meta = json.loads(meta_raw)
    elapsed = time.perf_counter() - started
    _routing.record_native_hit("source_cleanup", "strip_bbox_text_rects_from_pdf_copy")

    attempted = set(page_rects)
    changed_indices = frozenset(int(index) for index in meta["changed_page_indices"])
    # Native recurses forms in-doc, so no runtime form-page skipping occurs;
    # only the planning-level pre-skipped form pages are carried forward.
    skipped_form_xobject_page_indices = frozenset(pre_skipped_form_xobject_page_indices)
    strip_no_effect_page_indices = frozenset(
        (attempted - set(changed_indices)) | set(pre_strip_no_effect_page_indices)
    )

    if not meta["pages_changed"]:
        output_pdf_path.unlink(missing_ok=True)
        return BBoxTextStripResult(
            changed=False,
            pages_skipped_complex=skipped_complex,
            pages_skipped_no_text_overlap=skipped_no_text_overlap,
            pages_skipped_visual_background=skipped_visual_background,
            pages_skipped_form_xobject=len(skipped_form_xobject_page_indices),
            pages_strip_no_effect=len(strip_no_effect_page_indices),
            skipped_complex_page_indices=skipped_complex_page_indices,
            skipped_no_text_overlap_page_indices=skipped_no_text_overlap_page_indices,
            skipped_visual_background_page_indices=skipped_visual_background_page_indices,
            skipped_form_xobject_page_indices=skipped_form_xobject_page_indices,
            strip_no_effect_page_indices=strip_no_effect_page_indices,
        )

    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    output_pdf_path.write_bytes(out_bytes)
    print(
        f"bbox text strip native: mode=strip pages={meta['pages_changed']} "
        f"text_show_ops={meta['text_show_ops_removed']} forms={meta['forms_changed']} "
        f"skipped_form_xobject_pages={len(skipped_form_xobject_page_indices)} "
        f"strip_no_effect_pages={len(strip_no_effect_page_indices)} "
        f"elapsed={elapsed:.2f}s output={output_pdf_path}",
        flush=True,
    )
    return BBoxTextStripResult(
        changed=True,
        output_pdf_path=output_pdf_path,
        pages_changed=meta["pages_changed"],
        text_show_ops_removed=meta["text_show_ops_removed"],
        pages_skipped_complex=skipped_complex,
        pages_skipped_no_text_overlap=skipped_no_text_overlap,
        pages_skipped_visual_background=skipped_visual_background,
        pages_skipped_form_xobject=len(skipped_form_xobject_page_indices),
        pages_strip_no_effect=len(strip_no_effect_page_indices),
        forms_changed=meta["forms_changed"],
        changed_page_indices=changed_indices,
        skipped_complex_page_indices=skipped_complex_page_indices,
        skipped_no_text_overlap_page_indices=skipped_no_text_overlap_page_indices,
        skipped_visual_background_page_indices=skipped_visual_background_page_indices,
        skipped_form_xobject_page_indices=skipped_form_xobject_page_indices,
        strip_no_effect_page_indices=strip_no_effect_page_indices,
    )
