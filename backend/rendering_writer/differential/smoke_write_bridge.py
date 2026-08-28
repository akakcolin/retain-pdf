#!/usr/bin/env python3
"""Native bridge smoke test for the write-path primitives (Phase 7R A2).

Replays `rendering_writer/tests/write_corpus.json` through the routed shim
(`services.rendering.source._native`) and the pure-Python references on the
same inputs, then asserts the outputs are semantically equivalent:

  * sanitize (`prep_cases` name "sanitize") — `sanitize_pdf_copy` vs
    `_build_invalid_xobject_sanitized_pdf_copy_python`: result counts match
    plus per-page words/ink agree.
  * compress (`image_cases`) — `compress_images_only` vs
    `_compress_pdf_images_only_impl_python`: both commit the same decision and
    the recompressed image shrinks to the same display target (dimensions +
    DCTDecode filter). Encoded bytes differ by design (Rust `image` crate JPEG
    vs PIL `optimize=True`); this is a documented, safe-direction divergence.
  * extract (`subset_cases`) — `extract_pages` vs
    `_extract_pages_with_pikepdf_python`: output page count + per-page facts.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_write_bridge.py
"""

import base64
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.source import _native  # noqa: E402
from services.rendering.source.compression.image_pipeline import (  # noqa: E402
    _compress_pdf_images_only_impl_python,
)
from services.rendering.document.pikepdf_pages import (  # noqa: E402
    _extract_pages_with_pikepdf_python,
)
from services.rendering.source.preparation.xobject_sanitize import (  # noqa: E402
    _build_invalid_xobject_sanitized_pdf_copy_python,
)

CORPUS_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "write_corpus.json"))
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


def assert_facts_close(actual, expected, label):
    assert len(actual) == len(expected), f"{label}: page count {len(actual)} != {len(expected)}"
    for i, (a, e) in enumerate(zip(actual, expected)):
        rel = abs(a["words"] - e["words"]) / max(e["words"], 1)
        assert rel <= WORD_TOL, f"{label} p{i}: words {a['words']} vs {e['words']}"
        ink = abs(a["ink_ratio"] - e["ink_ratio"])
        assert ink <= INK_TOL, f"{label} p{i}: ink {a['ink_ratio']:.6f} vs {e['ink_ratio']:.6f}"


def write_input(tmp: Path, name: str, b64: str) -> Path:
    path = tmp / name
    path.write_bytes(base64.b64decode(b64))
    return path


def replay_sanitize(case, tmp: Path):
    src = write_input(tmp, "sanitize-in.pdf", case["input_pdf_b64"])
    native_out = tmp / "sanitize-native.pdf"
    py_out = tmp / "sanitize-py.pdf"

    native = _native.sanitize_pdf_copy(source_pdf_path=src, output_pdf_path=native_out)
    python = _build_invalid_xobject_sanitized_pdf_copy_python(
        source_pdf_path=src, output_pdf_path=py_out
    )
    assert native.invalid_image_xobjects == python.invalid_image_xobjects, (
        f"{case['name']}: invalid_image_xobjects native={native.invalid_image_xobjects} "
        f"python={python.invalid_image_xobjects}"
    )
    assert native.pages_changed == python.pages_changed, (
        f"{case['name']}: pages_changed native={native.pages_changed} python={python.pages_changed}"
    )
    assert native.invalid_image_xobjects == case["expected"]["invalid_image_xobjects"], (
        f"{case['name']}: invalid_image_xobjects vs corpus"
    )
    assert native.pages_changed == case["expected"]["pages_changed"], (
        f"{case['name']}: pages_changed vs corpus"
    )

    native_facts = page_facts(fitz.open(native_out)) if native_out.exists() else None
    py_facts = page_facts(fitz.open(py_out)) if py_out.exists() else None
    assert (native_out.exists()) == (py_out.exists()), f"{case['name']}: changed mismatch"
    if native_out.exists():
        assert_facts_close(native_facts, py_facts, f"{case['name']}: native vs python")
        assert_facts_close(native_facts, case["expected"]["output"]["pages"], f"{case['name']}: native vs corpus")


def replay_compress(case, tmp: Path):
    src = write_input(tmp, "compress-in.pdf", case["input_pdf_b64"])
    native_path = tmp / "compress-native.pdf"
    py_path = tmp / "compress-py.pdf"
    shutil.copy2(src, native_path)
    shutil.copy2(src, py_path)

    native_changed = _native.compress_images_only(native_path, dpi=case["dpi"])
    py_changed = _compress_pdf_images_only_impl_python(py_path, dpi=case["dpi"])
    assert native_changed == py_changed, (
        f"{case['name']}: changed native={native_changed} python={py_changed}"
    )
    assert native_changed == case["expected"]["changed"], f"{case['name']}: changed vs corpus"

    if native_changed:
        assert_facts_close(
            page_facts(fitz.open(native_path)),
            page_facts(fitz.open(py_path)),
            f"{case['name']}: native vs python",
        )
        assert_facts_close(
            page_facts(fitz.open(native_path)),
            case["expected"]["output"]["pages"],
            f"{case['name']}: native vs corpus",
        )
        # Recompressed image matches the display target (dims + filter).
        native_img = fitz.open(native_path)[0].get_images(full=True)[0]
        py_img = fitz.open(py_path)[0].get_images(full=True)[0]
        for label, img, expected in (
            ("native", native_img, case["expected"]["output_image"]),
            ("python", py_img, case["expected"]["output_image"]),
        ):
            xref = img[0]
            pix = fitz.Pixmap(fitz.open(py_path if label == "python" else native_path), xref)
            assert pix.width == expected["width"], f"{case['name']}: {label} width"
            assert pix.height == expected["height"], f"{case['name']}: {label} height"
        # get_images(full=True) tuple: index 8 is the /Filter list.
        assert "DCTDecode" in str(native_img[8]), f"{case['name']}: native filter not DCTDecode"
        assert len(native_path.read_bytes()) < len(src.read_bytes()), (
            f"{case['name']}: native output not smaller than input"
        )


def replay_extract(case, tmp: Path):
    src = write_input(tmp, "extract-in.pdf", case["input_pdf_b64"])
    native_out = tmp / "extract-native.pdf"
    py_out = tmp / "extract-py.pdf"

    _native.extract_pages(
        source_pdf_path=src,
        output_pdf_path=native_out,
        start_page=case["start_page"],
        end_page=case["end_page"],
    )
    _extract_pages_with_pikepdf_python(
        source_pdf_path=src,
        output_pdf_path=py_out,
        start_page=case["start_page"],
        end_page=case["end_page"],
    )
    native_doc = fitz.open(native_out)
    py_doc = fitz.open(py_out)
    assert native_doc.page_count == py_doc.page_count, (
        f"{case['name']}: page count native={native_doc.page_count} python={py_doc.page_count}"
    )
    assert native_doc.page_count == case["expected"]["output_page_count"], (
        f"{case['name']}: page count vs corpus"
    )
    assert_facts_close(
        page_facts(native_doc), page_facts(py_doc), f"{case['name']}: native vs python"
    )
    assert_facts_close(
        page_facts(native_doc), case["expected"]["output"]["pages"], f"{case['name']}: native vs corpus"
    )


def main() -> None:
    assert _native.NATIVE, "native module not built"

    corpus = json.load(open(CORPUS_PATH))
    assert corpus["schema"] == "retainpdf_write_corpus_v1"

    sanitized = [c for c in corpus["prep_cases"] if c["name"] == "sanitize"]
    assert len(sanitized) == 1, f"expected one sanitize prep case, got {len(sanitized)}"
    compressed = corpus["image_cases"]
    assert len(compressed) >= 1, "expected at least one image case"
    extracted = corpus["subset_cases"]
    assert len(extracted) >= 1, "expected at least one subset case"

    with tempfile.TemporaryDirectory(prefix="rps-write-smoke-") as tmp:
        tmp = Path(tmp)
        for case in sanitized:
            replay_sanitize(case, tmp)
            print(f"ok sanitize/{case['name']}")
        for case in compressed:
            replay_compress(case, tmp)
            print(f"ok compress/{case['name']}")
        for case in extracted:
            replay_extract(case, tmp)
            print(f"ok extract/{case['name']}")
    print("all write-path smoke tests pass")


if __name__ == "__main__":
    main()
