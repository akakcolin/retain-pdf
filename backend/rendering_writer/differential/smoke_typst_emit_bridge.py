#!/usr/bin/env python3
"""Native bridge smoke test for the Typst output layer emit wiring (Phase A1).

Four parts:

1. Bridge parity: the new `_native.emit_typst_book_overlay_source` shim
   (bridge `emit_typst_book_overlay_source` -> Rust
   `source_builder::build_typst_book_overlay_source`) reproduces the pure-Python
   `source_builder.build_typst_book_overlay_source` byte-exactly for prebuilt
   `RenderBlock`s covering the branch surface (plain, plain_line + cover fill,
   inline-math fit_to_box, preserved line boxes, toc entries, multi-size/weight).
   Runs with both `include_cover_rect` variants. A call counter on the bridge
   confirms native was actually hit.

2. Production routing: `resolve_prebuilt_overlay_source` (the live overlay emit
   path behind `build_book_typst_pdf`/`build_dual_book_pdf`) with raw
   translated-item dicts and the real `build_render_blocks` layout pipeline
   writes byte-identical source with NATIVE on and off, and hits native on a
   cache miss.

3. Compile routing: `compile_typst_book_overlay_pdf` (whole-book fallback) and
   `compile_typst_overlay_pdf` (single-page per-item fallback) emit through the
   shim — the written `.typ` matches the reference and the bridge is hit. The
   actual typst CLI compile outcome is not asserted (it may need network
   packages / fonts); the exception is swallowed and only the written source is
   checked.

4. Render-pages regression: the already-wired `_native.emit_typst_source`
   (render-pages path, `compiler.py:327`) still reproduces the pure-Python
   `emitter.build_typst_source_from_page_specs` byte-exactly and hits native.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python \
        ../rendering_writer/differential/smoke_typst_emit_bridge.py
"""

import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.layout.model.models import RenderBlock  # noqa: E402
from services.rendering.layout.model.models import RenderLayoutBlock  # noqa: E402
from services.rendering.layout.model.models import RenderLineBox  # noqa: E402
from services.rendering.layout.model.models import RenderPageSpec  # noqa: E402
from services.rendering.layout.model.models import RenderTocEntry  # noqa: E402
from services.rendering.output.typst import _native  # noqa: E402
from services.rendering.output.typst import source_pages  # noqa: E402
from services.rendering.output.typst.emitter import build_typst_source_from_page_specs  # noqa: E402
from services.rendering.output.typst.source_builder import build_typst_book_overlay_source  # noqa: E402

FONT_FAMILY = "Source Han Serif SC"


def _rblock(block_id, **overrides) -> RenderBlock:
    fields = dict(
        block_id=block_id,
        bbox=[10.0, 20.0, 210.0, 70.0],
        cover_bbox=[10.0, 20.0, 210.0, 70.0],
        inner_bbox=[12.0, 22.0, 208.0, 68.0],
        markdown_text="",
        plain_text="",
        render_kind="plain",
        font_size_pt=10.0,
        leading_em=1.4,
    )
    fields.update(overrides)
    return RenderBlock(**fields)


def _overlay_page_specs() -> list:
    return [
        (612.0, 792.0, [_rblock("a", render_kind="plain", plain_text="Alpha")]),
        (
            612.0,
            792.0,
            [
                _rblock(
                    "b",
                    render_kind="plain_line",
                    markdown_text="Beta",
                    plain_text="Beta",
                    use_cover_fill=True,
                ),
                _rblock(
                    "c",
                    markdown_text=r"See $x^2$",
                    plain_text="See formula",
                    fit_to_box=True,
                    fit_single_line=True,
                    fit_min_font_size_pt=6.0,
                    fit_max_font_size_pt=12.0,
                    font_size_pt=11.0,
                    leading_em=1.5,
                    font_weight="bold",
                ),
            ],
        ),
        (
            612.0,
            792.0,
            [
                _rblock(
                    "d",
                    render_kind="plain",
                    markdown_text="line one\nline two",
                    plain_text="line one\nline two",
                    preserve_line_breaks=True,
                    preserved_line_boxes=[
                        RenderLineBox(text="line one", bbox=[12.0, 22.0, 150.0, 45.0]),
                        RenderLineBox(text="line two", bbox=[12.0, 48.0, 160.0, 68.0]),
                    ],
                ),
                _rblock(
                    "e",
                    render_kind="plain",
                    markdown_text="Chapter",
                    plain_text="Chapter",
                    toc_entries=[
                        RenderTocEntry(
                            title="Chapter",
                            page_label="12",
                            bbox=[12.0, 22.0, 208.0, 68.0],
                            number="1",
                            level=1,
                        )
                    ],
                ),
            ],
        ),
    ]


def _install_counter(calls: dict, attr: str) -> dict:
    """Wrap a native bridge import, counting hits into `calls`."""
    saved = getattr(_native, attr)
    setattr(_native, attr, saved)

    def counting(*args, _orig=saved, **kwargs):
        calls["n"] += 1
        return _orig(*args, **kwargs)

    setattr(_native, attr, counting)
    return saved


def _restore(attr: str, saved) -> None:
    setattr(_native, attr, saved)


def _build_render_blocks_passthrough(items, **kwargs):
    return items


def check_bridge_parity() -> None:
    assert _native.NATIVE, "typst native module not built"
    page_specs = _overlay_page_specs()
    # The pure-Python reference runs `build_render_blocks` on dict items; feeding
    # prebuilt RenderBlocks requires the corpus-style passthrough (the layout
    # pipeline is out of scope — the Rust emitter receives prebuilt RenderBlocks).
    original = source_pages.build_render_blocks
    source_pages.build_render_blocks = _build_render_blocks_passthrough
    try:
        for include_cover_rect in (False, True):
            calls = {"n": 0}
            saved = _install_counter(calls, "_native_emit_typst_book_overlay_source")
            try:
                native = _native.emit_typst_book_overlay_source(
                    page_specs=page_specs,
                    font_family=FONT_FAMILY,
                    include_cover_rect=include_cover_rect,
                )
            finally:
                _restore("_native_emit_typst_book_overlay_source", saved)
            ref = build_typst_book_overlay_source(
                page_specs,
                font_family=FONT_FAMILY,
                include_cover_rect=include_cover_rect,
            )
            assert calls["n"] > 0, "book-overlay emit never hit the native bridge"
            assert native == ref, f"book-overlay emit diverges (cover={include_cover_rect})"
    finally:
        source_pages.build_render_blocks = original


def _fresh_book_specs() -> list:
    """Raw translated-item dicts (production shape) with the `build_render_blocks`
    contract keys; a fresh copy per run so the layout pipeline's mutation does
    not leak between the NATIVE and reference runs."""
    return [
        (
            612.0,
            792.0,
            [
                {"item_id": "p001-b001", "bbox": [10.0, 20.0, 80.0, 60.0], "translated_text": "hello"},
                {"item_id": "p001-b002", "bbox": [10.0, 80.0, 200.0, 120.0], "translated_text": "world"},
            ],
        ),
        (
            612.0,
            792.0,
            [
                {
                    "item_id": "p002-b001",
                    "bbox": [20.0, 40.0, 120.0, 90.0],
                    "translated_text": r"formula $x^2$",
                }
            ],
        ),
    ]


def check_production_routing(tmp: Path) -> None:
    from services.rendering.output.typst.overlay_source_cache import resolve_prebuilt_overlay_source

    def emit_into(root: Path) -> Path:
        active_path, _elapsed = resolve_prebuilt_overlay_source(
            prebuilt_source_path=None,
            temp_root=root,
            stem="ovl",
            book_specs=_fresh_book_specs(),
            font_family=FONT_FAMILY,
            include_cover_rect=True,
        )
        assert active_path is not None and active_path.exists()
        return active_path

    calls = {"n": 0}
    saved = _install_counter(calls, "_native_emit_typst_book_overlay_source")
    try:
        native_path = emit_into(tmp / "native")
        assert calls["n"] > 0, "resolve_prebuilt_overlay_source never hit the native bridge"
    finally:
        _restore("_native_emit_typst_book_overlay_source", saved)

    was = _native.NATIVE
    _native.NATIVE = False
    try:
        ref_path = emit_into(tmp / "ref")
    finally:
        _native.NATIVE = was

    native_text = native_path.read_text(encoding="utf-8")
    ref_text = ref_path.read_text(encoding="utf-8")
    assert native_text == ref_text, "resolve_prebuilt_overlay_source native vs ref diverge"
    assert "hello" in native_text, "expected translated content in emitted source"


def _check_compile_emit(tmp: Path, invoke, page_specs, *, name, stem, include_cover_rect) -> None:
    """Run one `compiler.compile_typst_*_pdf` invocation (the caller supplies the
    exact argument shape via `invoke(work_dir)`) and verify the written `.typ`
    matches the reference and the native bridge was hit. The typst CLI compile
    (network packages / fonts) is not asserted — only the emit routing."""
    calls = {"n": 0}
    saved = _install_counter(calls, "_native_emit_typst_book_overlay_source")
    try:
        try:
            invoke(tmp)
        except Exception:
            pass  # compile outcome is out of scope; the .typ was written before it
        assert calls["n"] > 0, f"{name} never hit the native bridge"
    finally:
        _restore("_native_emit_typst_book_overlay_source", saved)
    written = (tmp / f"{stem}.typ").read_text(encoding="utf-8")
    ref = build_typst_book_overlay_source(
        page_specs,
        font_family=FONT_FAMILY,
        include_cover_rect=include_cover_rect,
    )
    assert written == ref, f"{name} wrote a diverging source"


def check_compile_routing(tmp: Path) -> None:
    from services.rendering.output.typst.compiler import compile_typst_book_overlay_pdf
    from services.rendering.output.typst.compiler import compile_typst_overlay_pdf

    original = source_pages.build_render_blocks
    source_pages.build_render_blocks = _build_render_blocks_passthrough
    try:
        page_specs = _overlay_page_specs()
        _check_compile_emit(
            tmp / "book",
            lambda work: compile_typst_book_overlay_pdf(
                page_specs=page_specs,
                stem="book-overlay",
                work_dir=work,
                include_cover_rect=True,
            ),
            page_specs,
            name="compile_typst_book_overlay_pdf",
            stem="book-overlay",
            include_cover_rect=True,
        )
        width, height, blocks = page_specs[0]
        single_specs = [(width, height, blocks)]
        _check_compile_emit(
            tmp / "single",
            lambda work: compile_typst_overlay_pdf(
                page_width=width,
                page_height=height,
                translated_items=blocks,
                stem="page-overlay",
                work_dir=work,
                include_cover_rect=False,
            ),
            single_specs,
            name="compile_typst_overlay_pdf",
            stem="page-overlay",
            include_cover_rect=False,
        )
    finally:
        source_pages.build_render_blocks = original


def _minimal_render_page_spec(bg_pdf_path: Path) -> RenderPageSpec:
    block = RenderLayoutBlock(
        block_id="b0",
        page_index=0,
        background_rect=[10.0, 20.0, 210.0, 70.0],
        content_rect=[12.0, 22.0, 208.0, 68.0],
        content_kind="markdown",
        content_text="Hello render pages",
        plain_text="Hello render pages",
        math_map=[],
        font_size_pt=10.0,
        leading_em=1.4,
    )
    return RenderPageSpec(
        page_index=0,
        page_width_pt=300.0,
        page_height_pt=400.0,
        background_pdf_path=bg_pdf_path,
        blocks=[block],
    )


def check_render_pages_regression(tmp: Path) -> None:
    import fitz  # noqa: E402

    bg = tmp / "bg.pdf"
    doc = fitz.open()
    doc.new_page(width=300.0, height=400.0)
    doc.save(bg)
    doc.close()

    spec = _minimal_render_page_spec(bg)
    calls = {"n": 0}
    saved = _install_counter(calls, "_native_emit_typst_source")
    try:
        native = _native.emit_typst_source(
            background_pdf_path=bg,
            page_specs=[spec],
            work_dir=tmp,
            font_family=FONT_FAMILY,
        )
    finally:
        _restore("_native_emit_typst_source", saved)
    ref = build_typst_source_from_page_specs(
        background_pdf_path=bg,
        page_specs=[spec],
        work_dir=tmp,
        font_family=FONT_FAMILY,
    )
    assert calls["n"] > 0, "render-pages emit never hit the native bridge"
    assert native == ref, "render-pages emit native vs ref diverge"


def main() -> None:
    assert _native.NATIVE, "typst native module not built"
    with tempfile.TemporaryDirectory(prefix="rps-typst-") as tmp_dir:
        tmp = Path(tmp_dir)
        check_bridge_parity()
        check_production_routing(tmp)
        check_compile_routing(tmp)
        check_render_pages_regression(tmp)
    print("all smoke tests pass")


if __name__ == "__main__":
    main()
