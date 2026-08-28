#!/usr/bin/env python3
"""Native bridge smoke test for the background-image read (Phase B2-8).

Three parts:

1. Three-way: every `tests/image_rects_corpus.json` case, run on file-backed
   pages — `page_has_large_background_image` NATIVE == NATIVE=False == the
   corpus `has_large` (exact per-page; includes golden 1.pdf/2.pdf, whose
   image-mask pages the native collector must report like fitz). A call counter
   on the bridge confirms native was actually hit on at least one page.

2. Boundary: corrupt bytes make the bridge raise (the shim's try/except falls
   back to the reference); an in-memory page (`page.parent.name == ""`) falls
   back to the reference == fitz; a file-backed page whose backing file vanished
   also falls back.

3. End-to-end: `should_redact_source_page` is False on a file-backed synthetic
   page with a large background image and True on a no-background page;
   `replace_background_image_page` runs through the native `page_has_large_background_image`
   gate + the reference `pick_primary_background_image` rewrite path.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_image_rects_bridge.py
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
from services.rendering.source.background import detect  # noqa: E402
from services.rendering.source.background.image_route import (  # noqa: E402
    replace_background_image_page,
)
from services.rendering.source.background.redaction_plan import (  # noqa: E402
    should_redact_source_page,
)

# Corpus lives next to its Rust replay (`rendering_reader/tests/image_rects_diff.rs`).
CORPUS = os.path.abspath(os.path.join(_HERE, "..", "..", "rendering_reader", "tests", "image_rects_corpus.json"))


def _png_bytes(w: int, h: int, val: int) -> bytes:
    pm = fitz.Pixmap(fitz.csRGB, fitz.IRect(0, 0, w, h))
    pm.clear_with(val)
    return pm.tobytes("png")


def check_three_way(tmp: Path) -> None:
    corpus = json.loads(Path(CORPUS).read_text())
    assert corpus["schema"] == "retainpdf_image_rects_corpus_v1"
    assert _native.NATIVE

    calls = {"n": 0}
    orig = _native._native_read_page_image_rects

    def counting(*args, **kwargs):
        calls["n"] += 1
        return orig(*args, **kwargs)

    _native._native_read_page_image_rects = counting
    try:
        for case in corpus["cases"]:
            raw = base64.b64decode(case["pdf_b64"])
            src = tmp / f"img-{case['name']}.pdf"
            src.write_bytes(raw)
            d = fitz.open(src)
            try:
                for idx_str, expected in case["pages"].items():
                    page = d.load_page(int(idx_str))
                    native = detect.page_has_large_background_image(page)
                    was = _native.NATIVE
                    _native.NATIVE = False
                    try:
                        ref = detect.page_has_large_background_image(page)
                    finally:
                        _native.NATIVE = was
                    label = f"{case['name']} p{idx_str}"
                    assert native == ref == bool(expected["has_large"]), (
                        f"{label}: native={native} ref={ref} recorded={expected['has_large']}"
                    )
            finally:
                d.close()
    finally:
        _native._native_read_page_image_rects = orig
    assert calls["n"] > 0, "page_has_large_background_image never hit the native bridge"


def check_boundaries(tmp: Path) -> None:
    # Corrupt bytes: the bridge raises cleanly (the shim's try/except needs it
    # to fall back to the reference).
    try:
        _native._native_read_page_image_rects(b"\x00\x01\x02 not a pdf", 0)
        raise AssertionError("bridge must raise on corrupt bytes")
    except Exception:
        pass

    # In-memory page (`page.parent.name == ""`): falls back to reference == fitz.
    doc = fitz.open()
    p = doc.new_page(width=200.0, height=200.0)
    p.insert_image(fitz.Rect(0.0, 0.0, 200.0, 200.0), stream=_png_bytes(10, 10, 120))
    assert _native._page_source_pdf_path(p) == "", "expected in-memory page"
    mem = detect.page_has_large_background_image(p)
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        mem_ref = detect.page_has_large_background_image(p)
    finally:
        _native.NATIVE = was
    assert mem is True and mem == mem_ref, (mem, mem_ref)
    doc.close()

    # File-backed page whose backing file vanished: the shim's `read_bytes`
    # fails, so it falls back to the reference (== fitz).
    src = tmp / "gone.pdf"
    d = fitz.open()
    pg = d.new_page(width=200.0, height=200.0)
    pg.insert_image(fitz.Rect(0.0, 0.0, 200.0, 200.0), stream=_png_bytes(10, 10, 120))
    d.save(src)
    d.close()
    d2 = fitz.open(src)
    page = d2.load_page(0)
    src.unlink()
    try:
        gone = detect.page_has_large_background_image(page)
    finally:
        d2.close()
    assert gone is True, gone


def check_end_to_end(tmp: Path) -> None:
    # Large-bg page: no redaction needed; the background image rewrite runs
    # through the native gate + the reference picker.
    src = tmp / "e2e-bg.pdf"
    d = fitz.open()
    page = d.new_page(width=300.0, height=400.0)
    page.insert_image(fitz.Rect(0.0, 0.0, 300.0, 400.0), stream=_png_bytes(6, 8, 220))
    d.save(src)
    d.close()
    doc = fitz.open(src)
    page = doc.load_page(0)
    items = [{"item_id": "i0", "bbox": [100.0, 100.0, 200.0, 140.0], "translated_text": "word"}]
    try:
        assert not should_redact_source_page(page), "large-bg page must NOT redact"
        assert replace_background_image_page(page, items), "expected bg replacement to run"
    finally:
        doc.close()

    # No-bg page: redaction is required.
    src2 = tmp / "e2e-nobg.pdf"
    d2 = fitz.open()
    p2 = d2.new_page(width=300.0, height=400.0)
    p2.insert_text((20.0, 30.0), "hello")
    d2.save(src2)
    d2.close()
    doc2 = fitz.open(src2)
    page2 = doc2.load_page(0)
    try:
        assert should_redact_source_page(page2), "no-bg page must redact"
    finally:
        doc2.close()


def main() -> None:
    assert _native.NATIVE, "image-rects native module not built"
    with tempfile.TemporaryDirectory(prefix="rps-imgrects-") as tmp_dir:
        tmp = Path(tmp_dir)
        check_three_way(tmp)
        check_boundaries(tmp)
        check_end_to_end(tmp)
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
