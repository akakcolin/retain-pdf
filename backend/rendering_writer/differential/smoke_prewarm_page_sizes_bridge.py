#!/usr/bin/env python3
"""Native bridge smoke test for the prewarm page-count / page-width read (Phase B2-6).

Three parts:

1. Native availability: `source._native.NATIVE` (maturin build installed).

2. Three-way parity: `source._native.read_page_sizes_and_count` NATIVE ==
   NATIVE=False (pure-Python reference) == fitz `len(doc)` + `page.rect.width`
   on synthetic PDFs (multi-page, rotated page, cropbox-offset page) and the
   golden PDFs `resources/samples/golden-pdfs/{1,2,3}.pdf`. Page counts must
   match exactly; per-page widths within 1e-4 pt.

3. Boundary + end-to-end: an empty (0-byte) file and corrupt bytes both return
   `(0, {})` in every path without raising; `build_payload_prewarm` on a 3-page
   doc records `pixmap_reason` from the shim-sourced page count
   (default_disabled without env, env_enabled with `RETAIN_RENDER_PIXMAP_INDENT=1`).

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_prewarm_page_sizes_bridge.py
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

from foundation.config import layout  # noqa: E402
from services.rendering.source import _native  # noqa: E402
from services.rendering.source.prewarm_payload import (  # noqa: E402
    _read_source_page_sizes_and_count_python,
    build_payload_prewarm,
)

WIDTH_TOL = 1e-4
GOLDEN_ROOT = os.path.abspath(
    os.path.join(_HERE, "..", "..", "..", "resources", "samples", "golden-pdfs")
)


def build_synthetic_pdf() -> bytes:
    """3 pages: plain, rotated 90, cropbox-offset."""
    doc = fitz.open()
    p0 = doc.new_page(width=612.0, height=792.0)
    p0.insert_text((72.0, 72.0), "page 0", fontname="helv")
    p1 = doc.new_page(width=612.0, height=792.0)
    p1.set_rotation(90)
    p1.insert_text((72.0, 72.0), "page 1", fontname="helv")
    p2 = doc.new_page(width=595.0, height=842.0)
    p2.set_cropbox(fitz.Rect(26.0, 1.5, 580.0, 800.0))
    p2.insert_text((72.0, 72.0), "page 2", fontname="helv")
    raw = doc.tobytes(garbage=0)
    doc.close()
    return raw


def read_fitz_widths(raw: bytes) -> tuple[int, dict[int, float]]:
    doc = fitz.open(stream=raw, filetype="pdf")
    try:
        return doc.page_count, {i: float(doc[i].rect.width) for i in range(doc.page_count)}
    finally:
        doc.close()


def check_three_way(src: Path) -> None:
    native_count, native_widths = _native.read_page_sizes_and_count(source_pdf_path=src)
    was = _native.NATIVE
    _native.NATIVE = False
    try:
        ref_count, ref_widths = _native.read_page_sizes_and_count(source_pdf_path=src)
    finally:
        _native.NATIVE = was
    fitz_count, fitz_widths = read_fitz_widths(src.read_bytes())

    assert native_count == ref_count == fitz_count, (
        f"page_count {native_count} vs {ref_count} vs {fitz_count}"
    )
    assert native_widths.keys() == ref_widths.keys() == fitz_widths.keys(), (
        f"page keys {sorted(native_widths)} vs {sorted(ref_widths)} vs {sorted(fitz_widths)}"
    )
    for idx in fitz_widths:
        n, r, f = native_widths[idx], ref_widths[idx], fitz_widths[idx]
        assert max(n, r, f) - min(n, r, f) <= WIDTH_TOL, (
            f"{src.name} p{idx} width {n} vs {r} vs {f}"
        )


def _pixmap_candidate_item() -> dict:
    return {
        "item_id": "p001-b001",
        "block_type": "text",
        "block_kind": "text",
        "layout_role": "paragraph",
        "semantic_role": "body",
        "structure_role": "body",
        "bbox": [18.0, 30.0, 210.0, 88.0],
        "source_text": "First line second line third line",
        "protected_source_text": "First line second line third line",
        "lines": [],
    }


def main() -> None:
    assert _native.NATIVE, "prewarm page-sizes native module not built"

    raw = build_synthetic_pdf()
    with tempfile.TemporaryDirectory(prefix="rps-prewarm-") as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(raw)
        check_three_way(src)

        empty_src = Path(tmp) / "empty.pdf"
        empty_src.write_bytes(b"")
        assert _native.read_page_sizes_and_count(source_pdf_path=empty_src) == (0, {})
        assert _read_source_page_sizes_and_count_python(source_pdf_path=empty_src) == (0, {})

        corrupt_src = Path(tmp) / "corrupt.pdf"
        corrupt_src.write_bytes(b"\x00\x01\x02 not a pdf")
        assert _native.read_page_sizes_and_count(source_pdf_path=corrupt_src) == (0, {})
        was = _native.NATIVE
        _native.NATIVE = False
        try:
            assert _native.read_page_sizes_and_count(source_pdf_path=corrupt_src) == (0, {})
        finally:
            _native.NATIVE = was
        assert _read_source_page_sizes_and_count_python(source_pdf_path=corrupt_src) == (0, {})

    for name in ["1.pdf", "2.pdf", "3.pdf"]:
        golden = Path(GOLDEN_ROOT) / name
        assert golden.exists(), f"golden {golden} missing"
        check_three_way(golden)

    with tempfile.TemporaryDirectory(prefix="rps-prewarm-e2e-") as tmp:
        root = Path(tmp)
        source_pdf = root / "source.pdf"
        manifest_path = root / "artifacts" / "render_prewarm" / "render_source_prewarm_manifest.json"
        manifest_path.parent.mkdir(parents=True)
        doc = fitz.open()
        for _ in range(3):
            doc.new_page(width=200, height=200)
        doc.save(source_pdf)
        doc.close()

        os.environ.pop("RETAIN_RENDER_PIXMAP_INDENT", None)
        payload = build_payload_prewarm(
            source_pdf_path=source_pdf,
            translated_pages={0: [_pixmap_candidate_item()]},
            manifest_path=manifest_path,
            effective_render_mode="overlay",
            source_cleanup_strategy=layout.SOURCE_CLEANUP_TYPST_FILL,
        )
        diag = payload["first_line_indent_diagnostics"]
        assert diag["pixmap_enabled"] is False and diag["pixmap_reason"] == "default_disabled", diag

        os.environ["RETAIN_RENDER_PIXMAP_INDENT"] = "1"
        payload = build_payload_prewarm(
            source_pdf_path=source_pdf,
            translated_pages={0: [_pixmap_candidate_item()]},
            manifest_path=manifest_path,
            effective_render_mode="overlay",
            source_cleanup_strategy=layout.SOURCE_CLEANUP_TYPST_FILL,
        )
        diag = payload["first_line_indent_diagnostics"]
        assert diag["pixmap_enabled"] is True and diag["pixmap_reason"] == "env_enabled", diag

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
