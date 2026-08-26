#!/usr/bin/env python3
"""Native bridge smoke test (Phase 7R-5).

For every case in `rendering_writer/tests/stage_corpus.json`, run the shim
`_native.build_clean_background_pdf` (native path) and the pure-Python
`_build_clean_background_pdf_python` on the same source bytes, then assert the
two outputs are semantically equivalent per page (words ±10%, ink ±0.02) and
that both match the corpus's recorded `expected_output` facts. Also asserts the
shim routes native for every corpus case.

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


def main() -> None:
    assert _native.NATIVE, "native module not built"
    corpus = json.load(open(CORPUS_PATH))
    assert corpus["schema"] == "retainpdf_stage_corpus_v1"
    for case in corpus["cases"]:
        source_bytes = base64.b64decode(case["source_pdf_b64"])
        translated_pages = {int(k): v for k, v in case["translated_pages"].items()}
        strategy = case["redaction_strategy"]
        precleaned = case["precleaned_page_indices"]
        with tempfile.TemporaryDirectory(prefix="rps-smoke-") as tmp:
            src = Path(tmp) / "in.pdf"
            src.write_bytes(source_bytes)
            native_out = Path(tmp) / "native.pdf"
            py_out = Path(tmp) / "py.pdf"
            assert _native._native_eligible(None, None), (
                f"{case['name']}: shim should route native"
            )
            _native.build_clean_background_pdf(
                source_pdf_path=src,
                translated_pages=translated_pages,
                output_pdf_path=native_out,
                redaction_strategy=strategy,
                source_text_precleaned_page_indices=frozenset(precleaned),
            )
            _build_clean_background_pdf_python(
                source_pdf_path=src,
                translated_pages=translated_pages,
                output_pdf_path=py_out,
                redaction_strategy=strategy,
                source_text_precleaned_page_indices=frozenset(precleaned),
            )
            native_facts = page_facts(fitz.open(native_out))
            py_facts = page_facts(fitz.open(py_out))
            expected_facts = case["expected_output"]["pages"]
            assert_close(native_facts, py_facts, f"{case['name']}: native vs python")
            assert_close(native_facts, expected_facts, f"{case['name']}: native vs corpus")
            assert_close(py_facts, expected_facts, f"{case['name']}: python vs corpus")
        print(f"ok {case['name']}")
    print(f"all {len(corpus['cases'])} cases pass")


if __name__ == "__main__":
    main()
