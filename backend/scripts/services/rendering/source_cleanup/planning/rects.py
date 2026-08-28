from __future__ import annotations

from typing import Iterable

from services.rendering.source.rects import Rect
from services.rendering.source.rects import coerce
from services.rendering.source.rects import rect_area
from services.rendering.source.rects import rect_key


def merge_rects(rects: Iterable) -> list:
    """Dedup + drop-small merge. Type-preserving: pure `Rect` inputs return pure
    rects, fitz rects come back as fitz (the fitz branch is reference/test only —
    the planning cluster feeds pure rects from `coordinate_resolver`, which
    `fitz.Rect(rect)` cannot absorb)."""
    reference = None
    pure_rects: list[Rect] = []
    for rect in rects:
        if reference is None:
            reference = rect
        pure_rects.append(coerce(rect))
    deduped: dict[tuple[int, int, int, int], Rect] = {}
    for normalized in pure_rects:
        if normalized.is_empty or rect_area(normalized) <= 0.5:
            continue
        deduped.setdefault(rect_key(normalized), normalized)
    result = sorted(
        deduped.values(),
        key=lambda value: (round(value.y0, 2), round(value.x0, 2), round(value.y1, 2)),
    )
    if isinstance(reference, Rect):
        return result
    import fitz  # type: ignore  # reference output path only

    return [
        fitz.Rect(float(r.x0), float(r.y0), float(r.x1), float(r.y1))
        for r in result
    ]
