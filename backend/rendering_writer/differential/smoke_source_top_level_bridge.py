#!/usr/bin/env python3
"""Native bridge smoke test for the source top-level consolidation (Phase B-E).

Two parts:

1. `compression.analysis.source_pdf_has_vector_graphics` parity: synthetic
   file-backed PDFs covering the threshold ladder (no vector, some vector,
   per-page >=100 skip, total >=300 skip, missing path) run identically with
   NATIVE on and off. On the file-backed cases the native run
   must hit the `page_drawing_count` bridge (routed via
   `vector_profile.page_drawing_count`) with zero `get_drawings` /
   `get_cdrawings` calls; the reference run must call them.

2. `vector_profile.page_drawing_count` count parity on file-backed pages
   (native == fallback == fitz `len(get_drawings())`), plus the boundary: an
   in-memory page (`page.parent.name == ""`) falls back to the reference.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_source_top_level_bridge.py
"""

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
from services.rendering.source.compression.analysis import (  # noqa: E402
    VECTOR_SKIP_PAGE_DRAWINGS_THRESHOLD,
    VECTOR_SKIP_TOTAL_DRAWINGS_THRESHOLD,
    source_pdf_has_vector_graphics,
)
from services.rendering.source import vector_profile  # noqa: E402


def _install_bridge_counter(calls: dict) -> None:
    orig = _native._native_read_page_drawing_count

    def counting(*args, **kwargs):
        calls["n"] += 1
        return orig(*args, **kwargs)

    _native._native_read_page_drawing_count = counting


def _install_fitz_drawings_counters(calls: dict) -> None:
    """Count reference raw drawing reads (`get_drawings`/`get_cdrawings`)."""
    saved = {}

    for name in ("get_drawings", "get_cdrawings"):
        orig = getattr(fitz.Page, name)
        saved[name] = orig

        def counting(self, *args, _orig=orig, **kwargs):
            calls["n"] += 1
            return _orig(self, *args, **kwargs)

        setattr(fitz.Page, name, counting)
    return saved


def _restore_fitz_drawings_counters(saved: dict) -> None:
    for name, orig in saved.items():
        setattr(fitz.Page, name, orig)


def _make_pdf(path: Path, pages: int, drawings_per_page: int) -> None:
    """Synthetic file-backed PDF with `drawings_per_page` filled rects/page."""
    doc = fitz.open()
    for _ in range(pages):
        page = doc.new_page(width=612.0, height=792.0)
        for i in range(drawings_per_page):
            x0 = 50.0 + (i % 10) * 20.0
            y0 = 60.0 + (i // 10) * 30.0
            page.draw_rect(
                fitz.Rect(x0, y0, x0 + 10.0, y0 + 8.0),
                color=None,
                fill=(0.2, 0.4, 0.6),
            )
    doc.save(path)
    doc.close()


def _run_native(
    src: Path, bridge_calls: dict, fitz_calls: dict
) -> tuple[bool, int, int]:
    """Run one vector-check with native engaged; return (result, fitz_reads,
    bridge_hits) for just that run."""
    bridge_calls["n"] = 0
    fitz_calls["n"] = 0
    was = _native.NATIVE
    _native.NATIVE = True
    try:
        result = source_pdf_has_vector_graphics(src)
    finally:
        _native.NATIVE = was
    return result, fitz_calls["n"], bridge_calls["n"]


def _run_ref(src: Path, fitz_calls: dict) -> tuple[bool, int]:
    """Run one vector-check with native forced off; return (result, fitz_reads)."""
    fitz_calls["n"] = 0
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        result = source_pdf_has_vector_graphics(src)
    finally:
        _native.NATIVE = was
    return result, fitz_calls["n"]


def check_vector_thresholds(tmp: Path) -> None:
    assert _native.NATIVE

    # Ladder (pages, drawings-per-page, expected). The missing path short-circuit
    # is handled separately: it reads no page, so no bridge/fitz hits. (A
    # zero-page file is untestable here — PyMuPDF refuses to save empty docs —
    # and its `len(doc) == 0` early return is unchanged by this batch.)
    cases: list[tuple[str, int, int, bool]] = [
        ("no-vector", 1, 0, False),
        ("some-vector", 1, 5, False),
        ("page-threshold", 1, VECTOR_SKIP_PAGE_DRAWINGS_THRESHOLD, True),
        ("total-threshold", 4, VECTOR_SKIP_TOTAL_DRAWINGS_THRESHOLD // 4, True),
    ]
    paths: list[tuple[Path, bool]] = []
    for name, pages, per_page, expected in cases:
        src = tmp / f"be-{name}.pdf"
        _make_pdf(src, pages, per_page)
        paths.append((src, expected))

    bridge_calls = {"n": 0}
    fitz_calls = {"n": 0}
    _install_bridge_counter(bridge_calls)
    saved = _install_fitz_drawings_counters(fitz_calls)
    native_bridge_total = 0
    try:
        for src, expected in paths:
            native, native_fitz, native_bridge = _run_native(src, bridge_calls, fitz_calls)
            ref, ref_fitz = _run_ref(src, fitz_calls)
            assert native == ref == expected, (
                f"{src.name}: native={native} ref={ref} expected={expected}"
            )
            # Native path reads pages through the bridge only — zero raw drawing
            # reads; the ref path must genuinely read drawings (non-vacuous).
            assert native_fitz == 0, (
                f"{src.name}: native path called get_drawings/get_cdrawings {native_fitz}x"
            )
            assert native_bridge > 0, f"{src.name}: native path never hit the bridge"
            assert ref_fitz > 0, f"{src.name}: reference never read drawings"
            native_bridge_total += native_bridge

        # Missing path short-circuits before any routing.
        for src in (tmp / "does-not-exist.pdf",):
            native, native_fitz, native_bridge = _run_native(src, bridge_calls, fitz_calls)
            ref, ref_fitz = _run_ref(src, fitz_calls)
            assert native == ref is False, (src.name, native, ref)
            assert native_fitz == 0 and ref_fitz == 0, (
                f"{src.name}: expected no drawing reads on either path"
            )
            assert native_bridge == 0, f"{src.name}: expected no bridge hits"
    finally:
        _restore_fitz_drawings_counters(saved)
    assert native_bridge_total >= len(paths), (
        "source_pdf_has_vector_graphics never hit the page_drawing_count bridge"
    )


def check_page_drawing_count_parity(tmp: Path) -> None:
    assert _native.NATIVE

    src = tmp / "be-count.pdf"
    _make_pdf(src, 2, 7)
    d = fitz.open(src)
    try:
        for idx in range(d.page_count):
            page = d.load_page(idx)
            native = vector_profile.page_drawing_count(page)
            was = _native.NATIVE
            _native.NATIVE = False
            try:
                ref = vector_profile.page_drawing_count(page)
            finally:
                _native.NATIVE = was
            fitz_count = len(
                page.get_cdrawings() if hasattr(page, "get_cdrawings") else page.get_drawings()
            )
            assert native == ref == fitz_count == 7, (
                f"p{idx}: native={native} ref={ref} fitz={fitz_count}"
            )
    finally:
        d.close()

    # In-memory page falls back to the reference (== fitz).
    doc = fitz.open()
    p = doc.new_page(width=200.0, height=200.0)
    p.draw_rect(fitz.Rect(10.0, 10.0, 60.0, 20.0), color=None, fill=(0.1, 0.1, 0.1))
    assert _native._page_source_pdf_path(p) == "", "expected in-memory page"
    mem_count = vector_profile.page_drawing_count(p)
    fitz_count = len(p.get_drawings())
    assert mem_count == fitz_count == 1, (mem_count, fitz_count)
    doc.close()


def main() -> None:
    assert _native.NATIVE, "source top-level native module not built"
    with tempfile.TemporaryDirectory(prefix="rps-src-toplevel-") as tmp_dir:
        tmp = Path(tmp_dir)
        check_vector_thresholds(tmp)
        check_page_drawing_count_parity(tmp)
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
