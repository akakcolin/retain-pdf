"""Optional native (Rust) backend for the layout first-line-indent detection.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`detect_first_line_indents` (the fitz display-list pixmap render + pixel
analysis used by `layout/payload/prepare.py` and `source/prewarm_payload.py`)
through the Rust `rendering_bridge.detect_first_line_indents`. Without the
native module every call falls back to the pure-Python reference
`first_line_indent.detect_first_line_indent_pt_with_displaylist` (looped here),
so importing this module is always safe.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering import _routing
from services.rendering.layout.payload.first_line_indent import detect_first_line_indent_pt_with_displaylist

try:
    from rendering_bridge import apply_body_pipeline as _native_apply_body_pipeline
    from rendering_bridge import build_block_payloads as _native_build_block_payloads
    from rendering_bridge import detect_first_line_indents as _native_detect_first_line_indents
    from rendering_bridge import emit_render_blocks as _native_emit_render_blocks
    from rendering_bridge import mark_adjacent_collision_risk as _native_mark_adjacent_collision_risk
    from rendering_bridge import resolve_book_body_font_target as _native_resolve_book_body_font_target

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def build_block_payloads(
    *,
    translated_items: list[dict],
    page_width: float | None = None,
    page_height: float | None = None,
) -> tuple[list[dict], float]:
    """The C3-N2 seed boundary `block_seed.build_block_payloads`, routed to the
    native Rust port when built; otherwise the pure-Python reference. Native
    emits `title_fit` as a JSON object; reconstruct the `TitleFitDecision`
    dataclass so downstream body-pipeline consumers keep attribute access."""
    if not _routing.routed("layout_payload", "build_block_payloads", NATIVE):
        return _build_block_payloads_python(
            translated_items=translated_items,
            page_width=page_width,
            page_height=page_height,
        )
    payload = json.dumps(translated_items)
    raw = json.loads(_native_build_block_payloads(payload, page_width, page_height))
    block_payloads = raw["block_payloads"]
    from services.rendering.layout.title_binary_fit import TitleFitDecision

    for block in block_payloads:
        title_fit = block.get("title_fit")
        if title_fit is not None:
            block["title_fit"] = TitleFitDecision(**title_fit)
    _routing.record_native_hit("layout_payload", "build_block_payloads")
    return block_payloads, float(raw["page_text_width_med"])


def _build_block_payloads_python(
    *,
    translated_items: list[dict],
    page_width: float | None = None,
    page_height: float | None = None,
) -> tuple[list[dict], float]:
    from services.rendering.layout.payload.block_seed import build_block_payloads

    return build_block_payloads(
        translated_items,
        page_width=page_width,
        page_height=page_height,
    )


def _render_line_box_from_dict(d: dict):
    from services.rendering.layout.model.models import RenderLineBox

    return RenderLineBox(text=d.get("text", ""), bbox=list(d.get("bbox") or []))


def _render_toc_entry_from_dict(d: dict):
    from services.rendering.layout.model.models import RenderTocEntry

    return RenderTocEntry(
        title=d.get("title", ""),
        page_label=d.get("page_label", ""),
        bbox=list(d.get("bbox") or []),
        number=d.get("number", ""),
        level=d.get("level", 1),
    )


def _render_block_from_dict(d: dict):
    """Reconstruct a `RenderBlock` dataclass from the native `RenderBlock` DTO
    dict (the inverse of `_render_block_to_dict`): tuples come back as tuples,
    None-able lists round-trip through []/None like the Python emit would."""
    from services.rendering.layout.model.models import RenderBlock

    preserved = [_render_line_box_from_dict(lb) for lb in (d.get("preserved_line_boxes") or [])]
    toc = [_render_toc_entry_from_dict(te) for te in (d.get("toc_entries") or [])]
    return RenderBlock(
        block_id=d.get("block_id", ""),
        bbox=list(d.get("bbox") or []),
        cover_bbox=list(d.get("cover_bbox") or []),
        inner_bbox=list(d.get("inner_bbox") or []),
        markdown_text=d.get("markdown_text", ""),
        plain_text=d.get("plain_text", ""),
        render_kind=d.get("render_kind", ""),
        font_size_pt=float(d.get("font_size_pt") or 0.0),
        leading_em=float(d.get("leading_em") or 0.0),
        font_weight=d.get("font_weight", "regular"),
        fit_to_box=bool(d.get("fit_to_box")),
        fit_single_line=bool(d.get("fit_single_line")),
        fit_min_font_size_pt=float(d.get("fit_min_font_size_pt") or 0.0),
        fit_max_font_size_pt=float(d.get("fit_max_font_size_pt") or 0.0),
        fit_min_leading_em=float(d.get("fit_min_leading_em") or 0.0),
        fit_max_height_pt=float(d.get("fit_max_height_pt") or 0.0),
        fit_target_width_pt=float(d.get("fit_target_width_pt") or 0.0),
        fit_target_height_pt=float(d.get("fit_target_height_pt") or 0.0),
        fit_shift_up_pt=float(d.get("fit_shift_up_pt") or 0.0),
        first_line_indent_pt=float(d.get("first_line_indent_pt") or 0.0),
        justify_text=bool(d.get("justify_text")),
        text_color=tuple(float(c) for c in (d.get("text_color") or [0, 0, 0])),
        cover_fill=tuple(float(c) for c in (d.get("cover_fill") or [1, 1, 1])),
        use_cover_fill=bool(d.get("use_cover_fill")),
        math_map=list(d.get("math_map") or []),
        skip_reason=d.get("skip_reason", ""),
        source_item_id=d.get("source_item_id", ""),
        preserve_line_breaks=bool(d.get("preserve_line_breaks")),
        preserved_line_boxes=preserved or None,
        toc_entries=toc or None,
    )


def emit_render_blocks(block_payloads: list[dict]):
    """The C3-N2 emit boundary `emit.emit_render_blocks`, routed to the native
    Rust port when built; otherwise the pure-Python reference. `title_fit`
    dataclasses serialize as dicts for the boundary, and the native `RenderBlock`
    DTO dicts reconstruct into `RenderBlock` dataclasses so downstream
    attribute access is unchanged."""
    if not _routing.routed("layout_payload", "emit_render_blocks", NATIVE):
        return _emit_render_blocks_python(block_payloads)
    from dataclasses import asdict

    serialized: list[dict] = []
    for payload in block_payloads:
        entry = dict(payload)
        title_fit = entry.get("title_fit")
        if title_fit is not None:
            entry["title_fit"] = asdict(title_fit)
        serialized.append(entry)
    raw = json.loads(_native_emit_render_blocks(json.dumps(serialized)))
    _routing.record_native_hit("layout_payload", "emit_render_blocks")
    return [_render_block_from_dict(d) for d in raw]


def _emit_render_blocks_python(block_payloads: list[dict]):
    from services.rendering.layout.payload.emit import emit_render_blocks

    return emit_render_blocks(block_payloads)


def apply_body_pipeline(ordered_payloads: list[dict], *, page_text_width_med: float, book_body_font_target: float | None = None):
    """The C3-N3 body-pipeline boundary `blocks.build_render_blocks` runs
    (`body_pipeline.apply_body_payload_pipeline` plus the post-pipeline
    annotation stages), routed to the native Rust port when built; otherwise the
    pure-Python reference. The native path writes the updated dicts back onto
    the shared payload references (so the original `block_payloads` order sees
    the changes) and restores `title_fit` dataclasses."""
    if not _routing.routed("layout_payload", "apply_body_pipeline", NATIVE):
        return _apply_body_pipeline_python(
            ordered_payloads,
            page_text_width_med=page_text_width_med,
            book_body_font_target=book_body_font_target,
        )
    from dataclasses import asdict

    from foundation.config import layout
    from services.rendering.layout.title_binary_fit import TitleFitDecision

    serialized: list[dict] = []
    title_fits: list = []
    for payload in ordered_payloads:
        entry = dict(payload)
        title_fit = entry.get("title_fit")
        title_fits.append(title_fit)
        if title_fit is not None:
            entry["title_fit"] = asdict(title_fit)
        serialized.append(entry)
    raw = json.loads(
        _native_apply_body_pipeline(
            json.dumps(serialized),
            float(page_text_width_med),
            book_body_font_target,
            layout.FONT_UNIFY_MODE,
        )
    )
    for payload, updated, title_fit in zip(ordered_payloads, raw, title_fits):
        payload.clear()
        payload.update(updated)
        if title_fit is not None:
            payload["title_fit"] = title_fit
    _routing.record_native_hit("layout_payload", "apply_body_pipeline")


def mark_adjacent_collision_risk(ordered_payloads: list[dict]) -> None:
    """The C3-N4 collision boundary `blocks.build_render_blocks` runs
    (`collision.mark_adjacent_collision_risk`) after the body pipeline, routed
    to the native Rust port when built; otherwise the pure-Python reference. The
    native path writes the updated dicts back onto the shared payload references
    and restores `title_fit` dataclasses."""
    if not _routing.routed("layout_payload", "mark_adjacent_collision_risk", NATIVE):
        return _mark_adjacent_collision_risk_python(ordered_payloads)
    from dataclasses import asdict

    serialized: list[dict] = []
    title_fits: list = []
    for payload in ordered_payloads:
        entry = dict(payload)
        title_fit = entry.get("title_fit")
        title_fits.append(title_fit)
        if title_fit is not None:
            entry["title_fit"] = asdict(title_fit)
        serialized.append(entry)
    raw = json.loads(_native_mark_adjacent_collision_risk(json.dumps(serialized)))
    for payload, updated, title_fit in zip(ordered_payloads, raw, title_fits):
        payload.clear()
        payload.update(updated)
        if title_fit is not None:
            payload["title_fit"] = title_fit
    _routing.record_native_hit("layout_payload", "mark_adjacent_collision_risk")


def _mark_adjacent_collision_risk_python(ordered_payloads: list[dict]) -> None:
    from services.rendering.layout.payload.collision import mark_adjacent_collision_risk

    mark_adjacent_collision_risk(ordered_payloads)


def _apply_body_pipeline_python(
    ordered_payloads: list[dict],
    *,
    page_text_width_med: float,
    book_body_font_target: float | None = None,
) -> None:
    from foundation.config import layout
    from services.rendering.layout.payload.annotation_font_policy import recover_underfilled_annotation_density
    from services.rendering.layout.payload.annotation_font_policy import unify_annotation_fonts
    from services.rendering.layout.payload.body_pipeline import apply_body_payload_pipeline

    apply_body_payload_pipeline(
        ordered_payloads,
        page_text_width_med=page_text_width_med,
        book_body_font_target=book_body_font_target,
    )
    if layout.FONT_UNIFY_MODE != "off":
        unify_annotation_fonts(ordered_payloads)
    recover_underfilled_annotation_density(ordered_payloads)


def _strip_title_fit(blocks: list[dict]) -> list[dict]:
    return [{k: v for k, v in block.items() if k != "title_fit"} for block in blocks]


def resolve_book_body_font_target_from_payloads(
    page_payloads: list[tuple[list[dict], float]],
) -> float | None:
    """The whole-book body font target
    `blocks.resolve_book_body_font_target_from_payloads`, routed to the native
    Rust port when built; otherwise the pure-Python reference. Returns the low
    stable body font or None. `title_fit` dataclasses are stripped for the JSON
    boundary (the port only reads font/width/height fields)."""
    if not _routing.routed("layout_payload", "resolve_book_body_font_target", NATIVE):
        return _resolve_book_body_font_target_python(page_payloads)
    serialized = [[_strip_title_fit(blocks), float(width)] for blocks, width in page_payloads]
    raw = json.loads(_native_resolve_book_body_font_target(json.dumps(serialized)))
    _routing.record_native_hit("layout_payload", "resolve_book_body_font_target")
    return None if raw is None else float(raw)


def _resolve_book_body_font_target_python(page_payloads: list[tuple[list[dict], float]]) -> float | None:
    from services.rendering.layout.payload.body_font_unify_policy import resolve_book_body_font_target

    return resolve_book_body_font_target(page_payloads)


def detect_first_line_indents(
    *,
    source_pdf_path: Path,
    by_page: dict[int, tuple[float, list[tuple[dict, float]]]],
) -> dict[str, float]:
    """Batch first-line-indent detection routed to the native bridge when built;
    otherwise the pure-Python reference. `by_page[page_idx] = (page_text_width_med,
    [(item, font_size_pt), ...])`; items are the full production dicts (the
    reference's internal candidate gate reads their role fields). Returns
    `{item_id: indent_pt}` (0.0 entries included)."""
    if not _routing.routed("layout_payload", "detect_first_line_indents", NATIVE):
        return _detect_first_line_indents_python(source_pdf_path=source_pdf_path, by_page=by_page)
    page_indices = sorted(by_page)
    candidates_json: dict[str, list[list[float]]] = {}
    ids_by_page: dict[str, list[str]] = {}
    for page_idx in page_indices:
        _page_text_width_med, candidates = by_page[page_idx]
        page_cands: list[list[float]] = []
        page_ids: list[str] = []
        for item, font_size_pt in candidates:
            item_id = str(item.get("item_id", "") or "")
            if not item_id:
                continue
            bbox = item.get("bbox")
            if not isinstance(bbox, (list, tuple)) or len(bbox) != 4:
                page_ids.append(item_id)
                page_cands.append([0.0, 0.0, 0.0, 0.0, float(font_size_pt)])
                continue
            page_ids.append(item_id)
            page_cands.append(
                [float(bbox[0]), float(bbox[1]), float(bbox[2]), float(bbox[3]), float(font_size_pt)]
            )
        candidates_json[str(page_idx)] = page_cands
        ids_by_page[str(page_idx)] = page_ids
    raw = json.loads(
        _native_detect_first_line_indents(
            source_pdf_path.read_bytes(),
            json.dumps(page_indices),
            json.dumps(candidates_json),
        )
    )
    result: dict[str, float] = {}
    for page_idx in page_indices:
        key = str(page_idx)
        page_out = raw.get(key)
        if page_out is None:
            continue
        for cand_idx, item_id in enumerate(ids_by_page[key]):
            result[item_id] = float(page_out.get(str(cand_idx), 0.0) or 0.0)
    _routing.record_native_hit("layout_payload", "detect_first_line_indents")
    return result


def _detect_first_line_indents_python(
    *,
    source_pdf_path: Path,
    by_page: dict[int, tuple[float, list[tuple[dict, float]]]],
) -> dict[str, float]:
    import fitz

    source_doc = fitz.open(source_pdf_path)
    try:
        result: dict[str, float] = {}
        for page_idx, (page_text_width_med, candidates) in by_page.items():
            if page_idx < 0 or page_idx >= len(source_doc):
                continue
            displaylist = source_doc[page_idx].get_displaylist()
            for item, font_size_pt in candidates:
                item_id = str(item.get("item_id", "") or "")
                if not item_id:
                    continue
                result[item_id] = detect_first_line_indent_pt_with_displaylist(
                    source_doc,
                    displaylist,
                    item,
                    page_idx=page_idx,
                    font_size_pt=font_size_pt,
                    page_text_width_med=page_text_width_med,
                )
        return result
    finally:
        source_doc.close()
