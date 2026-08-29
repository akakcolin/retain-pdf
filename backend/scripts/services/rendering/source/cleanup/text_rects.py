from __future__ import annotations

from services.rendering.source.cleanup.redaction_padding import expand_item_rect
from services.rendering.source.cleanup.redaction_padding import expand_word_rect
from services.rendering.source.rects import Rect
from services.rendering.source.rects import rect_key


def word_entries_to_redaction_rects(entries: list[tuple[Rect, str]]) -> list[Rect]:
    rects: list[Rect] = []
    seen: set[tuple[int, int, int, int]] = set()
    for rect, _token in entries:
        expanded = expand_word_rect(rect)
        key = rect_key(expanded)
        if key in seen:
            continue
        seen.add(key)
        rects.append(expanded)
    return rects


def item_bbox_redaction_rect(rect: Rect) -> list[Rect]:
    expanded = expand_item_rect(rect)
    return [expanded] if not expanded.is_empty else []
