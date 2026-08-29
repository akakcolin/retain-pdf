#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N7 prepare boundary.

Four parts:

1. Native availability: the payload shim `layout/payload/_native` reports NATIVE
   (maturin build installed) and the bridge exports
   `prepare_render_payloads_by_page`.

2. Direct parity: a multi-page translated-item fixture runs through the shim's
   `prepare_render_payloads_by_page` on the native path, byte-exact against the
   same shim re-invoked with NATIVE=False (the pure-Python reference). Covers
   the plain-text passthrough, a two-member group unit split across boxes, the
   same-meaningful group that clears render fields, a continuation member that
   survives without re-split, a continuation unit split across boxes, a
   direct-typst math block excluded from the suspicious-OCR pass, and the
   suspicious OCR-glued block drop. The caller's input dicts stay untouched
   (native deep-copies).

3. Lookup passthrough: a precomputed first-line-indent lookup and an
   effective-inner-bbox lookup write `_render_first_line_indent_pt` and
   `_render_inner_bbox`; with neither lookup nor a source PDF the attach is
   skipped entirely.

4. Integration: `page_specs.build_render_page_specs` routes the prepare
   boundary through the native shim and produces byte-exact `RenderPageSpec`
   lists versus the payload shims forced to NATIVE=False, registering a native
   hit for the prepare stage and no `layout_payload` fallbacks.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_prepare_render_payloads_bridge.py
"""

import copy
import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402

import rendering_bridge  # noqa: E402
from services.rendering.layout.payload import _native  # noqa: E402


def _item(item_id: str, *, page_idx: int, y0: float, y1: float, **extra: object) -> dict:
    item: dict = {
        "item_id": item_id,
        "page_idx": page_idx,
        "block_type": "text",
        "bbox": [50.0, y0, 300.0, y1],
        "lines": [{"bbox": [50.0, y0, 300.0, y1], "spans": []}],
        "formula_map": [],
    }
    item.update(extra)
    return item


def prepare_fixtures() -> dict[int, list[dict]]:
    """One page per prepare branch the boundary must keep byte-exact."""
    return {
        0: [
            # Suspicious OCR-glued block: 1500 chars in a 40pt box, dropped
            # before the next block 20pt below.
            _item(
                "glue_a",
                page_idx=0,
                y0=20.0,
                y1=60.0,
                source_text="字" * 1500,
                translated_text="译" * 1500,
            ),
            _item(
                "glue_b",
                page_idx=0,
                y0=80.0,
                y1=120.0,
                source_text="next block",
                translated_text="下一块",
            ),
            # Plain body paragraph keeps its translation.
            _item(
                "normal",
                page_idx=0,
                y0=140.0,
                y1=180.0,
                source_text="Hello world",
                translated_text="你好世界",
            ),
            # Single-member group whose source and translation are the same
            # meaningful text -> render fields cleared.
            _item(
                "same_meaningful",
                page_idx=0,
                y0=200.0,
                y1=240.0,
                translation_unit_id="same",
                translation_unit_kind="group",
                translation_unit_protected_source_text="Same text",
                translation_unit_protected_translated_text="  Same   text ",
            ),
            # Two-member group unit split across the member boxes.
            _item(
                "group_m1",
                page_idx=0,
                y0=260.0,
                y1=300.0,
                translation_unit_id="g1",
                translation_unit_kind="group",
                translation_unit_protected_source_text="第一组来源文本",
                translation_unit_protected_translated_text="第一组翻译文本",
            ),
            _item(
                "group_m2",
                page_idx=0,
                y0=320.0,
                y1=360.0,
                translation_unit_id="g1",
                translation_unit_kind="group",
                translation_unit_protected_source_text="第一组来源文本",
                translation_unit_protected_translated_text="第一组翻译文本",
            ),
            # First-line-indent + effective-inner-bbox lookup targets.
            _item(
                "indent_item",
                page_idx=0,
                y0=380.0,
                y1=420.0,
                source_text="Indent paragraph text",
                translated_text="缩进段落文本",
            ),
            _item(
                "bbox_item",
                page_idx=0,
                y0=440.0,
                y1=480.0,
                source_text="Bbox adjusted text",
                translated_text="包围盒调整文本",
            ),
        ],
        1: [
            # Direct-typst math block: kept but excluded from the suspicious-OCR
            # drop pass.
            _item(
                "direct_math",
                page_idx=1,
                y0=20.0,
                y1=60.0,
                math_mode="direct_typst",
                source_text="x^2",
                translated_text="平方",
            ),
            # Continuation member with its own member text survives without a
            # re-split through the group loop.
            _item(
                "continuation_member",
                page_idx=1,
                y0=80.0,
                y1=120.0,
                continuation_group="cg1",
                protected_source_text="成员来源",
                protected_translated_text="成员翻译",
            ),
            # Continuation unit (no member text) grouped and split.
            _item(
                "cont_m1",
                page_idx=1,
                y0=140.0,
                y1=180.0,
                continuation_group="cg2",
                translation_unit_protected_source_text="续组来源文本",
                translation_unit_protected_translated_text="续组翻译文本",
            ),
            _item(
                "cont_m2",
                page_idx=1,
                y0=200.0,
                y1=240.0,
                continuation_group="cg2",
                translation_unit_protected_source_text="续组来源文本",
                translation_unit_protected_translated_text="续组翻译文本",
            ),
        ],
    }


FIRST_LINE_INDENT_LOOKUP = {"indent_item": 18.5}
EFFECTIVE_INNER_BBOX_LOOKUP = {"bbox_item": [55.0, 442.0, 295.0, 478.0]}


def _run_prepare_native(pages, *, indent_lookup, effective_lookup):
    return _native.prepare_render_payloads_by_page(
        pages,
        first_line_indent_lookup=indent_lookup,
        effective_inner_bbox_lookup=effective_lookup,
    )


def _run_prepare_reference(pages, *, indent_lookup, effective_lookup):
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        return _native.prepare_render_payloads_by_page(
            pages,
            first_line_indent_lookup=indent_lookup,
            effective_inner_bbox_lookup=effective_lookup,
        )
    finally:
        _native.NATIVE = was


def _by_id(pages: dict[int, list[dict]]) -> dict[str, dict]:
    return {item["item_id"]: item for items in pages.values() for item in items}


def build_source_pdf(pages) -> bytes:
    """`pages`: list of `(width, height, rotation)` tuples."""
    doc = fitz.open()
    for width, height, rotation in pages:
        page = doc.new_page(width=width, height=height)
        if rotation:
            page.set_rotation(rotation)
        page.insert_text((72.0, 72.0), f"page {len(doc)}", fontsize=12, fontname="helv")
    raw = doc.tobytes(garbage=0)
    doc.close()
    return raw


def simple_pages() -> dict[int, list[dict]]:
    """Production-shaped fixtures for the page_specs integration run."""
    return {
        idx: [
            {
                "item_id": f"p{idx:03d}-b001",
                "page_idx": idx,
                "block_type": "text",
                "bbox": [60.0, 80.0 + 40 * idx, 300.0, 100.0 + 40 * idx],
                "lines": [{"text": f"Line {idx}"}],
                "source_text": f"Line {idx}",
                "protected_source_text": f"Line {idx}",
                "protected_translated_text": f"译文 {idx}",
                "formula_map": [],
            }
        ]
        for idx in range(2)
    }


def main() -> None:
    assert _native.NATIVE, "native module not built"
    assert hasattr(rendering_bridge, "prepare_render_payloads_by_page"), (
        "prepare_render_payloads_by_page not exported"
    )

    # Part 2: byte-exact native-vs-reference parity on every branch.
    native_pages = _run_prepare_native(
        copy.deepcopy(prepare_fixtures()),
        indent_lookup=FIRST_LINE_INDENT_LOOKUP,
        effective_lookup=EFFECTIVE_INNER_BBOX_LOOKUP,
    )
    reference_pages = _run_prepare_reference(
        copy.deepcopy(prepare_fixtures()),
        indent_lookup=FIRST_LINE_INDENT_LOOKUP,
        effective_lookup=EFFECTIVE_INNER_BBOX_LOOKUP,
    )
    assert native_pages == reference_pages, "prepare parity diverged native vs reference"
    assert set(native_pages) == {0, 1}
    assert len(native_pages[0]) == 8 and len(native_pages[1]) == 4

    # Caller input untouched by the native path (deep-copy semantics).
    pristine = prepare_fixtures()
    _run_prepare_native(
        copy.deepcopy(pristine),
        indent_lookup=FIRST_LINE_INDENT_LOOKUP,
        effective_lookup=EFFECTIVE_INNER_BBOX_LOOKUP,
    )
    assert pristine == prepare_fixtures(), "native prepare mutated caller input"

    by_id = _by_id(native_pages)
    # Suspicious OCR-glued block dropped with a diagnostic.
    assert by_id["glue_a"]["render_skip_reason"] == "suspicious_ocr_glued_block"
    assert by_id["glue_a"]["render_protected_text"] == ""
    assert by_id["glue_b"]["render_protected_text"] == "下一块"
    # Plain text preserved.
    assert by_id["normal"]["render_protected_text"] == "你好世界"
    # Same-meaningful group clears render fields.
    assert by_id["same_meaningful"]["render_protected_text"] == ""
    assert by_id["same_meaningful"]["render_formula_map"] == []
    # Group split: member chunks concatenate back to the unit text.
    group_text = (
        by_id["group_m1"]["render_protected_text"] + by_id["group_m2"]["render_protected_text"]
    )
    assert group_text == "第一组翻译文本", f"group chunks {group_text!r} != unit text"
    # Continuation unit split likewise, member survives without re-split.
    cont_text = by_id["cont_m1"]["render_protected_text"] + by_id["cont_m2"]["render_protected_text"]
    assert cont_text == "续组翻译文本", f"continuation chunks {cont_text!r} != unit text"
    assert by_id["continuation_member"]["render_protected_text"] == "成员翻译"
    # Direct-typst math kept and not dropped.
    assert "render_skip_reason" not in by_id["direct_math"]
    assert by_id["direct_math"]["render_protected_text"] == "平方"
    # Lookups write their fields.
    assert by_id["indent_item"]["_render_first_line_indent_pt"] == 18.5
    assert by_id["bbox_item"]["_render_inner_bbox"] == [55.0, 442.0, 295.0, 478.0]

    # Part 3: no lookup + no source -> the attach is skipped.
    no_lookup = _run_prepare_native(
        copy.deepcopy(prepare_fixtures()),
        indent_lookup=None,
        effective_lookup=None,
    )
    for items in no_lookup.values():
        for item in items:
            assert "_render_first_line_indent_pt" not in item
            assert "_render_inner_bbox" not in item

    # Part 4: integration through build_render_page_specs.
    from services.rendering import _routing
    from services.rendering.layout.payload import _native as _payload_native
    from services.rendering.layout.page_specs import build_render_page_specs

    # The prepare shim records exactly one isolated native hit and no fallback.
    _routing.reset()
    _run_prepare_native(
        copy.deepcopy(prepare_fixtures()),
        indent_lookup=None,
        effective_lookup=None,
    )
    snap = _routing.snapshot()
    assert snap["hits"].get("layout_payload", 0) == 1, (
        f"prepare shim recorded {snap['hits'].get('layout_payload', 0)} hits, expected exactly 1"
    )
    assert "layout_payload" not in snap["fallbacks"], (
        f"prepare shim recorded layout_payload fallbacks {snap['fallbacks'].get('layout_payload')}"
    )

    raw = build_source_pdf([(612.0, 792.0, 0), (595.0, 842.0, 0)])
    with tempfile.TemporaryDirectory(prefix="rps-prep-") as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(raw)
        _routing.reset()
        specs_native = build_render_page_specs(
            source_pdf_path=src,
            translated_pages=copy.deepcopy(simple_pages()),
        )
        snap = _routing.snapshot()
        assert snap["hits"].get("layout_payload", 0) >= 9, (
            f"page_specs pipeline hit {snap['hits'].get('layout_payload', 0)} native stages, "
            "expected >= 9 (prepare + build_block_payloads/apply_body_pipeline/"
            "mark_adjacent_collision_risk/emit_render_blocks per page)"
        )
        assert "layout_payload" not in snap["fallbacks"], (
            f"page_specs pipeline recorded layout_payload fallbacks {snap['fallbacks'].get('layout_payload')}"
        )

        was = _payload_native.NATIVE
        _payload_native.NATIVE = False
        try:
            specs_ref = build_render_page_specs(
                source_pdf_path=src,
                translated_pages=copy.deepcopy(simple_pages()),
            )
        finally:
            _payload_native.NATIVE = was
        assert len(specs_native) == len(specs_ref) == 2
        for via_native, via_ref in zip(specs_native, specs_ref):
            assert via_native == via_ref, (
                f"page_specs prepare pipeline diverged native vs reference on page {via_native.page_index}"
            )

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
