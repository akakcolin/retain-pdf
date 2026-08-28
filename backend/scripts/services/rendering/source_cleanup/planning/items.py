from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass

from services.rendering.source.rects import Rect
from services.rendering.source_cleanup.planning.geometry import ocr_bbox_to_pdf_rect_with_ctm
from services.rendering.source_cleanup.planning.coordinate_resolver import PageBBoxResolver
from services.rendering.source_cleanup.planning.intent_classifier import classify_source_cleanup_intent
from services.rendering.source_cleanup.planning.page_context import PlanningPageContext
from services.rendering.source_cleanup.planning.page_context import _build_context_from_fitz
from services.rendering.source_cleanup.planning.rects import merge_rects


@dataclass(frozen=True)
class SourceCleanupItemRects:
    item: dict
    pdf_rect: Rect
    view_rect: Rect
    probe_rects: tuple[Rect, ...] = ()


def iter_strip_item_rect_pairs_for_page_ctx(
    ctx: PlanningPageContext,
    translated_items: list[dict],
    *,
    resolver: PageBBoxResolver | None = None,
    prefiltered: bool = False,
) -> Iterator[SourceCleanupItemRects]:
    active_resolver = resolver or PageBBoxResolver.build(ctx)
    for item in translated_items:
        if not prefiltered and not item_should_emit_strip_rect(item):
            continue
        pdf_rect = active_resolver.ocr_bbox_to_pdf_rect(item.get("bbox", []))
        view_rect = active_resolver.resolve_bbox_rect(item.get("bbox", []))
        probe_rects = active_resolver.resolve_bbox_probe_rects(item.get("bbox", []))
        if pdf_rect is not None and view_rect is not None:
            yield SourceCleanupItemRects(
                item=item,
                pdf_rect=pdf_rect,
                view_rect=view_rect,
                probe_rects=probe_rects or (view_rect,),
            )


def iter_strip_item_rect_pairs_for_page(
    page: object,
    translated_items: list[dict],
    *,
    resolver: PageBBoxResolver | None = None,
    prefiltered: bool = False,
) -> Iterator[SourceCleanupItemRects]:
    ctx = _build_context_from_fitz(None, page)
    yield from iter_strip_item_rect_pairs_for_page_ctx(
        ctx,
        translated_items,
        resolver=resolver,
        prefiltered=prefiltered,
    )


def iter_strip_item_rects_for_page_ctx(
    ctx: PlanningPageContext,
    translated_items: list[dict],
) -> Iterator[tuple[dict, Rect]]:
    for pair in iter_strip_item_rect_pairs_for_page_ctx(ctx, translated_items):
        yield pair.item, pair.pdf_rect


def iter_strip_item_rects_for_page(
    page: object,
    translated_items: list[dict],
) -> Iterator[tuple[dict, Rect]]:
    yield from iter_strip_item_rects_for_page_ctx(
        _build_context_from_fitz(None, page),
        translated_items,
    )


def iter_formula_item_rects_for_page_ctx(
    ctx: PlanningPageContext,
    translated_items: list[dict],
) -> Iterator[tuple[dict, Rect]]:
    for item in translated_items:
        if not classify_source_cleanup_intent(item).should_protect_source:
            continue
        rect = ocr_bbox_to_pdf_rect_with_ctm(ctx.inverse_ctm, item.get("bbox", []))
        if rect is not None:
            yield item, rect


def iter_formula_item_rects_for_page(
    page: object,
    translated_items: list[dict],
) -> Iterator[tuple[dict, Rect]]:
    yield from iter_formula_item_rects_for_page_ctx(
        _build_context_from_fitz(None, page),
        translated_items,
    )


def build_source_item_rects(page: object, translated_items: list[dict]) -> list[Rect]:
    rects: list[Rect] = []
    for pair in iter_strip_item_rect_pairs_for_page(page, translated_items):
        if not pair.view_rect.is_empty:
            rects.append(pair.view_rect)
    return merge_rects(rects)


def item_should_emit_strip_rect(item: dict) -> bool:
    return classify_source_cleanup_intent(item).should_strip_text
