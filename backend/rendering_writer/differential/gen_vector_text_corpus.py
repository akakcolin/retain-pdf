#!/usr/bin/env python3
"""Vector-text corpus generator (Phase 7R-6).

Builds deterministic single-page synthetic PDFs with hand-controlled vector
drawings (polylines, rects, beziers, a raw content stream with a `cm`
transform) and records the REAL production `collect_vector_text_rects` output:

  * the source PDF bytes (base64, trailer /ID pinned),
  * the page rect,
  * `target_rects` (the translated-item rects the overlap index is built from),
  * `expected_rects` (the exact `collect_vector_text_rects` result).

The generator asserts each case's production output before recording, so a
corpus case only captures a verified classification outcome. The Rust replay
(`tests/vector_text_diff.rs`) runs the ported
`background::vector_text::collect_vector_text_rects` (NativeDevice over the
page display list) on the same bytes and asserts it reproduces
`expected_rects` (rects compared with a 0.02 pt tolerance).

Corpus design pins the ported classification semantics:
  * item counting (rect=1 / line=1 / curve=1 / close=1 / move=0),
  * the black-fill threshold (max fill component <= 0.2) and small-glyph height
    limit (<= 20 pt) vs the large-text-cluster rule (>= 400 items, no height
    limit),
  * type filtering (only type "f" qualifies; "s" / "fs" are rejected),
  * the overlap index and the large-cluster first-target-intersection output,
  * the page transform (top-left origin, y down) including a real `cm`.

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_vector_text_corpus.py
"""

import base64
import json
import math
import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.source.vector_text import collect_vector_text_rects  # noqa: E402

OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "vector_text_corpus.json"))

PAGE_W = 612.0
PAGE_H = 792.0

FIXED_ID = (
    b"/ID[<30303030303030303030303030303030>"
    b"<30303030303030303030303030303030>]"
)


def _array_end(raw: bytes, open_bracket: int) -> int:
    i = open_bracket + 1
    n = len(raw)
    while i < n:
        c = raw[i]
        if c == 0x28:
            i += 1
            while i < n:
                c2 = raw[i]
                if c2 == 0x5C:
                    i += 2
                    continue
                if c2 == 0x29:
                    i += 1
                    break
                i += 1
        elif c == 0x3C:
            j = raw.find(b">", i)
            if j < 0:
                return -1
            i = j + 1
        elif c == 0x5D:
            return i
        else:
            i += 1
    return -1


def _pin_pdf_id(raw: bytes) -> bytes:
    start = 0
    while True:
        idx = raw.find(b"/ID", start)
        if idx < 0:
            return raw
        j = idx + 3
        while j < len(raw) and raw[j] in b" \t\r\n":
            j += 1
        if j < len(raw) and raw[j] == 0x5B:
            end = _array_end(raw, j)
            if end > 0:
                raw = raw[:j] + FIXED_ID + raw[end + 1 :]
                start = j + len(FIXED_ID)
                continue
        start = idx + 3


def _zigzag(x0: float, y: float, step_x: float, amp: float, n: int) -> list[tuple[float, float]]:
    return [(x0 + i * step_x, y + (amp if i % 2 else -amp)) for i in range(n)]


def _build_page(draw: callable) -> bytes:
    """Open a blank letter page, run `draw(page)`, pin /ID, return bytes."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    draw(page)
    raw = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()
    return raw


def _rect_list(rect: fitz.Rect) -> list[float]:
    return [float(rect.x0), float(rect.y0), float(rect.x1), float(rect.y1)]


def _run_case(name: str, raw: bytes, target_rects: list[list[float]], expected: list[list[float]]) -> dict:
    d = fitz.open(stream=raw, filetype="pdf")
    page_rect = [float(v) for v in d.load_page(0).rect]
    targets = [fitz.Rect(r) for r in target_rects]
    got = [_rect_list(r) for r in collect_vector_text_rects(d.load_page(0), targets)]
    d.close()
    assert len(got) == len(expected), f"{name}: rect count {got} != {expected}"
    for actual, exp in zip(got, expected):
        for a, e in zip(actual, exp):
            assert abs(a - e) <= 0.02, f"{name}: rect {actual} != {expected}"
    return {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw).decode("ascii"),
        "target_rects": target_rects,
        "expected_rects": expected,
    }


def main() -> None:
    cases = []

    # 1. Small black-filled glyph: 9-pt open polyline = 8 line items, height 10.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 100.0, 7.5, 5.0, 9), color=None, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("small_glyph_polyline", raw, [[205.0, 97.0, 250.0, 103.0]], [[200.0, 95.0, 260.0, 105.0]]))

    # 2. Large black text cluster: 401-pt polyline = 400 line items; output is
    #    the first overlapping target intersection.
    sine401 = [(200.0 + i * 0.5, 300.0 + math.sin(i / 10.0) * 20.0) for i in range(401)]
    raw = _build_page(lambda page: page.draw_polyline(sine401, color=None, fill=(0.05, 0.05, 0.05)))
    cases.append(_run_case("large_cluster_sine", raw, [[210.0, 290.0, 250.0, 310.0], [300.0, 290.0, 380.0, 315.0]], [[210.0, 290.0, 250.0, 310.0]]))

    # 3. Small-glyph candidate rejected: height 50 > 20, items 8 < 400.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 400.0, 5.0, 50.0, 9), color=None, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("too_tall_rejected", raw, [[205.0, 405.0, 235.0, 445.0]], []))

    # 4. Small-glyph candidate rejected: fill 0.5 > 0.2.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 500.0, 5.0, 5.0, 9), color=None, fill=(0.5, 0.5, 0.5)))
    cases.append(_run_case("too_light_rejected", raw, [[205.0, 501.0, 235.0, 504.0]], []))

    # 5. Filled rect: 1 item < 8, rejected.
    raw = _build_page(lambda page: page.draw_rect(fitz.Rect(60.0, 60.0, 260.0, 70.0), color=None, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("rect_single_item_rejected", raw, [[100.0, 62.0, 200.0, 68.0]], []))

    # 6. Stroke-only rect: type "s", rejected.
    raw = _build_page(lambda page: page.draw_rect(fitz.Rect(60.0, 80.0, 260.0, 90.0), color=(0.0, 0.0, 0.0), width=1.5))
    cases.append(_run_case("stroke_only_rejected", raw, [[100.0, 82.0, 200.0, 88.0]], []))

    # 7. Fill+stroke polyline: type "fs" (same-path fill and stroke merged),
    #    rejected even though it has 8 items and a black fill.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 150.0, 7.5, 5.0, 9), color=(0.0, 0.0, 0.0), width=1.5, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("fill_stroke_rejected", raw, [[205.0, 147.0, 250.0, 153.0]], []))

    # 8. Small glyph with no overlapping target, rejected.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 200.0, 7.5, 5.0, 9), color=None, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("no_overlap_rejected", raw, [[400.0, 500.0, 500.0, 550.0]], []))

    # 9. Empty target list: early return.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 250.0, 7.5, 5.0, 9), color=None, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("empty_targets", raw, [], []))

    # 10. Large cluster with height 60 (> 20): the height limit only applies to
    #     small glyphs; the cluster still qualifies and returns the first target
    #     intersection.
    sine_wide = [(200.0 + i * 0.5, 500.0 + math.sin(i / 7.0) * 30.0) for i in range(401)]
    raw = _build_page(lambda page: page.draw_polyline(sine_wide, color=None, fill=(0.05, 0.05, 0.05)))
    cases.append(_run_case("large_cluster_no_height_limit", raw, [[210.0, 490.0, 260.0, 510.0]], [[210.0, 490.0, 260.0, 510.0]]))

    # 11. Real `cm` transform: raw content stream `0.5 0 0 0.5 100 300 cm`,
    #     9-point polyline (8 items) in user space. The drawing rect is the
    #     transformed bbox in top-left coordinates [200, 334, 220, 342].
    def _cm_transform_page(page: fitz.Page) -> None:
        ops = (
            b"q\n0.5 0 0 0.5 100 300 cm\n"
            b"200 300 m\n205 302 l\n210 304 l\n215 306 l\n220 308 l\n"
            b"225 310 l\n230 312 l\n235 314 l\n240 316 l\n"
            b".1 .1 .1 rg f\nQ\n"
        )
        doc = page.parent
        xref = doc.get_new_xref()
        doc.update_object(xref, "<< /Length %d >>" % len(ops))
        doc.update_stream(xref, ops)
        page.set_contents(xref)

    raw = _build_page(_cm_transform_page)
    cases.append(_run_case("transform_cm_glyph", raw, [[202.0, 336.0, 218.0, 340.0]], [[200.0, 334.0, 220.0, 342.0]]))

    # 12. Single bezier: 1 item < 8, rejected.
    raw = _build_page(lambda page: page.draw_bezier(
        (300.0, 600.0), (320.0, 610.0), (340.0, 590.0), (360.0, 600.0), color=None, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("bezier_single_item_rejected", raw, [[310.0, 595.0, 350.0, 605.0]], []))

    # 13. Two small glyphs: both qualify, output in drawing order.
    def _two_glyphs(page: fitz.Page) -> None:
        page.draw_polyline(_zigzag(200.0, 600.0, 7.5, 5.0, 9), color=None, fill=(0.1, 0.1, 0.1))
        page.draw_polyline(_zigzag(300.0, 600.0, 7.5, 5.0, 9), color=None, fill=(0.1, 0.1, 0.1))

    raw = _build_page(_two_glyphs)
    cases.append(_run_case("two_glyphs_multiple_outputs", raw, [[205.0, 597.0, 250.0, 603.0], [305.0, 597.0, 350.0, 603.0]], [[200.0, 595.0, 260.0, 605.0], [300.0, 595.0, 360.0, 605.0]]))

    # 14. Near-boundary black fill 0.19: still below the 0.2 threshold, passes.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 650.0, 7.5, 5.0, 9), color=None, fill=(0.19, 0.19, 0.19)))
    cases.append(_run_case("near_boundary_fill_0_19", raw, [[205.0, 647.0, 250.0, 653.0]], [[200.0, 645.0, 260.0, 655.0]]))

    # 15. 8-point polyline = 7 line items < 8, rejected.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 700.0, 5.0, 5.0, 8), color=None, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("seven_item_rejected", raw, [[205.0, 701.0, 230.0, 704.0]], []))

    # 16. Closed 9-pt polyline = 9 items (8 line + 1 close), qualifies.
    raw = _build_page(lambda page: page.draw_polyline(
        _zigzag(200.0, 100.0, 7.5, 5.0, 9), color=None, fill=(0.1, 0.1, 0.1), closePath=True))
    cases.append(_run_case("closed_polyline_glyph", raw, [[205.0, 97.0, 250.0, 103.0]], [[200.0, 95.0, 260.0, 105.0]]))

    corpus = {"schema": "retainpdf_vector_text_corpus_v1", "cases": cases}
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
