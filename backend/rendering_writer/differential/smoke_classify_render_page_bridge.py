#!/usr/bin/env python3
"""Native bridge smoke test for B-A: per-page `classify_render_page` wiring.

Two-way parity between the new native-routed entry
`analysis.classifier.classify_render_page_pdf` (thin bridge `classify_render_page`
reusing the whole-document analysis reader primitives) and the fitz reference
`analysis.classifier.classify_render_page` on the golden PDFs 1.pdf/2.pdf/3.pdf
plus synthetic editable-text, scan, and vector-heavy pages.

Per-page parity:
  * exact: kind, large_background_image, drawing_count,
    route.{redaction, background, compose, layout, reason}
  * coverage_ratio within 1e-6
  * visible_text_traces / hidden_text_traces are allowlisted: a divergence is
    accepted ONLY when the kind still matches (native `text_traces` is empty,
    so the trace counts fall back — this only diverges on hidden-text pages,
    where the reference itself would change the classification).

Also: a native-hit counter (the routed entry must actually reach a bridge), a
fitz-call counter (while native is hit, classification touches zero fitz
document/render surfaces), and a NATIVE=False fallback parity check (the
allowlisted reference must produce the same result).

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_classify_render_page_bridge.py
"""

import os
import sys
import tempfile
from pathlib import Path

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402
from services.rendering.analysis import classifier  # noqa: E402

_GOLDEN_DIR = os.path.abspath(
    os.path.join(_HERE, "..", "..", "..", "resources", "samples", "golden-pdfs")
)

TOL_COVERAGE = 1e-6

_EXACT_FIELDS = (
    "kind",
    "large_background_image",
    "drawing_count",
)
_ROUTE_FIELDS = ("redaction", "background", "compose", "layout", "reason")
_TRACE_FIELDS = ("visible_text_traces", "hidden_text_traces")

_FITZ_SURFACES = (
    (fitz, "open"),
    (fitz.Document, "__len__"),
    (fitz.Document, "__getitem__"),
    (fitz.Document, "save"),
    (fitz.Document, "tobytes"),
    (fitz.Document, "subset_fonts"),
    (fitz.Page, "show_pdf_page"),
    (fitz.Page, "get_pixmap"),
    (fitz.Page, "get_text"),
    (fitz.Page, "get_drawings"),
)


def _install_fitz_counters(calls: dict) -> dict:
    saved = {}
    for obj, attr in _FITZ_SURFACES:
        orig = getattr(obj, attr)
        key = f"{obj.__name__}.{attr}"
        saved[key] = (obj, attr, orig)

        def counting(*args, _orig=orig, **kwargs):
            calls["n"] += 1
            return _orig(*args, **kwargs)

        setattr(obj, attr, counting)
    return saved


def _restore_fitz_counters(saved: dict) -> None:
    for _key, (obj, attr, orig) in saved.items():
        setattr(obj, attr, orig)


def _assert_parity(ref: classifier.RenderPageClassification, native: classifier.RenderPageClassification, label: str) -> None:
    for key in _EXACT_FIELDS:
        assert getattr(ref, key) == getattr(native, key), (
            f"{label}: {key} ref={getattr(ref, key)!r} native={getattr(native, key)!r}"
        )
    cov = abs(ref.background_coverage_ratio - native.background_coverage_ratio)
    assert cov <= TOL_COVERAGE, (
        f"{label}: coverage ref={ref.background_coverage_ratio:.9f} "
        f"native={native.background_coverage_ratio:.9f}"
    )
    assert ref.route is not None and native.route is not None, f"{label}: route missing"
    for key in _ROUTE_FIELDS:
        assert getattr(ref.route, key) == getattr(native.route, key), (
            f"{label}: route.{key} ref={getattr(ref.route, key)!r} native={getattr(native.route, key)!r}"
        )
    for key in _TRACE_FIELDS:
        if getattr(ref, key) != getattr(native, key):
            assert ref.kind == native.kind, (
                f"{label}: {key} divergence requires kind parity "
                f"ref={getattr(ref, key)} native={getattr(native, key)} kind={ref.kind}"
            )


def _assert_pdf_parity(src: Path, label: str) -> None:
    doc = fitz.open(src)
    try:
        for page_index in range(len(doc)):
            ref = classifier.classify_render_page(doc[page_index])
            native = classifier.classify_render_page_pdf(src, page_index)
            _assert_parity(ref, native, f"{label} p{page_index}")
    finally:
        doc.close()


def _check_golden() -> None:
    for name in ("1.pdf", "2.pdf", "3.pdf"):
        _assert_pdf_parity(Path(_GOLDEN_DIR) / name, f"golden {name}")


def _check_synthetic(tmp: Path) -> None:
    editable = tmp / "editable.pdf"
    doc = fitz.open()
    page = doc.new_page(width=612, height=792)
    page.insert_text((72, 72), "editable prose with enough words " * 8)
    doc.save(editable)
    doc.close()
    _assert_pdf_parity(editable, "synthetic editable")

    scan = tmp / "scan.pdf"
    doc = fitz.open()
    page = doc.new_page(width=612, height=792)
    pix = fitz.Pixmap(fitz.csGRAY, fitz.IRect(0, 0, 64, 64))
    pix.clear_with(128)
    page.insert_image(fitz.Rect(0, 0, 612, 792), pixmap=pix)
    doc.save(scan)
    doc.close()
    _assert_pdf_parity(scan, "synthetic scan")

    vector = tmp / "vector.pdf"
    doc = fitz.open()
    page = doc.new_page(width=612, height=792)
    for i in range(2100):
        x0 = (i % 60) * 10
        y0 = (i // 60) * 10
        page.draw_rect(fitz.Rect(x0, y0, x0 + 5, y0 + 5), color=(0, 0, 0), width=0.5)
    page.insert_text((72, 72), "lots of text words to keep editable " * 5)
    doc.save(vector)
    doc.close()
    _assert_pdf_parity(vector, "synthetic vector")


def _check_native_hit_and_fitz_zero(tmp: Path) -> None:
    src = Path(_GOLDEN_DIR) / "1.pdf"
    native_calls = {"n": 0}
    saved_native = classifier._native_classify_render_page
    classifier._native_classify_render_page = _counting(saved_native, native_calls)
    fitz_calls = {"n": 0}
    saved_fitz = _install_fitz_counters(fitz_calls)
    try:
        classifier.classify_render_page_pdf(src, 0)
        classifier.classify_render_page_pdf(src, 1)
    finally:
        classifier._native_classify_render_page = saved_native
        _restore_fitz_counters(saved_fitz)
    assert native_calls["n"] > 0, "classify_render_page_pdf never hit a native bridge"
    assert fitz_calls["n"] == 0, f"classify_render_page_pdf touched fitz {fitz_calls['n']} times"


def _counting(orig, calls: dict):
    def counting(*args, **kwargs):
        calls["n"] += 1
        return orig(*args, **kwargs)

    return counting


def _check_native_off_fallback(tmp: Path) -> None:
    src = Path(_GOLDEN_DIR) / "1.pdf"
    saved = classifier.NATIVE
    classifier.NATIVE = False
    try:
        doc = fitz.open(src)
        try:
            for page_index in range(len(doc)):
                ref = classifier.classify_render_page(doc[page_index])
                native = classifier.classify_render_page_pdf(src, page_index)
                _assert_parity(ref, native, f"native-off fallback p{page_index}")
        finally:
            doc.close()
    finally:
        classifier.NATIVE = saved


def main() -> None:
    assert classifier.NATIVE, "classify_render_page native module not built"
    _check_golden()
    with tempfile.TemporaryDirectory(prefix="rps-ba-classify-") as tmp_dir:
        _check_synthetic(Path(tmp_dir))
        _check_native_hit_and_fitz_zero(Path(tmp_dir))
        _check_native_off_fallback(Path(tmp_dir))
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
