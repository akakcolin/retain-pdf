"""Native (Rust) backend for source-cleanup planning page contexts.

Builds `PlanningPageContext` objects for a batch of pages through the pyo3
bridge (`rendering_bridge.read_page_cleanup_contexts`), which computes the
bboxlog, content-stream size, form-xobject flag, and page ctm natively with
mupdf-rs. The batch entry points are native-only: without the bridge they
raise instead of running the retired Python references.

The native path reads the source PDF once and skips pages the bridge omits
(load / ctm failure), so missing keys mean the same thing across callers.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering import _routing
from services.rendering.source.rects import Rect

try:
    from rendering_bridge import read_page_cleanup_contexts as _native_read_page_cleanup_contexts
    from rendering_bridge import plan_source_cleanup_native as _native_plan_source_cleanup
    from rendering_bridge import (
        uncovered_unsafe_vector_item_ids_native as _native_uncovered_unsafe_vector_item_ids,
    )

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def build_page_contexts(
    source_pdf_path: Path,
    page_indices: list[int],
):
    """`page_context` per-page contexts, native-only: requires the rendering_bridge
    `read_page_cleanup_contexts` primitive. The per-page reference helper
    `page_context._build_context_from_fitz` remains as the parity/test surface."""
    if not _routing.routed("source_cleanup_planning", "build_page_contexts", NATIVE):
        raise RuntimeError(
            "source_cleanup_planning.build_page_contexts is native-only: the "
            "rendering_bridge read_page_cleanup_contexts primitive is required"
        )
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
        page_rect = Rect(float(rect[0]), float(rect[1]), float(rect[2]), float(rect[3]))
        contexts[page_idx] = PlanningPageContext(
            page_index=page_idx,
            page_rect=page_rect,
            bboxlog_entries=decode_bboxlog_entries(data.get("bboxlog", [])),
            content_stream_size=int(data.get("content_stream_size", 0)),
            has_form_xobjects=bool(data.get("has_form_xobjects", False)),
            inverse_ctm=inverse_ctm_from_ctm(data.get("ctm", [1, 0, 0, 1, 0, 0])),
        )
    _routing.record_native_hit("source_cleanup_planning", "build_page_contexts")
    return contexts


def _candidates_from_payload(payload: dict) -> "BBoxTextStripCandidates":
    """Rebuild `BBoxTextStripCandidates` from the native bridge's serialized
    shape (list rects / list ids → the dataclass tuple/frozenset forms)."""
    from services.rendering.source_cleanup.types import BBoxTextStripCandidates

    def rects_by_page(value: object) -> dict[int, tuple[tuple[float, ...], ...]] | None:
        if not value:
            return None
        return {
            int(key): tuple(tuple(float(coord) for coord in rect) for rect in rects)
            for key, rects in value.items()
        }

    def int_set(value: object):
        return frozenset(int(index) for index in value)

    return BBoxTextStripCandidates(
        page_rects=rects_by_page(payload.get("page_rects")) or {},
        page_protected_rects=rects_by_page(payload.get("page_protected_rects")),
        uncovered_unsafe_vector_item_ids=frozenset(
            str(item_id) for item_id in payload.get("uncovered_unsafe_vector_item_ids", [])
        ),
        pages_skipped_complex=int(payload.get("pages_skipped_complex", 0)),
        pages_skipped_no_text_overlap=int(payload.get("pages_skipped_no_text_overlap", 0)),
        pages_skipped_visual_background=int(payload.get("pages_skipped_visual_background", 0)),
        skipped_complex_page_indices=int_set(payload.get("skipped_complex_page_indices", [])),
        skipped_no_text_overlap_page_indices=int_set(
            payload.get("skipped_no_text_overlap_page_indices", [])
        ),
        skipped_visual_background_page_indices=int_set(
            payload.get("skipped_visual_background_page_indices", [])
        ),
        page_features={
            int(key): dict(features) for key, features in payload.get("page_features", {}).items()
        },
    )


def plan_source_cleanup(
    *,
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]],
    protected_pages: dict[int, list[dict]] | None = None,
    skip_formula_pages: bool = False,
    skip_form_xobject_pages: bool = True,
    document_analysis=None,
    pdf_structure_profile=None,
) -> "BBoxTextStripCandidates":
    """`planner.plan_source_cleanup`, native-only: the whole candidates assembly
    runs in Rust (`rendering_bridge.plan_source_cleanup_native`). `document_analysis`
    supplies the per-page `allows_pikepdf_text_strip` route gate; pages absent
    from it have no analysis route and are never gated."""
    if not _routing.routed("source_cleanup_planning", "plan_source_cleanup", NATIVE):
        raise RuntimeError(
            "source_cleanup_planning.plan_source_cleanup is native-only: the "
            "rendering_bridge plan_source_cleanup_native primitive is required"
        )
    allows_map = None
    if document_analysis is not None:
        allows_map = {
            int(page_idx): bool(page.allows_pikepdf_text_strip)
            for page_idx, page in document_analysis.pages.items()
        }
    pdf_bytes = source_pdf_path.read_bytes()
    raw = _native_plan_source_cleanup(
        pdf_bytes,
        json.dumps(translated_pages),
        json.dumps(protected_pages or {}),
        json.dumps(
            {
                "skip_formula_pages": bool(skip_formula_pages),
                "skip_form_xobject_pages": bool(skip_form_xobject_pages),
                "allows_pikepdf_strip": allows_map,
            }
        ),
    )
    _routing.record_native_hit("source_cleanup_planning", "plan_source_cleanup")
    return _candidates_from_payload(json.loads(raw))


def item_ids_with_uncovered_unsafe_vector_overlap(
    *,
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]],
) -> frozenset[str]:
    """`planner.item_ids_with_uncovered_unsafe_vector_overlap`, native-only:
    requires the rendering_bridge `uncovered_unsafe_vector_item_ids_native`
    primitive. The per-page reference
    `planner.page_uncovered_unsafe_vector_item_ids_ctx` remains as the
    parity/test surface."""
    if not _routing.routed(
        "source_cleanup_planning", "item_ids_with_uncovered_unsafe_vector_overlap", NATIVE
    ):
        raise RuntimeError(
            "source_cleanup_planning.item_ids_with_uncovered_unsafe_vector_overlap is "
            "native-only: the rendering_bridge uncovered_unsafe_vector_item_ids_native "
            "primitive is required"
        )
    pdf_bytes = source_pdf_path.read_bytes()
    raw = _native_uncovered_unsafe_vector_item_ids(pdf_bytes, json.dumps(translated_pages))
    _routing.record_native_hit(
        "source_cleanup_planning", "item_ids_with_uncovered_unsafe_vector_overlap"
    )
    return frozenset(str(item_id) for item_id in json.loads(raw))
