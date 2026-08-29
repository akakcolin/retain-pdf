#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N5 seed boundary.

Three parts:

1. Native availability: the layout shim `layout/payload/_native` reports NATIVE
   (maturin build installed) and the bridge exports `seed_render_fields`.

2. Seed parity: hand-built translated item dicts run through the native
   `_native.seed_render_fields` equal the pure-Python reference (the shim
   re-invoked with NATIVE=False -> `render_item.seed_render_fields`). Covers the
   normal body paragraph, the display-math skip-clear branch, a structured
   single-line block that splits into preserved lines and flags
   `_render_preserve_line_breaks`, a caption block whose long lines collapse, the
   group/continuation unit-translation chains, a formula block that keeps its
   translation, and an untranslated formula that falls back to the source text.

3. Integration: `blocks.build_render_blocks` seeds through the shim and still
   emits render blocks, so the seed boundary is wired into the production path.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_seed_render_fields_bridge.py
"""

import copy
import os
import sys

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import rendering_bridge  # noqa: E402
from services.rendering.layout.payload import _native  # noqa: E402


def seed_fixtures() -> list[dict]:
    """One item per seed branch the boundary must keep byte-exact."""
    return [
        # Normal body paragraph: translated differs from source -> kept verbatim.
        {
            "item_id": "normal",
            "source_text": "Hello world",
            "translated_text": "你好世界",
            "should_translate": True,
        },
        # Same meaningful render text -> render_protected_text emptied.
        {
            "item_id": "same_meaningful",
            "source_text": "Same text",
            "translated_text": "  Same   text ",
            "should_translate": True,
        },
        # Display-math skip branch: fields cleared, render_source_text seeded.
        {
            "item_id": "skip_math",
            "source_text": "x^2",
            "translated_text": "$x^2$",
            "should_translate": False,
            "block_kind": "formula",
        },
        # Formula block keeps its translation (should_render_source_block).
        {
            "item_id": "formula_kept",
            "source_text": "x^2",
            "translated_text": "$x^2$",
            "should_translate": True,
            "block_kind": "formula",
        },
        # Untranslated formula falls back to the source text.
        {
            "item_id": "formula_fallback",
            "source_text": "x^2",
            "should_translate": True,
            "block_kind": "formula",
        },
        # Group unit-translation chain (group text wins).
        {
            "item_id": "group",
            "translation_unit_kind": "group",
            "group_protected_translated_text": "组翻译",
            "protected_source_text": "来源",
            "should_translate": True,
        },
        # Continuation member text chain.
        {
            "item_id": "continuation",
            "continuation_group": "g1",
            "protected_translated_text": "成员翻译",
            "protected_source_text": "来源",
            "should_translate": True,
        },
        # Structured single-line split -> preserved lines + flag.
        {
            "item_id": "structured",
            "source_text": "1. 甲\n2. 乙",
            "translated_text": "1. 甲内容 2. 乙内容",
            "should_translate": True,
            "text_flow": "preserve_lines",
            "source_line_texts": ["1. 甲", "2. 乙"],
        },
        # Caption with long multi-line text -> collapse to a single line.
        {
            "item_id": "caption_long",
            "block_type": "figure_caption",
            "layout_role": "caption",
            "source_text": "字" * 81,
            "translated_text": ("字" * 41) + "\n" + ("字" * 41),
            "should_translate": True,
        },
        # Formula-map chain: first non-empty map cloned verbatim.
        {
            "item_id": "formula_map",
            "source_text": "a+b",
            "translated_text": "a+b",
            "should_translate": True,
            "translation_unit_formula_map": [
                {"placeholder": "[[F1]]", "formula_text": "a+b", "extra": {"k": 1}}
            ],
            "formula_map": [{"x": 1}],
        },
        # Present non-list formula map -> empty map (Python isinstance guard).
        {
            "item_id": "formula_map_bad",
            "source_text": "x=1",
            "translated_text": "x=1",
            "should_translate": True,
            "block_kind": "formula",
            "render_formula_map": {"not": "a list"},
        },
    ]


def _run_seed_native(items: list[dict]) -> list[dict]:
    _native.seed_render_fields(items)
    return items


def _run_seed_reference(items: list[dict]) -> list[dict]:
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        _native.seed_render_fields(items)
    finally:
        _native.NATIVE = was
    return items


def main() -> None:
    assert _native.NATIVE, "native module not built"
    assert hasattr(rendering_bridge, "seed_render_fields"), "seed_render_fields not exported"

    # Parity: native vs reference must be byte-exact on every branch.
    native_items = copy.deepcopy(seed_fixtures())
    reference_items = copy.deepcopy(seed_fixtures())
    _run_seed_native(native_items)
    _run_seed_reference(reference_items)
    for native_item, reference_item in zip(native_items, reference_items):
        assert native_item == reference_item, (
            f"seed parity diverged for {native_item.get('item_id')}"
        )

    # Behavior assertions on the native output.
    by_id = {item["item_id"]: item for item in native_items}
    assert by_id["normal"]["render_protected_text"] == "你好世界"
    assert by_id["normal"]["render_source_text"] == "Hello world"
    assert by_id["same_meaningful"]["render_protected_text"] == ""
    assert by_id["skip_math"]["render_protected_text"] == ""
    assert by_id["skip_math"]["render_formula_map"] == []
    assert by_id["skip_math"]["render_source_text"] == "x^2"
    assert by_id["formula_kept"]["render_protected_text"] == "$x^2$"
    assert by_id["formula_fallback"]["render_protected_text"] == "x^2"
    assert by_id["group"]["render_protected_text"] == "组翻译"
    assert by_id["group"]["render_source_text"] == "来源"
    assert by_id["continuation"]["render_protected_text"] == "成员翻译"
    assert by_id["structured"]["_render_preserve_line_breaks"] is True
    assert by_id["structured"]["_render_line_structure"] == "structured_lines"
    assert by_id["structured"]["render_protected_text"] == "1. 甲内容\n2. 乙内容"
    # Caption long lines collapse to a single flow line (no preserve flag).
    assert "\n" not in by_id["caption_long"]["render_protected_text"]
    assert by_id["caption_long"].get("_render_preserve_line_breaks") is None
    assert by_id["formula_map"]["render_formula_map"] == [
        {"placeholder": "[[F1]]", "formula_text": "a+b", "extra": {"k": 1}}
    ]
    assert by_id["formula_map_bad"]["render_formula_map"] == []

    # Integration: the seed boundary is wired into build_render_blocks.
    from services.rendering.layout.payload.blocks import build_render_blocks

    translated = copy.deepcopy(seed_fixtures())
    for item in translated:
        item.setdefault("bbox", [72.0, 140.0, 460.0, 175.0])
        item.setdefault("lines", [{"bbox": [72.0, 140.0, 460.0, 175.0], "spans": []}])
        item.setdefault("formula_map", [])
        item.setdefault("math_mode", "placeholder")
    blocks = build_render_blocks(translated, page_width=595.0, page_height=842.0)
    assert blocks, "build_render_blocks returned no render blocks"
    for block in blocks:
        assert block.render_kind, f"render block {block.block_id} missing render_kind"

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
