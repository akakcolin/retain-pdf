#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N4 collision boundary
`mark_adjacent_collision_risk`.

Three parts:

1. Native availability: the layout shim `layout/payload/_native` reports NATIVE
   (maturin build installed) and the bridge exports `mark_adjacent_collision_risk`.

2. Collision parity: hand-built stacked body payloads run through the native
   `_native.mark_adjacent_collision_risk` equal the pure-Python reference (the
   shim re-invoked with NATIVE=False). Three branches are exercised: the re-fit
   branch (off the unified page font -> `adjacent_collision_risk` + font/leading
   re-fit to the available height), the unified-leading branch (at the page
   font -> `_body_collision_leading_only` + leading clamp, font untouched), and
   roomy/non-body pairs that must pass through untouched.

3. title_fit round-trip: a title payload carrying a real `TitleFitDecision`
   dataclass passes through the JSON boundary unchanged (asdict on the way in,
   restored after).

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_collision_bridge.py
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

LONG_TEXT = (
    "段落文字内容足够长的时候会占满整个高度并且还有更多的文本需要换行排列这样才能触发碰撞拟合逻辑的正常路径执行。"
    "这段内容再次重复一遍以确保它足够长，能够稳定地超出相邻块可用的垂直高度预算从而真正进入碰撞处理的分支而不是提前返回。"
    "再加上更多的文字让段落变得足够高，最终拟合时字体尺寸一定会被压缩到更小的数值以满足垂直空间的限制要求。"
)


def _payload(
    item_id: str,
    *,
    is_body: bool,
    top: float,
    bottom: float,
    font: float,
    leading: float,
    page_font: float,
    title_fit: TitleFitDecision | None = None,
) -> dict:
    return {
        "item": {
            "item_id": item_id,
            "block_type": "text",
            "block_kind": "text",
            "layout_role": "paragraph",
            "semantic_role": "body" if is_body else "paragraph",
        },
        "inner_bbox": [50.0, top, 250.0, bottom],
        "translated_text": LONG_TEXT,
        "formula_map": [],
        "font_size_pt": font,
        "leading_em": leading,
        "page_body_font_size_pt": page_font,
        "render_kind": "markdown",
        "is_body": is_body,
        "dense_small_box": False,
        "heavy_dense_small_box": False,
        "prefer_typst_fit": False,
        "title_fit": title_fit,
    }


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


def _run_native(payloads: list[dict]) -> list[dict]:
    return _native.mark_adjacent_collision_risk(payloads)


def _run_reference(payloads: list[dict]) -> list[dict]:
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        return _native.mark_adjacent_collision_risk(payloads)
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


def main() -> None:
    assert _native.NATIVE, "native module not built"
    assert hasattr(rendering_bridge, "mark_adjacent_collision_risk"), (
        "mark_adjacent_collision_risk not exported"
    )

    # Re-fit branch: off the unified page font -> collision risk + font re-fit.
    refit = [
        _payload("b1", is_body=True, top=0.0, bottom=100.0, font=11.0, leading=0.5, page_font=12.0),
        _payload("b2", is_body=True, top=101.5, bottom=200.0, font=12.0, leading=0.5, page_font=12.0),
    ]
    native = copy.deepcopy(refit)
    reference = copy.deepcopy(refit)
    _run_native(native)
    _run_reference(reference)
    _assert_payloads_equal(native, reference, "re-fit branch")
    body = native[0]
    assert body["adjacent_collision_risk"] is True, "re-fit block must be marked collision risk"
    assert body["prefer_typst_fit"] is True, "re-fit block must prefer typst fit"
    assert float(body["font_size_pt"]) < 11.0, "re-fit block font must be compressed"
    assert float(body["adjacent_available_height_pt"]) >= 6.0, "re-fit block must remember height limit"

    # Unified-leading branch: at the page font -> leading clamp only.
    unified = [
        _payload("c1", is_body=True, top=0.0, bottom=100.0, font=12.0, leading=0.58, page_font=12.0),
        _payload("c2", is_body=True, top=101.5, bottom=200.0, font=12.0, leading=0.5, page_font=12.0),
    ]
    native = copy.deepcopy(unified)
    reference = copy.deepcopy(unified)
    _run_native(native)
    _run_reference(reference)
    _assert_payloads_equal(native, reference, "unified-leading branch")
    body = native[0]
    assert body["_body_collision_leading_only"] is True, "unified block must clamp leading only"
    assert float(body["font_size_pt"]) == 12.0, "unified block font must be untouched"
    assert float(body["leading_em"]) <= 0.56, "unified block leading must clamp to 0.56"

    # Roomy pairs must pass through untouched; tightly-stacked non-body pairs
    # take the same re-fit path (the collision loop has no is_body gate).
    roomy = [
        _payload("d1", is_body=True, top=0.0, bottom=40.0, font=12.0, leading=0.5, page_font=12.0),
        _payload("d2", is_body=True, top=90.0, bottom=140.0, font=12.0, leading=0.5, page_font=12.0),
    ]
    native = copy.deepcopy(roomy)
    reference = copy.deepcopy(roomy)
    _run_native(native)
    _run_reference(reference)
    _assert_payloads_equal(native, reference, "roomy pair")
    assert native[0].get("adjacent_collision_risk") is None, "roomy pair must be untouched"
    assert float(native[0]["font_size_pt"]) == 12.0, "roomy pair font must be untouched"

    non_body = [
        _payload("e1", is_body=False, top=0.0, bottom=100.0, font=11.0, leading=0.5, page_font=12.0),
        _payload("e2", is_body=False, top=101.5, bottom=200.0, font=12.0, leading=0.5, page_font=12.0),
    ]
    native = copy.deepcopy(non_body)
    reference = copy.deepcopy(non_body)
    _run_native(native)
    _run_reference(reference)
    _assert_payloads_equal(native, reference, "non-body tight pair")
    assert native[0]["adjacent_collision_risk"] is True, "non-body tight pair must be marked collision risk"

    # title_fit round-trip through the JSON boundary.
    mixed = [
        _payload("t1", is_body=False, top=0.0, bottom=40.0, font=29.35, leading=0.28, page_font=0.0, title_fit=_title_fit_decision()),
        _payload("b1", is_body=True, top=50.0, bottom=100.0, font=11.0, leading=0.5, page_font=12.0),
        _payload("b2", is_body=True, top=101.5, bottom=200.0, font=12.0, leading=0.5, page_font=12.0),
    ]
    native = copy.deepcopy(mixed)
    reference = copy.deepcopy(mixed)
    _run_native(native)
    _run_reference(reference)
    _assert_payloads_equal(native, reference, "title_fit mixed payloads")
    assert isinstance(native[0]["title_fit"], TitleFitDecision), "title_fit must round-trip as the dataclass"
    assert native[0]["title_fit"].font_size_pt == 29.35, "title_fit fields must round-trip"

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
