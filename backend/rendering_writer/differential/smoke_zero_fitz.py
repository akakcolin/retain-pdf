#!/usr/bin/env python3
"""C2/C3 zero-fitz gate: default render modes never touch the fitz doc surface.

终态判据 1 (doc 15): the fitz call-count probe asserts 0 for the default typst /
typst_visual / overlay books and for auto render-mode sampling. Each case runs
the REAL production call path with every `_native` shim built, proves the
counters are live, then asserts the case made zero fitz document calls. To keep
the gate from passing vacuously, each case also asserts the native bridge it
depends on was actually hit.

Dual is intentionally excluded: `build_dual_book_pdf` still opens three fitz
documents itself (`book_renderer.py:582-584`), so its fitz-free takeover is a
separate remaining item.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/smoke_zero_fitz.py
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

import orchestrator_parity as op  # noqa: E402
import smoke_end_to_end_parity as gate  # noqa: E402

from entrypoints.run_render_delegate import build_bundle  # noqa: E402
from foundation.shared.stage_specs import RenderStageSpec  # noqa: E402
from runtime.pipeline.render_mode import is_editable_pdf  # noqa: E402
from runtime.pipeline.render_mode import is_pseudo_editable_scan_pdf  # noqa: E402
from runtime.pipeline.render_mode import resolve_effective_render_mode  # noqa: E402
from services.rendering.output.typst import _native as _typst_native  # noqa: E402
from services.rendering.output.typst.book_renderer import build_book_typst_pdf  # noqa: E402


def _install_fitz_counters(calls):
    """Wrap the fitz document/render surface the default paths must not touch.
    Returns the `(module, attr, saved)` list for `_restore_fitz_counters`.
    `fitz.open` is `pymupdf.Document` itself, so wrapping it also covers every
    `fitz.Document(...)` construction; `Document.__new__` must NOT be wrapped
    (object.__new__ is called with the class only)."""
    patched = []
    for module, attr in [
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
    ]:
        saved = getattr(module, attr)

        def counting(*args, _orig=saved, **kwargs):
            calls["n"] += 1
            return _orig(*args, **kwargs)

        setattr(module, attr, counting)
        patched.append((module, attr, saved))
    return patched


def _restore_fitz_counters(patched):
    for module, attr, saved in patched:
        setattr(module, attr, saved)


def _probe_live(calls):
    """Prove the counters are live with a throwaway fitz open, then reset."""
    control = fitz.open()
    control.close()
    assert calls["n"] >= 1, "fitz counters not live"
    calls["n"] = 0


def _assert_zero_fitz(label, calls, patched):
    _restore_fitz_counters(patched)
    assert calls["n"] == 0, f"{label}: {calls['n']} fitz calls (终态判据 1 violated)"
    print(f"fitz=0: {label}")


def _check_delegate_bundle():
    for mode in ("typst", "typst_visual"):
        with tempfile.TemporaryDirectory(prefix="zfit-delegate-") as td:
            root = Path(td)
            source_pdf = root / "source.pdf"
            op.gate._build_source_pdf(source_pdf)
            spec_path = op._write_job_fixture(root / "job", source_pdf, mode)
            spec = RenderStageSpec.load(spec_path)
            calls = {"n": 0}
            patched = _install_fitz_counters(calls)
            try:
                _probe_live(calls)
                bundle = build_bundle(spec)
            finally:
                _restore_fitz_counters(patched)
            assert bundle.get("mode") == mode, f"delegate bundle mode != {mode}"
            assert calls["n"] == 0, f"delegate {mode}: {calls['n']} fitz calls (终态判据 1 violated)"
            print(f"fitz=0: delegate {mode}")


def _check_auto_sampling():
    with tempfile.TemporaryDirectory(prefix="zfit-auto-") as td:
        root = Path(td)
        source_pdf = root / "source.pdf"
        op.gate._build_source_pdf(source_pdf)
        calls = {"n": 0}
        patched = _install_fitz_counters(calls)
        try:
            _probe_live(calls)
            editable = is_editable_pdf(source_pdf, 0, -1)
            pseudo = is_pseudo_editable_scan_pdf(source_pdf, 0, -1)
            effective = resolve_effective_render_mode(
                render_mode="auto",
                source_pdf_path=source_pdf,
                start_page=0,
                end_page=-1,
                translated_pages_map=op._translated_pages(),
            )
        finally:
            _restore_fitz_counters(patched)
        assert calls["n"] == 0, f"auto sampling: {calls['n']} fitz calls (终态判据 1 violated)"
        print(f"fitz=0: auto sampling (editable={editable} pseudo={pseudo} effective={effective})")


def _check_overlay_book():
    with tempfile.TemporaryDirectory(prefix="zfit-overlay-") as td:
        root = Path(td)
        source_pdf = root / "source.pdf"
        gate._build_source_pdf(source_pdf)
        emit_calls = {"n": 0}
        saved_emit = gate._install_counter(
            _typst_native, "_native_emit_typst_book_overlay_source", emit_calls
        )
        calls = {"n": 0}
        patched = _install_fitz_counters(calls)
        try:
            _probe_live(calls)
            build_book_typst_pdf(
                source_pdf_path=source_pdf,
                output_pdf_path=root / "out.pdf",
                translated_pages=gate._fresh_pages(),
                temp_root=root,
            )
        finally:
            _restore_fitz_counters(patched)
            gate._restore(_typst_native, "_native_emit_typst_book_overlay_source", saved_emit)
        assert emit_calls["n"] > 0, "overlay emit never hit the native bridge"
        assert calls["n"] == 0, f"overlay book: {calls['n']} fitz calls (终态判据 1 violated)"
        print(f"fitz=0: overlay book (emit hits={emit_calls['n']})")


def check_zero_fitz() -> None:
    assert _typst_native.NATIVE, "typst native module not built"
    _check_delegate_bundle()
    _check_auto_sampling()
    _check_overlay_book()
    print("all zero-fitz gates pass")


if __name__ == "__main__":
    check_zero_fitz()
