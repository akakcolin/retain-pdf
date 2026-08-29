#!/usr/bin/env python3
"""Native bridge smoke test for the layout page-specs size read (Phase B2-1).

Three parts:

1. Native availability: the layout shim `_native` reports NATIVE (maturin
   build installed).

2. Size parity: for a synthetic rotated multi-page PDF, the native
   `_native.read_source_page_sizes` equals the pure-Python reference
   `page_specs._read_source_page_sizes_python`, and both equal fitz
   `page.rect` width/height per page (out-of-range indices are skipped).

3. End-to-end parity: `build_render_page_specs(source_pdf_path=...)` (routes
   the size read through the shim -> native) produces the same `RenderPageSpec`
   list as the same call with `page_size_lookup` computed by the reference.
   `RETAIN_RENDER_TYPOGRAPHY_MEMORY=0` keeps the two runs independent.

4. Layout-pipeline parity (C3-N6): `build_render_page_specs`'s payload chain
   (`build_block_payloads` / `apply_body_pipeline` / `mark_adjacent_collision_risk`
   / `emit_render_blocks`) is routed through the native shims. Running the build
   with the payload shims native yields byte-exact `RenderPageSpec` lists versus
   `NATIVE=False` (the pure-Python reference), registers native hits for every
   routed stage, and records no `layout_payload` fallbacks.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_page_specs_bridge.py
"""

import copy
import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402

from services.rendering.layout import _native  # noqa: E402
from services.rendering.layout.page_specs import (  # noqa: E402
    _read_source_page_sizes_python,
    build_render_page_specs,
)


def build_source_pdf(pages) -> bytes:
    """`pages`: list of `(width, height, rotation)` tuples."""
    doc = fitz.open()
    for width, height, rotation in pages:
        page = doc.new_page(width=width, height=height)
        if rotation:
            page.set_rotation(rotation)
        page.insert_text((72.0, 72.0), f"page {len(doc)}", fontsize=12, fontname="helv")
    raw = doc.tobytes(garbage=0)
    doc.close()
    return raw


def translated_pages_for(page_count: int) -> dict[int, list[dict]]:
    return {
        idx: [
            {
                "item_id": f"p{idx:03d}-b001",
                "page_idx": idx,
                "block_type": "text",
                "bbox": [60.0, 80.0 + 40 * idx, 300.0, 100.0 + 40 * idx],
                "lines": [{"text": f"Line {idx}"}],
                "source_text": f"Line {idx}",
                "protected_source_text": f"Line {idx}",
                "protected_translated_text": f"译文 {idx}",
                "formula_map": [],
            }
        ]
        for idx in range(page_count)
    }


def main() -> None:
    assert _native.NATIVE, "native module not built"

    raw = build_source_pdf(
        [
            (612.0, 792.0, 0),
            (612.0, 792.0, 90),
            (595.0, 842.0, 270),
        ]
    )
    with tempfile.TemporaryDirectory(prefix="rps-pspec-") as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(raw)
        indices = [0, 1, 2, 3]  # 3 is out of range -> skipped by both paths

        native_sizes = _native.read_source_page_sizes(source_pdf_path=src, page_indices=indices)
        ref_sizes = _read_source_page_sizes_python(source_pdf_path=src, page_indices=indices)
        assert native_sizes == ref_sizes, f"native sizes {native_sizes} != reference {ref_sizes}"

        d = fitz.open(stream=raw, filetype="pdf")
        fitz_sizes = {
            i: (float(d[i].rect.width), float(d[i].rect.height)) for i in range(d.page_count)
        }
        d.close()
        assert native_sizes == fitz_sizes, f"native sizes {native_sizes} != fitz {fitz_sizes}"

        translated_pages = translated_pages_for(3)
        specs_via_shim = build_render_page_specs(source_pdf_path=src, translated_pages=translated_pages)
        specs_via_lookup = build_render_page_specs(
            source_pdf_path=src,
            translated_pages=translated_pages,
            page_size_lookup=ref_sizes,
        )
        assert len(specs_via_shim) == len(specs_via_lookup) == 3
        for via_shim, via_lookup in zip(specs_via_shim, specs_via_lookup):
            assert via_shim.page_index == via_lookup.page_index
            assert abs(via_shim.page_width_pt - via_lookup.page_width_pt) < 0.01
            assert abs(via_shim.page_height_pt - via_lookup.page_height_pt) < 0.01
            assert via_shim.blocks == via_lookup.blocks

        # 4. Layout-pipeline parity: native vs NATIVE=False must be byte-exact,
        # with a native hit for every routed stage and no layout_payload fallback.
        from services.rendering import _routing
        from services.rendering.layout.payload import _native as _payload_native

        _routing.reset()
        specs_native = build_render_page_specs(
            source_pdf_path=src,
            translated_pages=copy.deepcopy(translated_pages),
        )
        snap = _routing.snapshot()
        assert snap["hits"].get("layout_payload", 0) >= 12, (
            f"page_specs pipeline hit {snap['hits'].get('layout_payload', 0)} native"
            " stages, expected >= 12 (build_block_payloads/apply_body_pipeline/"
            "mark_adjacent_collision_risk/emit_render_blocks per page + resolve)"
        )
        assert "layout_payload" not in snap["fallbacks"], (
            f"page_specs pipeline recorded layout_payload fallbacks {snap['fallbacks'].get('layout_payload')}"
        )

        was = _payload_native.NATIVE
        _payload_native.NATIVE = False
        try:
            specs_ref = build_render_page_specs(
                source_pdf_path=src,
                translated_pages=copy.deepcopy(translated_pages),
            )
        finally:
            _payload_native.NATIVE = was
        assert len(specs_native) == len(specs_ref) == 3
        for via_native, via_ref in zip(specs_native, specs_ref):
            assert via_native == via_ref, (
                f"page_specs layout pipeline diverged native vs reference on page {via_native.page_index}"
            )

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
