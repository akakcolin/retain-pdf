from __future__ import annotations

from services.rendering.source.rects import Rect
from services.rendering.source_cleanup.pdf.constants import BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT
from services.rendering.source_cleanup.pdf.constants import BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT
from services.rendering.source_cleanup.planning.coordinate_resolver import raw_bbox_rect
from services.rendering.source_cleanup.planning.coordinate_resolver import resolve_bbox_rect
from services.rendering.source_cleanup.planning.segments import split_rect_around_guards


def rect_tuple(rect: Rect) -> tuple[float, float, float, float]:
    return (round(float(rect.x0), 3), round(float(rect.y0), 3), round(float(rect.x1), 3), round(float(rect.y1), 3))


def ocr_bbox_to_pdf_rect_with_ctm(inverse_ctm: object, bbox: object) -> Rect | None:
    rect = raw_bbox_rect(bbox)
    if rect is None:
        return None
    pdf_rect = rect * inverse_ctm
    return None if pdf_rect.is_empty else pdf_rect


def ocr_bbox_to_pdf_rect(page: object, bbox: object) -> Rect | None:
    return ocr_bbox_to_pdf_rect_with_ctm(~page.transformation_matrix, bbox)


def ocr_bbox_to_view_rect(page: object, bbox: object) -> Rect | None:
    return resolve_bbox_rect(page, bbox)


def formula_guard_rects(
    formula_rects: list[Rect],
    *,
    strip_rects: list[Rect] | None = None,
) -> list[Rect]:
    return [bbox_text_strip_formula_guard_rect(rect) for rect in formula_rects if not rect.is_empty]


def split_rect_away_from_formulas(rect: Rect, formula_rects: list[Rect]) -> list[Rect]:
    guards = [bbox_text_strip_formula_guard_rect(formula) for formula in formula_rects]
    return split_rect_around_guards(rect, guards)


def bbox_text_strip_formula_guard_rect(formula: Rect) -> Rect:
    return Rect(
        formula.x0 - BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT,
        formula.y0 - BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT,
        formula.x1 + BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_X_PT,
        formula.y1 + BBOX_TEXT_STRIP_FORMULA_GUARD_PAD_Y_PT,
    )


def shrink_rect_away_from_formulas(rect: Rect, formula_rects: list[Rect]) -> Rect:
    protected_segments = split_rect_away_from_formulas(rect, formula_rects)
    if not protected_segments:
        return Rect()
    if len(protected_segments) == 1:
        return protected_segments[0]
    largest = max(protected_segments, key=lambda segment: segment.width * segment.height)
    return largest
