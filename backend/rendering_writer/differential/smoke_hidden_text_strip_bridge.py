#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N9 hidden-text-strip wiring.

Production `build_hidden_text_stripped_pdf_copy` keeps the fitz candidate-page
pre-scan (`page_is_pseudo_editable_scan`) and delegates the per-page strip to
the native bridge `strip_hidden_text_pages`, which strips exactly those pages.
The Rust serializer is semantic-equivalent but not byte-exact vs qpdf (array
spacing differs), so parity is asserted at three levels:

1. outcome: native `pages_changed`/`text_objects_removed` equal the corpus
   oracle and the Python reference,
2. page facts: native output words/ink_ratio equal the corpus oracle and the
   reference output (the hidden text is gone, visible text remains),
3. content stream: native and reference changed-page streams tokenize to the
   same operator sequence and normalized operand values.

Also asserts candidate-page filtering (a 2-page doc: strip page 0 leaves page 1
untouched; stripping only page 1 is a no-op), the routing hit is recorded, and
the fallback path still produces valid output.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_hidden_text_strip_bridge.py
"""

import base64
import json
import os
import sys
import tempfile
from pathlib import Path

import fitz
import pikepdf

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering import _routing  # noqa: E402
from services.rendering.source import _native  # noqa: E402
from services.rendering.source.preparation.hidden_text_strip import (  # noqa: E402
    _build_hidden_text_stripped_pdf_copy_python,
    _collect_hidden_text_scan_pages,
    build_hidden_text_stripped_pdf_copy,
)

import rendering_bridge  # noqa: E402

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
    for idx, (got, want) in enumerate(zip(actual, expected)):
        if got["words"] == 0:
            assert want["words"] == 0, f"{label} page {idx}: words {got} vs {want}"
        else:
            assert abs(got["words"] - want["words"]) / max(want["words"], 1) <= WORD_TOL, (
                f"{label} page {idx}: words {got} vs {want}"
            )
        assert abs(got["ink_ratio"] - want["ink_ratio"]) <= INK_TOL, (
            f"{label} page {idx}: ink_ratio {got} vs {want}"
        )


def _norm_operand(value):
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, bytes):
        return value
    if isinstance(value, str):  # pikepdf.Name is a str subclass
        return str(value)
    if isinstance(value, pikepdf.Array):
        return [_norm_operand(item) for item in value]
    return repr(value)


def content_stream_tokens(pdf_path, page_idx):
    with pikepdf.Pdf.open(pdf_path) as pdf:
        contents = pdf.pages[page_idx].obj.get(pikepdf.Name("/Contents"))
        if contents is None:
            return []
        streams = contents if isinstance(contents, pikepdf.Array) else [contents]
        tokens = []
        for stream in streams:
            tokens.extend(pikepdf.parse_content_stream(stream))
        return tokens


def assert_streams_token_equivalent(native_path, reference_path, page_idx):
    native = content_stream_tokens(native_path, page_idx)
    reference = content_stream_tokens(reference_path, page_idx)
    assert len(native) == len(reference), (
        f"page {page_idx}: token count {len(native)} != {len(reference)}"
    )
    for idx, (a, b) in enumerate(zip(native, reference)):
        assert str(a[1]) == str(b[1]), (
            f"page {page_idx} token {idx}: operator {a[1]!r} != {b[1]!r}"
        )
        assert [_norm_operand(v) for v in a[0]] == [_norm_operand(v) for v in b[0]], (
            f"page {page_idx} token {idx} {a[1]!r}: operands diverge"
        )


def corpus_hidden_case():
    corpus = json.load(open(CORPUS_PATH, encoding="utf-8"))
    return next(c for c in corpus["prep_cases"] if c.get("name") == "hidden_text")


def two_page_fixture():
    """Page 0: a `page_is_pseudo_editable_scan` candidate (full-page gray image
    + visible + render-mode-3 hidden text). Page 1: plain text, no background
    image (never a candidate)."""
    doc = fitz.open()
    page = doc.new_page(width=300.0, height=200.0)
    pix = fitz.Pixmap(fitz.csGRAY, fitz.IRect(0, 0, 300, 200))
    pix.clear_with(200)
    page.insert_image(fitz.Rect(0, 0, 300, 200), pixmap=pix)
    page.insert_text((20.0, 40.0), "VISIBLE TEXT HERE", fontsize=12)
    page.insert_text((20.0, 70.0), "HIDDEN TEXT STRIP ME", fontsize=12, render_mode=3)
    page2 = doc.new_page(width=300.0, height=200.0)
    page2.insert_text((20.0, 40.0), "PLAIN PAGE", fontsize=12)
    raw = doc.tobytes()
    doc.close()
    return raw


def main() -> None:
    assert _native.NATIVE, "native module not built"
    assert hasattr(rendering_bridge, "strip_hidden_text_pages"), (
        "strip_hidden_text_pages not exported"
    )
    case = corpus_hidden_case()
    raw = base64.b64decode(case["input_pdf_b64"])
    expected_out = case["expected"]["output"]["pages"]

    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(raw)
        out = Path(tmp) / "out.pdf"
        out_ref = Path(tmp) / "out_ref.pdf"

        # Part 2: production wrapper (fitz pre-scan + native strip) replay.
        assert _collect_hidden_text_scan_pages(src) == {0}, "corpus pre-scan candidates"
        hits = []

        def wrapped(pdf_bytes, page_indices_json):
            hits.append(1)
            return rendering_bridge.strip_hidden_text_pages(pdf_bytes, page_indices_json)

        saved = _native._native_strip_hidden_text_pages
        _native._native_strip_hidden_text_pages = wrapped
        try:
            result = build_hidden_text_stripped_pdf_copy(src, out)
        finally:
            _native._native_strip_hidden_text_pages = saved
        assert result.changed, "corpus replay: expected changed"
        assert result.pages_changed == case["expected"]["pages_changed"], (
            f"corpus replay pages_changed {result.pages_changed}"
        )
        assert result.text_objects_removed == case["expected"]["text_objects_removed"], (
            f"corpus replay text_objects_removed {result.text_objects_removed}"
        )
        assert out.exists(), "corpus replay output missing"
        assert len(hits) == 1, f"native strip_hidden_text_pages hit count {len(hits)} != 1"
        snap = _routing.snapshot()
        assert snap["total_fallbacks"] == 0, (
            f"routed strip recorded fallbacks {snap['fallbacks']}"
        )
        assert snap["hits"].get("source", 0) >= 1, "pre-scan + strip recorded no source hits"
        with fitz.open(out) as out_doc:
            native_facts = page_facts(out_doc)
        assert_facts_close(native_facts, expected_out, "corpus native vs oracle")

        # Part 3: parity vs the pure-Python reference (same candidate set).
        reference = _build_hidden_text_stripped_pdf_copy_python(
            {0}, source_pdf_path=src, output_pdf_path=out_ref
        )
        assert reference.changed and reference.pages_changed == result.pages_changed, (
            "reference counts diverge"
        )
        assert reference.text_objects_removed == result.text_objects_removed, (
            "reference text_objects_removed diverges"
        )
        with fitz.open(out_ref) as ref_doc:
            reference_facts = page_facts(ref_doc)
        assert_facts_close(native_facts, reference_facts, "corpus native vs reference")
        assert_streams_token_equivalent(out, out_ref, 0)

        # Part 4: candidate-page filtering on a 2-page fixture.
        two_raw = two_page_fixture()
        two_src = Path(tmp) / "two.pdf"
        two_src.write_bytes(two_raw)
        assert _collect_hidden_text_scan_pages(two_src) == {0}, (
            "two-page fixture pre-scan candidates"
        )
        only_page1, meta1 = rendering_bridge.strip_hidden_text_pages(two_raw, "[1]")
        meta1 = json.loads(meta1)
        assert not meta1["changed"] and meta1["pages_changed"] == 0, (
            f"stripping only page 1 must be a no-op: {meta1}"
        )
        filtered, meta0 = rendering_bridge.strip_hidden_text_pages(two_raw, "[0]")
        meta0 = json.loads(meta0)
        assert meta0["changed"] and meta0["pages_changed"] == 1, (
            f"stripping page 0 must change exactly page 0: {meta0}"
        )
        two_out = Path(tmp) / "two_out.pdf"
        two_out.write_bytes(filtered)
        # Page 1 (excluded from the strip) keeps its original content tokens.
        original_tokens = content_stream_tokens(two_src, 1)
        assert original_tokens, "two-page fixture page 1 stream missing"
        assert_streams_token_equivalent(two_out, two_src, 1)
        with fitz.open(two_out) as stripped_doc:
            page0_words = len(stripped_doc[0].get_text("words"))
        assert page0_words == 3, f"page 0 hidden text not stripped: words={page0_words}"

        # Part 5: fallback path still works and matches the reference.
        was = _native.NATIVE
        _native.NATIVE = False
        try:
            fallback = build_hidden_text_stripped_pdf_copy(src, Path(tmp) / "fb.pdf")
        finally:
            _native.NATIVE = was
        assert fallback.changed and fallback.pages_changed == 1, "fallback diverged"
        assert fallback.text_objects_removed == 1, "fallback text_objects_removed diverged"

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
