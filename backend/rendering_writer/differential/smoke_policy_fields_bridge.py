#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N8 policy-fields boundary.

Four parts:

1. Native availability: the `policy/_native` shim reports NATIVE (maturin build
   installed) and the bridge exports `apply_render_pages_policy_fields`.

2. Direct parity: a two-page translated-item fixture runs through the shim's
   `apply_render_pages_policy_fields` on the native path, byte-exact against the
   same shim re-invoked with NATIVE=False (the pure-Python reference), under
   each of the three layout-config modes — default text-overlay cover fill
   (`delete_text`), typst-fill (`visual_cover`), and the no-op branch where both
   flags are off. Covers the formula-page region flag, non-translated items
   (status/decision/tags) excluded, empty-item-id skipped, and the caller's
   input dicts staying untouched (native deep-copies).

3. Single-page variant parity and `prepare_translated_pages_for_render`
   integration: prepare (native) + policy fields both native, byte-exact against
   both shims forced to NATIVE=False, registering one `policy` native hit and no
   `policy` fallbacks.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_policy_fields_bridge.py
"""

import copy
import os
import sys

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from foundation.config import layout  # noqa: E402
from services.rendering import _routing  # noqa: E402
from services.rendering.output.typst.book_support import prepare_translated_pages_for_render  # noqa: E402
from services.rendering.policy import _native  # noqa: E402
from services.rendering.policy import apply_render_pages_policy_fields  # noqa: E402
from services.rendering.policy import apply_render_page_policy_fields  # noqa: E402
from services.rendering.policy import item_render_policy  # noqa: E402

import rendering_bridge  # noqa: E402


def fixtures() -> dict[int, list[dict]]:
    """A formula page and a text page; item 3 is marked non-translated."""
    return {
        0: [
            {
                "item_id": "p001-b001",
                "page_idx": 0,
                "block_type": "text",
                "block_kind": "text",
                "bbox": [40.0, 40.0, 260.0, 70.0],
                "translated_text": "上文",
            },
            {
                "item_id": "p001-b002",
                "page_idx": 0,
                "block_type": "formula",
                "block_kind": "formula",
                "normalized_sub_type": "display_formula",
                "bbox": [96.0, 76.0, 224.0, 104.0],
            },
            {
                "item_id": "p001-b003",
                "page_idx": 0,
                "block_type": "text",
                "block_kind": "text",
                "bbox": [40.0, 80.0, 260.0, 110.0],
                "source_text": "kept source",
                "translation_status": "keep_origin",
            },
            {
                "item_id": "p001-b004",
                "page_idx": 0,
                "block_type": "text",
                "block_kind": "text",
                "bbox": [40.0, 120.0, 260.0, 150.0],
                "translated_text": "跳过标记",
                "decision": "skip_translation",
            },
            {
                "item_id": "p001-b005",
                "page_idx": 0,
                "block_type": "text",
                "block_kind": "text",
                "bbox": [40.0, 160.0, 260.0, 190.0],
                "translated_text": "标签标记",
                "tags": ["skip_translation"],
            },
        ],
        1: [
            {
                "item_id": "p002-b001",
                "page_idx": 1,
                "block_type": "text",
                "block_kind": "text",
                "bbox": [40.0, 40.0, 260.0, 70.0],
                "translated_text": "译文",
            },
        ],
    }


def run_native(pages):
    return apply_render_pages_policy_fields(copy.deepcopy(pages))


def run_reference(pages):
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        return apply_render_pages_policy_fields(copy.deepcopy(pages))
    finally:
        _native.NATIVE = was


def check_mode(pages, *, expected_reason, expected_cleanup_mode, expect_policies) -> None:
    native = run_native(pages)
    reference = run_reference(pages)
    assert native == reference, "policy-fields parity diverged native vs reference"
    if not expect_policies:
        for items in native.values():
            for item in items:
                assert "_render_policy" not in item
        return
    for item in native[0]:
        policy = item_render_policy(item)
        if item["item_id"] == "p001-b001":
            assert policy == {
                "cleanup_mode": expected_cleanup_mode,
                "overlay_fill": "sampled",
                "formula_protection_role": "none",
                "reason": expected_reason,
            }, f"unexpected policy {policy} for {item['item_id']}"
        else:
            assert policy == {}, f"unexpected policy {policy} for {item['item_id']}"
    assert item_render_policy(native[1][0])["reason"] == expected_reason


def main() -> None:
    assert _native.NATIVE, "native module not built"
    assert hasattr(rendering_bridge, "apply_render_pages_policy_fields"), (
        "apply_render_pages_policy_fields not exported"
    )

    # Part 2: byte-exact parity per layout-config mode.
    default_strategy = layout.normalize_source_cleanup_strategy(None)
    default_cover_fill = layout.use_default_text_overlay_cover_fill()
    try:
        layout.apply_layout_tuning(source_cleanup_strategy="pikepdf_text_strip", default_text_overlay_cover_fill=True)
        check_mode(
            fixtures(),
            expected_reason="default_text_overlay_cover_fill",
            expected_cleanup_mode="delete_text",
            expect_policies=True,
        )

        layout.apply_layout_tuning(source_cleanup_strategy="typst_fill", default_text_overlay_cover_fill=True)
        check_mode(
            fixtures(),
            expected_reason="typst_fill_default",
            expected_cleanup_mode="visual_cover",
            expect_policies=True,
        )

        layout.apply_layout_tuning(source_cleanup_strategy="pikepdf_text_strip", default_text_overlay_cover_fill=False)
        check_mode(fixtures(), expected_reason="", expected_cleanup_mode="", expect_policies=False)
    finally:
        layout.apply_layout_tuning(
            source_cleanup_strategy=default_strategy,
            default_text_overlay_cover_fill=default_cover_fill,
        )

    # Caller input untouched by the native path (deep-copy semantics).
    layout.apply_layout_tuning(source_cleanup_strategy="pikepdf_text_strip", default_text_overlay_cover_fill=True)
    pristine = fixtures()
    run_native(pristine)
    assert pristine == fixtures(), "native policy-fields mutated caller input"

    # Part 3: single-page variant parity.
    single_native = apply_render_page_policy_fields(copy.deepcopy(fixtures()[1]))
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        single_reference = apply_render_page_policy_fields(copy.deepcopy(fixtures()[1]))
    finally:
        _native.NATIVE = was
    assert single_native == single_reference, "single-page policy-fields parity diverged"
    assert item_render_policy(single_native[0])["reason"] == "default_text_overlay_cover_fill"

    # Part 4: prepare integration — prepare + policy fields both native,
    # byte-exact against both shims forced to NATIVE=False, with one policy hit
    # and no policy fallbacks.
    from services.rendering.layout.payload import _native as _payload_native

    simple_pages = {
        idx: [
            {
                "item_id": f"p{idx:03d}-b001",
                "page_idx": idx,
                "block_type": "text",
                "bbox": [60.0, 80.0 + 40 * idx, 300.0, 100.0 + 40 * idx],
                "source_text": f"Line {idx}",
                "protected_source_text": f"Line {idx}",
                "protected_translated_text": f"译文 {idx}",
                "translated_text": f"译文 {idx}",
                "formula_map": [],
            }
        ]
        for idx in range(2)
    }

    _routing.reset()
    prepared_native = prepare_translated_pages_for_render(None, copy.deepcopy(simple_pages))
    snap = _routing.snapshot()
    assert snap["hits"].get("policy", 0) == 1, (
        f"prepare integration recorded {snap['hits'].get('policy', 0)} policy hits, expected 1"
    )
    assert "policy" not in snap["fallbacks"], (
        f"prepare integration recorded policy fallbacks {snap['fallbacks'].get('policy')}"
    )
    assert item_render_policy(prepared_native[0][0])["reason"] == "default_text_overlay_cover_fill"
    assert item_render_policy(prepared_native[0][0])["cleanup_mode"] == "delete_text"

    payload_was = _payload_native.NATIVE
    policy_was = _native.NATIVE
    _payload_native.NATIVE = False
    _native.NATIVE = False
    try:
        prepared_reference = prepare_translated_pages_for_render(None, copy.deepcopy(simple_pages))
    finally:
        _payload_native.NATIVE = payload_was
        _native.NATIVE = policy_was
    assert prepared_native == prepared_reference, (
        "prepare integration diverged native vs reference (prepare + policy fields)"
    )

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
