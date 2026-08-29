from __future__ import annotations

import fitz

from services.rendering.source import _native
from services.rendering.source.rects import Rect
from services.rendering.source.rects import coerce


HEAVY_VECTOR_PAGE_DRAWINGS_THRESHOLD = 5000
VECTOR_HEAVY_PAGE_DRAWINGS_THRESHOLD = 2000


def collect_page_drawing_rects(page: fitz.Page) -> list[Rect]:
    return _native.collect_page_drawing_rects(page=page)


def _collect_page_drawing_rects_python(page: fitz.Page) -> list[Rect]:
    try:
        drawings = page.get_cdrawings() if hasattr(page, "get_cdrawings") else page.get_drawings()
    except Exception:
        return []
    return _rects_from_drawings(drawings)


def _rects_from_drawings(drawings: list[dict]) -> list[Rect]:
    """Shared drawing-rect loop for the reference (`_collect_page_drawing_rects_python`)
    and the native path (`_native.py`). `rect` may be a `fitz.Rect` (get_cdrawings)
    or a `[x0, y0, x1, y1]` list (bridge JSON); normalize before the zero-point
    check so both inputs behave identically."""
    rects: list[Rect] = []
    for drawing in drawings:
        raw_rect = drawing.get("rect")
        if not raw_rect:
            continue
        try:
            draw_rect = coerce(raw_rect)
        except Exception:
            continue
        if not any((draw_rect.x0, draw_rect.y0, draw_rect.x1, draw_rect.y1)):
            continue
        draw_rect = _expand_thin_drawing_rect(draw_rect, drawing)
        if draw_rect.is_empty:
            continue
        rects.append(draw_rect)
    return rects


def _expand_thin_drawing_rect(rect: Rect, drawing: dict) -> Rect:
    if not rect.is_empty:
        return rect
    stroke_width = _drawing_stroke_width(drawing)
    pad = max(stroke_width / 2.0, 0.5)
    x0, y0, x1, y1 = rect.x0, rect.y0, rect.x1, rect.y1
    if x0 == x1:
        x0 -= pad
        x1 += pad
    if y0 == y1:
        y0 -= pad
        y1 += pad
    return Rect(x0, y0, x1, y1)


def _drawing_stroke_width(drawing: dict) -> float:
    try:
        return float(drawing.get("width") or 1.0)
    except Exception:
        return 1.0


def page_drawing_count(page: fitz.Page) -> int:
    return _native.page_drawing_count(page=page)


def _page_drawing_count_python(page: fitz.Page) -> int:
    try:
        drawings = page.get_cdrawings() if hasattr(page, "get_cdrawings") else page.get_drawings()
    except Exception:
        return 0
    return len(drawings)


def page_should_use_cover_only(drawing_rects: list[Rect]) -> bool:
    return len(drawing_rects) >= HEAVY_VECTOR_PAGE_DRAWINGS_THRESHOLD


def page_is_vector_heavy(drawing_rects: list[Rect]) -> bool:
    return len(drawing_rects) >= VECTOR_HEAVY_PAGE_DRAWINGS_THRESHOLD


def page_should_use_cover_only_count(drawing_count: int) -> bool:
    return drawing_count >= HEAVY_VECTOR_PAGE_DRAWINGS_THRESHOLD


def page_is_vector_heavy_count(drawing_count: int) -> bool:
    return drawing_count >= VECTOR_HEAVY_PAGE_DRAWINGS_THRESHOLD
