#!/usr/bin/env python3
"""Full-page pixel parity gate for the wired production overlay path (B3).

Drives the REAL `book_renderer.build_book_typst_pdf` native-vs-python exactly
like `smoke_end_to_end_parity.py` (which this imports for the shared harness),
then, on top of the `.typ` byte-exact and per-page words/ink checks, renders
every output page to a full-page grayscale pixmap (scale 2.0) and asserts the
pixel buffers agree within tight statistical tolerances.

Calibrated on the baseline fixture (2026-08): the native and python runs render
pixel-IDENTICAL pages (mean abs diff 0.0, p95 0, max 0, sparse>2 0.0) — the
byte difference between the two output PDFs is metadata only (IDs/timestamps).
Tolerances carry ~2x headroom over that baseline so future fixtures with real
antialiasing noise do not flake:
  PIXEL_MEAN_TOL   = 0.01  (mean per-pixel abs diff, 0..255 scale; baseline 0.0)
  PIXEL_P95_TOL    = 1     (95th-percentile abs diff; baseline 0)
  PIXEL_SPARSE_TOL = 0.001 (fraction of pixels differing by >2 levels; base 0.0)

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/smoke_end_to_end_pixel_parity.py
"""

import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)
sys.path.insert(0, _HERE)

import fitz  # noqa: E402

import smoke_end_to_end_parity as gate  # noqa: E402

from services.rendering.output.typst import _native as _typst_native  # noqa: E402
from services.rendering.output.typst.book_renderer import build_book_typst_pdf  # noqa: E402

PIXEL_MEAN_TOL = 0.01
PIXEL_P95_TOL = 1
PIXEL_SPARSE_TOL = 0.001


def page_gray_stats(doc):
    stats = []
    for i in range(doc.page_count):
        pix = doc[i].get_pixmap(
            matrix=fitz.Matrix(2.0, 2.0), colorspace=fitz.csGRAY, alpha=False
        )
        stats.append((pix.width, pix.height, bytes(pix.samples)))
    return stats


def assert_pixel_close(actual_path: Path, expected_path: Path, label: str) -> None:
    actual = fitz.open(actual_path)
    expected = fitz.open(expected_path)
    try:
        a = page_gray_stats(actual)
        b = page_gray_stats(expected)
        assert len(a) == len(b), f"{label}: page count {len(a)} != {len(b)}"
        for i, ((aw, ah, asa), (bw, bh, bsa)) in enumerate(zip(a, b)):
            assert (aw, ah) == (bw, bh), f"{label} p{i}: pixmap dims {aw}x{ah} vs {bw}x{bh}"
            total = len(asa)
            if total == 0:
                continue
            diffs = sorted(abs(x - y) for x, y in zip(asa, bsa))
            mean = sum(diffs) / total
            p95 = diffs[int(total * 0.95)]
            sparse = sum(1 for d in diffs if d > 2) / total
            assert mean <= PIXEL_MEAN_TOL, f"{label} p{i}: mean pixel diff {mean:.6f}"
            assert p95 <= PIXEL_P95_TOL, f"{label} p{i}: p95 pixel diff {p95}"
            assert sparse <= PIXEL_SPARSE_TOL, f"{label} p{i}: sparse>2 {sparse:.6f}"
    finally:
        actual.close()
        expected.close()


def check_end_to_end_pixel_parity() -> None:
    assert _typst_native.NATIVE, "typst native module not built"
    with tempfile.TemporaryDirectory(prefix="rps-pixel-") as tmp_dir:
        root = Path(tmp_dir)
        source_pdf = root / "source.pdf"
        gate._build_source_pdf(source_pdf)

        emit_calls = {"n": 0}
        saved_emit = gate._install_counter(
            _typst_native, "_native_emit_typst_book_overlay_source", emit_calls
        )
        try:
            native_out, native_typ = gate._render_output(root / "native", source_pdf)
        finally:
            gate._restore(_typst_native, "_native_emit_typst_book_overlay_source", saved_emit)
        assert emit_calls["n"] > 0, "overlay emit never hit the native bridge end-to-end"

        gate._set_native_all(False)
        py_emit_calls = {"n": 0}
        saved_py_emit = gate._install_counter(
            _typst_native, "_native_emit_typst_book_overlay_source", py_emit_calls
        )
        try:
            py_out, py_typ = gate._render_output(root / "py", source_pdf)
        finally:
            gate._restore(_typst_native, "_native_emit_typst_book_overlay_source", saved_py_emit)
            gate._set_native_all(True)
        assert py_emit_calls["n"] == 0, "python run unexpectedly hit the native emit bridge"

        assert native_typ == py_typ, "emitted overlay .typ diverges native vs python"
        gate.assert_close(
            gate.page_facts(fitz.open(native_out)),
            gate.page_facts(fitz.open(py_out)),
            "final pdf native vs python",
        )
        assert_pixel_close(native_out, py_out, "final pdf native vs python")
        print("all end-to-end pixel parity tests pass")


if __name__ == "__main__":
    check_end_to_end_pixel_parity()
