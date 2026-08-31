from __future__ import annotations

from services.rendering import _routing
from services.rendering.document.page_map import RenderPageMap


def copy_toc(
    source_doc: fitz.Document,
    target_doc: fitz.Document,
    *,
    start_page: int = 0,
    end_page: int | None = None,
) -> fitz.Document:
    """Copy the source outline into `target_doc`, remapped to the
    `[start_page, end_page]` page range. Returns the caller's `target_doc`
    (unchanged when the source has no outline) or a fresh document from the
    native bridge (the caller applies the swap pattern, like
    `show_pdf_page_on_doc`).

    Native-only: the bridge is mandatory. When it is unavailable (or the
    feature flag forces it off) this raises instead of running the retired
    fitz reference; a native bridge failure also propagates after recording
    the fallback for D5.
    """
    import services.rendering.document._native as _native

    if not _routing.routed("source", "copy_toc", _native.NATIVE):
        raise RuntimeError(
            "source.copy_toc is native-only: the rendering_bridge copy_toc is required"
        )
    try:
        replaced = _native.copy_toc(
            source_doc,
            target_doc,
            start_page=start_page,
            end_page=end_page,
        )
        _routing.record_native_hit("source", "copy_toc")
        return replaced
    except Exception:
        _routing.record_fallback(
            "source", "copy_toc", _routing.FallbackReason.NATIVE_BRIDGE_ERROR
        )
        raise


def copy_toc_for_page_map(
    source_doc: fitz.Document,
    target_doc: fitz.Document,
    *,
    page_map: RenderPageMap | None = None,
    source_page_indices: list[int] | None = None,
) -> fitz.Document:
    """Copy the source outline into `target_doc`, remapped through the page map
    (target page = output slot + 1). Native-only: the bridge is mandatory; when
    it is unavailable this raises instead of running the retired fitz
    reference."""
    import services.rendering.document._native as _native

    if not _routing.routed("source", "copy_toc_for_page_map", _native.NATIVE):
        raise RuntimeError(
            "source.copy_toc_for_page_map is native-only: the rendering_bridge "
            "copy_toc_for_page_map is required"
        )
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
        _routing.record_native_hit("source", "copy_toc_for_page_map")
        return replaced
    except Exception:
        _routing.record_fallback(
            "source",
            "copy_toc_for_page_map",
            _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
        )
        raise
