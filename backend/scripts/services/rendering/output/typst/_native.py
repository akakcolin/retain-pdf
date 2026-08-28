"""Optional native (Rust) backend for the Typst output layer.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes the Typst source
emitter through the ported Rust implementation. Without the native module every
function falls back to the pure-Python implementation, so importing this module
is always safe.

Native output is identical to the Python emitter modulo numeric literal
formatting (e.g. `260pt` vs `260.0pt`); both parse to the same Typst value.
"""

from __future__ import annotations

import json
from pathlib import Path

from foundation.config import fonts
from services.rendering.layout.model.models import RenderBlock
from services.rendering.output.typst.emitter import build_typst_source_from_page_specs
from services.rendering.output.typst.source_builder import build_typst_book_overlay_source

try:
    from rendering_bridge import clean_background as _native_clean_background
    from rendering_bridge import emit_typst_source as _native_emit_typst_source
    from rendering_bridge import emit_typst_book_overlay_source as _native_emit_typst_book_overlay_source

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def _line_box_to_dict(line_box) -> dict:
    """Serialize a `RenderLineBox` (or already-dict shape) into the Rust
    `RenderLineBox` DTO fields."""
    if isinstance(line_box, dict):
        return {"text": line_box.get("text", ""), "bbox": list(line_box.get("bbox") or [])}
    return {"text": line_box.text, "bbox": list(line_box.bbox)}


def _toc_entry_to_dict(entry) -> dict:
    """Serialize a `RenderTocEntry` (or already-dict shape) into the Rust
    `RenderTocEntry` DTO fields."""
    if isinstance(entry, dict):
        return {
            "title": entry.get("title", ""),
            "page_label": entry.get("page_label", ""),
            "bbox": list(entry.get("bbox") or []),
            "number": entry.get("number", ""),
            "level": entry.get("level", 1),
        }
    return {
        "title": entry.title,
        "page_label": entry.page_label,
        "bbox": list(entry.bbox),
        "number": entry.number,
        "level": entry.level,
    }


def _block_to_dict(block) -> dict:
    text_color = tuple(block.text_color or (0, 0, 0))
    cover_fill = tuple(block.cover_fill or (1, 1, 1))
    return {
        "block_id": block.block_id,
        "page_index": block.page_index,
        "background_rect": list(block.background_rect),
        "content_rect": list(block.content_rect),
        "content_kind": block.content_kind,
        "content_text": block.content_text,
        "plain_text": block.plain_text,
        "math_map": block.math_map or [],
        "font_size_pt": block.font_size_pt,
        "leading_em": block.leading_em,
        "font_weight": block.font_weight,
        "fit_to_box": block.fit_to_box,
        "fit_single_line": block.fit_single_line,
        "fit_min_font_size_pt": block.fit_min_font_size_pt,
        "fit_max_font_size_pt": block.fit_max_font_size_pt,
        "fit_min_leading_em": block.fit_min_leading_em,
        "fit_max_height_pt": block.fit_max_height_pt,
        "fit_target_width_pt": block.fit_target_width_pt,
        "fit_target_height_pt": block.fit_target_height_pt,
        "fit_shift_up_pt": block.fit_shift_up_pt,
        "first_line_indent_pt": block.first_line_indent_pt,
        "justify_text": block.justify_text,
        "text_color": [text_color[0], text_color[1], text_color[2]],
        "cover_fill": [cover_fill[0], cover_fill[1], cover_fill[2]],
        "use_cover_fill": block.use_cover_fill,
        "skip_reason": block.skip_reason,
        "preserve_line_breaks": block.preserve_line_breaks,
        "preserved_line_boxes": [
            _line_box_to_dict(lb) for lb in (block.preserved_line_boxes or [])
        ],
        "toc_entries": [_toc_entry_to_dict(te) for te in (block.toc_entries or [])],
    }


def _page_spec_to_dict(spec) -> dict:
    return {
        "page_index": spec.page_index,
        "page_width_pt": spec.page_width_pt,
        "page_height_pt": spec.page_height_pt,
        "background_pdf_path": str(spec.background_pdf_path) if spec.background_pdf_path else None,
        "blocks": [_block_to_dict(b) for b in spec.blocks],
    }


def emit_typst_source(
    *,
    background_pdf_path: Path,
    page_specs: list,
    work_dir: Path,
    font_family: str = fonts.TYPST_DEFAULT_FONT_FAMILY,
) -> str:
    """`emitter.build_typst_source_from_page_specs`, routed to the native Rust
    emitter when the module is built; otherwise the pure-Python emitter."""
    if NATIVE:
        payload = json.dumps([_page_spec_to_dict(spec) for spec in page_specs])
        return _native_emit_typst_source(
            payload,
            str(background_pdf_path),
            str(work_dir),
            font_family,
        )
    return build_typst_source_from_page_specs(
        background_pdf_path=background_pdf_path,
        page_specs=page_specs,
        work_dir=work_dir,
        font_family=font_family,
    )


def _render_block_to_dict(block: RenderBlock) -> dict:
    """Serialize a `RenderBlock` dataclass into the Rust `RenderBlock` DTO
    fields (mirrors `gen_typst_book_corpus.py`'s `dto_dict`: tuple -> list)."""
    return {
        "block_id": block.block_id,
        "bbox": list(block.bbox),
        "cover_bbox": list(block.cover_bbox),
        "inner_bbox": list(block.inner_bbox),
        "markdown_text": block.markdown_text,
        "plain_text": block.plain_text,
        "render_kind": block.render_kind,
        "font_size_pt": block.font_size_pt,
        "leading_em": block.leading_em,
        "font_weight": block.font_weight,
        "fit_to_box": block.fit_to_box,
        "fit_single_line": block.fit_single_line,
        "fit_min_font_size_pt": block.fit_min_font_size_pt,
        "fit_max_font_size_pt": block.fit_max_font_size_pt,
        "fit_min_leading_em": block.fit_min_leading_em,
        "fit_max_height_pt": block.fit_max_height_pt,
        "fit_target_width_pt": block.fit_target_width_pt,
        "fit_target_height_pt": block.fit_target_height_pt,
        "fit_shift_up_pt": block.fit_shift_up_pt,
        "first_line_indent_pt": block.first_line_indent_pt,
        "justify_text": block.justify_text,
        "text_color": list(block.text_color),
        "cover_fill": list(block.cover_fill),
        "use_cover_fill": block.use_cover_fill,
        "math_map": list(block.math_map or []),
        "skip_reason": block.skip_reason,
        "source_item_id": block.source_item_id,
        "preserve_line_breaks": block.preserve_line_breaks,
        "preserved_line_boxes": [
            _line_box_to_dict(lb) for lb in (block.preserved_line_boxes or [])
        ],
        "toc_entries": [_toc_entry_to_dict(te) for te in (block.toc_entries or [])],
    }


def _as_render_blocks(width: float, height: float, items: list) -> list[RenderBlock]:
    """Normalize a page's items to prebuilt `RenderBlock`s. Production
    `book_specs` carry raw translated-item dicts, so run the layout pipeline
    `build_render_blocks` (identical to the pure-Python reference path); already
    prebuilt `RenderBlock` instances (corpus/smoke fixtures) pass through."""
    if not items:
        return []
    if isinstance(items[0], RenderBlock):
        return items
    from services.rendering.layout.payload.blocks import build_render_blocks

    return build_render_blocks(items, page_width=width, page_height=height)


def emit_typst_book_overlay_source(
    *,
    page_specs: list[tuple[float, float, list[dict]]],
    font_family: str = fonts.TYPST_DEFAULT_FONT_FAMILY,
    include_cover_rect: bool = False,
) -> str:
    """`source_builder.build_typst_book_overlay_source`, routed to the native
    Rust emitter when the module is built; otherwise the pure-Python reference.
    The page `items` may be raw translated-item dicts (production; the layout
    pipeline `build_render_blocks` converts them identically on both paths) or
    already-built `RenderBlock` instances (corpus/smoke fixtures)."""
    if NATIVE:
        payload = json.dumps(
            [
                [width, height, [_render_block_to_dict(b) for b in _as_render_blocks(width, height, items)]]
                for width, height, items in page_specs
            ]
        )
        return _native_emit_typst_book_overlay_source(payload, font_family, include_cover_rect)
    return build_typst_book_overlay_source(
        page_specs,
        font_family=font_family,
        include_cover_rect=include_cover_rect,
    )


def _apply_adaptive_overlay_colors_batch_python(
    *,
    source_pdf_path: Path,
    pages: dict[int, list[dict]],
    precomputed_colors_by_item_id: dict[str, dict[str, tuple[float, float, float]]] | None = None,
) -> dict[int, list[dict]]:
    """NATIVE=False fallback: the fitz reference in `color_adapt`."""
    from services.rendering.output.typst.color_adapt import apply_adaptive_overlay_colors_batch

    return apply_adaptive_overlay_colors_batch(
        source_pdf_path=source_pdf_path,
        pages=pages,
        precomputed_colors_by_item_id=precomputed_colors_by_item_id,
    )


def apply_adaptive_overlay_colors_batch(
    *,
    source_pdf_path: Path,
    pages: dict[int, list[dict]],
    precomputed_colors_by_item_id: dict[str, dict[str, tuple[float, float, float]]] | None = None,
) -> dict[int, list[dict]]:
    """`color_adapt.apply_adaptive_overlay_colors_batch`, routed to the native
    primitives when the module is built; otherwise the pure-Python reference in
    `color_adapt`. Builds every non-rendering decision input (batch sampler
    rects, span clips, title-visual probes) from the translated items, calls the
    three source primitives (three PDF opens), then runs the shared
    `_apply_adaptive_overlay_colors_with_data` decision tree. Out-of-range pages
    pass through as shallow copies."""
    if not NATIVE:
        return _apply_adaptive_overlay_colors_batch_python(
            source_pdf_path=source_pdf_path,
            pages=pages,
            precomputed_colors_by_item_id=precomputed_colors_by_item_id,
        )
    import fitz

    from services.rendering.layout._native import read_source_page_sizes
    from services.rendering.output.typst.color_adapt import DEFAULT_COVER_FILL
    from services.rendering.output.typst.color_adapt import PAGE_TEXT_COLOR_SAMPLER_MIN_TITLES
    from services.rendering.output.typst.color_adapt import SpanColorSample
    from services.rendering.output.typst.color_adapt import _apply_adaptive_overlay_colors_with_data
    from services.rendering.output.typst.color_adapt import _item_needs_local_color_sampling
    from services.rendering.output.typst.color_adapt import _item_uses_explicit_white_fill
    from services.rendering.output.typst.color_adapt import _local_sampling_rects
    from services.rendering.output.typst.color_adapt import cover_bbox
    from services.rendering.output.typst.color_adapt import is_title_like_block
    from services.rendering.output.typst.color_adapt import should_probe_title_visual_color
    from services.rendering.source.background import _native as _source_native

    def cover_rect(item: dict):
        bbox = cover_bbox(item)
        if len(bbox) != 4:
            return None
        rect = fitz.Rect(bbox)
        if rect.is_empty or rect.is_infinite:
            return None
        return rect

    def decode_spans(entries: list) -> list[SpanColorSample]:
        samples: list[SpanColorSample] = []
        for entry in entries:
            color_int = int(entry[4])
            samples.append(
                SpanColorSample(
                    rect=fitz.Rect(float(entry[0]), float(entry[1]), float(entry[2]), float(entry[3])),
                    text=str(entry[5]),
                    rgb=((color_int >> 16) & 255, (color_int >> 8) & 255, color_int & 255),
                )
            )
        return samples

    page_indices = sorted(pages)
    sizes = read_source_page_sizes(source_pdf_path=source_pdf_path, page_indices=page_indices)

    fills_cfg: dict[int, dict[str, object]] = {}
    span_clips: dict[int, list[list[float] | None]] = {}
    span_clip_ids: dict[int, list[str]] = {}
    span_whole_page: dict[int, bool] = {}
    for page_idx in page_indices:
        size = sizes.get(page_idx)
        if size is None:
            continue
        page_rect = fitz.Rect(0, 0, size[0], size[1])
        items = pages[page_idx]
        target_ids: list[str] = []
        target_rects: list[fitz.Rect] = []
        for item in items:
            if not _item_needs_local_color_sampling(item):
                continue
            rect = cover_rect(item)
            if rect is None:
                continue
            target_ids.append(str(item.get("item_id") or ""))
            target_rects.append(rect)
        for item in items:
            if _item_needs_local_color_sampling(item) or _item_uses_explicit_white_fill(item):
                continue
            if not is_title_like_block(item):
                continue
            rect = cover_rect(item)
            if rect is None:
                continue
            target_ids.append(str(item.get("item_id") or ""))
            target_rects.append(rect)
        fills_cfg[page_idx] = {
            "batch_rects": [[r.x0, r.y0, r.x1, r.y1] for r in _local_sampling_rects(items)],
            "target_rects": [[r.x0, r.y0, r.x1, r.y1] for r in target_rects],
            "target_ids": target_ids,
        }
        title_count = sum(1 for item in items if is_title_like_block(item))
        whole_page = title_count >= PAGE_TEXT_COLOR_SAMPLER_MIN_TITLES
        clips: list[list[float] | None] = []
        clip_ids: list[str] = []
        if whole_page:
            clips.append(None)
            clip_ids.append("__whole_page__")
        else:
            for item in items:
                if not is_title_like_block(item):
                    continue
                rect = cover_rect(item)
                if rect is None:
                    continue
                clip_rect = rect & page_rect
                clips.append([clip_rect.x0, clip_rect.y0, clip_rect.x1, clip_rect.y1])
                clip_ids.append(str(item.get("item_id") or ""))
        span_clips[page_idx] = clips
        span_clip_ids[page_idx] = clip_ids
        span_whole_page[page_idx] = whole_page

    fills_raw = _source_native.sample_page_color_fills(
        source_pdf_path=source_pdf_path,
        by_page={
            page_idx: {
                "batch_rects": cfg["batch_rects"],
                "target_rects": cfg["target_rects"],
            }
            for page_idx, cfg in fills_cfg.items()
        },
    )
    probe_cfg: dict[int, list[dict]] = {}
    probe_ids: dict[int, list[str]] = {}
    for page_idx in page_indices:
        cfg = fills_cfg.get(page_idx)
        if cfg is None:
            continue
        page_fills = fills_raw.get(str(page_idx), {}).get("targets", {})
        fill_by_item_id: dict[str, tuple[float, float, float]] = {}
        for i, item_id in enumerate(cfg["target_ids"]):
            target_key = str(i)
            if target_key in page_fills:
                fill = page_fills[target_key]
                fill_by_item_id[item_id] = (float(fill[0]), float(fill[1]), float(fill[2]))
        probes: list[dict] = []
        ids: list[str] = []
        for item in pages[page_idx]:
            if not is_title_like_block(item):
                continue
            rect = cover_rect(item)
            if rect is None:
                continue
            item_id = str(item.get("item_id") or "")
            fill = fill_by_item_id.get(item_id, DEFAULT_COVER_FILL)
            if should_probe_title_visual_color(fill):
                probes.append({"rect": [rect.x0, rect.y0, rect.x1, rect.y1], "fill": list(fill)})
                ids.append(item_id)
        if probes:
            probe_cfg[page_idx] = probes
            probe_ids[page_idx] = ids

    probes_raw = _source_native.sample_title_visual_colors(
        source_pdf_path=source_pdf_path,
        visuals_by_page=probe_cfg,
    )
    spans_raw = _source_native.extract_page_span_dicts(
        source_pdf_path=source_pdf_path,
        clips_by_page=span_clips,
    )

    results: dict[int, list[dict]] = {}
    for page_idx in page_indices:
        size = sizes.get(page_idx)
        if size is None:
            results[page_idx] = list(pages[page_idx])
            continue
        items = pages[page_idx]
        cfg = fills_cfg[page_idx]
        page_fills = fills_raw.get(str(page_idx), {}).get("targets", {})
        fill_by_item_id: dict[str, tuple[float, float, float]] = {}
        for i, item_id in enumerate(cfg["target_ids"]):
            target_key = str(i)
            if target_key in page_fills:
                fill = page_fills[target_key]
                fill_by_item_id[item_id] = (float(fill[0]), float(fill[1]), float(fill[2]))
        page_spans = spans_raw.get(str(page_idx), {})
        span_sampler_samples: list[SpanColorSample] | None = None
        span_clip_by_item_id: dict[str, list[SpanColorSample]] = {}
        if span_whole_page[page_idx]:
            span_sampler_samples = decode_spans(page_spans.get("0", []))
        else:
            for i, item_id in enumerate(span_clip_ids[page_idx]):
                span_clip_by_item_id[item_id] = decode_spans(page_spans.get(str(i), []))
        visual_by_item_id: dict[str, tuple[float, float, float] | None] = {}
        page_probes = probes_raw.get(str(page_idx), {})
        for i, item_id in enumerate(probe_ids.get(page_idx, [])):
            key = str(i)
            visual_by_item_id[item_id] = (
                tuple(float(c) for c in page_probes[key]) if key in page_probes else None
            )
        results[page_idx] = _apply_adaptive_overlay_colors_with_data(
            items,
            fill_by_item_id=fill_by_item_id,
            span_sampler_samples=span_sampler_samples,
            span_clip_by_item_id=span_clip_by_item_id,
            visual_by_item_id=visual_by_item_id,
            precomputed_colors_by_item_id=precomputed_colors_by_item_id,
        )
    return results
