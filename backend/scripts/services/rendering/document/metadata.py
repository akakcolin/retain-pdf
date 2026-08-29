from __future__ import annotations

import fitz

from services.rendering import _routing
from services.rendering.document.page_map import RenderPageMap


def _normalize_toc_levels(toc: list[list]) -> list[list]:
    normalized: list[list] = []
    previous_level = 0
    for entry in toc:
        if len(entry) < 3:
            continue
        level = int(entry[0] or 1)
        if not normalized:
            level = 1
        else:
            level = max(1, min(level, previous_level + 1))
        normalized.append([level, entry[1], entry[2]])
        previous_level = level
    return normalized


def _copy_toc_python(
    source_doc: fitz.Document,
    target_doc: fitz.Document,
    *,
    start_page: int = 0,
    end_page: int | None = None,
) -> fitz.Document:
    """Pure-fitz reference for `copy_toc`: reads the source outline, remaps the
    page range, and writes the target outline in place (returns the same
    `target_doc`)."""
    try:
        source_toc = source_doc.get_toc()
    except Exception:
        return target_doc
    if not source_toc:
        return target_doc

    last_source_page = len(source_doc) - 1
    first = max(0, start_page)
    last = last_source_page if end_page is None or end_page < 0 else min(end_page, last_source_page)
    if first > last:
        return target_doc

    remapped: list[list] = []
    target_page_count = len(target_doc)
    for level, title, page, *_rest in source_toc:
        source_page = int(page or 0) - 1
        if not (first <= source_page <= last):
            continue
        target_page = source_page - first + 1
        if not (1 <= target_page <= target_page_count):
            continue
        remapped.append([level, title, target_page])

    remapped = _normalize_toc_levels(remapped)
    if not remapped:
        return target_doc
    try:
        target_doc.set_toc(remapped)
    except Exception:
        return target_doc
    return target_doc


def copy_toc(
    source_doc: fitz.Document,
    target_doc: fitz.Document,
    *,
    start_page: int = 0,
    end_page: int | None = None,
) -> fitz.Document:
    """Copy the source outline into `target_doc`, remapped to the
    `[start_page, end_page]` page range. Returns `target_doc` (pure-fitz write
    in place, or unchanged when the source has no outline) or a fresh document
    from the native bridge (the caller applies the swap pattern, like
    `show_pdf_page_on_doc`)."""
    import services.rendering.document._native as _native

    if _routing.routed("source", "copy_toc", _native.NATIVE):
        try:
            replaced = _native.copy_toc(
                source_doc,
                target_doc,
                start_page=start_page,
                end_page=end_page,
            )
        except Exception:
            _routing.record_fallback(
                "source", "copy_toc", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
            )
        else:
            _routing.record_native_hit("source", "copy_toc")
            return replaced
    return _copy_toc_python(source_doc, target_doc, start_page=start_page, end_page=end_page)


def _copy_toc_for_page_map_python(
    source_doc: fitz.Document,
    target_doc: fitz.Document,
    *,
    page_map: RenderPageMap | None = None,
    source_page_indices: list[int] | None = None,
) -> fitz.Document:
    """Pure-fitz reference for `copy_toc_for_page_map` (returns same target)."""
    try:
        source_toc = source_doc.get_toc()
    except Exception:
        return target_doc
    if page_map is None and source_page_indices is not None:
        page_map = RenderPageMap(source_page_indices=list(source_page_indices))
    if page_map is None or not source_toc or not page_map.source_page_indices:
        return target_doc

    target_pages_by_source = {
        int(source_page_idx): output_idx + 1
        for output_idx, source_page_idx in enumerate(page_map.source_page_indices)
        if 0 <= int(source_page_idx) < len(source_doc)
    }
    if not target_pages_by_source:
        return target_doc

    target_page_count = len(target_doc)
    remapped: list[list] = []
    for level, title, page, *_rest in source_toc:
        source_page = int(page or 0) - 1
        target_page = target_pages_by_source.get(source_page)
        if target_page is None or not (1 <= target_page <= target_page_count):
            continue
        remapped.append([level, title, target_page])

    remapped = _normalize_toc_levels(remapped)
    if not remapped:
        return target_doc
    try:
        target_doc.set_toc(remapped)
    except Exception:
        return target_doc
    return target_doc


def copy_toc_for_page_map(
    source_doc: fitz.Document,
    target_doc: fitz.Document,
    *,
    page_map: RenderPageMap | None = None,
    source_page_indices: list[int] | None = None,
) -> fitz.Document:
    """Copy the source outline into `target_doc`, remapped through the page map
    (target page = output slot + 1). Returns `target_doc` (pure-fitz write in
    place) or a fresh document from the native bridge."""
    import services.rendering.document._native as _native

    if _routing.routed("source", "copy_toc_for_page_map", _native.NATIVE):
        try:
            if page_map is not None:
                indices = [int(i) for i in page_map.source_page_indices]
            else:
                indices = [int(i) for i in (source_page_indices or [])]
            replaced = _native.copy_toc_for_page_map(
                source_doc,
                target_doc,
                source_page_indices=indices,
            )
        except Exception:
            _routing.record_fallback(
                "source",
                "copy_toc_for_page_map",
                _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
            )
        else:
            _routing.record_native_hit("source", "copy_toc_for_page_map")
            return replaced
    return _copy_toc_for_page_map_python(
        source_doc,
        target_doc,
        page_map=page_map,
        source_page_indices=source_page_indices,
    )
