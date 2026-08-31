from __future__ import annotations

import fitz

from services.rendering.source.rects import Rect


def word_rect(entry: tuple) -> fitz.Rect | None:
    if len(entry) < 5:
        return None
    try:
        return fitz.Rect(entry[:4])
    except Exception:
        return None


def extract_page_words(page: fitz.Page) -> list[tuple]:
    try:
        return page.get_text("words")
    except Exception:
        return []


def extract_page_text_blocks(page: fitz.Page) -> list[tuple[Rect, str]]:
    """`page.get_text("blocks")` text blocks (type 0, stripped non-empty text),
    routed through the native bridge. Native-only: raises when the bridge is
    unavailable or the page is in-memory.

    The `_native` import is lazy: `_native`'s module-top imports must not pull
    this module in at import time."""
    from services.rendering.source import _native

    return _native.extract_page_text_blocks(page=page)


def extract_page_text_spans(page: fitz.Page) -> list[tuple[Rect, str]]:
    """`page.get_text("dict")` spans (text blocks, stripped non-empty text),
    routed through the native bridge. Native-only: raises when the bridge is
    unavailable or the page is in-memory."""
    from services.rendering.source import _native

    return _native.extract_page_text_spans(page=page)


def extract_item_word_entries(
    page: fitz.Page,
    rect: fitz.Rect,
    page_words: list[tuple] | None = None,
) -> list[tuple[fitz.Rect, str]]:
    from services.rendering.source.rects import clip_rect

    clip = clip_rect(rect)
    raw_words = page.get_text("words", clip=clip) if page_words is None else page_words
    entries: list[tuple[fitz.Rect, str]] = []
    for entry in raw_words:
        candidate_rect = word_rect(entry)
        if candidate_rect is None or (clip & candidate_rect).is_empty:
            continue
        token = str(entry[4]).strip().lower() if len(entry) >= 5 else ""
        if not token:
            continue
        entries.append((candidate_rect, token))
    return entries


def rect_contains_point(rect: fitz.Rect, x: float, y: float) -> bool:
    return rect.x0 <= x <= rect.x1 and rect.y0 <= y <= rect.y1


def rect_center(rect: fitz.Rect) -> tuple[float, float]:
    return ((rect.x0 + rect.x1) / 2.0, (rect.y0 + rect.y1) / 2.0)
