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
    from rendering_bridge import build_block_payloads as _native_build_block_payloads
    from rendering_bridge import detect_first_line_indents as _native_detect_first_line_indents

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
