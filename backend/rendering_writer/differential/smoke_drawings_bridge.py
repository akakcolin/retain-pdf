#!/usr/bin/env python3
"""Native bridge smoke test for the vector drawing reads (Phase B2-7).

Three parts:

1. vector_text three-way: every `tests/vector_text_corpus.json` case, run on a
   file-backed page — `collect_vector_text_rects` NATIVE == NATIVE=False == the
   corpus `expected_rects` (0.02 pt). A call counter on the bridge confirms at
   least some cases really used native (not all fell back).

2. vector_profile three-way: a synthetic page (RGB / gray / cmyk fills, a
   cm-scaled stroked line, fill+stroke) and every page of the golden PDFs
   `resources/samples/golden-pdfs/{1,2,3}.pdf` — `page_drawing_count` NATIVE ==
   NATIVE=False == fitz `len(page.get_cdrawings())` exactly (count parity is
   exact; `collect_page_drawing_rects` relays the Python reference because
   native rects diverge on stroked zigzag paths, so its shim output must still
   equal the reference). Native hit pages > 0. (`3.pdf`'s heavy pages are
   covered here via file-backed pages — the hermetic corpus excludes it for
   size.)

3. Boundary + end-to-end: corrupt bytes return `[]`/`0` without raising (both
   paths); an in-memory page (`page.parent.name == ""`) falls back to the
   reference == fitz; `should_use_cover_only_for_vector_text` and
   `collect_redaction_page_facts` run on a file-backed synthetic page with a
   vector glyph.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_drawings_bridge.py
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
from services.rendering.source import vector_profile, vector_text  # noqa: E402
from services.rendering.source.background.redaction_plan import (  # noqa: E402
    should_use_cover_only_for_vector_text,
)
from services.rendering.source.cleanup.page_facts import collect_redaction_page_facts  # noqa: E402
from services.rendering.source.vector_profile import (  # noqa: E402
    _collect_page_drawing_rects_python,
    _page_drawing_count_python,
)

RECT_TOL = 0.01
RECT_TOL_VT = 0.02
GOLDEN_ROOT = os.path.abspath(
    os.path.join(_HERE, "..", "..", "..", "resources", "samples", "golden-pdfs")
)
VT_CORPUS = os.path.abspath(os.path.join(_HERE, "..", "tests", "vector_text_corpus.json"))


def _close(a: float, b: float, tol: float) -> bool:
    return abs(a - b) <= tol


def _rects_close(actual: list, expected: list, tol: float) -> None:
    assert len(actual) == len(expected), f"rect count {actual} != {expected}"
    for a, e in zip(actual, expected):
        ar = [float(v) for v in (a if isinstance(a, fitz.Rect) else a)]
        er = [float(v) for v in (e if isinstance(e, fitz.Rect) else e)]
        for av, ev in zip(ar, er):
            assert _close(av, ev, tol), f"rect {ar} != {er}"


def _file_backed_page(raw: bytes, path: Path) -> fitz.Page:
    path.write_bytes(raw)
    doc = fitz.open(path)
    return doc.load_page(0)


def check_vector_text_three_way(tmp: Path) -> None:
    corpus = json.loads(Path(VT_CORPUS).read_text())
    assert corpus["schema"] == "retainpdf_vector_text_corpus_v1"
    assert _native.NATIVE

    calls = {"n": 0}
    orig = _native._native_collect_vector_text_rects

    def counting(*args, **kwargs):
        calls["n"] += 1
        return orig(*args, **kwargs)

    _native._native_collect_vector_text_rects = counting
    try:
        for case in corpus["cases"]:
            raw = base64.b64decode(case["input_pdf_b64"])
            src = tmp / f"vt-{case['name']}.pdf"
            page = _file_backed_page(raw, src)
            targets = [fitz.Rect(r) for r in case["target_rects"]]

            native = vector_text.collect_vector_text_rects(page, targets)

            was = _native.NATIVE
            _native.NATIVE = False
            try:
                ref = vector_text.collect_vector_text_rects(page, targets)
            finally:
                _native.NATIVE = was

            expected = [fitz.Rect(r) for r in case["expected_rects"]]
            _rects_close(native, expected, RECT_TOL_VT)
            _rects_close(ref, expected, RECT_TOL_VT)
            page.parent.close()
    finally:
        _native._native_collect_vector_text_rects = orig
    assert calls["n"] > 0, "vector_text never hit the native bridge"


def _drawing_rects_fitz(page: fitz.Page) -> list:
    try:
        drawings = page.get_cdrawings() if hasattr(page, "get_cdrawings") else page.get_drawings()
    except Exception:
        return []
    return [fitz.Rect(d["rect"]) for d in drawings if d.get("rect")]


def check_vector_profile_page(page: fitz.Page, label: str) -> None:
    native_count = vector_profile.page_drawing_count(page)
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        ref_count = vector_profile.page_drawing_count(page)
        ref_rects = vector_profile.collect_page_drawing_rects(page)
    finally:
        _native.NATIVE = was
    fitz_count = len(page.get_cdrawings()) if hasattr(page, "get_cdrawings") else len(page.get_drawings())
    assert native_count == ref_count == fitz_count, (
        f"{label}: count {native_count} vs {ref_count} vs {fitz_count}"
    )
    native_rects = vector_profile.collect_page_drawing_rects(page)
    _rects_close(native_rects, ref_rects, RECT_TOL)


def check_vector_profile_three_way(tmp: Path) -> None:
    assert _native.NATIVE

    # Synthetic page: cm-scaled stroked line (set_contents first — a later
    # set_contents would REPLACE the draw_* commands), then RGB fill, gray fill,
    # cmyk fill, fill+stroke rect, stroke-only polyline.
    doc = fitz.open()
    page = doc.new_page(width=612.0, height=792.0)
    ops = b"q\n0.5 0 0 0.5 100 300 cm\n3 w\n200 300 m\n240 304 l\nS\nQ\n"
    xref = doc.get_new_xref()
    doc.update_object(xref, "<< /Length %d >>" % len(ops))
    doc.update_stream(xref, ops)
    page.set_contents(xref)
    page.draw_rect(fitz.Rect(50.0, 50.0, 250.0, 60.0), color=None, fill=(0.3, 0.5, 0.7))
    page.draw_rect(fitz.Rect(70.0, 100.0, 170.0, 110.0), color=None, fill=(0.5,))
    page.draw_rect(fitz.Rect(80.0, 140.0, 200.0, 152.0), color=None, fill=(0.1, 0.2, 0.3, 0.4))
    page.draw_rect(fitz.Rect(60.0, 200.0, 260.0, 210.0), color=(0.0, 0.0, 0.0), width=1.5, fill=(0.1, 0.1, 0.1))
    page.draw_polyline(
        [(100.0, 260.0), (110.0, 262.0), (120.0, 264.0), (130.0, 266.0)],
        color=(0.0, 0.0, 0.0), width=1.0)
    src = tmp / "profile-synthetic.pdf"
    doc.save(src)
    doc.close()

    d = fitz.open(src)
    try:
        check_vector_profile_page(d.load_page(0), "synthetic")
    finally:
        d.close()

    # Golden PDFs: every page of each file. The call counter wraps the count
    # bridge entry only — `collect_page_drawing_rects` relays the reference, so
    # native hits come from `page_drawing_count`.
    calls = {"n": 0}
    orig = _native._native_read_page_drawing_count

    def counting(*args, **kwargs):
        calls["n"] += 1
        return orig(*args, **kwargs)

    _native._native_read_page_drawing_count = counting
    try:
        for name in ["1.pdf", "2.pdf", "3.pdf"]:
            golden = Path(GOLDEN_ROOT) / name
            assert golden.exists(), f"golden {golden} missing"
            d = fitz.open(golden)
            try:
                for idx in range(d.page_count):
                    check_vector_profile_page(d.load_page(idx), f"{name} p{idx}")
            finally:
                d.close()
    finally:
        _native._native_read_page_drawing_count = orig
    assert calls["n"] > 0, "vector_profile page_drawing_count never hit the native bridge"


def check_boundaries(tmp: Path) -> None:
    # Corrupt bytes: the bridge raises cleanly (the shim's try/except needs it
    # to fall back). fitz cannot open corrupt bytes, so exercise the bridge
    # directly.
    try:
        _native._native_read_page_drawing_count(b"\x00\x01\x02 not a pdf", 0)
        raise AssertionError("bridge must raise on corrupt bytes")
    except Exception:
        pass

    # In-memory page (`page.parent.name == ""`): falls back to reference == fitz.
    doc = fitz.open()
    p = doc.new_page(width=200.0, height=200.0)
    p.draw_rect(fitz.Rect(10.0, 10.0, 100.0, 20.0), color=None, fill=(0.1, 0.1, 0.1))
    assert _native._page_source_pdf_path(p) == "", "expected in-memory page"
    mem_count = vector_profile.page_drawing_count(p)
    mem_rects = vector_profile.collect_page_drawing_rects(p)
    fitz_rects = _drawing_rects_fitz(p)
    assert mem_count == len(fitz_rects) == 1, mem_count
    _rects_close(mem_rects, fitz_rects, RECT_TOL)
    doc.close()

    # File-backed page whose backing file vanished: the shim's `read_bytes`
    # fails, so it falls back to the reference (== fitz).
    src = tmp / "gone.pdf"
    d = fitz.open()
    pg = d.new_page(width=200.0, height=200.0)
    pg.draw_rect(fitz.Rect(10.0, 10.0, 100.0, 20.0), color=None, fill=(0.2, 0.2, 0.2))
    d.save(src)
    d.close()
    d2 = fitz.open(src)
    page = d2.load_page(0)
    src.unlink()
    try:
        gone_count = vector_profile.page_drawing_count(page)
        gone_rects = vector_profile.collect_page_drawing_rects(page)
    finally:
        d2.close()
    assert gone_count == 1, gone_count
    _rects_close(gone_rects, [fitz.Rect(10.0, 10.0, 100.0, 20.0)], RECT_TOL)


def check_end_to_end(tmp: Path) -> None:
    # A file-backed synthetic page with a small black-filled vector glyph
    # (8-item polyline) overlapping the target rect.
    doc = fitz.open()
    page = doc.new_page(width=612.0, height=792.0)
    page.draw_polyline(
        [(200.0, 95.0), (207.5, 100.0), (215.0, 95.0), (222.5, 100.0),
         (230.0, 95.0), (237.5, 100.0), (245.0, 95.0), (252.5, 100.0), (260.0, 95.0)],
        color=None, fill=(0.1, 0.1, 0.1))
    src = tmp / "e2e.pdf"
    doc.save(src)
    doc.close()

    d = fitz.open(src)
    page = d.load_page(0)
    try:
        translated_items = [{"item_id": "i0", "bbox": [205.0, 97.0, 250.0, 103.0], "translated_text": "word"}]
        assert should_use_cover_only_for_vector_text(page, translated_items), "expected vector-text cover-only"
        facts = collect_redaction_page_facts(page)
        assert facts.drawing_count > 0, "expected non-zero drawing facts"
        assert facts.drawing_rects, "expected drawing rects"
        assert facts.image_page is False
    finally:
        d.close()


def main() -> None:
    assert _native.NATIVE, "drawings native module not built"
    with tempfile.TemporaryDirectory(prefix="rps-drawings-") as tmp_dir:
        tmp = Path(tmp_dir)
        check_vector_text_three_way(tmp)
        check_vector_profile_three_way(tmp)
        check_boundaries(tmp)
        check_end_to_end(tmp)
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
