from __future__ import annotations

from typing import Iterable

from services.rendering.source.rects import Rect
from services.rendering.source.rects import coerce
from services.rendering.source.rects import rect_area
from services.rendering.source.rects import rect_key


def merge_rects(rects: Iterable) -> list[Rect]:
    """Dedup + drop-small merge. Always returns pure `Rect`; any fitz rects are
    coerced on entry (the planning cluster feeds pure rects from
    `coordinate_resolver`, which `fitz.Rect(rect)` cannot absorb)."""
    pure_rects: list[Rect] = [coerce(rect) for rect in rects]
    deduped: dict[tuple[int, int, int, int], Rect] = {}
    for normalized in pure_rects:
        if normalized.is_empty or rect_area(normalized) <= 0.5:
            continue
        deduped.setdefault(rect_key(normalized), normalized)
    return sorted(
        deduped.values(),
        key=lambda value: (round(value.y0, 2), round(value.x0, 2), round(value.y1, 2)),
    )
