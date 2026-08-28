#!/usr/bin/env python3
"""Native bridge smoke test (Phase 7R-5, extended in 7R-7/7R-8).

Two parts:

1. Corpus replay: for every case in
   `rendering_writer/tests/stage_corpus.json`, reconstruct the ORIGINAL
   `translated_pages` plus the `page_specs` / `visual_profile_fill_map` the
   7R-8 bridge consumes, run the shim `_native.build_clean_background_pdf`
   (native path) and the pure-Python `_build_clean_background_pdf_python` on
   the same inputs, then assert the outputs are semantically equivalent per
   page (words ±10%, ink ±0.02) and that both match the corpus's recorded
   `expected_output` facts.

2. Dedicated synthetic smoke: build the real book_renderer scenario (original
   items + page_specs + loaded/not-loaded `VisualProfileRuntime`, strategy=None
   with a formula item flipping page 0 to visual_cover) and assert the shim
   (which sends originals + specs + fill map and lets Rust do the replacement)
   and the pure-Python stage produce equivalent output. This is the path 7R-8
   makes native-served in production.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_bridge.py
"""

import base64
import json
import os
import sys
import tempfile
from pathlib import Path

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)
sys.path.insert(0, _HERE)

import gen_stage_corpus as gen  # noqa: E402

from services.rendering import _routing  # noqa: E402
from services.rendering.source.background import _native  # noqa: E402
from services.rendering.source.background.stage import _build_clean_background_pdf_python  # noqa: E402

CORPUS_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "stage_corpus.json"))
WORD_TOL = 0.10
INK_TOL = 0.02


def page_facts(doc):
    facts = []
    for i in range(doc.page_count):
        page = doc[i]
        words = len(page.get_text("words"))
        pix = page.get_pixmap(
            matrix=fitz.Matrix(2.0, 2.0), colorspace=fitz.csGRAY, alpha=False
        )
        total = pix.width * pix.height
        dark = sum(1 for b in pix.samples if b < 250)
        facts.append({"words": words, "ink_ratio": (dark / total) if total else 0.0})
    return facts


def assert_close(actual, expected, label):
    assert len(actual) == len(expected), f"{label}: page count {len(actual)} != {len(expected)}"
    for i, (a, e) in enumerate(zip(actual, expected)):
        rel = abs(a["words"] - e["words"]) / max(e["words"], 1)
        assert rel <= WORD_TOL, f"{label} p{i}: words {a['words']} vs {e['words']}"
        ink = abs(a["ink_ratio"] - e["ink_ratio"])
        assert ink <= INK_TOL, f"{label} p{i}: ink {a['ink_ratio']:.6f} vs {e['ink_ratio']:.6f}"


def _run_shim_and_python(source_bytes, translated_pages, strategy, precleaned, tmp, *, page_specs=None, visual_profile=None):
    src = Path(tmp) / "in.pdf"
    src.write_bytes(source_bytes)
    native_out = Path(tmp) / "native.pdf"
    py_out = Path(tmp) / "py.pdf"
    _native.build_clean_background_pdf(
        source_pdf_path=src,
        translated_pages=translated_pages,
        output_pdf_path=native_out,
        redaction_strategy=strategy,
        page_specs=page_specs,
        source_text_precleaned_page_indices=frozenset(precleaned),
        visual_profile=visual_profile,
    )
    _build_clean_background_pdf_python(
        source_pdf_path=src,
        translated_pages=translated_pages,
        output_pdf_path=py_out,
        redaction_strategy=strategy,
        page_specs=page_specs,
        source_text_precleaned_page_indices=frozenset(precleaned),
        visual_profile=visual_profile,
    )
    return page_facts(fitz.open(native_out)), page_facts(fitz.open(py_out))


def replay_case(case):
    """Replay a corpus case through the shim vs the pure-Python stage using the
    ORIGINAL `translated_pages` plus the reconstructed `page_specs` and
    `visual_profile_fill_map` (7R-8: the shim sends originals + specs + fill map
    to the native stage). Both must match the corpus's recorded output facts."""
    source_bytes = base64.b64decode(case["source_pdf_b64"])
    translated_pages = {int(k): v for k, v in case["translated_pages"].items()}
    strategy = case["redaction_strategy"]
    precleaned = case["precleaned_page_indices"]
    page_specs = [gen.page_spec_from_dict(d) for d in case.get("page_specs", [])]
    visual_profile = gen.visual_profile_from_fill_map(
        case.get("visual_profile_fill_map", {})
    )
    with tempfile.TemporaryDirectory(prefix="rps-smoke-") as tmp:
        native_facts, py_facts = _run_shim_and_python(
            source_bytes,
            translated_pages,
            strategy,
            precleaned,
            tmp,
            page_specs=page_specs,
            visual_profile=visual_profile,
        )
        expected_facts = case["expected_output"]["pages"]
        assert_close(native_facts, py_facts, f"{case['name']}: native vs python")
        assert_close(native_facts, expected_facts, f"{case['name']}: native vs corpus")
        assert_close(py_facts, expected_facts, f"{case['name']}: python vs corpus")
    print(f"ok {case['name']}")


def synthetic_book_renderer_scenario(name, *, loaded_profile):
    """Replay the real production call: original items + page_specs + a
    VisualProfileRuntime, strategy=None with a formula item flipping page 0 to
    visual_cover. The shim precomputes the replacement + per-item profile fill;
    the pure-Python stage computes them inline. Both must agree."""
    pages = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    toc = [[1, "Intro", 1], [1, "Chapter Two", 2]]
    source_bytes = gen.build_source_pdf(pages, toc)
    d = fitz.open(stream=source_bytes, filetype="pdf")
    formula_item = {
        "bbox": [250.0, 300.0, 350.0, 320.0],
        "translated_text": "",
        "block_type": "formula",
        "normalized_sub_type": "display_formula",
    }
    translated_pages = {
        0: gen.span_items_for(d.load_page(0), pages[0]) + [dict(formula_item)],
        1: gen.span_items_for(d.load_page(1), pages[1]),
    }
    d.close()
    specs = [gen.make_page_spec(0, [gen.make_block("block-0", [50.0, 85.0, 320.0, 200.0], "Alpha Beta")])]
    visual_profile = (
        gen.make_visual_profile({"block-0": (0.82, 0.84, 0.86)})
        if loaded_profile
        else gen.make_not_loaded_visual_profile()
    )
    with tempfile.TemporaryDirectory(prefix="rps-smoke-synth-") as tmp:
        native_facts, py_facts = _run_shim_and_python(
            source_bytes, translated_pages, None, [], tmp, page_specs=specs, visual_profile=visual_profile
        )
    assert_close(native_facts, py_facts, f"{name}: native vs python")
    print(f"ok {name}")


def main() -> None:
    assert _native.NATIVE, "native module not built"
    eligible, reason = _native._native_eligible(None)
    assert eligible and reason is None, "auto/visual_cover should route native"
    eligible, reason = _native._native_eligible("text_layer_only")
    assert eligible and reason is None, "text_layer_only should route native"
    eligible, reason = _native._native_eligible("text_redaction")
    assert eligible and reason is None, "text_redaction should route native"

    corpus = json.load(open(CORPUS_PATH))
    assert corpus["schema"] == "retainpdf_stage_corpus_v1"
    for case in corpus["cases"]:
        # 7R-8 records page_specs + fill map separately, so every case
        # (including the formula-flip j/k) reconstructs and replays directly.
        replay_case(case)
    print(f"corpus replay: {len(corpus['cases'])} replayed")

    synthetic_book_renderer_scenario("synthetic_profile_loaded", loaded_profile=True)
    synthetic_book_renderer_scenario("synthetic_profile_not_loaded", loaded_profile=False)
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
