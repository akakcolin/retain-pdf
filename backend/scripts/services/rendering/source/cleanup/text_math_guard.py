from __future__ import annotations

from services.rendering.source.items import get_item_formula_map
from services.rendering.source.rects import Rect
from services.rendering.source.rects import rects_overlap_area


DISPLAY_INTRUSIVE_OVERLAP_AREA_MIN = 6.0


def rect_intersects_intrusive_display_text(rect: Rect, intrusive_rects: list[Rect]) -> bool:
    for intrusive_rect in intrusive_rects:
        if rects_overlap_area(rect, intrusive_rect) >= DISPLAY_INTRUSIVE_OVERLAP_AREA_MIN:
            return True
    return False


def filter_rects_away_from_special_math(
    rects: list[Rect],
    special_math_rects: list[Rect] | None,
) -> list[Rect]:
    if not rects or not special_math_rects:
        return rects
    filtered: list[Rect] = []
    for rect in rects:
        if any(rects_overlap_area(rect, math_rect) > 0.5 for math_rect in special_math_rects):
            continue
        filtered.append(rect)
    return filtered


def item_has_formula(item: dict) -> bool:
    return bool(get_item_formula_map(item))
