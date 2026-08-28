#!/usr/bin/env python3
"""End-to-end native-vs-python parity gate for the wired production overlay path.

Drives the REAL `book_renderer.build_book_typst_pdf` (the production overlay
render behind `workflow/modes.run_overlay_render`) with a small fitz-built
source PDF + production-shaped translated pages, once with every `_native` shim
NATIVE=True and once with all NATIVE=False, then asserts:

1. The overlay emit native emitter is actually hit end-to-end:
   `output/typst/_native.emit_typst_book_overlay_source` (Phase A1). (The
   background native emitter `build_clean_background_pdf` is a separate
   production path covered by `smoke_bridge.py`, not the overlay chain.)
2. The emitted whole-book overlay `.typ` is byte-identical native vs python
   (the A1 byte-exact emit guarantee, re-checked through the full production
   call path).
3. The final merged PDFs are semantically equivalent per page (words ±10%,
   ink ±0.02) — covering source prep + typst compile + pikepdf merge + save.

The typst CLI compiles for real; the emitted source is deterministic and the env
has the backend font dir. Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/smoke_end_to_end_parity.py
"""

import importlib
import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402

from services.rendering.output.typst import _native as _typst_native  # noqa: E402
from services.rendering.output.typst.book_renderer import build_book_typst_pdf  # noqa: E402

WORD_TOL = 0.10
INK_TOL = 0.02

_ALL_SHIM_MODULES = [
    "services.rendering.output.typst._native",
    "services.rendering.source._native",
    "services.rendering.source.background._native",
    "services.rendering.layout._native",
    "services.rendering.layout.payload._native",
    "services.rendering.visual_profile._native",
    "services.rendering.source_cleanup.planning._native",
]


def _set_native_all(value: bool) -> None:
    for module_path in _ALL_SHIM_MODULES:
        # Fail loudly if a shim module can't be resolved: a silent skip would
        # leave that shim native during the python run and the gate would pass
        # without ever testing its native-vs-python parity.
        importlib.import_module(module_path).NATIVE = value


def _install_counter(module, attr: str, calls: dict):
    saved = getattr(module, attr)

    def counting(*args, _orig=saved, **kwargs):
        calls["n"] += 1
        return _orig(*args, **kwargs)

    setattr(module, attr, counting)
    return saved


def _restore(module, attr: str, saved) -> None:
    setattr(module, attr, saved)


def page_facts(doc):
    facts = []
    for i in range(doc.page_count):
        page = doc[i]
        words = len(page.get_text("words"))
        pix = page.get_pixmap(
            matrix=fitz.Matrix(2.0, 2.0), colorspace=fitz.csGRAY, alpha=False
        )
        total = pix.width * pix.height
        dark = sum(1 for b in pix.samples if b < 250)
        facts.append({"words": words, "ink_ratio": (dark / total) if total else 0.0})
    return facts


def assert_close(actual, expected, label):
    assert len(actual) == len(expected), f"{label}: page count {len(actual)} != {len(expected)}"
    for i, (a, e) in enumerate(zip(actual, expected)):
        rel = abs(a["words"] - e["words"]) / max(e["words"], 1)
        assert rel <= WORD_TOL, f"{label} p{i}: words {a['words']} vs {e['words']}"
        ink = abs(a["ink_ratio"] - e["ink_ratio"])
        assert ink <= INK_TOL, f"{label} p{i}: ink {a['ink_ratio']:.6f} vs {e['ink_ratio']:.6f}"


def _build_source_pdf(source_pdf: Path) -> None:
    doc = fitz.open()
    page = doc.new_page(width=400, height=400)
    page.insert_text((40, 60), "Alpha Beta Gamma", fontsize=12)
    page = doc.new_page(width=400, height=400)
    page.insert_text((40, 60), "Delta Epsilon", fontsize=12)
    doc.save(source_pdf)
    doc.close()


def _fresh_pages() -> dict[int, list[dict]]:
    """Production-shaped translated pages. Fresh copy per run: the layout
    pipeline `build_render_blocks` mutates the dicts it consumes."""
    return {
        0: [
            {
                "item_id": "p001-b001",
                "bbox": [40.0, 40.0, 360.0, 80.0],
                "translated_text": "你好世界",
                "protected_translated_text": "你好世界",
            }
        ],
        1: [
            {
                "item_id": "p002-b001",
                "bbox": [40.0, 40.0, 360.0, 80.0],
                "translated_text": "再见世界",
                "protected_translated_text": "再见世界",
            }
        ],
    }


def _render_output(root: Path, source_pdf: Path) -> tuple[Path, str]:
    """Run the full production overlay render and return (output_pdf, emitted
    whole-book `.typ` source)."""
    out_pdf = root / "out.pdf"
    build_book_typst_pdf(
        source_pdf_path=source_pdf,
        output_pdf_path=out_pdf,
        translated_pages=_fresh_pages(),
        temp_root=root,
    )
    typ_path = root / "book-overlay-sources" / "book-overlay.typ.prebuilt"
    assert typ_path.exists(), f"emitted overlay source missing: {typ_path}"
    content = typ_path.read_bytes()
    # strip the 2 leading comment header lines (version marker + fingerprint)
    # to recover the raw emit output for the byte-exact comparison.
    first_nl = content.index(b"\n")
    second_nl = content.index(b"\n", first_nl + 1) + 1
    return out_pdf, content[second_nl:].decode("utf-8")


def check_end_to_end_parity() -> None:
    assert _typst_native.NATIVE, "typst native module not built"
    with tempfile.TemporaryDirectory(prefix="rps-e2e-") as tmp_dir:
        root = Path(tmp_dir)
        source_pdf = root / "source.pdf"
        _build_source_pdf(source_pdf)

        emit_calls = {"n": 0}
        saved_emit = _install_counter(
            _typst_native, "_native_emit_typst_book_overlay_source", emit_calls
        )
        try:
            native_out, native_typ = _render_output(root / "native", source_pdf)
        finally:
            _restore(_typst_native, "_native_emit_typst_book_overlay_source", saved_emit)
        assert emit_calls["n"] > 0, "overlay emit never hit the native bridge end-to-end"
        print(f"native hit: emit={emit_calls['n']}")

        _set_native_all(False)
        py_emit_calls = {"n": 0}
        saved_py_emit = _install_counter(
            _typst_native, "_native_emit_typst_book_overlay_source", py_emit_calls
        )
        try:
            py_out, py_typ = _render_output(root / "py", source_pdf)
        finally:
            _restore(_typst_native, "_native_emit_typst_book_overlay_source", saved_py_emit)
            _set_native_all(True)
        assert py_emit_calls["n"] == 0, "python run unexpectedly hit the native emit bridge"

        assert native_typ == py_typ, "emitted overlay .typ diverges native vs python"
        assert_close(
            page_facts(fitz.open(native_out)),
            page_facts(fitz.open(py_out)),
            "final pdf native vs python",
        )
        print("all end-to-end parity tests pass")


if __name__ == "__main__":
    check_end_to_end_parity()
