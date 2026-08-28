from __future__ import annotations

from services.rendering.source_cleanup.policy.adapter import should_skip_page_for_bbox_text_strip
from services.rendering.source_cleanup.types import BBOX_TEXT_STRIP_PAGE_SKIP_COMPLEX
from services.rendering.source_cleanup.types import BBOX_TEXT_STRIP_PAGE_SKIP_NONE


def bbox_text_strip_items_skip_reason(
    items: list[dict],
    *,
    skip_formula_pages: bool,
) -> str:
    return (
        BBOX_TEXT_STRIP_PAGE_SKIP_COMPLEX
        if should_skip_page_for_bbox_text_strip(items, skip_formula_pages=skip_formula_pages)
        else BBOX_TEXT_STRIP_PAGE_SKIP_NONE
    )
