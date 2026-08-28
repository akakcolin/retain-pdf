from __future__ import annotations

from services.rendering.source.rects import Rect
from services.rendering.source.rects import coerce

from services.rendering.source.cleanup.text_extract import extract_page_text_spans


DISPLAY_INTRUSIVE_HEIGHT_RATIO = 3.0
DISPLAY_INTRUSIVE_MAX_TEXT_LEN = 2


def collect_page_intrusive_display_text_rects(page: object) -> list[Rect]:
    """Large short-text spans that may intrude into display-math regions.

    Reads through the native spans primitive (`extract_page_text_spans`); the
    primitive's non-empty-text contract drops whitespace-only spans that the
    raw `get_text("dict")` reference used to flag (see 分歧台账)."""
    span_heights: list[float] = []
    candidates: list[tuple[Rect, str, float]] = []
    for raw_rect, text in extract_page_text_spans(page):
        rect = coerce(raw_rect)
        height = max(0.0, rect.y1 - rect.y0)
        if height <= 0.5:
            continue
        span_heights.append(height)
        candidates.append((rect, text, height))

    if not span_heights:
        return []
    span_heights.sort()
    baseline_height = span_heights[len(span_heights) // 2]
    if baseline_height <= 0.5:
        return []

    intrusive: list[Rect] = []
    for rect, text, height in candidates:
        compact_text = "".join(text.split())
        if len(compact_text) > DISPLAY_INTRUSIVE_MAX_TEXT_LEN:
            continue
        if height < baseline_height * DISPLAY_INTRUSIVE_HEIGHT_RATIO:
            continue
        intrusive.append(rect)
    return intrusive
