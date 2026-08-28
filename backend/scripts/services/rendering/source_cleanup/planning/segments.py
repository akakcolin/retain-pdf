from __future__ import annotations

from services.rendering.source.rects import Rect
from services.rendering.source.rects import coerce
from services.rendering.source.rects import preserve
from services.rendering.source.rects import preserve_list


MIN_CLEANUP_SEGMENT_WIDTH_PT = 1.0
MIN_CLEANUP_SEGMENT_HEIGHT_PT = 1.0
MIN_CLEANUP_SEGMENT_AREA_PT2 = 2.0
STRIP_SEGMENT_PAD_X_PT = 1.0
STRIP_SEGMENT_PAD_Y_PT = 1.0
FORMULA_SPLIT_SEGMENT_PAD_X_PT = 1.0
FORMULA_SPLIT_SEGMENT_PAD_Y_PT = 0.0
BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT = 1.0
BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT = 1.0


def strip_segments_for_text_rect(text_rect: Rect, formula_rects: list[Rect]) -> list[Rect]:
    formula_guards = [bbox_text_strip_formula_guard_rect(formula) for formula in formula_rects if not formula.is_empty]
    segments = split_rect_around_guards(text_rect, formula_guards, min_height_pt=2.0, min_area_pt2=2.0)
    was_split_for_formula = len(segments) != 1 or (segments and segments[0] != text_rect)
    padded_segments: list[Rect] = []
    for segment in segments:
        if segment.is_empty:
            continue
        if was_split_for_formula:
            padded_segments.append(
                segment
                + (
                    -FORMULA_SPLIT_SEGMENT_PAD_X_PT,
                    -FORMULA_SPLIT_SEGMENT_PAD_Y_PT,
                    FORMULA_SPLIT_SEGMENT_PAD_X_PT,
                    FORMULA_SPLIT_SEGMENT_PAD_Y_PT,
                )
            )
        else:
            padded_segments.append(
                segment
                + (
                    -STRIP_SEGMENT_PAD_X_PT,
                    -STRIP_SEGMENT_PAD_Y_PT,
                    STRIP_SEGMENT_PAD_X_PT,
                    STRIP_SEGMENT_PAD_Y_PT,
                )
            )
    return padded_segments


def bbox_text_strip_formula_guard_rect(formula: Rect) -> Rect:
    return Rect(
        formula.x0 - BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT,
        formula.y0 - BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT,
        formula.x1 + BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT,
        formula.y1 + BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT,
    )


def split_rect_around_guards(
    rect: Rect,
    guards: list[Rect],
    *,
    min_width_pt: float = MIN_CLEANUP_SEGMENT_WIDTH_PT,
    min_height_pt: float = MIN_CLEANUP_SEGMENT_HEIGHT_PT,
    min_area_pt2: float = MIN_CLEANUP_SEGMENT_AREA_PT2,
) -> list[Rect]:
    pure_rect = coerce(rect)
    if pure_rect.is_empty:
        return []
    fragments: list[Rect] = [pure_rect]
    for guard in guards:
        pure_guard = coerce(guard)
        if pure_guard.is_empty:
            continue
        next_fragments: list[Rect] = []
        for fragment in fragments:
            next_fragments.extend(
                subtract_guard_from_rect(
                    fragment,
                    pure_guard,
                    min_width_pt=min_width_pt,
                    min_height_pt=min_height_pt,
                    min_area_pt2=min_area_pt2,
                )
            )
        fragments = next_fragments
        if not fragments:
            break
    return preserve_list(rect, fragments)


def subtract_guard_from_rect(
    rect: Rect,
    guard: Rect,
    *,
    min_width_pt: float = MIN_CLEANUP_SEGMENT_WIDTH_PT,
    min_height_pt: float = MIN_CLEANUP_SEGMENT_HEIGHT_PT,
    min_area_pt2: float = MIN_CLEANUP_SEGMENT_AREA_PT2,
) -> list[Rect]:
    pure_rect = coerce(rect)
    pure_guard = coerce(guard)
    overlap = pure_rect & pure_guard
    if overlap.is_empty:
        return [preserve(rect, pure_rect)]

    candidates = [
        Rect(pure_rect.x0, pure_rect.y0, pure_rect.x1, overlap.y0),
        Rect(pure_rect.x0, overlap.y1, pure_rect.x1, pure_rect.y1),
        Rect(pure_rect.x0, overlap.y0, overlap.x0, overlap.y1),
        Rect(overlap.x1, overlap.y0, pure_rect.x1, overlap.y1),
    ]
    usable = [
        fragment
        for fragment in candidates
        if _is_usable_fragment(
            fragment,
            min_width_pt=min_width_pt,
            min_height_pt=min_height_pt,
            min_area_pt2=min_area_pt2,
        )
    ]
    return preserve_list(rect, usable)


def _is_usable_fragment(
    rect: Rect,
    *,
    min_width_pt: float,
    min_height_pt: float,
    min_area_pt2: float,
) -> bool:
    return (
        not rect.is_empty
        and rect.width >= min_width_pt
        and rect.height >= min_height_pt
        and rect.width * rect.height >= min_area_pt2
    )
