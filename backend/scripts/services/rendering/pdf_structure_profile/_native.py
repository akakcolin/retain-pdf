"""Optional native (Rust) backend for `build_pdf_structure_profile`.

Builds each `PdfStructurePageProfile` from the pyo3 bridge primitives
(`read_page_cleanup_contexts` for rect/ctm/bboxlog, `read_page_text_spans` for
the text spans, `read_page_form_xobjects` for the form XObjects) plus the pure
`coordinate_resolver` / `source.rects` geometry helpers, so the prewarm path
needs no live fitz page. Without the native module — or when a native call
fails — `build_pdf_structure_profile` falls back to the pure-Python reference
`sampler._build_pdf_structure_profile_python`, so importing this module is
always safe.

Documented divergence: `form_xobjects` reads the (inherited) `/Resources/XObject`
resource dict — one entry per named form — while fitz `page.get_xobjects()`
reports each `Do` instance, so a form that invokes another form yields extra
entries in the reference. The field is manifest-only (no routing consumes it),
and the corpus pins single-level forms where both paths agree.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering import _routing
from services.rendering.pdf_structure_profile.contracts import PDF_STRUCTURE_PROFILE_ALGORITHM_VERSION
from services.rendering.pdf_structure_profile.contracts import PdfObjectBox
from services.rendering.pdf_structure_profile.contracts import PdfStructureDocumentProfile
from services.rendering.pdf_structure_profile.contracts import PdfStructureItemHit
from services.rendering.pdf_structure_profile.contracts import PdfStructurePageProfile
from services.rendering.pdf_structure_profile.contracts import bbox_from_rect
from services.rendering.source.rects import Rect
from services.rendering.source.rects import inverse_affine
from services.rendering.source.rects import rect_area
from services.rendering.source_cleanup.planning.coordinate_resolver import TextRectIndex
from services.rendering.source_cleanup.planning.coordinate_resolver import bboxlog_kind
from services.rendering.source_cleanup.planning.coordinate_resolver import bboxlog_rect
from services.rendering.source_cleanup.planning.coordinate_resolver import choose_page_coordinate_candidate_with_inverse_ctm
from services.rendering.source_cleanup.planning.coordinate_resolver import raw_bbox_rect
from services.rendering.source_cleanup.planning.drawing_classifier import bboxlog_path_blocks_text_strip

MIN_ITEM_TEXT_OBJECT_OVERLAP_RATIO = 0.2

try:
    from rendering_bridge import read_page_cleanup_contexts as _native_read_page_cleanup_contexts
    from rendering_bridge import read_page_form_xobjects as _native_read_page_form_xobjects
    from rendering_bridge import read_page_geometry as _native_read_page_geometry
    from rendering_bridge import read_page_text_spans as _native_read_page_text_spans

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def build_pdf_structure_profile(
    source_pdf_path: Path,
    pages: dict[int, list[dict]] | None = None,
) -> PdfStructureDocumentProfile:
    """`sampler.build_pdf_structure_profile`, routed to the native bridge when
    built; otherwise the pure-Python reference `_build_pdf_structure_profile_python`.
    Any native failure falls back to the reference (the prewarm caller treats a
    profile failure as optional, so parity beats raising)."""
    if not _routing.routed("pdf_structure_profile", "build_pdf_structure_profile", NATIVE):
        return _build_python(source_pdf_path, pages)
    try:
        result = _build_native(source_pdf_path, pages)
        _routing.record_native_hit("pdf_structure_profile", "build_pdf_structure_profile")
        return result
    except Exception as exc:
        _routing.record_fallback(
            "pdf_structure_profile",
            "build_pdf_structure_profile",
            _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
        )
        print(
            f"pdf structure profile native failed {type(exc).__name__}: {exc}; falling back to reference",
            flush=True,
        )
        return _build_python(source_pdf_path, pages)


def _build_python(source_pdf_path: Path, pages: dict[int, list[dict]] | None) -> PdfStructureDocumentProfile:
    # Lazy: sampler's public delegator imports this shim, so a top-level
    # reference import here would be circular.
    from services.rendering.pdf_structure_profile.sampler import _build_pdf_structure_profile_python

    return _build_pdf_structure_profile_python(source_pdf_path, pages)


def _build_native(source_pdf_path: Path, pages: dict[int, list[dict]] | None) -> PdfStructureDocumentProfile:
    pdf_bytes = source_pdf_path.read_bytes()
    if pages is None:
        page_count = int(json.loads(_native_read_page_geometry(pdf_bytes))["page_count"])
        page_indices = list(range(page_count))
        items_by_page = {index: [] for index in page_indices}
    else:
        page_indices = [int(index) for index in pages]
        items_by_page = pages
    if not page_indices:
        return PdfStructureDocumentProfile(algorithm=PDF_STRUCTURE_PROFILE_ALGORITHM_VERSION, pages={})

    contexts_raw = json.loads(
        _native_read_page_cleanup_contexts(pdf_bytes, json.dumps(page_indices), 0)
    )
    profiles: dict[int, PdfStructurePageProfile] = {}
    for index_key, ctx in contexts_raw.items():
        page_index = int(index_key)
        profiles[page_index] = _assemble_page_profile(
            page_index=page_index,
            ctx=ctx,
            items=items_by_page.get(page_index, []),
            pdf_bytes=pdf_bytes,
        )
    return PdfStructureDocumentProfile(
        algorithm=PDF_STRUCTURE_PROFILE_ALGORITHM_VERSION,
        pages=profiles,
    )


def _assemble_page_profile(
    *,
    page_index: int,
    ctx: dict,
    items: list[dict],
    pdf_bytes: bytes,
) -> PdfStructurePageProfile:
    rect = ctx["rect"]
    page_rect = Rect(float(rect[0]), float(rect[1]), float(rect[2]), float(rect[3]))
    text_objects, image_objects, path_objects = _bboxlog_objects(page_index, ctx.get("bboxlog", []))
    text_spans = _text_span_objects(
        page_index,
        json.loads(_native_read_page_text_spans(pdf_bytes, page_index)),
    )
    form_xobjects = _form_xobject_objects(
        page_index,
        json.loads(_native_read_page_form_xobjects(pdf_bytes, page_index)),
    )
    item_hits = _item_hits(
        page_index,
        items,
        text_objects,
        inverse_affine(ctx.get("ctm", [1, 0, 0, 1, 0, 0])),
    )
    return PdfStructurePageProfile(
        page_index=page_index,
        page_width_pt=float(page_rect.width),
        page_height_pt=float(page_rect.height),
        text_objects=tuple(text_objects),
        text_spans=tuple(text_spans),
        path_objects=tuple(path_objects),
        image_objects=tuple(image_objects),
        form_xobjects=tuple(form_xobjects),
        item_hits=tuple(item_hits),
    )


def _bboxlog_objects(page_index: int, entries: list) -> tuple[list[PdfObjectBox], list[PdfObjectBox], list[PdfObjectBox]]:
    text_objects: list[PdfObjectBox] = []
    image_objects: list[PdfObjectBox] = []
    path_objects: list[PdfObjectBox] = []
    for index, entry in enumerate(entries):
        kind = bboxlog_kind(entry)
        rect = bboxlog_rect(entry)
        if rect is None:
            continue
        if "text" in kind:
            text_objects.append(_object_box(page_index, index, "text_object", rect, "bboxlog", flags=(kind,)))
        elif "image" in kind:
            image_objects.append(_object_box(page_index, index, "image_object", rect, "bboxlog", flags=(kind,)))
        elif "path" in kind or bboxlog_path_blocks_text_strip(kind, rect):
            flags = (kind, "blocks_text_strip") if bboxlog_path_blocks_text_strip(kind, rect) else (kind,)
            path_objects.append(_object_box(page_index, index, "path_object", rect, "bboxlog", flags=flags))
    return text_objects, image_objects, path_objects


def _text_span_objects(page_index: int, spans: list) -> list[PdfObjectBox]:
    objects: list[PdfObjectBox] = []
    span_index = 0
    for span in spans:
        rect = _rect_from_coords(span[:4])
        text = str(span[4] or "").strip() if len(span) >= 5 else ""
        if rect is None or not text:
            continue
        objects.append(
            PdfObjectBox(
                object_id=f"p{page_index + 1:03d}-span-{span_index:04d}",
                page_index=page_index,
                object_type="text_span",
                bbox=bbox_from_rect(rect),
                source="text_dict",
                text=text,
            )
        )
        span_index += 1
    return objects


def _form_xobject_objects(page_index: int, entries: list) -> list[PdfObjectBox]:
    objects: list[PdfObjectBox] = []
    for index, entry in enumerate(entries):
        rect = _rect_from_coords(entry.get("bbox", []))
        if rect is None:
            continue
        objects.append(
            _object_box(
                page_index,
                index,
                "form_xobject",
                rect,
                "xobjects",
                flags=(str(entry.get("xref") or 0),),
            )
        )
    return objects


def _item_hits(
    page_index: int,
    items: list[dict],
    text_objects: list[PdfObjectBox],
    inverse_ctm,
) -> list[PdfStructureItemHit]:
    text_rects = tuple(Rect(*obj.bbox) for obj in text_objects)
    if not text_rects:
        return []
    candidate = choose_page_coordinate_candidate_with_inverse_ctm(
        inverse_ctm,
        (item.get("bbox", []) for item in items),
        TextRectIndex.build(text_rects),
    )
    hits: list[PdfStructureItemHit] = []
    for item in items:
        item_id = str(item.get("item_id") or "").strip()
        raw_rect = raw_bbox_rect(item.get("bbox", []))
        if not item_id or raw_rect is None:
            continue
        item_rect = candidate.transform(inverse_ctm, raw_rect)
        if item_rect.is_empty:
            continue
        best_object: PdfObjectBox | None = None
        best_ratio = 0.0
        for obj, text_rect in zip(text_objects, text_rects):
            ratio = _overlap_ratio(item_rect, text_rect)
            if ratio > best_ratio:
                best_ratio = ratio
                best_object = obj
        if best_object is not None and best_ratio >= MIN_ITEM_TEXT_OBJECT_OVERLAP_RATIO:
            hits.append(
                PdfStructureItemHit(
                    item_id=item_id,
                    object_id=best_object.object_id,
                    object_type=best_object.object_type,
                    overlap_ratio=round(best_ratio, 4),
                )
            )
    return hits


def _object_box(
    page_index: int,
    index: int,
    object_type: str,
    rect: Rect,
    source: str,
    *,
    flags: tuple[str, ...] = (),
) -> PdfObjectBox:
    return PdfObjectBox(
        object_id=f"p{page_index + 1:03d}-{object_type}-{index:04d}",
        page_index=page_index,
        object_type=object_type,
        bbox=bbox_from_rect(rect),
        source=source,
        flags=tuple(flag for flag in flags if flag),
    )


def _rect_from_coords(value: object) -> Rect | None:
    if not isinstance(value, (list, tuple)) or len(value) < 4:
        return None
    try:
        rect = Rect(float(value[0]), float(value[1]), float(value[2]), float(value[3]))
    except (TypeError, ValueError):
        return None
    return None if rect.is_empty else rect


def _overlap_ratio(left: Rect, right: Rect) -> float:
    left_area = rect_area(left)
    right_area = rect_area(right)
    if left_area <= 0.0 or right_area <= 0.0:
        return 0.0
    overlap = rect_area(left & right)
    return max(overlap / left_area, overlap / right_area)
