from __future__ import annotations

import fitz

from services.rendering.source.rects import rect_area


MAX_TEXT_LIKE_FILL_PATH_HEIGHT_PT = 32.0
MAX_TEXT_LIKE_FILL_PATH_AREA_PT2 = 3500.0


def bboxlog_path_blocks_text_strip(kind: str, rect: fitz.Rect) -> bool:
    if kind.startswith("stroke-") and "path" in kind:
        return True
    return is_text_like_fill_path(kind, rect)


def is_text_like_fill_path(kind: str, rect: fitz.Rect | None) -> bool:
    normalized_kind = str(kind or "").strip().lower()
    if normalized_kind in {"f", "fs"}:
        return rect_is_text_like_fill_path(rect)
    if "path" not in normalized_kind or not normalized_kind.startswith("fill-"):
        return False
    return rect_is_text_like_fill_path(rect)


def rect_is_text_like_fill_path(rect: fitz.Rect | None) -> bool:
    if rect is None or rect.is_empty:
        return False
    if rect.height <= 0.0 or rect.height > MAX_TEXT_LIKE_FILL_PATH_HEIGHT_PT:
        return False
    return rect_area(rect) <= MAX_TEXT_LIKE_FILL_PATH_AREA_PT2
