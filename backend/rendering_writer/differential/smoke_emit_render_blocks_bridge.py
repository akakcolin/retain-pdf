#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N2 emit boundary `emit_render_blocks`.

Three parts:

1. Native availability: the layout shim `layout/payload/_native` reports NATIVE
   (maturin build installed) and the bridge exports `emit_render_blocks`.

2. Payload parity: for realistic translated items (title / heading / multi-line
   body / formula / explicitly colored) seeded with `seed_render_fields` and
   assembled by the native `build_block_payloads`, the native
   `_native.emit_render_blocks` equals the pure-Python reference (the shim
   re-invoked with NATIVE=False -> payload/emit fallback). The title_fit
   branch must be exercised.

3. Fit-decision branch: a body payload carrying the body-pipeline fit keys
   (`prefer_typst_fit`, `dense_small_box`, `page_body_font_size_pt`) runs
   `resolve_typst_binary_fit` on both paths and reproduces the reference
   numbers exactly (fit_to_box, 11.14pt min font, 0.35em min leading, 60pt max
   height).

4. Preserve-line-break branch: a hand-built payload with newline translated
   text and `_render_preserve_line_breaks` zips the translated lines onto the
   source bboxes identically on both paths.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_emit_render_blocks_bridge.py
"""

import os
import sys

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import rendering_bridge  # noqa: E402
from services.rendering.layout.payload import _native  # noqa: E402
from services.rendering.layout.payload.render_item import seed_render_fields  # noqa: E402

PAGE_WIDTH = 612.0
PAGE_HEIGHT = 792.0


def _item(
    item_id: str,
    *,
    block_kind: str = "text",
    layout_role: str = "paragraph",
    bbox: list[float],
    source_text: str,
    translated_text: str,
    formula_map: list[dict] | None = None,
    lines: list[dict] | None = None,
    text_color: list[float] | None = None,
    cover_fill: list[float] | None = None,
) -> dict:
    out = {
        "item_id": item_id,
        "page_idx": 0,
        "block_type": "text",
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
            formula_map=[
                {
                    "placeholder": "@@F0@@",
                    "formula_text": r"E = mc^2",
                    "latex": r"E = mc^2",
                    "bbox": [100.0, 210.0, 500.0, 240.0],
                }
            ],
        ),
        _item(
            "p000-b005",
            bbox=[72.0, 250.0, 200.0, 274.0],
            source_text="Two lines",
            translated_text="两行\n文本",
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


def _fit_decision_payload() -> dict:
    """A body payload carrying the body-pipeline fit keys (prefer_typst_fit,
    dense_small_box, page_body_font_size_pt); `resolve_typst_binary_fit` must
    reproduce the reference numbers (fit_to_box, 11.14pt / 0.35em / 60pt)."""
    text = "这是一个相当长的正文段落，包含多行流动的中文文本内容。"
    return {
        "index": 2,
        "item": {
            "item_id": "p000-b300",
            "math_mode": "placeholder",
            "semantic_role": "body",
            "layout_role": "paragraph",
            "bbox": [72.0, 140.0, 460.0, 200.0],
            "lines": [
                {"bbox": [72.0, 140.0, 460.0, 160.0], "spans": []},
                {"bbox": [72.0, 160.0, 460.0, 180.0], "spans": []},
                {"bbox": [72.0, 180.0, 460.0, 200.0], "spans": []},
            ],
            "source_text": (
                "This is a fairly long body paragraph with multiple lines "
                "of flowing English text."
            ),
            "translated_text": text,
            "protected_translated_text": text,
            "formula_map": [],
        },
        "bbox": [72.0, 140.0, 460.0, 200.0],
        "cover_bbox": [72.0, 140.0, 460.0, 200.0],
        "inner_bbox": [72.0, 140.0, 460.0, 200.0],
        "translated_text": text,
        "formula_map": [],
        "render_kind": "markdown",
        "font_size_pt": 12.0,
        "leading_em": 0.35,
        "first_line_indent_pt": 0.0,
        "font_weight": "regular",
        "page_body_font_size_pt": 12.0,
        "is_body": True,
        "dense_small_box": True,
        "heavy_dense_small_box": False,
        "prefer_typst_fit": True,
        "title_fit": None,
        "preserve_line_breaks": False,
        "adjacent_collision_risk": False,
        "adjacent_available_height_pt": None,
        "text_color": [0.0, 0.0, 0.0],
        "cover_fill": [1.0, 1.0, 1.0],
    }


def _preserve_line_breaks_payload() -> dict:
    """A hand-built preserve-line-break payload (the build flow only flags
    structured-line blocks whose translated text retains newlines; emit's
    `preserved_line_boxes_for_item` then zips those lines with the source line
    bboxes)."""
    return {
        "index": 4,
        "item": {
            "item_id": "p000-b400",
            "math_mode": "placeholder",
            "formula_map": [],
            "_render_preserve_line_breaks": True,
            "lines": [
                {"bbox": [72.0, 250.0, 200.0, 260.0]},
                {"bbox": [72.0, 260.0, 200.0, 270.0]},
            ],
        },
        "bbox": [72.0, 250.0, 200.0, 270.0],
        "cover_bbox": [72.0, 250.0, 200.0, 270.0],
        "inner_bbox": [72.0, 250.0, 200.0, 270.0],
        "translated_text": "保留\n换行",
        "formula_map": [],
        "render_kind": "plain_line",
        "font_size_pt": 12.0,
        "leading_em": 0.3,
        "first_line_indent_pt": 0.0,
        "font_weight": "regular",
        "page_body_font_size_pt": None,
        "is_body": False,
        "dense_small_box": False,
        "heavy_dense_small_box": False,
        "prefer_typst_fit": False,
        "title_fit": None,
        "preserve_line_breaks": True,
        "adjacent_collision_risk": False,
        "adjacent_available_height_pt": None,
        "text_color": [0.0, 0.0, 0.0],
        "cover_fill": [1.0, 1.0, 1.0],
    }


def _emit_native(payloads: list[dict]):
    return _native.emit_render_blocks(payloads)


def _emit_reference(payloads: list[dict]):
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        return _native.emit_render_blocks(payloads)
    finally:
        _native.NATIVE = was


def _assert_blocks_equal(native_blocks, reference_blocks, context: str) -> None:
    assert len(native_blocks) == len(reference_blocks), (
        f"[{context}] block count native {len(native_blocks)} != "
        f"reference {len(reference_blocks)}"
    )
    for native_block, reference_block in zip(native_blocks, reference_blocks):
        assert native_block == reference_block, (
            f"[{context}] block {native_block.block_id} "
            f"({native_block.source_item_id}) diverged"
        )


def main() -> None:
    assert _native.NATIVE, "native module not built"
    assert hasattr(rendering_bridge, "emit_render_blocks"), "emit_render_blocks not exported"

    items = translated_items()
    for item in items:
        seed_render_fields(item)
    payloads, _page_text_width_med = _native.build_block_payloads(
        translated_items=items,
        page_width=PAGE_WIDTH,
        page_height=PAGE_HEIGHT,
    )

    native_blocks = _emit_native(payloads)
    reference_blocks = _emit_reference(payloads)
    _assert_blocks_equal(native_blocks, reference_blocks, "build-flow payloads")

    by_id = {block.source_item_id: block for block in native_blocks}
    title = by_id["p000-b001"]
    assert title.fit_single_line, "title block must take the title_fit branch"
    assert title.font_weight == "bold", "title font_weight must be bold"
    assert title.font_size_pt > 0, "title font_size_pt must be positive"

    # fit-decision branch parity (hand-built body payload, not reachable through
    # the raw build flow which lacks the body-pipeline fit keys)
    fit_payload = _fit_decision_payload()
    fit_native = _emit_native([fit_payload])
    fit_reference = _emit_reference([fit_payload])
    _assert_blocks_equal(fit_native, fit_reference, "fit-decision payload")
    body = fit_native[0]
    assert body.fit_to_box, "fit-decision body must fit to box"
    assert abs(body.fit_min_font_size_pt - 11.14) < 1e-9, (
        f"fit-decision min font {body.fit_min_font_size_pt} != 11.14"
    )
    assert abs(body.fit_min_leading_em - 0.35) < 1e-9, (
        f"fit-decision min leading {body.fit_min_leading_em} != 0.35"
    )
    assert body.fit_max_height_pt == 60.0, (
        f"fit-decision max height {body.fit_max_height_pt} != 60.0"
    )
    assert body.justify_text, "fit-decision body must justify text"
    assert body.source_item_id == "p000-b300", "fit-decision source_item_id must round-trip"

    # preserve-line-break branch parity (hand-built payload with newline text)
    preserve_payload = _preserve_line_breaks_payload()
    preserve_native = _emit_native([preserve_payload])
    preserve_reference = _emit_reference([preserve_payload])
    _assert_blocks_equal(preserve_native, preserve_reference, "preserve-line-break payload")
    preserved = preserve_native[0]
    assert preserved.preserve_line_breaks, "preserve-line-break flag must round-trip"
    assert preserved.render_kind == "plain_line", "preserve block must keep render_kind"
    assert not preserved.fit_to_box, "preserve block must not fit to box"
    assert len(preserved.preserved_line_boxes) == 2, (
        f"preserved_line_boxes must have 2 entries, got {len(preserved.preserved_line_boxes)}"
    )
    assert preserved.preserved_line_boxes[0].text == "保留", (
        "preserved line 0 must be the first translated line"
    )
    assert preserved.preserved_line_boxes[1].bbox == [72.0, 260.0, 200.0, 270.0], (
        "preserved line 1 must carry the second source bbox"
    )

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
