#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N3 body-pipeline boundary.

Three parts:

1. Native availability: the layout shim `layout/payload/_native` reports NATIVE
   (maturin build installed) and the bridge exports `apply_body_pipeline` and
   `resolve_book_body_font_target`.

2. Body-pipeline parity: hand-built body payloads (markdown, same column, wide
   body boxes) run through the native `_native.apply_body_pipeline` equal the
   pure-Python reference (the shim re-invoked with NATIVE=False ->
   body_pipeline/annotation fallback). All body fonts must converge to the low
   stable target, `page_body_font_size_pt` annotated, `_body_font_unified`
   set. A title payload carrying a real `TitleFitDecision` dataclass round-trips
   through the JSON boundary unchanged (asdict on the way in, restored after).

3. Annotation parity: caption (`item.block_type: "figure_caption"`) and footnote
   (`item.layout_role: "footnote"`) payloads appended to the same ordered list
   run through `unify_annotation_fonts` + `recover_underfilled_annotation_density`
   on both paths: captions converge to the body-font-capped target and the
   footnote grows toward the underfilled density recovery target.

4. Book-target parity: two pages of the same body fixture resolve the whole-book
   body font target identically on both paths (`resolve_book_body_font_target`).

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_apply_body_pipeline_bridge.py
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
from services.rendering.layout.title_binary_fit import TitleFitDecision  # noqa: E402

PAGE_TEXT_WIDTH_MED = 368.0


def _payload(
    item_id: str,
    *,
    bbox: list[float],
    font_size_pt: float,
    leading_em: float,
    text: str,
    is_body: bool,
    block_type: str = "text",
    layout_role: str = "paragraph",
    title_fit: TitleFitDecision | None = None,
) -> dict:
    item = {
        "item_id": item_id,
        "page_idx": 0,
        "block_type": block_type,
        "block_kind": "text",
        "layout_role": layout_role,
        "semantic_role": "body" if is_body else layout_role,
        "bbox": bbox,
        "lines": [{"bbox": bbox, "spans": []}],
        "source_text": "s",
        "protected_source_text": "s",
        "translated_text": text,
        "protected_translated_text": text,
        "formula_map": [],
        "should_translate": True,
        "math_mode": "placeholder",
    }
    return {
        "index": 0,
        "item": item,
        "bbox": bbox,
        "cover_bbox": bbox,
        "inner_bbox": bbox,
        "translated_text": text,
        "formula_map": [],
        "render_kind": "markdown",
        "font_size_pt": font_size_pt,
        "leading_em": leading_em,
        "first_line_indent_pt": 0.0,
        "font_weight": "regular",
        "page_body_font_size_pt": None,
        "is_body": is_body,
        "dense_small_box": False,
        "heavy_dense_small_box": False,
        "prefer_typst_fit": False,
        "title_fit": title_fit,
        "preserve_line_breaks": False,
        "adjacent_collision_risk": False,
        "adjacent_available_height_pt": None,
        "text_color": [0.0, 0.0, 0.0],
        "cover_fill": [1.0, 1.0, 1.0],
    }


BODY_TEXT = "这是一个相当长的正文段落，包含多行流动的中文文本内容用于测量排版密度。"
CAPTION_TEXT = "图 1：示例标题文字，用于排版密度测量。"
FOOTNOTE_TEXT = "脚注文字示例，用于排版密度测量。"


def _title_fit_decision() -> TitleFitDecision:
    return TitleFitDecision(
        font_size_pt=29.35,
        leading_em=0.28,
        fit_to_box=True,
        fit_single_line=True,
        fit_min_font_size_pt=18.0,
        fit_max_font_size_pt=30.0,
        fit_min_leading_em=0.2,
        fit_max_height_pt=40.0,
        fit_target_width_pt=468.0,
        fit_target_height_pt=40.0,
    )


def body_fixture() -> list[dict]:
    """Four same-column body payloads (width 388 >= 0.58*368, height 35 >= 30)
    so every payload is a font-unify anchor; fonts [12.0, 11.5, 11.0, 10.0]
    must converge to the low stable target 10.0."""
    return [
        _payload("b1", bbox=[72.0, 140.0, 460.0, 175.0], font_size_pt=12.0, leading_em=0.35, text=BODY_TEXT, is_body=True),
        _payload("b2", bbox=[72.0, 180.0, 460.0, 215.0], font_size_pt=11.5, leading_em=0.35, text=BODY_TEXT, is_body=True),
        _payload("b3", bbox=[72.0, 220.0, 460.0, 255.0], font_size_pt=11.0, leading_em=0.35, text=BODY_TEXT, is_body=True),
        _payload("b4", bbox=[72.0, 260.0, 460.0, 295.0], font_size_pt=10.0, leading_em=0.35, text=BODY_TEXT, is_body=True),
    ]


def annotation_fixture() -> list[dict]:
    """Two bodies plus two captions and one footnote; the title carries a real
    TitleFitDecision so the JSON boundary round-trip is exercised. Bodies unify
    to 11.0, captions cap to 9.68 (11.0 * CAPTION_BODY_FONT_CAP_RATIO), and the
    footnote recovers toward the underfilled density target."""
    return [
        _payload("t1", bbox=[72.0, 40.0, 540.0, 80.0], font_size_pt=29.35, leading_em=0.28, text="第一章", is_body=False, layout_role="title", title_fit=_title_fit_decision()),
        _payload("b1", bbox=[72.0, 140.0, 460.0, 175.0], font_size_pt=12.0, leading_em=0.35, text=BODY_TEXT, is_body=True),
        _payload("b2", bbox=[72.0, 180.0, 460.0, 215.0], font_size_pt=11.0, leading_em=0.35, text=BODY_TEXT, is_body=True),
        _payload("c1", bbox=[72.0, 230.0, 460.0, 255.0], font_size_pt=11.0, leading_em=0.35, text=CAPTION_TEXT, is_body=False, block_type="figure_caption"),
        _payload("c2", bbox=[72.0, 260.0, 460.0, 285.0], font_size_pt=10.5, leading_em=0.35, text=CAPTION_TEXT, is_body=False, block_type="figure_caption"),
        _payload("f1", bbox=[72.0, 300.0, 400.0, 322.0], font_size_pt=9.5, leading_em=0.30, text=FOOTNOTE_TEXT, is_body=False, layout_role="footnote"),
    ]


def _run_apply_native(payloads: list[dict]) -> list[dict]:
    return _native.apply_body_pipeline(payloads, page_text_width_med=PAGE_TEXT_WIDTH_MED)


def _run_apply_reference(payloads: list[dict]) -> list[dict]:
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        return _native.apply_body_pipeline(payloads, page_text_width_med=PAGE_TEXT_WIDTH_MED)
    finally:
        _native.NATIVE = was


def _assert_payloads_equal(native_payloads, reference_payloads, context: str) -> None:
    assert len(native_payloads) == len(reference_payloads), (
        f"[{context}] payload count native {len(native_payloads)} != "
        f"reference {len(reference_payloads)}"
    )
    for native_payload, reference_payload in zip(native_payloads, reference_payloads):
        assert native_payload == reference_payload, (
            f"[{context}] payload {native_payload['item']['item_id']} diverged"
        )


def _font_and_leading(payload: dict) -> tuple[float, float]:
    return float(payload["font_size_pt"]), float(payload["leading_em"])


def main() -> None:
    assert _native.NATIVE, "native module not built"
    assert hasattr(rendering_bridge, "apply_body_pipeline"), "apply_body_pipeline not exported"
    assert hasattr(rendering_bridge, "resolve_book_body_font_target"), "resolve_book_body_font_target not exported"

    # Body-pipeline parity: fonts converge to the low stable target 10.0.
    native_payloads = copy.deepcopy(body_fixture())
    reference_payloads = copy.deepcopy(body_fixture())
    _run_apply_native(native_payloads)
    _run_apply_reference(reference_payloads)
    _assert_payloads_equal(native_payloads, reference_payloads, "body-pipeline fixture")
    by_id = {payload["item"]["item_id"]: payload for payload in native_payloads}
    for item_id in ("b1", "b2", "b3", "b4"):
        font, leading = _font_and_leading(by_id[item_id])
        assert font == 10.0, f"{item_id} font {font} != 10.0 after unify"
        assert leading == 0.7, f"{item_id} leading {leading} != 0.7"
        assert by_id[item_id].get("page_body_font_size_pt") == 10.0, (
            f"{item_id} page_body_font_size_pt not annotated to 10.0"
        )
        assert by_id[item_id].get("_body_font_unified") is True, f"{item_id} not marked unified"

    # Annotation + title_fit round-trip parity.
    native_payloads = copy.deepcopy(annotation_fixture())
    reference_payloads = copy.deepcopy(annotation_fixture())
    _run_apply_native(native_payloads)
    _run_apply_reference(reference_payloads)
    _assert_payloads_equal(native_payloads, reference_payloads, "annotation fixture")
    by_id = {payload["item"]["item_id"]: payload for payload in native_payloads}
    assert isinstance(by_id["t1"]["title_fit"], TitleFitDecision), "title_fit must round-trip as the dataclass"
    assert by_id["t1"]["title_fit"].font_size_pt == 29.35, "title_fit fields must round-trip"
    assert _font_and_leading(by_id["b1"]) == (11.0, 0.7), "body 1 must unify to 11.0/0.7"
    assert _font_and_leading(by_id["b2"]) == (11.0, 0.7), "body 2 must unify to 11.0/0.7"
    assert by_id["b1"].get("_body_font_unified") is True, "body 1 not marked unified"
    assert by_id["b1"].get("page_body_font_size_pt") == 11.0, "body 1 page_body_font_size_pt != 11.0"
    assert _font_and_leading(by_id["c1"]) == (9.68, 0.45), "caption 1 must cap to 9.68"
    assert _font_and_leading(by_id["c2"]) == (9.68, 0.45), "caption 2 must cap to 9.68"
    assert _font_and_leading(by_id["f1"]) == (9.02, 0.4), "footnote must recover to 9.02/0.4"

    # Whole-book body font target parity (low stable body font across pages).
    two_pages = [(body_fixture(), PAGE_TEXT_WIDTH_MED), (body_fixture(), PAGE_TEXT_WIDTH_MED)]
    native_target = _native.resolve_book_body_font_target_from_payloads(two_pages)
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        reference_target = _native.resolve_book_body_font_target_from_payloads(two_pages)
    finally:
        _native.NATIVE = was
    assert native_target == reference_target, (
        f"book font target native {native_target} != reference {reference_target}"
    )
    assert native_target == 10.0, f"book font target {native_target} != 10.0"

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
