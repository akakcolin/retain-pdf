"""Optional native (Rust) backend for source-cleanup planning page contexts.

Builds `PlanningPageContext` objects for a batch of pages through the pyo3
bridge (`rendering_bridge.read_page_cleanup_contexts`), which computes the
bboxlog, content-stream size, form-xobject flag, and page ctm natively with
mupdf-rs. Without the native module every call falls back to the pure-Python
reference in `page_context.py` (fitz), so importing this module is always safe.

The native path reads the source PDF once and skips pages the bridge omits
(load / ctm failure) — mirroring the reference `fitz.open` loop's out-of-range
skip, so missing keys mean the same thing in both paths.
"""

from __future__ import annotations

import json
from pathlib import Path

try:
    from rendering_bridge import read_page_cleanup_contexts as _native_read_page_cleanup_contexts

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def build_page_contexts(
    source_pdf_path: Path,
    page_indices: list[int],
):
    """`page_context` per-page contexts, routed to the native primitives when
    the module is built; otherwise the pure-Python reference in `page_context`."""
    if not NATIVE:
        from services.rendering.source_cleanup.planning.page_context import _build_page_contexts_python

        return _build_page_contexts_python(source_pdf_path=source_pdf_path, page_indices=page_indices)
    import fitz

    from services.rendering.source_cleanup.planning.page_context import PlanningPageContext
    from services.rendering.source_cleanup.planning.page_context import decode_bboxlog_entries
    from services.rendering.source_cleanup.planning.page_context import inverse_ctm_from_ctm
    from services.rendering.source_cleanup.pdf.constants import BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD

    pdf_bytes = source_pdf_path.read_bytes()
    raw = _native_read_page_cleanup_contexts(
        pdf_bytes,
        json.dumps([int(index) for index in page_indices]),
        BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD,
    )
    payload = json.loads(raw)
    contexts: dict[int, PlanningPageContext] = {}
    for index_key, data in payload.items():
        page_idx = int(index_key)
        rect = data["rect"]
        page_rect = fitz.Rect(float(rect[0]), float(rect[1]), float(rect[2]), float(rect[3]))
        contexts[page_idx] = PlanningPageContext(
            page_index=page_idx,
            page_rect=page_rect,
            bboxlog_entries=decode_bboxlog_entries(data.get("bboxlog", [])),
            content_stream_size=int(data.get("content_stream_size", 0)),
            has_form_xobjects=bool(data.get("has_form_xobjects", False)),
            inverse_ctm=inverse_ctm_from_ctm(data.get("ctm", [1, 0, 0, 1, 0, 0])),
        )
    return contexts
