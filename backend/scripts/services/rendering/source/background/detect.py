"""Background-image detection (the image-read family of `source.background`).

`page_has_large_background_image` is the single production boolean this module
runs (five call sites, all default kwargs). It routes through the native bridge
(`source._native`) when built on a file-backed page: the bridge returns image
placement rects (display-list `fill_image` + `fill_image_mask` bounds == fitz
`get_image_info` bboxes) and the boolean is computed from them here via the
shared `_has_large_background_image_from_rects` / `_tiled_images_covered`
helpers.

`pick_primary_background_image` deliberately stays on the pure-Python
reference: it returns the image xref that drives the actual rewrite
(`image_route.replace_background_image_page`), and mupdf-rs exposes no
placement→xref association, so native cannot reproduce it. The reference
implementation of the routed boolean lives in `_page_has_large_background_image_python`.
"""

from __future__ import annotations

import fitz

from services.rendering.source.rects import rect_area


TILED_BACKGROUND_IMAGE_MIN_COUNT = 8
TILED_BACKGROUND_IMAGE_COVERAGE_RATIO = 0.65
TILED_BACKGROUND_IMAGE_MIN_WIDTH_RATIO = 0.60


def _page_image_infos(page: fitz.Page, *, xrefs: bool = False) -> list[dict]:
    try:
        return list(page.get_image_info(hashes=False, xrefs=xrefs))
    except Exception:
        return []


def page_has_large_background_image(
    page: fitz.Page,
    *,
    coverage_ratio_threshold: float = 0.75,
) -> bool:
    """Route through the native bridge (image-read family) when built on a
    file-backed page; otherwise the pure-Python reference
    `_page_has_large_background_image_python`.

    The `_native` import is lazy: `_native`'s top-level imports pull in
    `document_ops`, which imports this module at top, so a module-top import
    here would be circular."""
    from services.rendering.source import _native

    return _native.page_has_large_background_image(
        page=page, coverage_ratio_threshold=coverage_ratio_threshold
    )


def _page_has_large_background_image_python(
    page: fitz.Page,
    *,
    coverage_ratio_threshold: float = 0.75,
) -> bool:
    if _page_has_primary_background_image(page, coverage_ratio_threshold=coverage_ratio_threshold):
        return True
    return page_has_tiled_background_images(page)


def _has_large_background_image_from_rects(
    raw_rects: list[fitz.Rect],
    page_rect: fitz.Rect,
    *,
    coverage_ratio_threshold: float = 0.75,
) -> bool:
    """Primary coverage check OR tiled-coverage check over raw placement rects
    (native bridge output). Mirrors `_page_has_large_background_image_python`
    but skips the xref requirement — the native placements carry no xref, so
    the primary check reduces to "some placement covers >= threshold". The
    reference's `get_images`/`get_image_rects` fallback path
    (`_pick_primary_background_image_from_xref_rects`) is intentionally not
    mirrored; when `get_image_info` fails it can still find a large image, but
    native would have fallen back to the reference already in that case."""
    rects = _intersect_image_rects(raw_rects, page_rect)
    page_area = max(rect_area(page_rect), 1.0)
    if any(rect_area(rect) / page_area >= coverage_ratio_threshold for rect in rects):
        return True
    return _tiled_images_covered(rects, page_rect)


def _intersect_image_rects(raw_rects: list[fitz.Rect], page_rect: fitz.Rect) -> list[fitz.Rect]:
    """Clip raw placement rects to the page rect and drop empty intersections
    (== `_image_rects(page)` given the raw rect list)."""
    return [rect & page_rect for rect in raw_rects if not (rect & page_rect).is_empty]


def _page_has_primary_background_image(
    page: fitz.Page,
    *,
    coverage_ratio_threshold: float,
) -> bool:
    return pick_primary_background_image(page, coverage_ratio_threshold=coverage_ratio_threshold) is not None


def pick_primary_background_image(
    page: fitz.Page,
    *,
    coverage_ratio_threshold: float = 0.75,
) -> tuple[int, fitz.Rect] | None:
    page_area = max(rect_area(page.rect), 1.0)
    best: tuple[float, int, fitz.Rect] | None = None

    for info in _page_image_infos(page, xrefs=True):
        try:
            xref = int(info.get("xref") or 0)
            rect = fitz.Rect(info.get("bbox"))
        except Exception:
            continue
        if xref <= 0 or rect.is_empty:
            continue
        coverage_ratio = rect_area(rect & page.rect) / page_area
        if coverage_ratio < coverage_ratio_threshold:
            continue
        candidate = (coverage_ratio, xref, rect)
        if best is None or candidate[0] > best[0]:
            best = candidate
    if best is None:
        best = _pick_primary_background_image_from_xref_rects(
            page,
            coverage_ratio_threshold=coverage_ratio_threshold,
        )
    if best is None:
        return None
    return best[1], best[2]


def _pick_primary_background_image_from_xref_rects(
    page: fitz.Page,
    *,
    coverage_ratio_threshold: float,
) -> tuple[float, int, fitz.Rect] | None:
    page_area = max(rect_area(page.rect), 1.0)
    best: tuple[float, int, fitz.Rect] | None = None
    try:
        image_entries = list(page.get_images(full=True))
    except Exception:
        return None

    for entry in image_entries:
        try:
            xref = int(entry[0])
        except Exception:
            continue
        if xref <= 0:
            continue
        try:
            rects = list(page.get_image_rects(xref))
        except Exception:
            rects = []
        for rect in rects:
            rect = fitz.Rect(rect)
            if rect.is_empty:
                continue
            coverage_ratio = rect_area(rect & page.rect) / page_area
            if coverage_ratio < coverage_ratio_threshold:
                continue
            candidate = (coverage_ratio, xref, rect)
            if best is None or candidate[0] > best[0]:
                best = candidate
    return best


def page_has_tiled_background_images(
    page: fitz.Page,
    *,
    coverage_ratio_threshold: float = TILED_BACKGROUND_IMAGE_COVERAGE_RATIO,
    min_image_count: int = TILED_BACKGROUND_IMAGE_MIN_COUNT,
    min_width_ratio: float = TILED_BACKGROUND_IMAGE_MIN_WIDTH_RATIO,
) -> bool:
    return _tiled_images_covered(
        _image_rects(page),
        page.rect,
        coverage_ratio_threshold=coverage_ratio_threshold,
        min_image_count=min_image_count,
        min_width_ratio=min_width_ratio,
    )


def _tiled_images_covered(
    rects: list[fitz.Rect],
    page_rect: fitz.Rect,
    *,
    coverage_ratio_threshold: float = TILED_BACKGROUND_IMAGE_COVERAGE_RATIO,
    min_image_count: int = TILED_BACKGROUND_IMAGE_MIN_COUNT,
    min_width_ratio: float = TILED_BACKGROUND_IMAGE_MIN_WIDTH_RATIO,
) -> bool:
    """Tiled-background heuristic over page-clipped rects (the body of the
    former `page_has_tiled_background_images`, parameterized by page rect so
    the native path can reuse it with bridge rects)."""
    page_area = max(rect_area(page_rect), 1.0)
    page_width = max(float(page_rect.width), 1.0)
    if len(rects) < min_image_count:
        return False

    page_wide_rects = [
        rect
        for rect in rects
        if (rect & page_rect).width / page_width >= min_width_ratio
    ]
    if len(page_wide_rects) < min_image_count:
        return False

    merged = _merge_vertical_image_bands(page_wide_rects)
    covered_area = sum(rect_area(rect & page_rect) for rect in merged)
    return covered_area / page_area >= coverage_ratio_threshold


def _image_rects(page: fitz.Page) -> list[fitz.Rect]:
    rects: list[fitz.Rect] = []
    for info in _page_image_infos(page):
        try:
            rect = fitz.Rect(info.get("bbox"))
        except Exception:
            continue
        inter = rect & page.rect
        if not inter.is_empty:
            rects.append(inter)
    return rects


def _merge_vertical_image_bands(rects: list[fitz.Rect], *, y_tolerance: float = 1.0) -> list[fitz.Rect]:
    merged: list[fitz.Rect] = []
    for rect in sorted(rects, key=lambda r: (round(r.y0, 3), round(r.x0, 3))):
        if not merged:
            merged.append(fitz.Rect(rect))
            continue
        previous = merged[-1]
        if rect.y0 <= previous.y1 + y_tolerance:
            previous.include_rect(rect)
        else:
            merged.append(fitz.Rect(rect))
    return merged


__all__ = [
    "page_has_large_background_image",
    "page_has_tiled_background_images",
    "pick_primary_background_image",
]
