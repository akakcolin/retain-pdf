from __future__ import annotations

from services.rendering.source.rects import Rect
from services.rendering.source.rects import rect_area
from services.rendering.source_cleanup.planning.spatial_index import RectOverlapIndex


MIN_UNSAFE_VECTOR_OVERLAP_AREA_PT2 = 0.5


def rect_overlaps_any_unsafe_vector(rect: Rect, unsafe_rects: RectOverlapIndex | list[Rect]) -> bool:
    if isinstance(unsafe_rects, RectOverlapIndex):
        return unsafe_rects.overlaps_any(rect, min_overlap_area=MIN_UNSAFE_VECTOR_OVERLAP_AREA_PT2)
    return any(rect_overlap_area(rect, unsafe_rect) > MIN_UNSAFE_VECTOR_OVERLAP_AREA_PT2 for unsafe_rect in unsafe_rects)


def rect_overlap_area(left: Rect, right: Rect) -> float:
    return rect_area(left & right)
