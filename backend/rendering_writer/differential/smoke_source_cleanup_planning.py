#!/usr/bin/env python3
"""Native bridge smoke test for source-cleanup planning (Phase B2-5).

Exercises the five planning context primitives (bboxlog, content-stream size,
form-xobject flag, page ctm, page rect) through the pyo3 bridge and the
end-to-end `plan_source_cleanup` / `item_ids_with_uncovered_unsafe_vector_overlap`
routing.

Parts:

1. Native availability: `source_cleanup.planning._native.NATIVE` (maturin build).

2. Three-way context parity: a synthetic PDF covering an unrotated text+vector
   page, a 90-degree-rotated page, an annotation page, a cropbox-offset page, a
   form-xobject page, a full-page-image page, a stroke page, and an empty page.
   Per page `_native.build_page_contexts` NATIVE == NATIVE=False == fitz
   reference: page_rect exact, inverse_ctm / bboxlog rects within 1e-4, bboxlog
   kinds exact, content_stream_size exact, has_form_xobjects exact.

3. End-to-end parity: `plan_source_cleanup` NATIVE == NATIVE=False ==
   `_plan_source_cleanup_python`, comparing strip rects / protected rects
   (within 1e-4), uncovered ids, skip sets, and page features exactly.

4. Uncovered-id parity: `item_ids_with_uncovered_unsafe_vector_overlap`
   NATIVE == NATIVE=False == `_item_ids_with_uncovered_unsafe_vector_overlap_python`.

5. Boundaries: out-of-range page 99 absent; the empty page emits no strip rects;
   the form-xobject page routes through `_plan_form_xobject_page` (strip rects
   present); the full-page-image page is skipped as visual background; the stroke
   page's caption item (and p0's caption over the text-like fill path) is
   reported uncovered while p0's paragraph item is not.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_source_cleanup_planning.py
"""

import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_PIXMAP_INDENT"] = "1"
os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402

from services.rendering.source_cleanup.planning import _native  # noqa: E402
from services.rendering.source_cleanup.planning.page_context import (  # noqa: E402
    _build_page_contexts_python,
)
from services.rendering.source_cleanup.planning.planner import (  # noqa: E402
    _item_ids_with_uncovered_unsafe_vector_overlap_python,
    _plan_source_cleanup_python,
    item_ids_with_uncovered_unsafe_vector_overlap,
    plan_source_cleanup,
)

PAGE_WIDTH = 612.0
PAGE_HEIGHT = 792.0
RECT_TOL = 1e-4


def build_source_pdf() -> bytes:
    """Synthetic pages covering every planning geometry risk:
    p0 unrotated text + text-like fill path (unsafe vector) + large fill path
    (ignored); p1 rotation 90; p2 highlight + strikeout annotations; p3 cropbox
    offset; p4 embedded Form XObject; p5 full-page image + small text; p6 stroke
    line; p7 empty page."""
    doc = fitz.open()

    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page.insert_text((50.0, 100.0), "This is some body text to strip.", fontsize=16, fontname="helv")
    page.draw_rect(fitz.Rect(60.0, 96.0, 120.0, 116.0), color=None, fill=(0.1, 0.1, 0.1))
    page.draw_rect(fitz.Rect(300.0, 300.0, 500.0, 400.0), color=None, fill=(0.6, 0.6, 0.6))

    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page.insert_text((50.0, 100.0), "Rotated page text.", fontsize=16, fontname="helv")
    page.set_rotation(90)

    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page.insert_text((50.0, 100.0), "Annotated body text.", fontsize=16, fontname="helv")
    annot_rect = fitz.Rect(50.0, 88.0, 220.0, 106.0)
    page.add_highlight_annot(annot_rect)
    page.add_strikeout_annot(annot_rect)

    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page.insert_text((150.0, 200.0), "Cropped body text.", fontsize=16, fontname="helv")
    page.set_cropbox(fitz.Rect(100.0, 100.0, 612.0, 792.0))

    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    src = fitz.open()
    src_page = src.new_page(width=200.0, height=200.0)
    src_page.insert_text((20.0, 40.0), "embedded", fontsize=12, fontname="helv")
    page.show_pdf_page(fitz.Rect(50.0, 50.0, 250.0, 250.0), src, 0)
    src.close()

    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    pix = fitz.Pixmap(fitz.csRGB, fitz.IRect(0, 0, int(PAGE_WIDTH), int(PAGE_HEIGHT)))
    pix.clear_with(230)
    page.insert_image(fitz.Rect(0.0, 0.0, PAGE_WIDTH, PAGE_HEIGHT), pixmap=pix)
    page.insert_text((30.0, 50.0), "overlay", fontsize=12, fontname="helv")

    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page.insert_text((50.0, 100.0), "Stroke body text.", fontsize=16, fontname="helv")
    page.draw_line(fitz.Point(60.0, 96.0), fitz.Point(220.0, 96.0), color=(0.1, 0.1, 0.1), width=2.0)

    doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)

    raw = doc.tobytes(garbage=0)
    doc.close()
    return raw


def _strip_item(item_id: str, bbox: list[float], role: str) -> dict:
    return {
        "item_id": item_id,
        "block_type": "text",
        "block_kind": "text",
        "layout_role": role,
        "translation_overlay_text": "译文",
        "bbox": bbox,
    }


def _formula_item(item_id: str, bbox: list[float]) -> dict:
    return {
        "item_id": item_id,
        "block_type": "formula",
        "block_kind": "formula",
        "source_text": "x^2 + y^2",
        "bbox": bbox,
    }


def translated_pages(src: Path) -> dict[int, list[dict]]:
    """One caption item per page that should exercise the strip path, plus p0's
    paragraph item (strip-eligible but cover-fallback-averse). Item bboxes for
    text-overlap pages are the first text block's rect padded by 2pt, in the
    same fitz display space the bboxlog reports."""
    doc = fitz.open(src)

    def text_bbox(page_idx: int) -> list[float]:
        blocks = doc[page_idx].get_text("blocks")
        if not blocks:
            return [10.0, 10.0, 200.0, 40.0]
        r = fitz.Rect(blocks[0][:4])
        return [r.x0 - 2.0, r.y0 - 2.0, r.x1 + 2.0, r.y1 + 2.0]

    pages = {
        0: [
            _strip_item("p0-caption", text_bbox(0), role="caption"),
            _strip_item("p0-body", text_bbox(0), role="paragraph"),
            # Formula sub-region overlapping only the left part of the text so the
            # strip segments around it remain non-empty.
            _formula_item("p0-formula", [52.0, 86.0, 130.0, 105.0]),
        ],
        1: [_strip_item("p1-caption", text_bbox(1), role="caption")],
        2: [_strip_item("p2-caption", text_bbox(2), role="caption")],
        3: [_strip_item("p3-caption", text_bbox(3), role="caption")],
        4: [_strip_item("p4-caption", [60.0, 60.0, 240.0, 240.0], role="caption")],
        5: [_strip_item("p5-caption", text_bbox(5), role="caption")],
        6: [_strip_item("p6-caption", text_bbox(6), role="caption")],
        7: [],
        99: [_strip_item("p99-caption", [10.0, 10.0, 60.0, 40.0], role="caption")],
    }
    doc.close()
    return pages


def assert_contexts_close(native_ctxs, ref_ctxs, fitz_ctxs, label: str) -> None:
    assert native_ctxs.keys() == ref_ctxs.keys() == fitz_ctxs.keys(), (
        f"{label}: keys {native_ctxs.keys()} vs {ref_ctxs.keys()} vs {fitz_ctxs.keys()}"
    )
    for page_idx in native_ctxs:
        n, r, f = native_ctxs[page_idx], ref_ctxs[page_idx], fitz_ctxs[page_idx]
        assert n.page_rect == r.page_rect == f.page_rect, (
            f"{label} p{page_idx}: page_rect {n.page_rect} vs {r.page_rect} vs {f.page_rect}"
        )
        for comp in range(6):
            a, b, c = (getattr(x.inverse_ctm, "abcdef"[comp]) for x in (n, r, f))
            assert max(a, b, c) - min(a, b, c) <= RECT_TOL, (
                f"{label} p{page_idx}: inverse_ctm[{comp}] {a} vs {b} vs {c}"
            )
        assert n.content_stream_size == r.content_stream_size == f.content_stream_size, (
            f"{label} p{page_idx}: content_stream_size {n.content_stream_size} vs "
            f"{r.content_stream_size} vs {f.content_stream_size}"
        )
        assert n.has_form_xobjects == r.has_form_xobjects == f.has_form_xobjects, (
            f"{label} p{page_idx}: has_form_xobjects {n.has_form_xobjects} vs "
            f"{r.has_form_xobjects} vs {f.has_form_xobjects}"
        )
        assert len(n.bboxlog_entries) == len(r.bboxlog_entries) == len(f.bboxlog_entries), (
            f"{label} p{page_idx}: bboxlog len {len(n.bboxlog_entries)} vs "
            f"{len(r.bboxlog_entries)} vs {len(f.bboxlog_entries)}"
        )
        for na, ra, fa in zip(n.bboxlog_entries, r.bboxlog_entries, f.bboxlog_entries):
            assert na[0] == ra[0] == fa[0], f"{label} p{page_idx}: kind {na[0]} vs {ra[0]} vs {fa[0]}"
            for comp in range(4):
                a, b, c = na[1][comp], ra[1][comp], fa[1][comp]
                assert max(a, b, c) - min(a, b, c) <= RECT_TOL, (
                    f"{label} p{page_idx}: bboxlog {na[0]} rect[{comp}] {a} vs {b} vs {c}"
                )


def _rect_tuples_close(left, right, tol: float) -> bool:
    if left.keys() != right.keys():
        return False
    for page_idx in left:
        if len(left[page_idx]) != len(right[page_idx]):
            return False
        for ra, rb in zip(left[page_idx], right[page_idx]):
            if any(abs(a - b) > tol for a, b in zip(ra, rb)):
                return False
    return True


def assert_candidates_close(left, right, label: str) -> None:
    assert _rect_tuples_close(left.page_rects, right.page_rects, RECT_TOL), (
        f"{label}: strip rects {left.page_rects} != {right.page_rects}"
    )
    assert _rect_tuples_close(left.page_protected_rects or {}, right.page_protected_rects or {}, RECT_TOL), (
        f"{label}: protected rects {left.page_protected_rects} != {right.page_protected_rects}"
    )
    assert left.uncovered_unsafe_vector_item_ids == right.uncovered_unsafe_vector_item_ids, (
        f"{label}: uncovered {left.uncovered_unsafe_vector_item_ids} != {right.uncovered_unsafe_vector_item_ids}"
    )
    assert left.skipped_complex_page_indices == right.skipped_complex_page_indices, f"{label}: skipped_complex"
    assert left.skipped_no_text_overlap_page_indices == right.skipped_no_text_overlap_page_indices, (
        f"{label}: skipped_no_text_overlap"
    )
    assert left.skipped_visual_background_page_indices == right.skipped_visual_background_page_indices, (
        f"{label}: skipped_visual_background"
    )
    assert left.page_features == right.page_features, f"{label}: page_features"


def main() -> None:
    assert _native.NATIVE, "source_cleanup planning native module not built"

    raw = build_source_pdf()
    with tempfile.TemporaryDirectory(prefix="rps-scp-") as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(raw)
        pages = translated_pages(src)
        page_indices = sorted(pages)

        # ---- 3-way context parity ------------------------------------------
        native_ctxs = _native.build_page_contexts(src, page_indices)
        was = _native.NATIVE
        _native.NATIVE = False
        try:
            reference_ctxs = _native.build_page_contexts(src, page_indices)
        finally:
            _native.NATIVE = was
        fitz_ctxs = _build_page_contexts_python(source_pdf_path=src, page_indices=page_indices)
        assert_contexts_close(native_ctxs, reference_ctxs, fitz_ctxs, "contexts")

        # ---- end-to-end 3-way parity ----------------------------------------
        native_candidates = plan_source_cleanup(source_pdf_path=src, translated_pages=pages)
        _native.NATIVE = False
        try:
            reference_candidates = plan_source_cleanup(source_pdf_path=src, translated_pages=pages)
        finally:
            _native.NATIVE = was
        fitz_candidates = _plan_source_cleanup_python(
            source_pdf_path=src,
            translated_pages=pages,
            protected_pages={},
            skip_formula_pages=False,
            skip_form_xobject_pages=True,
            document_analysis=None,
        )
        assert_candidates_close(native_candidates, reference_candidates, "plan native vs reference")
        assert_candidates_close(reference_candidates, fitz_candidates, "plan reference vs fitz")

        # ---- uncovered-id 3-way parity --------------------------------------
        native_ids = item_ids_with_uncovered_unsafe_vector_overlap(
            source_pdf_path=src,
            translated_pages=pages,
        )
        _native.NATIVE = False
        try:
            reference_ids = item_ids_with_uncovered_unsafe_vector_overlap(
                source_pdf_path=src,
                translated_pages=pages,
            )
        finally:
            _native.NATIVE = was
        fitz_ids = _item_ids_with_uncovered_unsafe_vector_overlap_python(
            source_pdf_path=src,
            translated_pages=pages,
        )
        assert native_ids == reference_ids == fitz_ids, (
            f"uncovered ids {native_ids} vs {reference_ids} vs {fitz_ids}"
        )

        # ---- boundaries -----------------------------------------------------
        assert 99 not in native_ctxs, f"out-of-range page 99 must be absent, got {native_ctxs.keys()}"
        assert 99 not in native_candidates.page_rects, "out-of-range page must not strip"
        assert 7 not in native_candidates.page_rects, "empty page must produce no strip rects"
        assert 4 in native_candidates.page_rects, "form-xobject page must route through _plan_form_xobject_page"
        assert 5 in native_candidates.skipped_visual_background_page_indices, (
            f"full-page-image page must be skipped visual background, got "
            f"{native_candidates.skipped_visual_background_page_indices}"
        )
        assert "p0-caption" in native_candidates.uncovered_unsafe_vector_item_ids, (
            f"p0 caption over the text-like fill path must be uncovered, got "
            f"{native_candidates.uncovered_unsafe_vector_item_ids}"
        )
        assert "p0-body" not in native_candidates.uncovered_unsafe_vector_item_ids, (
            "p0 paragraph (no cover fallback) must not be uncovered"
        )
        assert "p6-caption" in native_candidates.uncovered_unsafe_vector_item_ids, (
            "p6 caption over the stroke must be uncovered"
        )
        assert 0 in (native_candidates.page_protected_rects or {}), (
            "p0 formula guard must emit protected rects"
        )

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
