from __future__ import annotations

import os
import sys
from pathlib import Path

import pytest


REPO_SCRIPTS_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_SCRIPTS_ROOT))

_REPO_ROOT = REPO_SCRIPTS_ROOT.parent
_FONTS_DIR = _REPO_ROOT / "fonts"

import fitz  # noqa: E402

from services.rendering import _routing  # noqa: E402
from services.rendering.output.typst import shared as _typst_shared  # noqa: E402
from services.rendering.output.typst.book_renderer import build_book_typst_background_pdf  # noqa: E402
from services.rendering.output.typst.book_renderer import build_book_typst_pdf  # noqa: E402

# The fitz document/render surface the default render modes must not touch
# (终态判据 1). `fitz.open` / `Document.tobytes` remain as the accepted
# materialization boundary feeding the native TOC-copy/save bridge; pymupdf
# implements `tobytes()` via `write()` -> `save()`, so `Document.save` is
# aliased by `tobytes`.
_FULL_SURFACE = [
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
    (fitz.Page, "get_cdrawings"),
]


class _Counter:
    def __init__(self) -> None:
        self.n = 0
        self.by_attr: dict[str, int] = {}

    def install(self) -> list[tuple[object, str, object]]:
        patched: list[tuple[object, str, object]] = []
        for module, attr in _FULL_SURFACE:
            saved = getattr(module, attr)
            name = f"{module.__name__}.{attr}"

            def counting(*args, _orig=saved, _name=name, _counter=self, **kwargs):
                _counter.n += 1
                _counter.by_attr[_name] = _counter.by_attr.get(_name, 0) + 1
                return _orig(*args, **kwargs)

            setattr(module, attr, counting)
            patched.append((module, attr, saved))
        return patched

    @staticmethod
    def restore(patched: list[tuple[object, str, object]]) -> None:
        for module, attr, saved in patched:
            setattr(module, attr, saved)

    def reset(self) -> None:
        self.n = 0
        self.by_attr = {}

    def prove_live(self) -> None:
        control = fitz.open()
        control.close()
        assert self.n >= 1, "fitz counters not live"
        self.reset()


@pytest.fixture(autouse=True)
def _typst_runtime_env() -> None:
    os.environ.setdefault("RETAIN_PDF_TYPST_FONT_DIRS", str(_FONTS_DIR))
    os.environ.setdefault("RETAIN_PDF_TYPST_FONT_FAMILY", "Source Han Serif SC")
    if not Path(_typst_shared.TYPST_BIN).exists() or not _FONTS_DIR.is_dir():
        pytest.skip("typst runtime not available (TYPST_BIN or fonts dir missing)")


def _source_pdf(path: Path) -> None:
    doc = fitz.open()
    page = doc.new_page(width=300, height=400)
    page.insert_textbox(
        fitz.Rect(30, 60, 270, 220),
        "Intermolecular Heck Coupling with Hindered Alkenes",
        fontsize=14,
    )
    doc.save(path)
    doc.close()


def _translated_pages() -> dict[int, list[dict]]:
    return {
        0: [
            {
                "item_id": "b1",
                "bbox": [25.0, 50.0, 275.0, 230.0],
                "source_text": "Intermolecular Heck Coupling with Hindered Alkenes",
                "translated_text": "受阻烯烃分子间Heck偶联",
                "protected_translated_text": "受阻烯烃分子间Heck偶联",
                "formula_map": [],
            }
        ]
    }


def test_default_overlay_book_zero_fitz(tmp_path: Path) -> None:
    # Auto mode for editable PDFs resolves to overlay; the real production overlay
    # book path (`build_book_typst_pdf` -> pikepdf merge, `use_typst_overlay_fill_only`
    # hardcoded True) must make zero fitz calls on the full surface.
    source_pdf = tmp_path / "source.pdf"
    _source_pdf(source_pdf)
    counter = _Counter()
    patched = counter.install()
    before = _routing.snapshot()
    try:
        counter.prove_live()
        build_book_typst_pdf(
            source_pdf_path=source_pdf,
            output_pdf_path=tmp_path / "out.pdf",
            translated_pages=_translated_pages(),
        )
    finally:
        _Counter.restore(patched)
    after = _routing.snapshot()
    assert counter.n == 0, f"default overlay book: {counter.by_attr} fitz calls"
    assert after["total_hits"] > before["total_hits"], "native bridge never hit"
    assert (tmp_path / "out.pdf").exists()


def test_background_typst_book_work_surface_zero_fitz(tmp_path: Path) -> None:
    # typst / typst_visual background mode (auto mode for scans + explicit typst):
    # the native-replaceable work surface must be untouched; the only allowed fitz
    # calls are the accepted materialization boundaries in
    # `save_background_pdf_to_output` (`fitz.open` x2, `tobytes` x3) feeding the
    # native TOC-copy + save bridge.
    source_pdf = tmp_path / "source.pdf"
    _source_pdf(source_pdf)
    counter = _Counter()
    patched = counter.install()
    before = _routing.snapshot()
    try:
        counter.prove_live()
        build_book_typst_background_pdf(
            source_pdf_path=source_pdf,
            output_pdf_path=tmp_path / "out.pdf",
            translated_pages=_translated_pages(),
        )
    finally:
        _Counter.restore(patched)
    after = _routing.snapshot()
    work = {
        name: count
        for name, count in counter.by_attr.items()
        if name not in {"fitz.open", "Document.tobytes", "Document.save"}
    }
    assert work == {}, f"background book work surface: {work}"
    assert counter.by_attr.get("Document.save", 0) <= counter.by_attr.get("Document.tobytes", 0)
    assert counter.by_attr.get("fitz.open", 0) <= 2
    assert counter.by_attr.get("Document.tobytes", 0) <= 3
    assert after["total_hits"] > before["total_hits"], "native bridge never hit"
    assert (tmp_path / "out.pdf").exists()
