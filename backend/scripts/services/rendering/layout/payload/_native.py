"""Optional native (Rust) backend for the layout first-line-indent detection.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`detect_first_line_indents` (the fitz display-list pixmap render + pixel
analysis used by `layout/payload/prepare.py` and `source/prewarm_payload.py`)
through the Rust `rendering_bridge.detect_first_line_indents`.
`detect_first_line_indents` is native-only: when the native path is unavailable
(routing gate off) the call raises instead of falling back to the fitz
pixel-sampling reference, so that reference is fully retired from production.
Every routed payload boundary below is likewise native-only: when the native
path is unavailable the call raises instead of falling back to its pure-Python
reference, so those references are fully retired from production (D1 双实现清零).
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering import _routing

try:
    from rendering_bridge import apply_body_pipeline as _native_apply_body_pipeline
    from rendering_bridge import build_block_payloads as _native_build_block_payloads
    from rendering_bridge import detect_first_line_indents as _native_detect_first_line_indents
    from rendering_bridge import emit_render_blocks as _native_emit_render_blocks
    from rendering_bridge import mark_adjacent_collision_risk as _native_mark_adjacent_collision_risk
    from rendering_bridge import prepare_render_payloads_by_page as _native_prepare_render_payloads_by_page
    from rendering_bridge import resolve_book_body_font_target as _native_resolve_book_body_font_target
    from rendering_bridge import seed_render_fields as _native_seed_render_fields

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
    native Rust port. Native-only: raises when the bridge is unavailable (the
    pure-Python reference is retired). Native emits `title_fit` as a JSON object;
    reconstruct the `TitleFitDecision` dataclass so downstream body-pipeline
    consumers keep attribute access."""
    if not _routing.routed("layout_payload", "build_block_payloads", NATIVE):
        raise RuntimeError(
            "layout_payload.build_block_payloads is native-only: the rendering_bridge "
            "build_block_payloads is required"
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


def seed_render_fields(translated_items: list[dict]) -> None:
    """The C3-N5 seed boundary `blocks.build_render_blocks` runs
    (`render_item.seed_render_fields` per translated item) before building block
    payloads, routed to the native Rust port. Native-only: raises when the bridge
    is unavailable (the pure-Python reference is retired). The native path writes
    the updated dicts back onto the shared item references, so callers see the
    seeded render_* fields in place."""
    if not _routing.routed("layout_payload", "seed_render_fields", NATIVE):
        raise RuntimeError(
            "layout_payload.seed_render_fields is native-only: the rendering_bridge "
            "seed_render_fields is required"
        )
    raw = json.loads(_native_seed_render_fields(json.dumps(translated_items)))
    for item, updated in zip(translated_items, raw):
        item.clear()
        item.update(updated)
    _routing.record_native_hit("layout_payload", "seed_render_fields")


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
    Rust port. Native-only: raises when the bridge is unavailable (the pure-Python
    reference is retired). `title_fit` dataclasses serialize as dicts for the
    boundary, and the native `RenderBlock` DTO dicts reconstruct into `RenderBlock`
    dataclasses so downstream attribute access is unchanged."""
    if not _routing.routed("layout_payload", "emit_render_blocks", NATIVE):
        raise RuntimeError(
            "layout_payload.emit_render_blocks is native-only: the rendering_bridge "
            "emit_render_blocks is required"
        )
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


def apply_body_pipeline(ordered_payloads: list[dict], *, page_text_width_med: float, book_body_font_target: float | None = None):
    """The C3-N3 body-pipeline boundary `blocks.build_render_blocks` runs
    (`body_pipeline.apply_body_payload_pipeline` plus the post-pipeline
    annotation stages), routed to the native Rust port. Native-only: raises when
    the bridge is unavailable (the pure-Python reference is retired). The native
    path writes the updated dicts back onto the shared payload references (so the
    original `block_payloads` order sees the changes) and restores `title_fit`
    dataclasses."""
    if not _routing.routed("layout_payload", "apply_body_pipeline", NATIVE):
        raise RuntimeError(
            "layout_payload.apply_body_pipeline is native-only: the rendering_bridge "
            "apply_body_pipeline is required"
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
    to the native Rust port. Native-only: raises when the bridge is unavailable
    (the pure-Python reference is retired). The native path writes the updated
    dicts back onto the shared payload references and restores `title_fit`
    dataclasses."""
    if not _routing.routed("layout_payload", "mark_adjacent_collision_risk", NATIVE):
        raise RuntimeError(
            "layout_payload.mark_adjacent_collision_risk is native-only: the "
            "rendering_bridge mark_adjacent_collision_risk is required"
        )
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


def _strip_title_fit(blocks: list[dict]) -> list[dict]:
    return [{k: v for k, v in block.items() if k != "title_fit"} for block in blocks]


def resolve_book_body_font_target_from_payloads(
    page_payloads: list[tuple[list[dict], float]],
) -> float | None:
    """The whole-book body font target
    `blocks.resolve_book_body_font_target_from_payloads`, routed to the native
    Rust port. Native-only: raises when the bridge is unavailable (the pure-Python
    reference is retired). Returns the low stable body font or None. `title_fit`
    dataclasses are stripped for the JSON boundary (the port only reads
    font/width/height fields)."""
    if not _routing.routed("layout_payload", "resolve_book_body_font_target", NATIVE):
        raise RuntimeError(
            "layout_payload.resolve_book_body_font_target is native-only: the "
            "rendering_bridge resolve_book_body_font_target is required"
        )
    serialized = [[_strip_title_fit(blocks), float(width)] for blocks, width in page_payloads]
    raw = json.loads(_native_resolve_book_body_font_target(json.dumps(serialized)))
    _routing.record_native_hit("layout_payload", "resolve_book_body_font_target")
    return None if raw is None else float(raw)


def detect_first_line_indents(
    *,
    source_pdf_path: Path,
    by_page: dict[int, tuple[float, list[tuple[dict, float]]]],
) -> dict[str, float]:
    """Batch first-line-indent detection routed to the native bridge.
    `by_page[page_idx] = (page_text_width_med, [(item, font_size_pt), ...])`;
    items are the full production dicts. Returns `{item_id: indent_pt}` (0.0
    entries included). Native-only: without the bridge the call fails closed."""
    if not _routing.routed("layout_payload", "detect_first_line_indents", NATIVE):
        raise RuntimeError("layout_payload.detect_first_line_indents is native-only: the rendering_bridge detect_first_line_indents is required")
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
    try:
        raw = json.loads(
            _native_detect_first_line_indents(
                source_pdf_path.read_bytes(),
                json.dumps(page_indices),
                json.dumps(candidates_json),
            )
        )
    except Exception:
        _routing.record_fallback("layout_payload", "detect_first_line_indents", _routing.FallbackReason.NATIVE_BRIDGE_ERROR)
        raise
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


def prepare_render_payloads_by_page(
    translated_pages: dict[int, list[dict]],
    *,
    source_pdf_path: Path | None = None,
    first_line_indent_lookup: dict[str, float] | None = None,
    effective_inner_bbox_lookup: dict[str, list[float]] | None = None,
) -> dict[int, list[dict]]:
    """The C3-N7 prepare boundary `prepare.prepare_render_payloads_by_page`,
    routed to the native Rust port. Native-only: raises when the bridge is
    unavailable (the pure-Python reference is retired). The first-line-indent
    lookup is resolved on the Python side (the Rust assembler is a leaf crate and
    cannot open the source PDF): the caller-provided `first_line_indent_lookup`
    passes through, else candidates are built from `source_pdf_path` via
    `prepare._build_page_metrics` + `_resolve_first_line_indent_lookup`, and the
    resolved lookup is handed to the native assembler."""
    if not _routing.routed("layout_payload", "prepare_render_payloads_by_page", NATIVE):
        raise RuntimeError(
            "layout_payload.prepare_render_payloads_by_page is native-only: the "
            "rendering_bridge prepare_render_payloads_by_page is required"
        )
    from services.rendering.layout.payload.prepare import _build_page_metrics
    from services.rendering.layout.payload.prepare import _resolve_first_line_indent_lookup

    page_metrics = _build_page_metrics(translated_pages)
    indent_lookup = _resolve_first_line_indent_lookup(
        translated_pages,
        page_metrics,
        source_pdf_path=source_pdf_path,
        first_line_indent_lookup=first_line_indent_lookup,
    )
    raw = json.loads(
        _native_prepare_render_payloads_by_page(
            json.dumps(translated_pages),
            None if indent_lookup is None else json.dumps(indent_lookup),
            None if effective_inner_bbox_lookup is None else json.dumps(effective_inner_bbox_lookup),
        )
    )
    result: dict[int, list[dict]] = {}
    for page_idx_str, items in raw.items():
        result[int(page_idx_str)] = items
    _routing.record_native_hit("layout_payload", "prepare_render_payloads_by_page")
    return result
