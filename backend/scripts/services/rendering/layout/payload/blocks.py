from __future__ import annotations

from foundation.config import layout
from services.rendering.layout.payload import _native as _payload_native
from services.rendering.layout.payload.collision import mark_adjacent_collision_risk
from services.rendering.layout.model.models import RenderBlock
from services.rendering.layout.payload.render_item import seed_render_fields
from services.rendering.layout.typography_memory.learning import observe_payload_typography


def build_render_blocks(
    translated_items: list[dict],
    *,
    page_width: float | None = None,
    page_height: float | None = None,
    book_body_font_target: float | None = None,
) -> list[RenderBlock]:
    for item in translated_items:
        seed_render_fields(item)
    block_payloads, page_text_width_med = _payload_native.build_block_payloads(
        translated_items=translated_items,
        page_width=page_width,
        page_height=page_height,
    )
    ordered_payloads = sorted(block_payloads, key=lambda payload: (payload["inner_bbox"][1], payload["inner_bbox"][0]))
    _payload_native.apply_body_pipeline(
        ordered_payloads,
        page_text_width_med=page_text_width_med,
        book_body_font_target=book_body_font_target,
    )
    mark_adjacent_collision_risk(ordered_payloads)
    observe_payload_typography(ordered_payloads)
    return _payload_native.emit_render_blocks(block_payloads)


def build_render_block_payloads(
    translated_items: list[dict],
    *,
    page_width: float | None = None,
    page_height: float | None = None,
) -> tuple[list[dict], float]:
    del page_height
    return _payload_native.build_block_payloads(translated_items=translated_items, page_width=page_width)


def resolve_book_body_font_target_from_payloads(page_payloads: list[tuple[list[dict], float]]) -> float | None:
    if layout.FONT_UNIFY_MODE == "off":
        return None
    return _payload_native.resolve_book_body_font_target_from_payloads(page_payloads)
