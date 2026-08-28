#!/usr/bin/env python3
"""Native bridge smoke test for B-A: `render_mode` auto-mode probing.

Two-way parity between the new production path
`runtime.pipeline.render_mode` (routed through the `analysis/_native.py` shim,
NATIVE=True) and the previous fitz reference (`build_render_page_profile`
per-page sampling) for the auto-mode decisions `is_pseudo_editable_scan_pdf` /
`is_editable_pdf` and the resolved `auto` render mode, on the golden PDFs
1.pdf/2.pdf/3.pdf plus synthetic editable-text and scan pages.

Also:
  * a native-hit counter — the production path must actually reach a bridge;
  * a fitz-call counter — while native is hit, the render_mode decision must
    touch zero fitz document/render surfaces (B-A goal: auto sampling zero fitz).

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_render_mode_bridge.py
"""

import math
import os
import sys
import tempfile
from pathlib import Path

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402
from services.rendering.analysis import _native as analysis_native  # noqa: E402
from services.rendering.analysis.profile.builder import build_render_page_profile  # noqa: E402
from runtime.pipeline import render_mode  # noqa: E402

_GOLDEN_DIR = os.path.abspath(
    os.path.join(_HERE, "..", "..", "..", "resources", "samples", "golden-pdfs")
)

_NATIVE_BRIDGES = (
    "_native_build_render_document_analysis",
    "_native_read_page_geometry",
)

_FITZ_SURFACES = (
    (fitz, "open"),
    (fitz.Document, "__len__"),
    (fitz.Document, "__getitem__"),
    (fitz.Document, "save"),
    (fitz.Document, "tobytes"),
    (fitz.Document, "subset_fonts"),
    (fitz.Page, "show_pdf_page"),
    (fitz.Page, "get_pixmap"),
)


def _restore_counters(saved: dict) -> None:
    for name, (obj, orig) in saved.items():
        setattr(obj, name, orig)


def _install_native_counters(calls: dict) -> dict:
    saved = {}
    for name in _NATIVE_BRIDGES:
        orig = getattr(analysis_native, name)
        saved[name] = (analysis_native, orig)

        def counting(*args, _orig=orig, **kwargs):
            calls["n"] += 1
            return _orig(*args, **kwargs)

        setattr(analysis_native, name, counting)
    return saved


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


def _reference_pseudo_editable_scan(doc, start_page: int, end_page: int) -> bool:
    sample_pages = range(start_page, min(end_page, start_page + 2) + 1)
    sampled = 0
    pseudo_scan_pages = 0
    for page_idx in sample_pages:
        if 0 <= page_idx < len(doc):
            sampled += 1
            if build_render_page_profile(doc[page_idx]).kind == "pseudo_editable_scan":
                pseudo_scan_pages += 1
    return sampled > 0 and pseudo_scan_pages >= max(1, math.ceil(sampled / 2))


def _reference_editable(doc, start_page: int, end_page: int) -> bool:
    sample_pages = range(start_page, min(end_page, start_page + 2) + 1)
    sampled = 0
    editable_pages = 0
    pseudo_scan_pages = 0
    for page_idx in sample_pages:
        if 0 <= page_idx < len(doc):
            sampled += 1
            profile = build_render_page_profile(doc[page_idx])
            if profile.kind == "pseudo_editable_scan":
                pseudo_scan_pages += 1
            if profile.text_layer.editable and profile.kind == "editable_text":
                editable_pages += 1
    if sampled == 0:
        return False
    if pseudo_scan_pages >= sampled:
        return False
    return editable_pages >= max(1, math.ceil(sampled / 2))


def _reference_resolve_mode(src: Path, start_page: int, end_page: int) -> str:
    doc = fitz.open(src)
    try:
        total_pages = len(doc)
        sample_stop = total_pages - 1 if end_page < 0 else min(end_page, total_pages - 1)
        if _reference_pseudo_editable_scan(doc, start_page, sample_stop):
            return "typst_visual"
        if not _reference_editable(doc, start_page, sample_stop):
            return "typst_visual"
        return "overlay"
    finally:
        doc.close()


def _assert_decision_parity(src: Path, label: str) -> None:
    doc = fitz.open(src)
    try:
        total_pages = len(doc)
        ref_pseudo = _reference_pseudo_editable_scan(doc, 0, total_pages - 1)
        ref_editable = _reference_editable(doc, 0, total_pages - 1)
        ref_mode = _reference_resolve_mode(src, 0, -1)
    finally:
        doc.close()

    native_pseudo = render_mode.is_pseudo_editable_scan_pdf(src, 0, -1)
    native_editable = render_mode.is_editable_pdf(src, 0, -1)
    native_mode = render_mode.resolve_effective_render_mode(
        render_mode="auto",
        source_pdf_path=src,
        start_page=0,
        end_page=-1,
        translated_pages_map={0: [{"source_text": "probe", "bbox": [0, 0, 10, 10]}]},
    )

    assert native_pseudo == ref_pseudo, (
        f"{label}: pseudo-editable-scan ref={ref_pseudo} native={native_pseudo}"
    )
    assert native_editable == ref_editable, (
        f"{label}: editable ref={ref_editable} native={native_editable}"
    )
    assert native_mode == ref_mode, (
        f"{label}: resolved auto mode ref={ref_mode} native={native_mode}"
    )


def _check_golden() -> None:
    for name in ("1.pdf", "2.pdf", "3.pdf"):
        _assert_decision_parity(Path(_GOLDEN_DIR) / name, f"golden {name}")


def _check_synthetic(tmp: Path) -> None:
    editable = tmp / "editable.pdf"
    doc = fitz.open()
    page = doc.new_page(width=612, height=792)
    page.insert_text((72, 72), "editable prose with enough words " * 8)
    doc.save(editable)
    doc.close()
    _assert_decision_parity(editable, "synthetic editable")

    scan = tmp / "scan.pdf"
    doc = fitz.open()
    page = doc.new_page(width=612, height=792)
    pix = fitz.Pixmap(fitz.csGRAY, fitz.IRect(0, 0, 64, 64))
    pix.clear_with(128)
    page.insert_image(fitz.Rect(0, 0, 612, 792), pixmap=pix)
    doc.save(scan)
    doc.close()
    _assert_decision_parity(scan, "synthetic scan")


def _check_native_hit_and_fitz_zero(tmp: Path) -> None:
    src = Path(_GOLDEN_DIR) / "1.pdf"
    native_calls = {"n": 0}
    saved_native = _install_native_counters(native_calls)
    fitz_calls = {"n": 0}
    saved_fitz = _install_fitz_counters(fitz_calls)
    try:
        render_mode.is_pseudo_editable_scan_pdf(src, 0, -1)
        render_mode.is_editable_pdf(src, 0, -1)
        render_mode.resolve_effective_render_mode(
            render_mode="auto",
            source_pdf_path=src,
            start_page=0,
            end_page=-1,
            translated_pages_map={0: [{"source_text": "probe", "bbox": [0, 0, 10, 10]}]},
        )
    finally:
        _restore_counters(saved_native)
        _restore_fitz_counters(saved_fitz)
    assert native_calls["n"] > 0, "render_mode production never hit a native bridge"
    assert fitz_calls["n"] == 0, f"render_mode native decision touched fitz {fitz_calls['n']} times"


def main() -> None:
    assert analysis_native.NATIVE, "render document analysis native module not built"
    _check_golden()
    with tempfile.TemporaryDirectory(prefix="rps-ba-") as tmp_dir:
        _check_synthetic(Path(tmp_dir))
        _check_native_hit_and_fitz_zero(Path(tmp_dir))
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
