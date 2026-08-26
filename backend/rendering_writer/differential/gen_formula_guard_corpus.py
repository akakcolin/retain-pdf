#!/usr/bin/env python3
"""Formula-guard corpus generator (Phase 7R-6).

Runs the REAL production `protect_formula_regions_in_redaction_items` on
hand-built translated/redaction item dicts and records the output projection
(bbox + `_formula_guard_fragment` + `_formula_guard_fragment_index`) per output
item. The config is pinned via
`apply_layout_tuning(source_cleanup_strategy="pikepdf_text_strip",
default_text_overlay_cover_fill=False)` so `build_render_page_policy` yields
empty item policies and the policy-field application is identity.

Each case asserts the production projection equals the hand-written expected
projection before recording, so a corpus case only captures a verified split /
drop / unchanged outcome. The Rust replay (`tests/formula_guard_diff.rs`) runs
the ported `background::formula_guard::protect_formula_regions_in_redaction_items`
on the same DTOs and asserts the identical projection.

Deterministic: no RNG, stable iteration, sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_formula_guard_corpus.py
"""

import json
import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from foundation.config.layout import apply_layout_tuning  # noqa: E402
from services.rendering.policy.formula_guard import (  # noqa: E402
    protect_formula_regions_in_redaction_items,
)

OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "formula_guard_corpus.json"))

apply_layout_tuning(
    source_cleanup_strategy="pikepdf_text_strip",
    default_text_overlay_cover_fill=False,
)


def _project(item: dict) -> dict:
    return {
        "bbox": item.get("bbox"),
        "_formula_guard_fragment": bool(item.get("_formula_guard_fragment", False)),
        "_formula_guard_fragment_index": item.get("_formula_guard_fragment_index"),
    }


def _run_case(
    name: str,
    translated_items: list[dict],
    redaction_items: list[dict],
    expected: list[dict],
) -> dict:
    real = protect_formula_regions_in_redaction_items(
        list(redaction_items),
        translated_items,
    )
    real_proj = [_project(item) for item in real]
    assert real_proj == expected, f"{name}:\n  real={real_proj!r}\n  exp ={expected!r}"
    return {
        "name": name,
        "translated_items": translated_items,
        "redaction_items": redaction_items,
        "expected": expected,
    }


def _formula(bbox: list[float], text: str = "f") -> dict:
    return {"bbox": bbox, "translated_text": text, "block_type": "formula"}


def _text(bbox: list[float], text: str) -> dict:
    return {"bbox": bbox, "translated_text": text, "block_type": "text"}


def _frag(bbox: list[float] | None, index: int | None) -> dict:
    return {
        "bbox": bbox,
        "_formula_guard_fragment": index is not None,
        "_formula_guard_fragment_index": index,
    }


def main() -> None:
    cases = []

    # 1. Formula in the middle of a tall text item -> four quadrant fragments.
    cases.append(_run_case(
        "split_quadrants",
        [_formula([100.0, 100.0, 200.0, 120.0]), _text([50.0, 50.0, 250.0, 250.0], "t")],
        [{"bbox": [50.0, 50.0, 250.0, 250.0], "translated_text": "t", "item_id": "item-t"}],
        [
            _frag([50.0, 50.0, 250.0, 88.0], 0),
            _frag([50.0, 132.0, 250.0, 250.0], 1),
            _frag([50.0, 88.0, 92.0, 132.0], 2),
            _frag([208.0, 88.0, 250.0, 132.0], 3),
        ],
    ))

    # 2. Item bbox fully inside its own expanded guard -> dropped.
    cases.append(_run_case(
        "fully_contained_dropped",
        [_formula([100.0, 100.0, 200.0, 120.0])],
        [{"bbox": [100.0, 100.0, 200.0, 120.0], "translated_text": "f", "item_id": "item-f"}],
        [],
    ))

    # 3. Far item passes through unchanged; a second item splits around the guard.
    cases.append(_run_case(
        "unchanged_far_item_mixed",
        [_formula([100.0, 300.0, 200.0, 320.0])],
        [
            {"bbox": [50.0, 50.0, 250.0, 100.0], "translated_text": "far", "item_id": "item-far"},
            {"bbox": [150.0, 250.0, 350.0, 450.0], "translated_text": "near", "item_id": "item-near"},
        ],
        [
            _frag([50.0, 50.0, 250.0, 100.0], None),
            _frag([150.0, 250.0, 350.0, 288.0], 0),
            _frag([150.0, 332.0, 350.0, 450.0], 1),
            _frag([208.0, 288.0, 350.0, 332.0], 2),
        ],
    ))

    # 4. Non-4-length bbox (len 3) and absent bbox pass through unchanged.
    cases.append(_run_case(
        "bbox_len_not_four_unchanged",
        [_formula([100.0, 100.0, 200.0, 120.0])],
        [
            {"bbox": [10.0, 10.0, 20.0], "translated_text": "a", "item_id": "item-a"},
            {"translated_text": "b", "item_id": "item-b"},
        ],
        [
            _frag([10.0, 10.0, 20.0], None),
            _frag(None, None),
        ],
    ))

    # 5. Len-4 empty bbox dropped; the other item still splits.
    cases.append(_run_case(
        "empty_bbox_dropped",
        [_formula([100.0, 100.0, 200.0, 120.0])],
        [
            {"bbox": [100.0, 100.0, 100.0, 100.0], "translated_text": "e", "item_id": "item-e"},
            {"bbox": [50.0, 50.0, 250.0, 250.0], "translated_text": "c", "item_id": "item-c"},
        ],
        [
            _frag([50.0, 50.0, 250.0, 88.0], 0),
            _frag([50.0, 132.0, 250.0, 250.0], 1),
            _frag([50.0, 88.0, 92.0, 132.0], 2),
            _frag([208.0, 88.0, 250.0, 132.0], 3),
        ],
    ))

    # 6. No formula items -> identity (early return).
    cases.append(_run_case(
        "no_formula_identity",
        [_text([50.0, 50.0, 250.0, 250.0], "t")],
        [
            {"bbox": [50.0, 50.0, 250.0, 250.0], "translated_text": "t", "item_id": "item-t"},
            {"bbox": [10.0, 10.0, 20.0], "translated_text": "a", "item_id": "item-a"},
        ],
        [
            _frag([50.0, 50.0, 250.0, 250.0], None),
            _frag([10.0, 10.0, 20.0], None),
        ],
    ))

    # 7. Item vertically inside the guard -> left/right-only split.
    cases.append(_run_case(
        "horizontal_only_split",
        [_formula([200.0, 50.0, 300.0, 150.0])],
        [{"bbox": [150.0, 80.0, 350.0, 120.0], "translated_text": "h", "item_id": "item-h"}],
        [
            _frag([150.0, 80.0, 192.0, 120.0], 0),
            _frag([308.0, 80.0, 350.0, 120.0], 1),
        ],
    ))

    # 8. Item horizontally inside the guard -> top/bottom-only split.
    cases.append(_run_case(
        "vertical_only_split",
        [_formula([250.0, 100.0, 300.0, 120.0])],
        [{"bbox": [242.0, 50.0, 308.0, 180.0], "translated_text": "v", "item_id": "item-v"}],
        [
            _frag([242.0, 50.0, 308.0, 88.0], 0),
            _frag([242.0, 132.0, 308.0, 180.0], 1),
        ],
    ))

    # 9. 0.5pt-wide left strip filtered by min_width (1.0).
    cases.append(_run_case(
        "min_width_fragment_filtered",
        [_formula([100.0, 100.0, 200.0, 120.0])],
        [{"bbox": [91.5, 50.0, 93.0, 300.0], "translated_text": "m", "item_id": "item-m"}],
        [
            _frag([91.5, 50.0, 93.0, 88.0], 0),
            _frag([91.5, 132.0, 93.0, 300.0], 1),
        ],
    ))

    # 10. Two formulas whose expanded guards are same-row with a 2pt gap merge
    #     into one guard, so no gap strip leaks between them.
    cases.append(_run_case(
        "guard_merge_two_formulas",
        [
            _formula([100.0, 100.0, 120.0, 120.0]),
            _formula([138.0, 100.0, 158.0, 120.0]),
        ],
        [{"bbox": [50.0, 50.0, 200.0, 200.0], "translated_text": "g", "item_id": "item-g"}],
        [
            _frag([50.0, 50.0, 200.0, 88.0], 0),
            _frag([50.0, 132.0, 200.0, 200.0], 1),
            _frag([50.0, 88.0, 92.0, 132.0], 2),
            _frag([166.0, 88.0, 200.0, 132.0], 3),
        ],
    ))

    # 11. Fractional coordinates exercise rect_list's 3-decimal rounding.
    cases.append(_run_case(
        "fractional_rounding",
        [_formula([100.1234, 100.5678, 200.9876, 120.4321])],
        [{"bbox": [50.0, 50.0, 250.0, 250.0], "translated_text": "r", "item_id": "item-r"}],
        [
            _frag([50.0, 50.0, 250.0, 88.568], 0),
            _frag([50.0, 132.432, 250.0, 250.0], 1),
            _frag([50.0, 88.568, 92.123, 132.432], 2),
            _frag([208.988, 88.568, 250.0, 132.432], 3),
        ],
    ))

    corpus = {"schema": "retainpdf_formula_guard_corpus_v1", "cases": cases}
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
