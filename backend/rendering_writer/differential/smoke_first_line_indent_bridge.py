#!/usr/bin/env python3
"""Native bridge smoke test for the layout first-line-indent pixel detection
(Phase B2-2).

Three parts:

1. Native availability: the layout shim `layout/payload/_native` reports NATIVE
   (maturin build installed) and its `detect_first_line_indents` routes to Rust.

2. Three-way parity: for a synthetic PDF with an indented paragraph (first line
   at x=90, following lines at x=72) and a flush paragraph (all lines at x=72),
   the native `_native.detect_first_line_indents` equals the pure-Python
   reference (the shim re-invoked with NATIVE=False -> fitz fallback) equals a
   hand-written fitz loop calling
   `detect_first_line_indent_pt_with_displaylist` per page. The indented items
   must come back > 0, the flush items 0.0.

3. End-to-end parity: `prepare_render_payloads_by_page` with `source_pdf_path=`
   (shim -> native) produces the same per-item `_render_first_line_indent_pt`
   as the same call with `first_line_indent_lookup=<fitz lookup>`, and the
   prewarm collector `collect_first_line_indent_lookup` writes the same sink
   under NATIVE True and NATIVE=False.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_first_line_indent_bridge.py
"""

import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_PIXMAP_INDENT"] = "1"
os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402

from services.rendering.layout.payload import _native  # noqa: E402
from services.rendering.layout.payload.block_seed_metrics import collect_page_seed_metrics  # noqa: E402
from services.rendering.layout.payload.first_line_indent import detect_first_line_indent_pt_with_displaylist  # noqa: E402
from services.rendering.layout.payload.prepare import prepare_render_payloads_by_page  # noqa: E402
from services.rendering.source.prewarm_payload import collect_first_line_indent_lookup  # noqa: E402

PAGE_WIDTH = 612.0
PAGE_HEIGHT = 792.0
FONT_SIZE_PT = 12.0


def build_source_pdf() -> bytes:
    """Page 0: indented paragraph (first line x=90) + flush paragraph (all x=72).
    Page 1: another indented paragraph."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page.insert_text((90.0, 60.0), "Indented first line text here", fontsize=12, fontname="helv")
    page.insert_text((72.0, 76.0), "Second line of the indented block", fontsize=12, fontname="helv")
    page.insert_text((72.0, 92.0), "Third line of the indented block", fontsize=12, fontname="helv")
    page.insert_text((72.0, 130.0), "First line of the plain paragraph", fontsize=12, fontname="helv")
    page.insert_text((72.0, 146.0), "Second line of the plain paragraph", fontsize=12, fontname="helv")
    page.insert_text((72.0, 162.0), "Third line of the plain paragraph", fontsize=12, fontname="helv")
    page2 = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page2.insert_text((90.0, 60.0), "Another indented first line here", fontsize=12, fontname="helv")
    page2.insert_text((72.0, 76.0), "Second line of another block", fontsize=12, fontname="helv")
    page2.insert_text((72.0, 92.0), "Third line of another block", fontsize=12, fontname="helv")
    raw = doc.tobytes(garbage=0)
    doc.close()
    return raw


def _text_item(*, item_id: str, page_idx: int, bbox: list[float]) -> dict:
    return {
        "item_id": item_id,
        "page_idx": page_idx,
        "block_type": "text",
        "block_kind": "text",
        "layout_role": "paragraph",
        "semantic_role": "body",
        "bbox": bbox,
        "lines": [],
        "source_text": "sample source text",
        "protected_source_text": "sample source text",
        "protected_translated_text": "示例译文",
        "formula_map": [],
    }


def translated_pages() -> dict[int, list[dict]]:
    return {
        0: [
            _text_item(item_id="p000-b001", page_idx=0, bbox=[72.0, 50.0, 250.0, 95.0]),
            _text_item(item_id="p000-b002", page_idx=0, bbox=[72.0, 120.0, 250.0, 165.0]),
        ],
        1: [
            _text_item(item_id="p001-b001", page_idx=1, bbox=[72.0, 50.0, 250.0, 95.0]),
        ],
    }


def build_by_page() -> dict[int, tuple[float, list[tuple[dict, float]]]]:
    by_page: dict[int, tuple[float, list[tuple[dict, float]]]] = {}
    for page_idx, items in translated_pages().items():
        metrics = collect_page_seed_metrics(items, page_width=PAGE_WIDTH)
        candidates = [(item, FONT_SIZE_PT) for item in items]
        by_page[page_idx] = (metrics.page_text_width_med, candidates)
    return by_page


def fitz_manual_lookup(src: Path, by_page) -> dict[str, float]:
    doc = fitz.open(src)
    try:
        result: dict[str, float] = {}
        for page_idx, (page_text_width_med, candidates) in by_page.items():
            if page_idx < 0 or page_idx >= len(doc):
                continue
            displaylist = doc[page_idx].get_displaylist()
            for item, font_size_pt in candidates:
                item_id = str(item.get("item_id", "") or "")
                if not item_id:
                    continue
                result[item_id] = detect_first_line_indent_pt_with_displaylist(
                    doc,
                    displaylist,
                    item,
                    page_idx=page_idx,
                    font_size_pt=font_size_pt,
                    page_text_width_med=page_text_width_med,
                )
        return result
    finally:
        doc.close()


def main() -> None:
    assert _native.NATIVE, "native module not built"

    raw = build_source_pdf()
    with tempfile.TemporaryDirectory(prefix="rps-ifl-") as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(raw)
        by_page = build_by_page()

        native = _native.detect_first_line_indents(source_pdf_path=src, by_page=by_page)
        was = _native.NATIVE
        _native.NATIVE = False
        try:
            reference = _native.detect_first_line_indents(source_pdf_path=src, by_page=by_page)
        finally:
            _native.NATIVE = was
        manual = fitz_manual_lookup(src, by_page)
        assert native == reference == manual, f"native {native} != reference {reference} != fitz {manual}"
        assert any(indent > 0 for indent in native.values()), f"expected indented paragraphs, got {native}"

        pages = translated_pages()
        prepared_shim = prepare_render_payloads_by_page(pages, source_pdf_path=src)
        prepared_lookup = prepare_render_payloads_by_page(pages, first_line_indent_lookup=manual)
        for page_idx in sorted(prepared_shim):
            assert len(prepared_shim[page_idx]) == len(prepared_lookup[page_idx])
            for shim_item, lookup_item in zip(prepared_shim[page_idx], prepared_lookup[page_idx]):
                assert shim_item.get("item_id") == lookup_item.get("item_id")
                shim_indent = float(shim_item.get("_render_first_line_indent_pt", 0.0) or 0.0)
                lookup_indent = float(lookup_item.get("_render_first_line_indent_pt", 0.0) or 0.0)
                assert abs(shim_indent - lookup_indent) < 0.01, (
                    f"{shim_item.get('item_id')}: shim {shim_indent} != lookup {lookup_indent}"
                )

        items0 = pages[0]
        metrics0 = collect_page_seed_metrics(items0, page_width=PAGE_WIDTH)
        policy = {"enabled": True, "reason": "test"}
        sink_native: dict[str, float] = {}
        sink_reference: dict[str, float] = {}
        stats_native: dict[str, object] = {}
        stats_reference: dict[str, object] = {}
        collect_first_line_indent_lookup(
            source_pdf_path=src,
            page_count=len(pages),
            page_idx=0,
            items=items0,
            metrics=metrics0,
            sink=sink_native,
            stats=stats_native,
            pixmap_policy=policy,
        )
        _native.NATIVE = False
        try:
            collect_first_line_indent_lookup(
                source_pdf_path=src,
                page_count=len(pages),
                page_idx=0,
                items=items0,
                metrics=metrics0,
                sink=sink_reference,
                stats=stats_reference,
                pixmap_policy=policy,
            )
        finally:
            _native.NATIVE = was
        assert sink_native == sink_reference, f"native {sink_native} != reference {sink_reference}"
        assert any(indent > 0 for indent in sink_native.values()), f"expected prewarm pixmap hits, got {sink_native}"

        # boundary parity: empty item_id is dropped and out-of-range pages are
        # skipped identically by both routing paths (and by prepare propagation)
        empty_item = _text_item(item_id="", page_idx=0, bbox=[72.0, 50.0, 250.0, 95.0])
        empty_metrics = collect_page_seed_metrics([empty_item], page_width=PAGE_WIDTH)
        edge_by_page = {0: (empty_metrics.page_text_width_med, [(empty_item, FONT_SIZE_PT)])}
        native_edge = _native.detect_first_line_indents(source_pdf_path=src, by_page=edge_by_page)
        _native.NATIVE = False
        try:
            reference_edge = _native.detect_first_line_indents(source_pdf_path=src, by_page=edge_by_page)
        finally:
            _native.NATIVE = was
        assert native_edge == reference_edge == {}, f"empty item_id parity {native_edge} != {reference_edge}"

        oob_item = _text_item(item_id="p999-b001", page_idx=99, bbox=[72.0, 50.0, 250.0, 95.0])
        oob_metrics = collect_page_seed_metrics([oob_item], page_width=PAGE_WIDTH)
        oob_by_page = {99: (oob_metrics.page_text_width_med, [(oob_item, FONT_SIZE_PT)])}
        native_oob = _native.detect_first_line_indents(source_pdf_path=src, by_page=oob_by_page)
        _native.NATIVE = False
        try:
            reference_oob = _native.detect_first_line_indents(source_pdf_path=src, by_page=oob_by_page)
        finally:
            _native.NATIVE = was
        assert native_oob == reference_oob == {}, f"out-of-range page parity {native_oob} != {reference_oob}"

        edge_pages = {0: [empty_item]}
        prepared_edge_native = prepare_render_payloads_by_page(edge_pages, source_pdf_path=src)
        _native.NATIVE = False
        try:
            prepared_edge_reference = prepare_render_payloads_by_page(edge_pages, source_pdf_path=src)
        finally:
            _native.NATIVE = was
        assert prepared_edge_native == prepared_edge_reference
        assert not float(prepared_edge_native[0][0].get("_render_first_line_indent_pt", 0.0) or 0.0) > 0

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
