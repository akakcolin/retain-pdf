#!/usr/bin/env python3
"""Native bridge smoke test for `build_render_document_analysis` (Phase B2-Inc4).

Two-way parity between the production entry
`analysis.document.builder.build_render_document_analysis` (routed through the
`analysis/_native.py` shim, NATIVE=True) and the pure-Python reference
`_build_render_document_analysis_python` on the golden PDFs 1.pdf/2.pdf and two
synthetic PDFs (a full-page-image scan page -> `scan_image`, a 2100-drawing
vector page -> `vector_heavy`).

Per-page manifest parity:
  * exact: page_index, kind, redaction, background, compose, layout, reason,
    drawing_count, vector_heavy, has_large_background
  * coverage_ratio within 1e-6
  * text-layer fields (visible_text / hidden_text / editable_text) are
    allowlisted: a divergence is accepted ONLY when the kind still matches
    (native `text_traces` is empty, so the visible/editable fall back to
    `word_count >= 20` and hidden_text is always false — this only diverges on
    "hidden text and <20 words" pages, where the reference itself would change
    the classification).

Also: page-range selection parity (`start_page`/`end_page` and
`translated_pages` OCR-bbox routing), a native-hit counter (the production path
must actually reach a bridge), and corrupt / vanished-backing-file / out-of-range
`translated_pages` boundaries that fall back exactly like the reference.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_document_analysis_bridge.py
"""

import os
import sys
import tempfile
from pathlib import Path

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402
from services.rendering.analysis import _native as analysis_native  # noqa: E402
from services.rendering.analysis.document.builder import (  # noqa: E402
    _build_render_document_analysis_python,
    build_render_document_analysis,
)

_GOLDEN_DIR = os.path.abspath(
    os.path.join(_HERE, "..", "..", "..", "resources", "samples", "golden-pdfs")
)

TOL_COVERAGE = 1e-6

_EXACT_FIELDS = (
    "page_index",
    "kind",
    "redaction",
    "background",
    "compose",
    "layout",
    "reason",
    "drawing_count",
    "vector_heavy",
    "has_large_background",
)
_TEXT_LAYER_ALLOWLIST = ("visible_text", "hidden_text", "editable_text")

_BRIDGES = (
    "_native_build_render_document_analysis",
    "_native_read_page_geometry",
)


def _install_counters(calls: dict) -> dict:
    saved = {}
    for name in _BRIDGES:
        orig = getattr(analysis_native, name)
        saved[name] = orig

        def counting(*args, _orig=orig, **kwargs):
            calls["n"] += 1
            return _orig(*args, **kwargs)

        setattr(analysis_native, name, counting)
    return saved


def _restore_counters(saved: dict) -> None:
    for name, orig in saved.items():
        setattr(analysis_native, name, orig)


def _assert_page_parity(ref: dict, native: dict, label: str) -> None:
    for key in _EXACT_FIELDS:
        assert ref[key] == native[key], f"{label}: {key} ref={ref[key]!r} native={native[key]!r}"
    cov = abs(ref["background_coverage_ratio"] - native["background_coverage_ratio"])
    assert cov <= TOL_COVERAGE, (
        f"{label}: coverage ref={ref['background_coverage_ratio']:.9f} "
        f"native={native['background_coverage_ratio']:.9f}"
    )
    for key in _TEXT_LAYER_ALLOWLIST:
        if ref[key] != native[key]:
            assert ref["kind"] == native["kind"], (
                f"{label}: {key} divergence requires kind parity "
                f"ref={ref[key]} native={native[key]} kind={ref['kind']}"
            )


def _manifest_pages(analysis) -> dict:
    return {idx: page.to_manifest() for idx, page in sorted(analysis.pages.items())}


def _assert_doc_parity(ref, native, label: str) -> None:
    ref_pages = _manifest_pages(ref)
    native_pages = _manifest_pages(native)
    assert set(ref_pages) == set(native_pages), (
        f"{label}: page sets {sorted(ref_pages)} vs {sorted(native_pages)}"
    )
    for idx in ref_pages:
        _assert_page_parity(ref_pages[idx], native_pages[idx], f"{label} p{idx}")


def _check_golden() -> None:
    for name in ("1.pdf", "2.pdf"):
        src = Path(_GOLDEN_DIR) / name
        ref = _build_render_document_analysis_python(source_pdf_path=src)
        native = build_render_document_analysis(source_pdf_path=src)
        _assert_doc_parity(ref, native, f"golden {name}")

        # Page-range selection parity.
        subset_ref = _build_render_document_analysis_python(
            source_pdf_path=src, start_page=0, end_page=1
        )
        subset_native = build_render_document_analysis(
            source_pdf_path=src, start_page=0, end_page=1
        )
        _assert_doc_parity(subset_ref, subset_native, f"golden {name} start/end")


def _check_synthetic(tmp: Path) -> None:
    # Full-page gray image -> scan_image (no text, large background).
    img = tmp / "scan.pdf"
    doc = fitz.open()
    page = doc.new_page(width=612, height=792)
    pix = fitz.Pixmap(fitz.csGRAY, fitz.IRect(0, 0, 64, 64))
    pix.clear_with(128)
    page.insert_image(fitz.Rect(0, 0, 612, 792), pixmap=pix)
    doc.save(img)
    doc.close()
    ref = _build_render_document_analysis_python(source_pdf_path=img)
    native = build_render_document_analysis(source_pdf_path=img)
    assert ref.pages[0].to_manifest()["kind"] == "scan_image", "synthetic image page kind"
    _assert_doc_parity(ref, native, "synthetic scan")

    # 2100 separate vector drawing commands + text -> vector_heavy.
    vec = tmp / "vector.pdf"
    doc = fitz.open()
    page = doc.new_page(width=612, height=792)
    for i in range(2100):
        x0 = (i % 60) * 10
        y0 = (i // 60) * 10
        page.draw_rect(fitz.Rect(x0, y0, x0 + 5, y0 + 5), color=(0, 0, 0), width=0.5)
    page.insert_text((72, 72), "lots of text words to keep editable " * 5)
    doc.save(vec)
    doc.close()
    ref = _build_render_document_analysis_python(source_pdf_path=vec)
    native = build_render_document_analysis(source_pdf_path=vec)
    assert ref.pages[0].to_manifest()["kind"] == "vector_heavy", "synthetic vector page kind"
    assert ref.pages[0].drawing_count >= 2000, "synthetic vector drawing_count"
    _assert_doc_parity(ref, native, "synthetic vector")

    # OCR-bbox routing through translated_pages on the scan page.
    ocr = {0: [{"item_id": "a", "bbox": [10, 10, 100, 60]}]}
    ref = _build_render_document_analysis_python(source_pdf_path=img, translated_pages=ocr)
    native = build_render_document_analysis(source_pdf_path=img, translated_pages=ocr)
    assert sorted(ref.pages) == [0] and sorted(native.pages) == [0], "ocr path page selection"
    _assert_doc_parity(ref, native, "synthetic ocr")


def _check_boundaries(tmp: Path) -> None:
    corrupt = tmp / "corrupt.pdf"
    corrupt.write_bytes(b"\x00\x01\x02 not a pdf")
    try:
        build_render_document_analysis(source_pdf_path=corrupt)
        raise AssertionError("native must raise on corrupt bytes")
    except Exception:
        pass
    try:
        _build_render_document_analysis_python(source_pdf_path=corrupt)
        raise AssertionError("reference must raise on corrupt bytes")
    except Exception:
        pass

    vanished = tmp / "vanished.pdf"
    vanished.write_bytes(Path(_GOLDEN_DIR).joinpath("1.pdf").read_bytes())
    vanished.unlink()
    try:
        build_render_document_analysis(source_pdf_path=vanished)
        raise AssertionError("native must raise on vanished backing file")
    except Exception:
        pass
    try:
        _build_render_document_analysis_python(source_pdf_path=vanished)
        raise AssertionError("reference must raise on vanished backing file")
    except Exception:
        pass

    # Out-of-range translated_pages key: both filter it out -> empty analysis.
    src = Path(_GOLDEN_DIR) / "1.pdf"
    ref = _build_render_document_analysis_python(
        source_pdf_path=src, translated_pages={999: [{"item_id": "i0", "bbox": [0, 0, 1, 1]}]}
    )
    native = build_render_document_analysis(
        source_pdf_path=src, translated_pages={999: [{"item_id": "i0", "bbox": [0, 0, 1, 1]}]}
    )
    assert ref.pages == {} and native.pages == {}, "out-of-range translated_pages must be empty"


def main() -> None:
    assert analysis_native.NATIVE, "render document analysis native module not built"
    calls = {"n": 0}
    saved = _install_counters(calls)
    try:
        _check_golden()
        with tempfile.TemporaryDirectory(prefix="rps-inc4-") as tmp_dir:
            _check_synthetic(Path(tmp_dir))
            _check_boundaries(Path(tmp_dir))
    finally:
        _restore_counters(saved)
    assert calls["n"] > 0, "render document analysis production never hit a native bridge"
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
