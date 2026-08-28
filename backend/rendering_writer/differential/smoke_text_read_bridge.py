#!/usr/bin/env python3
"""Native bridge smoke test for the cleanup text-read family (Phase B2-9 / B-C).

Four parts:

1. Three-way: every `tests/text_read_corpus.json` case, run on file-backed
   pages — `extract_page_text_spans` / `extract_page_text_blocks` /
   `collect_page_math_protection_rects` / `collect_page_non_math_span_heights`
   with NATIVE == with NATIVE=False == the corpus records (positionally: rects
   within 0.01 pt, heights within 0.05 pt, text exact; includes the golden
   1.pdf/2.pdf pages). A call counter on the four native bridges confirms native
   was actually hit on at least one page.

2. Boundary: corrupt bytes make each bridge raise (the shims' try/except needs
   that to fall back to the reference); an in-memory page
   (`page.parent.name == ""`) falls back to the reference == fitz; a file-backed
   page whose backing file vanished also falls back.

3. End-to-end: `item_removable_text_rects` (span -> block -> word text-read
   chain) and `apply_auto_redaction` (math-rect + span-height chain) run
   identically with NATIVE on and off on file-backed synthetic pages.

4. B-C consumers: `text_intrusion.collect_page_intrusive_display_text_rects`
   and `margin_text_cleanup._candidate_margin_block_rects` now read through the
   native spans/blocks primitives — on file-backed corpus pages the read path
   hits the bridges with zero `page.get_text` calls, and native agrees with the
   NATIVE=False fallback.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_text_read_bridge.py
"""

import base64
import json
import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402

from services.rendering.source import _native  # noqa: E402
from services.rendering.source.cleanup import auto  # noqa: E402
from services.rendering.source.cleanup import margin_text_cleanup  # noqa: E402
from services.rendering.source.cleanup import math_spans  # noqa: E402
from services.rendering.source.cleanup import text_extract  # noqa: E402
from services.rendering.source.cleanup import text_intrusion  # noqa: E402
from services.rendering.source.cleanup.text_matching import item_removable_text_rects  # noqa: E402

# Corpus lives next to its Rust replay (`rendering_reader/tests/text_read_diff.rs`).
CORPUS = os.path.abspath(
    os.path.join(_HERE, "..", "..", "rendering_reader", "tests", "text_read_corpus.json")
)

TOL = 0.01
TOL_HEIGHT = 0.05

_BRIDGES = (
    "_native_read_page_text_spans",
    "_native_read_page_text_blocks",
    "_native_read_page_math_rects",
    "_native_read_page_span_heights",
)


def _entries(raw: list) -> list[tuple[fitz.Rect, str]]:
    """Corpus `[[x0,y0,x1,y1,text], ...]` -> `(Rect, text)` pairs."""
    return [(fitz.Rect(item[:4]), str(item[4])) for item in raw]


def _close(a: float, b: float, tol: float) -> bool:
    return abs(a - b) <= tol


def _rect_close(a: fitz.Rect, b: fitz.Rect, tol: float = TOL) -> bool:
    return all(_close(va, vb, tol) for va, vb in zip(a, b))


def _entries_close(a, b) -> bool:
    if len(a) != len(b):
        return False
    return all(ta == tb and _rect_close(ra, rb) for (ra, ta), (rb, tb) in zip(a, b))


def _rects_close(a, b) -> bool:
    if len(a) != len(b):
        return False
    return all(_rect_close(ra, rb) for ra, rb in zip(a, b))


def _heights_close(a, b) -> bool:
    if len(a) != len(b):
        return False
    return all(_close(x, y, TOL_HEIGHT) for x, y in zip(a, b))


def _run_all(page: fitz.Page) -> tuple:
    return (
        text_extract.extract_page_text_spans(page),
        text_extract.extract_page_text_blocks(page),
        math_spans.collect_page_math_protection_rects(page),
        math_spans.collect_page_non_math_span_heights(page),
    )


def _assert_all_equal(native: tuple, ref: tuple, label: str) -> None:
    assert _entries_close(native[0], ref[0]), f"{label}: spans"
    assert _entries_close(native[1], ref[1]), f"{label}: blocks"
    assert _rects_close(native[2], ref[2]), f"{label}: math"
    assert _heights_close(native[3], ref[3]), f"{label}: heights"


def _install_counters(calls: dict) -> dict:
    """Wrap the four native text-read bridges, counting hits into `calls`."""
    saved = {}
    for name in _BRIDGES:
        orig = getattr(_native, name)
        saved[name] = orig

        def counting(*args, _orig=orig, **kwargs):
            calls["n"] += 1
            return _orig(*args, **kwargs)

        setattr(_native, name, counting)
    return saved


def _restore_counters(saved: dict) -> None:
    for name, orig in saved.items():
        setattr(_native, name, orig)


def _install_fitz_counter(calls: dict) -> object:
    """Count `fitz.Page.get_text` calls into `calls` (for the B-C read-path
    zero-fitz assertions on file-backed pages)."""
    orig = fitz.Page.get_text

    def counting(self, *args, **kwargs):
        calls["n"] += 1
        return orig(self, *args, **kwargs)

    fitz.Page.get_text = counting
    return orig


def _restore_fitz_counter(saved: object) -> None:
    fitz.Page.get_text = saved


def check_three_way(tmp: Path) -> None:
    corpus = json.loads(Path(CORPUS).read_text())
    assert corpus["schema"] == "retainpdf_text_read_corpus_v1"
    assert _native.NATIVE

    calls = {"n": 0}
    saved = _install_counters(calls)
    try:
        for case in corpus["cases"]:
            raw = base64.b64decode(case["pdf_b64"])
            src = tmp / f"txt-{case['name']}.pdf"
            src.write_bytes(raw)
            d = fitz.open(src)
            try:
                for idx_str, expected in case["pages"].items():
                    page = d.load_page(int(idx_str))
                    label = f"{case['name']} p{idx_str}"

                    was = _native.NATIVE
                    _native.NATIVE = False
                    try:
                        ref = _run_all(page)
                    finally:
                        _native.NATIVE = was

                    native = _run_all(page)
                    _assert_all_equal(native, ref, label)

                    assert _entries_close(ref[0], _entries(expected["text_spans"])), f"{label}: spans ref vs corpus"
                    assert _entries_close(ref[1], _entries(expected["text_blocks"])), f"{label}: blocks ref vs corpus"
                    assert _rects_close(ref[2], [fitz.Rect(item) for item in expected["math_rects"]]), f"{label}: math ref vs corpus"
                    assert _heights_close(ref[3], expected["span_heights"]), f"{label}: heights ref vs corpus"
            finally:
                d.close()
    finally:
        _restore_counters(saved)
    assert calls["n"] > 0, "text-read production never hit a native bridge"


def check_boundaries(tmp: Path) -> None:
    # Corrupt bytes: each bridge raises cleanly (the shims' try/except needs that
    # to fall back to the reference).
    for name in _BRIDGES:
        try:
            getattr(_native, name)(b"\x00\x01\x02 not a pdf", 0)
            raise AssertionError(f"{name} must raise on corrupt bytes")
        except Exception:
            pass

    # In-memory page (`page.parent.name == ""`): falls back to reference == fitz.
    doc = fitz.open()
    p = doc.new_page(width=200.0, height=200.0)
    p.insert_text((20.0, 40.0), "in memory text", fontsize=14)
    assert _native._page_source_pdf_path(p) == "", "expected in-memory page"
    native = _run_all(p)
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        ref = _run_all(p)
    finally:
        _native.NATIVE = was
    _assert_all_equal(native, ref, "in-memory")
    assert native[0], "in-memory page should still report text spans"
    doc.close()

    # File-backed page whose backing file vanished: `read_bytes` fails, so the
    # shims fall back to the reference (== fitz).
    src = tmp / "gone.pdf"
    d = fitz.open()
    pg = d.new_page(width=200.0, height=200.0)
    pg.insert_text((20.0, 40.0), "gone file text", fontsize=14)
    d.save(src)
    d.close()

    doc_ref = fitz.open(src)
    ref_page = doc_ref.load_page(0)
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        expected = _run_all(ref_page)
    finally:
        _native.NATIVE = was
    doc_ref.close()

    d2 = fitz.open(src)
    page = d2.load_page(0)
    src.unlink()
    try:
        gone = _run_all(page)
    finally:
        d2.close()
    _assert_all_equal(gone, expected, "vanished-file")
    assert expected[0], "vanished-file page should still report text spans"


def _text_page_with_text() -> fitz.Document:
    doc = fitz.open()
    page = doc.new_page(width=300.0, height=400.0)
    page.insert_text((20.0, 60.0), "hello world", fontsize=14)
    return doc


def check_end_to_end(tmp: Path) -> None:
    # item_removable_text_rects: no page mutation, so both runs share one page.
    src = tmp / "e2e-item.pdf"
    d = _text_page_with_text()
    d.save(src)
    d.close()
    doc = fitz.open(src)
    page = doc.load_page(0)
    span_bbox = fitz.Rect(page.get_text("dict")["blocks"][0]["lines"][0]["spans"][0]["bbox"])
    item = {"item_id": "i0", "source_text": "hello world"}
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        ref_rects = item_removable_text_rects(page, item, span_bbox)
    finally:
        _native.NATIVE = was
    native_rects = item_removable_text_rects(page, item, span_bbox)
    assert native_rects, "expected at least one removable rect for the item"
    assert _rects_close(native_rects, ref_rects), "item_removable_text_rects native vs ref"
    doc.close()

    # apply_auto_redaction: mutates the page (covers + redaction), so native and
    # ref each run on their own identical copy.
    src_a = tmp / "e2e-auto-a.pdf"
    src_b = tmp / "e2e-auto-b.pdf"
    for src in (src_a, src_b):
        d = _text_page_with_text()
        d.save(src)
        d.close()
    valid_items = [(span_bbox, dict(item), "hello world")]
    doc_a = fitz.open(src_a)
    page_a = doc_a.load_page(0)
    was = _native.NATIVE
    _native.NATIVE = True
    try:
        diag_native = auto.apply_auto_redaction(page_a, valid_items)
    finally:
        _native.NATIVE = was
    doc_a.close()
    doc_b = fitz.open(src_b)
    page_b = doc_b.load_page(0)
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        diag_ref = auto.apply_auto_redaction(page_b, valid_items)
    finally:
        _native.NATIVE = was
    doc_b.close()

    assert diag_native["uses_pymupdf_redaction"], "expected pymupdf redaction to run in the auto path"
    assert diag_native == diag_ref, f"apply_auto_redaction diagnostics diverge: {diag_native} != {diag_ref}"


def check_intrusion_and_margin(tmp: Path) -> None:
    """B-C: `text_intrusion` / `margin_text_cleanup` read through the native
    spans/blocks primitives. On file-backed corpus pages the read path must hit
    the bridges and never call `page.get_text`, and native must agree with the
    NATIVE=False fallback. NOTE: `collect_page_intrusive_display_text_rects`
    no longer flags whitespace-only spans (the spans primitive's non-empty-text
    contract) — the divergence from raw `get_text("dict")` is logged in the
    status doc's 分歧台账."""
    corpus = json.loads(Path(CORPUS).read_text())
    calls = {"n": 0}
    fitz_reads = {"n": 0}
    saved_bridge = _install_counters(calls)
    saved_fitz = _install_fitz_counter(fitz_reads)
    pages = 0
    try:
        for case in corpus["cases"]:
            raw = base64.b64decode(case["pdf_b64"])
            src = tmp / f"txt-{case['name']}.pdf"
            src.write_bytes(raw)
            d = fitz.open(src)
            try:
                for idx_str in case["pages"]:
                    page = d.load_page(int(idx_str))
                    label = f"{case['name']} p{idx_str}"
                    pages += 1

                    # Native read path: bridges hit, no page.get_text.
                    fitz_reads["n"] = 0
                    nat_i = text_intrusion.collect_page_intrusive_display_text_rects(page)
                    nat_m = margin_text_cleanup._candidate_margin_block_rects(page)
                    assert fitz_reads["n"] == 0, f"{label}: native read path touched page.get_text {fitz_reads['n']}x"

                    # Fallback path must agree.
                    was = _native.NATIVE
                    _native.NATIVE = False
                    try:
                        ref_i = text_intrusion.collect_page_intrusive_display_text_rects(page)
                        ref_m = margin_text_cleanup._candidate_margin_block_rects(page)
                    finally:
                        _native.NATIVE = was
                    assert _rects_close(nat_i, ref_i), f"{label}: text_intrusion native vs ref"
                    assert _rects_close(nat_m, ref_m), f"{label}: margin_text native vs ref"
            finally:
                d.close()
    finally:
        _restore_counters(saved_bridge)
        _restore_fitz_counter(saved_fitz)
    assert pages > 0
    assert calls["n"] > 0, "intrusion/margin read path never hit a native bridge"


def main() -> None:
    assert _native.NATIVE, "text-read native module not built"
    with tempfile.TemporaryDirectory(prefix="rps-txtread-") as tmp_dir:
        tmp = Path(tmp_dir)
        check_three_way(tmp)
        check_boundaries(tmp)
        check_end_to_end(tmp)
        check_intrusion_and_margin(tmp)
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
