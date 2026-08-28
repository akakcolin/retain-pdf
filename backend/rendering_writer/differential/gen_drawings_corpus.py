#!/usr/bin/env python3
"""Page-drawings corpus generator (Phase B2-7).

Records the REAL fitz `page.get_cdrawings()` per-drawing facts (type
"f"/"s"/"fs", rect, stroke width) for deterministic single-page synthetic PDFs
and every page of the golden PDFs `resources/samples/golden-pdfs/1.pdf` and
`2.pdf` (the 19 MB `3.pdf` is excluded — its vector-heavy pages make the
embedded corpus ~40 MB, impractical for the replay's `include_str!`; the smoke
test covers `3.pdf` via file-backed pages):

  * the source PDF bytes (base64, trailer /ID pinned),
  * the page rect,
  * per-page `drawings`: `[{"type", "rect", "width"}, ...]` in drawing order
    plus the drawing `count`.

`width` is `None` for fills (matches mupdf-rs `Drawing.width`) and
`line_width * path_factor` for strokes. The Rust replay
(`tests/drawings_diff.rs`) runs `rendering_reader::PdfDocument::page_drawings`
on the same bytes and asserts it reproduces the recorded type/count exactly,
rect within 0.01 pt, and width within 0.01 (None aligned with None).

Synthetic coverage pins the parity surface: RGB fill, cm-scaled stroke (width
formula `line_width * path_factor`), fill+stroke merge (type "fs"), gray / cmyk
fill (fill colorspace divergence must NOT leak into type/rect/width), bezier,
and multiple drawings in a defined order.

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_drawings_corpus.py
"""

import base64
import json
import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

# Corpus lives next to its replay (`rendering_reader/tests/drawings_diff.rs`).
OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "..", "rendering_reader", "tests", "drawings_corpus.json"))
GOLDEN_ROOT = os.path.abspath(
    os.path.join(_HERE, "..", "..", "..", "resources", "samples", "golden-pdfs")
)

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


def _build_page(draw: callable) -> bytes:
    """Open a blank letter page, run `draw(page)`, pin /ID, return bytes."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    draw(page)
    raw = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()
    return raw


def _record_drawings(raw: bytes) -> dict:
    """Record fitz get_cdrawings facts per page for `raw`."""
    doc = fitz.open(stream=raw, filetype="pdf")
    pages = {}
    for idx in range(doc.page_count):
        page = doc.load_page(idx)
        rect = [float(v) for v in page.rect]
        drawings = []
        for d in page.get_cdrawings():
            drawings.append(
                {
                    "type": d.get("type"),
                    "rect": [float(v) for v in d["rect"]],
                    "width": None if not d.get("width") else float(d["width"]),
                }
            )
        pages[str(idx)] = {"rect": rect, "drawings": drawings, "count": len(drawings)}
    doc.close()
    return pages


def _run_case(name: str, raw: bytes) -> dict:
    return {
        "name": name,
        "pdf_b64": base64.b64encode(raw).decode("ascii"),
        "pages": _record_drawings(raw),
    }


def _content_stream_page(page: fitz.Page, ops: bytes) -> None:
    doc = page.parent
    xref = doc.get_new_xref()
    doc.update_object(xref, "<< /Length %d >>" % len(ops))
    doc.update_stream(xref, ops)
    page.set_contents(xref)


def main() -> None:
    cases = []

    # 1. RGB filled rect: type "f", width null.
    raw = _build_page(lambda p: p.draw_rect(
        fitz.Rect(50.0, 50.0, 250.0, 60.0), color=None, fill=(0.3, 0.5, 0.7)))
    cases.append(_run_case("rgb_fill_rect", raw))

    # 2. cm-scaled stroked line: width 3.0 * 0.5 = 1.5, type "s".
    def _cm_stroke(page: fitz.Page) -> None:
        _content_stream_page(page, (
            b"q\n0.5 0 0 0.5 100 300 cm\n3 w\n200 300 m\n240 304 l\nS\nQ\n"
        ))

    raw = _build_page(_cm_stroke)
    cases.append(_run_case("cm_scaled_stroke", raw))

    # 3. Fill+stroke rect: type "fs", width 1.5 (fill and stroke merged).
    raw = _build_page(lambda p: p.draw_rect(
        fitz.Rect(60.0, 400.0, 260.0, 410.0), color=(0.0, 0.0, 0.0), width=1.5, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("fill_stroke_rect", raw))

    # 4. Gray fill: fill colorspace must not leak; type "f", width null.
    def _gray_fill(page: fitz.Page) -> None:
        _content_stream_page(page, b"q\n0.5 g\n70 500 100 10 re\nf\nQ\n")

    raw = _build_page(_gray_fill)
    cases.append(_run_case("gray_fill_rect", raw))

    # 5. CMYK fill: same as gray — type/rect/width parity only.
    def _cmyk_fill(page: fitz.Page) -> None:
        _content_stream_page(page, b"q\n0.1 0.2 0.3 0.4 k\n80 600 120 12 re\nf\nQ\n")

    raw = _build_page(_cmyk_fill)
    cases.append(_run_case("cmyk_fill_rect", raw))

    # 6. Filled bezier: type "f", width null.
    raw = _build_page(lambda p: p.draw_bezier(
        (300.0, 600.0), (320.0, 610.0), (340.0, 590.0), (360.0, 600.0),
        color=None, fill=(0.1, 0.2, 0.3)))
    cases.append(_run_case("bezier_fill", raw))

    # 7. Multiple drawings in a defined order: fill rect, stroke rect, filled
    #    polyline, gray fill — order and count must match. All four use the
    #    PyMuPDF drawing API (a `set_contents` call would REPLACE the earlier
    #    commands).
    def _multi(page: fitz.Page) -> None:
        page.draw_rect(fitz.Rect(40.0, 40.0, 240.0, 50.0), color=None, fill=(0.3, 0.5, 0.7))
        page.draw_rect(fitz.Rect(40.0, 80.0, 240.0, 90.0), color=(0.0, 0.0, 0.0), width=2.0)
        page.draw_polyline(
            [(200.0, 120.0), (210.0, 122.0), (220.0, 124.0), (230.0, 126.0)],
            color=None, fill=(0.1, 0.1, 0.1))
        page.draw_rect(fitz.Rect(70.0, 160.0, 170.0, 170.0), color=None, fill=(0.5,))

    raw = _build_page(_multi)
    cases.append(_run_case("multi_drawings_order", raw))

    # 8. Zero-height horizontal stroke: exercises the thin-rect expansion input
    #    (fitz reports the raw path bbox; width 2.0, type "s").
    def _thin_stroke(page: fitz.Page) -> None:
        _content_stream_page(page, b"q\n2 w\n100 300 m\n400 300 l\nS\nQ\n")

    raw = _build_page(_thin_stroke)
    cases.append(_run_case("thin_stroke_line", raw))

    # 9. Filled+stroked polyline: type "fs".
    raw = _build_page(lambda p: p.draw_polyline(
        [(100.0, 300.0), (110.0, 302.0), (120.0, 304.0), (130.0, 306.0), (140.0, 308.0)],
        color=(0.0, 0.0, 0.0), width=1.0, fill=(0.1, 0.1, 0.1)))
    cases.append(_run_case("fill_stroke_polyline", raw))

    # 10. Golden PDFs: every page of each embedded file. `3.pdf` is excluded
    #     (see module docstring) — its parity is covered by the smoke test.
    for name in ["1.pdf", "2.pdf"]:
        golden = os.path.join(GOLDEN_ROOT, name)
        assert os.path.exists(golden), f"golden {golden} missing"
        cases.append(_run_case(f"golden_{name[:-4]}", open(golden, "rb").read()))

    corpus = {"schema": "retainpdf_drawings_corpus_v1", "cases": cases}
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
