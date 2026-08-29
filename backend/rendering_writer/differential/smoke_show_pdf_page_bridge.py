#!/usr/bin/env python3
"""Native bridge smoke test for `show_pdf_page` / `build_dual_doc_pages`.

Exercises the PRODUCTION routing shims in
`services.rendering.output.typst._native`:

  * native `show_pdf_page_on_doc` output is pixel-identical to the pure-fitz
    reference across probe cases (blank, cropbox, rotated source, rotated
    target, dual placement) and records exactly one native hit per call,
  * native `build_dual_doc_pages` output is pixel-identical to the pure-fitz
    dual-page reference and records a native hit,
  * the fallback path still works: with the shim's ``NATIVE`` forced off, the
    shim mutates the caller's document in place, returns the same object, and
    records a fallback.

The bridge itself (`rendering_bridge.show_pdf_page`) is exercised through the
shims; the raw bridge calls are also replayed for direct parity evidence.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_show_pdf_page_bridge.py
"""

import os
import sys
import tempfile
from pathlib import Path

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import rendering_bridge  # noqa: E402

from services.rendering import _routing  # noqa: E402
from services.rendering.output.typst import _native  # noqa: E402


def make_src(width=150, height=100):
    doc = fitz.open()
    page = doc.new_page(width=width, height=height)
    page.draw_rect(fitz.Rect(10, 10, width - 10, height - 10), color=None, fill=(0.2, 0.8, 0.2))
    page.insert_text((20, 60), "SRC", fontsize=20)
    return doc


def make_src_crop():
    doc = fitz.open()
    page = doc.new_page(width=500, height=400)
    page.draw_rect(fitz.Rect(0, 0, 500, 400), color=None, fill=(0.6, 0.6, 0.9))
    page.insert_text((60, 200), "CROPPED", fontsize=24)
    page.set_cropbox(fitz.Rect(50, 50, 350, 350))
    return doc


def make_src_rot():
    doc = fitz.open()
    page = doc.new_page(width=300, height=200)
    page.draw_rect(fitz.Rect(20, 20, 280, 180), color=None, fill=(0.9, 0.7, 0.2))
    page.insert_text((40, 120), "ROT", fontsize=24)
    page.set_rotation(90)
    return doc


def make_target_existing():
    doc = fitz.open()
    page = doc.new_page(width=400, height=400)
    page.draw_rect(fitz.Rect(0, 0, 400, 400), color=None, fill=(0.95, 0.95, 0.95))
    page.insert_text((300, 30), "T", fontsize=14)
    return doc


def make_target_rot():
    doc = fitz.open()
    page = doc.new_page(width=300, height=400)
    page.set_rotation(90)
    return doc


def render_page(pdf_bytes):
    doc = fitz.open(stream=pdf_bytes, filetype="pdf")
    pix = doc[0].get_pixmap(colorspace=fitz.csRGB, alpha=False)
    doc.close()
    return pix.samples


def assert_pixels(label, ref_bytes, cand_bytes):
    ref = render_page(ref_bytes)
    cand = render_page(cand_bytes)
    n = min(len(ref), len(cand))
    diff = sum(1 for a, b in zip(ref, cand) if a != b)
    assert ref == cand, f"{label}: pixel diff {diff}/{n}"
    print(f"  ok {label} (pixel diff {diff}/{n})")


def check_show_pdf_page(label, src_doc, target_doc, target_idx, rect):
    target_bytes = target_doc.tobytes()
    src_bytes = src_doc.tobytes()
    ref = fitz.open(stream=target_bytes, filetype="pdf")
    ref_src = fitz.open(stream=src_bytes, filetype="pdf")
    ref[target_idx].show_pdf_page(fitz.Rect(*rect), ref_src, 0, overlay=True)
    ref_bytes = ref.tobytes()
    ref_src.close()
    ref.close()
    cand = rendering_bridge.show_pdf_page(target_bytes, src_bytes, target_idx, 0, tuple(float(v) for v in rect))
    assert_pixels(label, ref_bytes, cand)


def check_dual_native(label, src_doc, trl_doc):
    src_b = src_doc.tobytes()
    trl_b = trl_doc.tobytes()
    dual_native = rendering_bridge.build_dual_doc_pages(src_b, trl_b, 0, -1)
    py_src = fitz.open(stream=src_b, filetype="pdf")
    py_trl = fitz.open(stream=trl_b, filetype="pdf")
    py_dual = fitz.open()
    p0 = py_dual.new_page(width=py_src[0].rect.width + py_trl[0].rect.width,
                          height=max(py_src[0].rect.height, py_trl[0].rect.height))
    p0.show_pdf_page(fitz.Rect(0, 0, py_src[0].rect.width, py_src[0].rect.height), py_src, 0, overlay=True)
    p0.show_pdf_page(fitz.Rect(py_src[0].rect.width, 0,
                               py_src[0].rect.width + py_trl[0].rect.width, py_trl[0].rect.height),
                     py_trl, 0, overlay=True)
    py_dual_b = py_dual.tobytes()
    py_dual.close(); py_src.close(); py_trl.close()
    assert_pixels(label, py_dual_b, dual_native)


def main() -> None:
    assert _native.NATIVE, "native module not built"
    _routing.reset()

    print("raw bridge parity:")
    check_show_pdf_page("simple->existing", make_src(), make_target_existing(), 0, (0, 0, 200, 200))
    check_show_pdf_page("cropbox->blank", make_src_crop(), make_target_existing(), 0, (10, 10, 290, 290))
    check_show_pdf_page("rotated->blank", make_src_rot(), make_target_existing(), 0, (30, 40, 370, 360))
    check_show_pdf_page("simple->rotated-target", make_src(), make_target_rot(), 0, (0, 0, 200, 300))

    src_a = make_src(150, 100)
    src_b = make_src(120, 80)
    target = fitz.open()
    target.new_page(width=270, height=100)
    tgt_bytes = target.tobytes()
    target.close()
    step1 = rendering_bridge.show_pdf_page(tgt_bytes, src_a.tobytes(), 0, 0, (0.0, 0.0, 150.0, 100.0))
    step2 = rendering_bridge.show_pdf_page(step1, src_b.tobytes(), 0, 0, (150.0, 0.0, 270.0, 80.0))
    refd = fitz.open()
    rp = refd.new_page(width=270, height=100)
    rp.show_pdf_page(fitz.Rect(0, 0, 150, 100), fitz.open(stream=src_a.tobytes(), filetype="pdf"), 0, overlay=True)
    rp.show_pdf_page(fitz.Rect(150, 0, 270, 80), fitz.open(stream=src_b.tobytes(), filetype="pdf"), 0, overlay=True)
    ref_bytes = refd.tobytes()
    refd.close()
    assert_pixels("dual-placement", ref_bytes, step2)

    check_dual_native("dual-doc-batch", make_src(200, 150), make_src(180, 140))

    print("production shim routing:")
    _routing.reset()
    overlay_doc = make_src(150, 100)
    target_doc = fitz.open()
    target_doc.new_page(width=200, height=200)
    native_result = _native.show_pdf_page_on_doc(target_doc, overlay_doc, 0, 0, fitz.Rect(20, 30, 170, 130))
    assert native_result is not target_doc, "native show_pdf_page_on_doc must return a fresh doc"
    snap = _routing.snapshot()
    assert snap["hits"].get("typst", 0) == 1, f"expected 1 native show_pdf_page hit, got {snap}"
    print(f"  ok show_pdf_page_on_doc native hit=1")
    ref = fitz.open(stream=target_doc.tobytes(), filetype="pdf")
    ref_src = fitz.open(stream=overlay_doc.tobytes(), filetype="pdf")
    ref[0].show_pdf_page(fitz.Rect(20, 30, 170, 130), ref_src, 0, overlay=True)
    ref_bytes = ref.tobytes()
    ref_src.close(); ref.close()
    assert_pixels("shim show_pdf_page parity", ref_bytes, native_result.tobytes())

    _routing.reset()
    dual = fitz.open()
    native_dual = _native.build_dual_doc_pages(make_src(200, 150), make_src(180, 140), dual, start_page=0, end_page=-1)
    assert native_dual is not dual, "native build_dual_doc_pages must return a fresh doc"
    snap = _routing.snapshot()
    assert snap["hits"].get("typst", 0) == 1, f"expected 1 native build_dual hit, got {snap}"
    print(f"  ok build_dual_doc_pages native hit=1")
    check_dual_native("shim build_dual parity", make_src(200, 150), make_src(180, 140))
    shim_src = make_src(200, 150)
    shim_trl = make_src(180, 140)
    py_s = fitz.open(stream=shim_src.tobytes(), filetype="pdf")
    py_t = fitz.open(stream=shim_trl.tobytes(), filetype="pdf")
    py_d = fitz.open()
    p0 = py_d.new_page(width=380, height=150)
    p0.show_pdf_page(fitz.Rect(0, 0, 200, 150), py_s, 0, overlay=True)
    p0.show_pdf_page(fitz.Rect(200, 0, 380, 140), py_t, 0, overlay=True)
    py_dual_bytes = py_d.tobytes()
    py_d.close(); py_s.close(); py_t.close()
    assert_pixels("shim build_dual output parity", py_dual_bytes, native_dual.tobytes())

    print("page_overlay.overlay_pages_from_single_pdf routing:")
    from services.rendering.output.typst.source_page_overlay import overlay_pages_from_single_pdf

    tmp_root = Path(tempfile.mkdtemp(prefix="po-"))
    src_path = tmp_root / "src.pdf"
    ovl_path = tmp_root / "ovl.pdf"
    src_pdf = fitz.open()
    for i in range(2):
        p = src_pdf.new_page(width=200, height=150)
        p.draw_rect(fitz.Rect(0, 0, 200, 150), color=None, fill=(0.7, 0.7, 0.7))
        p.insert_text((20, 40), f"SRC{i}", fontsize=16)
    src_pdf.save(src_path)
    src_pdf.close()
    ovl_pdf = fitz.open()
    for i in range(2):
        p = ovl_pdf.new_page(width=200, height=150)
        p.draw_rect(fitz.Rect(30, 30, 170, 120), color=None, fill=(0.9, 0.3, 0.2))
        p.insert_text((40, 80), f"OVL{i}", fontsize=16)
    ovl_pdf.save(ovl_path)
    ovl_pdf.close()

    _routing.reset()
    slot: dict[str, object] = {}
    translated = {
        i: [{"item_id": f"p{i}-b", "bbox": [40.0, 40.0, 90.0, 70.0]}]
        for i in range(2)
    }
    diagnostics = overlay_pages_from_single_pdf(
        None,
        [0, 1],
        translated,
        ovl_path,
        apply_source_overlay=False,
        skip_visual_cover=True,
        source_base_pdf_path=src_path,
        doc_slot=slot,
    )
    snap = _routing.snapshot()
    assert snap["hits"].get("typst", 0) == 2, f"expected 2 native show_pdf_page hits, got {snap}"
    swapped = slot.get("doc")
    assert swapped is not None, "doc_slot['doc'] must be set after native page_overlay loop"
    assert len(swapped) == 2, "swapped doc must keep page count"
    assert int(diagnostics.get("legacy_pymupdf_overlay_pages", 0) or 0) == 0, (
        "native path must not record legacy overlay counters"
    )
    ref = fitz.open(src_path)
    ref_ovl = fitz.open(ovl_path)
    for i in range(2):
        ref[i].show_pdf_page(ref[i].rect, ref_ovl, i, overlay=True)
    ref_bytes = ref.tobytes()
    ref.close()
    ref_ovl.close()
    assert_pixels("page_overlay loop parity", ref_bytes, swapped.tobytes())
    swapped.close()
    print("  ok overlay_pages_from_single_pdf native hits=2 + doc_slot swap + pixel parity")

    print("fallback path:")
    _routing.reset()
    saved = _native.NATIVE
    _native.NATIVE = False
    try:
        fb_target = fitz.open()
        fb_target.new_page(width=200, height=200)
        fb_overlay = make_src(150, 100)
        fb_result = _native.show_pdf_page_on_doc(fb_target, fb_overlay, 0, 0, fitz.Rect(20, 30, 170, 130))
        assert fb_result is fb_target, "fallback show_pdf_page_on_doc must return the same doc"
        snap = _routing.snapshot()
        assert snap["fallbacks_by_reason"].get("native_not_built", 0) >= 1, f"expected fallback, got {snap}"
        print(f"  ok show_pdf_page_on_doc fallback same-doc")

        _routing.reset()
        fb_dual = fitz.open()
        fb_dual_result = _native.build_dual_doc_pages(make_src(120, 90), make_src(100, 80), fb_dual,
                                                      start_page=0, end_page=-1)
        assert fb_dual_result is fb_dual, "fallback build_dual_doc_pages must return the same doc"
        assert len(fb_dual) == 1, f"fallback build_dual produced {len(fb_dual)} pages"
        snap = _routing.snapshot()
        assert snap["fallbacks_by_reason"].get("native_not_built", 0) >= 1, f"expected fallback, got {snap}"
        print(f"  ok build_dual_doc_pages fallback same-doc")

        _routing.reset()
        fb_slot: dict[str, object] = {}
        fb_diag = overlay_pages_from_single_pdf(
            None,
            [0, 1],
            translated,
            ovl_path,
            apply_source_overlay=False,
            skip_visual_cover=True,
            source_base_pdf_path=src_path,
            doc_slot=fb_slot,
        )
        fb_doc = fb_slot.get("doc")
        assert fb_doc is not None, "fallback must open source doc into doc_slot"
        assert int(fb_diag.get("legacy_pymupdf_overlay_pages", 0) or 0) == 2, (
            "fallback must record legacy overlay counters"
        )
        snap = _routing.snapshot()
        assert snap["fallbacks_by_reason"].get("native_not_built", 0) >= 2, f"expected 2 fallbacks, got {snap}"
        fb_doc.close()
        print("  ok overlay_pages_from_single_pdf fallback same-doc + legacy counters")
    finally:
        _native.NATIVE = saved

    print("all show_pdf_page bridge smoke tests pass")


if __name__ == "__main__":
    main()
