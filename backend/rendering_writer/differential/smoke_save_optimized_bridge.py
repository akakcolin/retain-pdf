#!/usr/bin/env python3
"""Native bridge smoke test for the final-save subset + byte compaction.

Replays `rendering_writer/tests/write_corpus.json` `save_cases` through the
PRODUCTION `document.pdf_ops.save_optimized_pdf` (`doc.tobytes()` then native
`subset_and_clean` — mupdf `pdf_subset_fonts` + garbage=4/stream compression in
one pass) and the pure-fitz reference `_save_optimized_pdf_python` on the same
inputs, then asserts:

  * native is actually hit (the bridge
    `_native_subset_and_save_optimized_pdf` is called exactly once per save),
  * native output page facts equal the fitz reference facts and the corpus
    oracle,
  * native output stays within `SIZE_K` of the fitz reference size (no
    compaction regression), and
  * the fallback path still works: when the native bridge raises, production
    falls back to the pure-fitz save and the output remains valid.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_save_optimized_bridge.py
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

from services.rendering.source import _native  # noqa: E402
from services.rendering.document.pdf_ops import (  # noqa: E402
    _save_optimized_pdf_python,
    save_optimized_pdf,
)

CORPUS_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "write_corpus.json"))
WORD_TOL = 0.10
INK_TOL = 0.02
SIZE_K = 1.15


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


def assert_facts_close(actual, expected, label):
    assert len(actual) == len(expected), f"{label}: page count {len(actual)} != {len(expected)}"
    for i, (a, e) in enumerate(zip(actual, expected)):
        rel = abs(a["words"] - e["words"]) / max(e["words"], 1)
        assert rel <= WORD_TOL, f"{label} p{i}: words {a['words']} vs {e['words']}"
        ink = abs(a["ink_ratio"] - e["ink_ratio"])
        assert ink <= INK_TOL, f"{label} p{i}: ink {a['ink_ratio']:.6f} vs {e['ink_ratio']:.6f}"


def replay_save(case, tmp: Path):
    input_bytes = base64.b64decode(case["input_pdf_b64"])
    native_out = tmp / f"save-{case['name']}-native.pdf"
    py_out = tmp / f"save-{case['name']}-py.pdf"
    fallback_out = tmp / f"save-{case['name']}-fallback.pdf"

    # Production path: tobytes -> native subset + compaction. Count hits.
    hits = []
    original = _native._native_subset_and_save_optimized_pdf

    def counting(pdf_bytes):
        hits.append(1)
        return original(pdf_bytes)

    _native._native_subset_and_save_optimized_pdf = counting
    doc = fitz.open(stream=input_bytes, filetype="pdf")
    try:
        save_optimized_pdf(doc, native_out)
    finally:
        doc.close()
        _native._native_subset_and_save_optimized_pdf = original
    assert len(hits) == 1, f"{case['name']}: native subset+save hit count {len(hits)} != 1"

    # Pure-fitz reference.
    ref_doc = fitz.open(stream=input_bytes, filetype="pdf")
    _save_optimized_pdf_python(ref_doc, py_out)
    ref_doc.close()

    native_facts = page_facts(fitz.open(native_out))
    py_facts = page_facts(fitz.open(py_out))
    assert_facts_close(native_facts, py_facts, f"{case['name']}: native vs python")
    assert_facts_close(
        native_facts, case["expected"]["output"]["pages"], f"{case['name']}: native vs corpus"
    )
    ratio = native_out.stat().st_size / py_out.stat().st_size
    assert ratio <= SIZE_K, (
        f"{case['name']}: native/fitz size {native_out.stat().st_size}/"
        f"{py_out.stat().st_size} = {ratio:.3f} > k={SIZE_K}"
    )

    # Fallback: force the native call to raise; production must still produce a
    # valid PDF via the pure-fitz save.
    fallback_doc = fitz.open(stream=input_bytes, filetype="pdf")
    original = _native._native_subset_and_save_optimized_pdf

    def raising(pdf_bytes):
        raise RuntimeError("forced native failure")

    _native._native_subset_and_save_optimized_pdf = raising
    try:
        save_optimized_pdf(fallback_doc, fallback_out)
    finally:
        _native._native_subset_and_save_optimized_pdf = original
        fallback_doc.close()
    assert fallback_out.exists() and fallback_out.read_bytes().startswith(b"%PDF-"), (
        f"{case['name']}: fallback save did not produce a valid PDF"
    )

    print(f"ok save/{case['name']} native_hit=1 ratio={ratio:.3f} fallback=ok")


def main() -> None:
    assert _native.NATIVE, "native module not built"

    corpus = json.load(open(CORPUS_PATH))
    assert corpus["schema"] == "retainpdf_write_corpus_v1"
    save_cases = corpus.get("save_cases", [])
    assert len(save_cases) >= 1, "expected at least one save case"

    with tempfile.TemporaryDirectory(prefix="rps-save-smoke-") as tmp:
        for case in save_cases:
            replay_save(case, Path(tmp))
    print("all save_optimized bridge smoke tests pass")


if __name__ == "__main__":
    main()
