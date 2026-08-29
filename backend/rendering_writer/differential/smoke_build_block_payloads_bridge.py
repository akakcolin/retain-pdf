#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N2 seed boundary `build_block_payloads`.

Three parts:

1. Native availability: the layout shim `layout/payload/_native` reports NATIVE
   (maturin build installed) and its `build_block_payloads` routes to Rust.

2. Payload parity: for realistic translated items (title / heading / multi-line
   body / formula / preserved-line-break / explicitly colored) with
   `seed_render_fields` applied, the native `_native.build_block_payloads`
   equals the pure-Python reference (the shim re-invoked with NATIVE=False ->
   block_seed fallback). `title_fit` is compared via `dataclasses.asdict` and
   the `text_color` / `cover_fill` arrays are compared as tuples (native emits
   JSON lists, the reference emits tuples; `emit.py` normalizes both).

3. Boundary parity: an empty rendered text and a missing bbox are dropped
   identically by both routing paths.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_build_block_payloads_bridge.py
"""

import os
import sys
from dataclasses import asdict

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.layout.payload import _native  # noqa: E402
from services.rendering.layout.payload.render_item import seed_render_fields  # noqa: E402

PAGE_WIDTH = 612.0
PAGE_HEIGHT = 792.0


def _item(
    item_id: str,
    *,
    block_type: str = "text",
    block_kind: str = "text",
    layout_role: str = "paragraph",
    bbox: list[float],
    source_text: str,
    translated_text: str,
    formula_map: list[dict] | None = None,
    lines: list[dict] | None = None,
    render_preserve_line_breaks: bool = False,
    text_color: list[float] | None = None,
    cover_fill: list[float] | None = None,
) -> dict:
    out = {
        "item_id": item_id,
        "page_idx": 0,
        "block_type": block_type,
        "block_kind": block_kind,
        "layout_role": layout_role,
        "semantic_role": layout_role,
        "bbox": bbox,
        "lines": lines if lines is not None else [],
        "source_text": source_text,
        "protected_source_text": source_text,
        "translated_text": translated_text,
        "protected_translated_text": translated_text,
        "formula_map": formula_map if formula_map is not None else [],
        "render_preserve_line_breaks": render_preserve_line_breaks,
        "should_translate": True,
    }
    if text_color is not None:
        out["_render_text_color"] = text_color
    if cover_fill is not None:
        out["_render_cover_fill"] = cover_fill
    return out


def translated_items() -> list[dict]:
    return [
        _item(
            "p000-b001",
            layout_role="title",
            bbox=[72.0, 50.0, 540.0, 90.0],
            source_text="Chapter One",
            translated_text="第一章",
        ),
        _item(
            "p000-b002",
            layout_role="heading",
            bbox=[72.0, 100.0, 420.0, 130.0],
            source_text="Introduction and Background",
            translated_text="引言与背景",
        ),
        _item(
            "p000-b003",
            bbox=[72.0, 140.0, 460.0, 200.0],
            source_text=(
                "This is a fairly long body paragraph with multiple lines "
                "of flowing English text."
            ),
            translated_text="这是一个相当长的正文段落，包含多行流动的中文文本内容。",
            lines=[
                {"bbox": [72.0, 140.0, 460.0, 160.0], "spans": []},
                {"bbox": [72.0, 160.0, 460.0, 180.0], "spans": []},
                {"bbox": [72.0, 180.0, 460.0, 200.0], "spans": []},
            ],
        ),
        _item(
            "p000-b004",
            block_kind="formula",
            layout_role="paragraph",
            bbox=[100.0, 210.0, 500.0, 240.0],
            source_text=r"$E = mc^2$",
            translated_text=r"$E = mc^2$",
            formula_map=[{"latex": r"E = mc^2", "bbox": [100.0, 210.0, 500.0, 240.0]}],
        ),
        _item(
            "p000-b005",
            bbox=[72.0, 250.0, 200.0, 280.0],
            source_text="Preserved",
            translated_text="保留\n换行",
            render_preserve_line_breaks=True,
        ),
        _item(
            "p000-b006",
            layout_role="title",
            bbox=[72.0, 290.0, 540.0, 320.0],
            source_text="Colored Title",
            translated_text="彩色标题",
            text_color=[0.9, 0.2, 0.1],
            cover_fill=[0.0, 0.0, 0.0],
        ),
    ]


def _normalize(payload: dict) -> dict:
    out = dict(payload)
    title_fit = out.get("title_fit")
    if title_fit is not None:
        out["title_fit"] = asdict(title_fit)
    for key in ("text_color", "cover_fill"):
        if key in out:
            out[key] = tuple(float(component) for component in out[key])
    return out


def _run_native(items: list[dict]) -> tuple[list[dict], float]:
    return _native.build_block_payloads(
        translated_items=items,
        page_width=PAGE_WIDTH,
        page_height=PAGE_HEIGHT,
    )


def main() -> None:
    assert _native.NATIVE, "native module not built"

    items = translated_items()
    for item in items:
        seed_render_fields(item)

    native_payloads, native_width = _run_native(items)
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        reference_payloads, reference_width = _run_native(items)
    finally:
        _native.NATIVE = was

    assert len(native_payloads) == len(reference_payloads), (
        f"payload count native {len(native_payloads)} != reference {len(reference_payloads)}"
    )
    assert abs(native_width - reference_width) < 1e-9, (
        f"page_text_width_med native {native_width} != reference {reference_width}"
    )
    for native_payload, reference_payload in zip(native_payloads, reference_payloads):
        assert _normalize(native_payload) == _normalize(reference_payload), (
            f"payload {native_payload.get('index')} "
            f"{native_payload.get('item', {}).get('item_id')} diverged"
        )

    # the title branch must have been exercised (fit decision + bold weight)
    title_payload = next(
        payload for payload in native_payloads if payload.get("item", {}).get("item_id") == "p000-b001"
    )
    assert title_payload.get("title_fit") is not None, "expected a TitleFitDecision on the title item"
    assert title_payload.get("font_weight") == "bold", "title font_weight must be bold"
    assert title_payload.get("font_size_pt") > 0, "title font_size_pt must be positive"

    # boundary parity: empty rendered text and missing bbox are dropped identically
    empty_items = [
        _item("p000-b100", bbox=[72.0, 50.0, 200.0, 80.0], source_text="A", translated_text=""),
        _item("p000-b101", bbox=[], source_text="No box", translated_text="无框"),
    ]
    for item in empty_items:
        seed_render_fields(item)
    native_empty, _ = _run_native(empty_items)
    _native.NATIVE = False
    try:
        reference_empty, _ = _run_native(empty_items)
    finally:
        _native.NATIVE = was
    assert native_empty == reference_empty == [], (
        f"boundary parity native {native_empty} != reference {reference_empty}"
    )

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
