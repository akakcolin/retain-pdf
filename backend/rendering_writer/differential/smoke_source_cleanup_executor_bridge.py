#!/usr/bin/env python3
"""Native bridge smoke test for the C3-N10 bbox-text-strip executor wiring.

Production `strip_bbox_text_rects_from_pdf_copy` now dispatches to the native
bridge `strip_bbox_text_rects` (a mupdf-rs port of the per-page strip plus Form
recursion) when eligible, falling back to the pure-Python reference
`_strip_bbox_text_rects_from_pdf_copy_python`. The Rust serializer is
semantic-equivalent but not byte-exact vs qpdf, so parity is asserted at three
levels (mirroring the hidden-text-strip smoke):

1. outcome: native `pages_changed`/`text_show_ops_removed`/`forms_changed`
   equal the write-corpus oracle and the Python reference,
2. page facts: native output words/ink_ratio equal the corpus oracle,
3. content stream: native and reference changed-page streams tokenize to the
   same operator sequence and normalized operand values.

Also asserts the routing gate: `skip_form_xobject_pages=True` and a
`max_elapsed_seconds` deadline keep the call on the Python reference (the
native engine has no per-page form-skip detection or deadline budget), the
routing hit is recorded, and the fallback (`NATIVE=False`) still produces the
reference result.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_source_cleanup_executor_bridge.py
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
from services.rendering.source_cleanup.pdf import _native  # noqa: E402
from services.rendering.source_cleanup.pdf import document as cleanup_document  # noqa: E402

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


def strip_cases():
    corpus = json.load(open(CORPUS_PATH, encoding="utf-8"))
    return corpus["cases"]


def replay_case(case, tmp):
    src = Path(tmp) / f"{case['name']}-in.pdf"
    out = Path(tmp) / f"{case['name']}-out.pdf"
    out_ref = Path(tmp) / f"{case['name']}-ref.pdf"
    src.write_bytes(base64.b64decode(case["input_pdf_b64"]))
    page_rects = {int(k): [fitz.Rect(r) for r in v] for k, v in case["page_rects"].items()}

    hits = []

    def wrapped(pdf_bytes, rects_json, protected_json, recurse):
        hits.append(1)
        return rendering_bridge.strip_bbox_text_rects(
            pdf_bytes, rects_json, protected_json, recurse
        )

    saved = _native._native_strip_bbox_text_rects
    _native._native_strip_bbox_text_rects = wrapped
    try:
        result = cleanup_document.strip_bbox_text_rects_from_pdf_copy(
            source_pdf_path=src,
            output_pdf_path=out,
            page_rects=page_rects,
            recurse_forms=True,
        )
    finally:
        _native._native_strip_bbox_text_rects = saved

    expected = case["expected"]
    assert result.changed, f"{case['name']}: expected changed"
    assert result.pages_changed == expected["pages_changed"], (
        f"{case['name']}: pages_changed {result.pages_changed} != {expected['pages_changed']}"
    )
    assert result.text_show_ops_removed == expected["text_show_ops_removed"], (
        f"{case['name']}: text_show_ops_removed {result.text_show_ops_removed} "
        f"!= {expected['text_show_ops_removed']}"
    )
    assert result.forms_changed == expected["forms_changed"], (
        f"{case['name']}: forms_changed {result.forms_changed} != {expected['forms_changed']}"
    )
    assert set(result.changed_page_indices) == set(expected["changed_page_indices"]), (
        f"{case['name']}: changed_page_indices {result.changed_page_indices} "
        f"!= {expected['changed_page_indices']}"
    )
    assert result.pages_skipped_form_xobject == 0, (
        f"{case['name']}: unexpected skipped form pages"
    )
    assert result.pages_strip_no_effect == 0, (
        f"{case['name']}: unexpected strip-no-effect pages"
    )
    assert out.exists(), f"{case['name']}: output missing"
    assert len(hits) == 1, f"{case['name']}: native hit count {len(hits)} != 1"

    with fitz.open(out) as out_doc:
        native_facts = page_facts(out_doc)
    assert_facts_close(native_facts, expected["output"]["pages"], f"{case['name']} native vs oracle")

    reference = cleanup_document._strip_bbox_text_rects_from_pdf_copy_python(
        source_pdf_path=src,
        output_pdf_path=out_ref,
        page_rects=page_rects,
        recurse_forms=True,
    )
    assert reference.changed, f"{case['name']}: reference unchanged"
    assert reference.pages_changed == result.pages_changed, (
        f"{case['name']}: reference pages_changed diverges"
    )
    assert reference.text_show_ops_removed == result.text_show_ops_removed, (
        f"{case['name']}: reference text_show_ops_removed diverges"
    )
    assert reference.forms_changed == result.forms_changed, (
        f"{case['name']}: reference forms_changed diverges"
    )
    for page_idx in expected["changed_page_indices"]:
        assert_streams_token_equivalent(out, out_ref, page_idx)


def main() -> None:
    assert _native.NATIVE, "native module not built"
    assert hasattr(rendering_bridge, "strip_bbox_text_rects"), (
        "strip_bbox_text_rects not exported"
    )
    cases = strip_cases()
    assert len(cases) == 5, f"expected 5 strip corpus cases, got {len(cases)}"

    with tempfile.TemporaryDirectory() as tmp:
        for case in cases:
            replay_case(case, tmp)

        snap = _routing.snapshot()
        assert snap["total_fallbacks"] == 0, (
            f"routed strip recorded fallbacks {snap['fallbacks']}"
        )
        assert snap["hits"].get("source_cleanup", 0) >= len(cases), (
            f"source_cleanup hits {snap['hits'].get('source_cleanup')} < {len(cases)}"
        )

        # Gating: skip_form_xobject_pages / deadline stay on the Python reference.
        src = Path(tmp) / "gate-in.pdf"
        src.write_bytes(base64.b64decode(cases[0]["input_pdf_b64"]))
        page_rects = {int(k): [fitz.Rect(r) for r in v] for k, v in cases[0]["page_rects"].items()}
        hits = []

        def wrapped(pdf_bytes, rects_json, protected_json, recurse):
            hits.append(1)
            return rendering_bridge.strip_bbox_text_rects(
                pdf_bytes, rects_json, protected_json, recurse
            )

        saved = _native._native_strip_bbox_text_rects
        _native._native_strip_bbox_text_rects = wrapped
        try:
            skipped = cleanup_document.strip_bbox_text_rects_from_pdf_copy(
                source_pdf_path=src,
                output_pdf_path=Path(tmp) / "gate-skip.pdf",
                page_rects=page_rects,
                recurse_forms=True,
                skip_form_xobject_pages=True,
            )
            assert len(hits) == 0, (
                f"skip_form_xobject_pages routed to native ({len(hits)} hits)"
            )
            assert skipped.changed, "skip_form_xobject_pages fallback unchanged"

            deadline = cleanup_document.strip_bbox_text_rects_from_pdf_copy(
                source_pdf_path=src,
                output_pdf_path=Path(tmp) / "gate-deadline.pdf",
                page_rects=page_rects,
                recurse_forms=True,
                max_elapsed_seconds=5.0,
            )
            assert len(hits) == 0, f"deadline routed to native ({len(hits)} hits)"
            assert deadline.changed, "deadline fallback unchanged"
        finally:
            _native._native_strip_bbox_text_rects = saved

        # Fallback: NATIVE=False still produces the reference result.
        was = _native.NATIVE
        _native.NATIVE = False
        try:
            fallback = cleanup_document.strip_bbox_text_rects_from_pdf_copy(
                source_pdf_path=src,
                output_pdf_path=Path(tmp) / "fallback.pdf",
                page_rects=page_rects,
                recurse_forms=True,
            )
        finally:
            _native.NATIVE = was
        assert fallback.changed, "fallback unchanged"
        assert fallback.pages_changed == cases[0]["expected"]["pages_changed"], (
            "fallback pages_changed diverges"
        )

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
