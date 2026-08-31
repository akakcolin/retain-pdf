from __future__ import annotations

from pathlib import Path

from services.rendering.output.typst.color_adapt import apply_adaptive_overlay_colors_batch


RenderColorProfile = dict[str, dict[str, tuple[float, float, float]]]


def apply_overlay_page_colors(
    doc: fitz.Document,
    page_indices: list[int],
    translated_pages: dict[int, list[dict]],
    *,
    precomputed_colors_by_item_id: RenderColorProfile | None = None,
    source_pdf_path: Path | None = None,
) -> dict[int, list[dict]]:
    """`color_adapt` adaptation for the given page subset. When a source file is
    available it routes through the batch entry point (out-of-range pages get the
    default cover/text colors); when `source_pdf_path` is None — production
    callers always pass one — every item falls back to the default colors."""
    if source_pdf_path is None:
        return {
            page_idx: [
                {
                    **item,
                    "_render_cover_fill": item.get("_render_cover_fill", (1, 1, 1)),
                    "_render_text_color": item.get("_render_text_color", (0, 0, 0)),
                }
                for item in translated_pages.get(page_idx, [])
            ]
            for page_idx in page_indices
        }
    adapted = apply_adaptive_overlay_colors_batch(
        source_pdf_path=source_pdf_path,
        pages={page_idx: translated_pages.get(page_idx, []) for page_idx in page_indices},
        precomputed_colors_by_item_id=precomputed_colors_by_item_id,
    )
    return {
        page_idx: [
            {
                **item,
                "_render_cover_fill": item.get("_render_cover_fill", (1, 1, 1)),
                "_render_text_color": item.get("_render_text_color", (0, 0, 0)),
            }
            for item in adapted.get(page_idx, [])
        ]
        for page_idx in page_indices
    }


__all__ = ["RenderColorProfile", "apply_overlay_page_colors"]
