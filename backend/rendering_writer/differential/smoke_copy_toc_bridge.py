#!/usr/bin/env python3
"""Native bridge smoke test for `copy_toc` / `copy_toc_for_page_map`.

Exercises the PRODUCTION routing shims in
`services.rendering.document.metadata` (which route through
`document._native`):

  * raw `rendering_bridge.copy_toc` output reproduces the pure-fitz reference
    `get_toc()` (count, titles, pages) across full / range / single-page and
    page-map cases, and returns count 0 with the target untouched for a
    no-outline source,
  * the production shim records one native hit and returns a swapped fresh
    document on the native path,
  * the fallback path (``NATIVE`` forced off) mutates the caller's document in
    place, returns the same object, and records a fallback.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_copy_toc_bridge.py
"""

import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import rendering_bridge  # noqa: E402

from services.rendering import _routing  # noqa: E402
from services.rendering.document import _native as _doc_native  # noqa: E402
from services.rendering.document.metadata import (  # noqa: E402
    _copy_toc_for_page_map_python,
    _copy_toc_python,
    copy_toc,
    copy_toc_for_page_map,
)
from services.rendering.document.page_map import RenderPageMap  # noqa: E402


def make_source():
    doc = fitz.open()
    for _i in range(4):
        doc.new_page(width=200, height=300)
    doc.set_toc(
        [
            [1, "Chapter 1", 1],
            [2, "Section 1.1", 2],
            [1, "Chapter 2", 3],
            [2, "Section 2.1", 4],
        ]
    )
    return doc


def make_target(pages):
    doc = fitz.open()
    for _i in range(pages):
        doc.new_page(width=200, height=300)
    return doc


def ref_copy_toc(source_bytes, target_bytes, start_page, end_page):
    src = fitz.open(stream=source_bytes, filetype="pdf")
    tgt = fitz.open(stream=target_bytes, filetype="pdf")
    _copy_toc_python(src, tgt, start_page=start_page, end_page=end_page)
    toc = tgt.get_toc()
    tgt.close()
    src.close()
    return toc


def ref_copy_toc_for_page_map(source_bytes, target_bytes, indices):
    src = fitz.open(stream=source_bytes, filetype="pdf")
    tgt = fitz.open(stream=target_bytes, filetype="pdf")
    _copy_toc_for_page_map_python(
        src, tgt, page_map=RenderPageMap(source_page_indices=indices)
    )
    toc = tgt.get_toc()
    tgt.close()
    src.close()
    return toc


def assert_toc_parity(label, got_toc, expected_toc):
    assert got_toc == expected_toc, f"{label}: native {got_toc} != fitz {expected_toc}"
    print(f"  ok {label} (entries={len(got_toc)})")


def main() -> None:
    assert _doc_native.NATIVE, "native module not built"
    _routing.reset()

    src = make_source()
    src_bytes = src.tobytes()
    src.close()

    print("raw bridge parity:")
    for label, start, end in [
        ("full-doc", 0, -1),
        ("range 1..3", 1, 3),
        ("single page 2", 2, 2),
    ]:
        tgt = make_target(4)
        tgt_bytes = tgt.tobytes()
        tgt.close()
        out_bytes, count = rendering_bridge.copy_toc(src_bytes, tgt_bytes, start, end)
        native_toc = fitz.open(stream=out_bytes, filetype="pdf").get_toc()
        ref_toc = ref_copy_toc(src_bytes, tgt_bytes, start, end)
        assert count == len(ref_toc), f"{label}: count {count} != {len(ref_toc)}"
        assert_toc_parity(label, native_toc, ref_toc)

    no_toc = make_target(4)
    no_toc_bytes = no_toc.tobytes()
    no_toc.close()
    out_bytes, count = rendering_bridge.copy_toc(no_toc_bytes, no_toc_bytes, 0, -1)
    assert count == 0, f"empty-TOC source must yield count 0, got {count}"
    print("  ok empty-TOC source (count 0)")

    tgt = make_target(2)
    tgt_bytes = tgt.tobytes()
    tgt.close()
    out_bytes, count = rendering_bridge.copy_toc_for_page_map(src_bytes, tgt_bytes, [0, 2])
    native_toc = fitz.open(stream=out_bytes, filetype="pdf").get_toc()
    ref_toc = ref_copy_toc_for_page_map(src_bytes, tgt_bytes, [0, 2])
    assert count == len(ref_toc), f"page-map: count {count} != {len(ref_toc)}"
    assert_toc_parity("page-map [0,2]", native_toc, ref_toc)

    print("production shim routing:")
    _routing.reset()
    src_doc = make_source()
    tgt_doc = make_target(4)
    replaced = copy_toc(src_doc, tgt_doc)
    snap = _routing.snapshot()
    assert snap["hits"].get("source", 0) == 1, f"expected 1 copy_toc native hit, got {snap}"
    assert replaced is not tgt_doc, "native copy_toc must return a fresh doc"
    assert_toc_parity(
        "shim copy_toc",
        replaced.get_toc(),
        src_doc.get_toc(),
    )
    tgt_doc.close()
    replaced.close()
    src_doc.close()
    print("  ok copy_toc native hit=1 + doc swap")

    _routing.reset()
    src_doc = make_source()
    tgt_doc = make_target(2)
    replaced = copy_toc_for_page_map(
        src_doc, tgt_doc, page_map=RenderPageMap(source_page_indices=[0, 2])
    )
    snap = _routing.snapshot()
    assert snap["hits"].get("source", 0) == 1, (
        f"expected 1 copy_toc_for_page_map native hit, got {snap}"
    )
    assert replaced is not tgt_doc, "native copy_toc_for_page_map must return a fresh doc"
    assert_toc_parity(
        "shim copy_toc_for_page_map",
        replaced.get_toc(),
        [[1, "Chapter 1", 1], [1, "Chapter 2", 2]],
    )
    tgt_doc.close()
    replaced.close()
    src_doc.close()
    print("  ok copy_toc_for_page_map native hit=1 + doc swap")

    print("fallback path:")
    _routing.reset()
    saved = _doc_native.NATIVE
    _doc_native.NATIVE = False
    try:
        src_doc = make_source()
        tgt_doc = make_target(4)
        replaced = copy_toc(src_doc, tgt_doc)
        assert replaced is tgt_doc, "fallback copy_toc must return the same doc"
        assert_toc_parity("fallback copy_toc", tgt_doc.get_toc(), src_doc.get_toc())
        snap = _routing.snapshot()
        assert snap["fallbacks_by_reason"].get("native_not_built", 0) >= 1, (
            f"expected fallback, got {snap}"
        )
        tgt_doc.close()
        src_doc.close()
        print("  ok copy_toc fallback same-doc + in-place")

        src_doc = make_source()
        tgt_doc = make_target(2)
        replaced = copy_toc_for_page_map(
            src_doc, tgt_doc, page_map=RenderPageMap(source_page_indices=[0, 2])
        )
        assert replaced is tgt_doc, "fallback copy_toc_for_page_map must return the same doc"
        assert_toc_parity(
            "fallback copy_toc_for_page_map",
            tgt_doc.get_toc(),
            [[1, "Chapter 1", 1], [1, "Chapter 2", 2]],
        )
        tgt_doc.close()
        src_doc.close()
        print("  ok copy_toc_for_page_map fallback same-doc + in-place")
    finally:
        _doc_native.NATIVE = saved

    print("all copy_toc bridge smoke tests pass")


if __name__ == "__main__":
    main()
