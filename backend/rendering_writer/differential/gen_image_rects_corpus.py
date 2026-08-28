#!/usr/bin/env python3
"""Image-placement corpus generator (Phase B2-8).

Records the REAL fitz `page.get_image_info(hashes=False)` placement bboxes for
deterministic single-page synthetic PDFs and every page of the golden PDFs
`resources/samples/golden-pdfs/1.pdf` and `2.pdf` (the 19 MB `3.pdf` is
excluded — its parity is covered by the file-backed smoke test), plus the
reference `page_has_large_background_image` boolean:

  * the source PDF bytes (base64, trailer /ID pinned),
  * the page rect,
  * per-page `image_rects`: the RAW `get_image_info` bboxes in paint order
    (unclipped — fitz reports fully/partially off-page placements exactly as
    the native display-list device does; the consumer intersects with the page
    rect), plus the `has_large` boolean (computed from the reference
    `_page_has_large_background_image_python`, NOT the routed production entry).

The Rust replay (`tests/image_rects_diff.rs`) runs
`rendering_reader::PdfDocument::page_image_placement_rects` on the same bytes
and asserts it reproduces the recorded rect set (exact count, each rect within
0.01 pt after sorting by (y0, x0)).

Synthetic coverage pins the parity surface: full-page large background, many
small images, wide-strip tiled background (tiled heuristic, not a PDF tiling
pattern), partially + fully off-page placements, a Form XObject carrying two
images, a masked (SMask) large image, multiple placements with one large, and a
90-degree-rotated page (get_image_info reports content-space bboxes, so the
native collector must clear rotation).

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_image_rects_corpus.py
"""

import base64
import json
import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

# Corpus lives next to its replay (`rendering_reader/tests/image_rects_diff.rs`).
OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "..", "rendering_reader", "tests", "image_rects_corpus.json"))
GOLDEN_ROOT = os.path.abspath(
    os.path.join(_HERE, "..", "..", "..", "resources", "samples", "golden-pdfs")
)

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
    """Open a blank 300x400 page, run `draw(page)`, pin /ID, return bytes. The
    synthetic cases draw in 300-wide coordinates so a full-page placement hits
    the 0.75 coverage threshold."""
    doc = fitz.open()
    page = doc.new_page(width=300.0, height=400.0)
    draw(page)
    raw = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()
    return raw


def _png_bytes(w: int, h: int, val: int) -> bytes:
    pm = fitz.Pixmap(fitz.csRGB, fitz.IRect(0, 0, w, h))
    pm.clear_with(val)
    return pm.tobytes("png")


def _record_images(raw: bytes) -> dict:
    """Record fitz get_image_info placement bboxes + reference has_large per page."""
    from services.rendering.source.background.detect import _page_has_large_background_image_python

    doc = fitz.open(stream=raw, filetype="pdf")
    pages = {}
    for idx in range(doc.page_count):
        page = doc.load_page(idx)
        rect = [float(v) for v in page.rect]
        rects = []
        for info in page.get_image_info(hashes=False):
            rects.append([float(v) for v in fitz.Rect(info["bbox"])])
        has_large = bool(_page_has_large_background_image_python(page))
        pages[str(idx)] = {"rect": rect, "image_rects": rects, "has_large": has_large}
    doc.close()
    return pages


def _run_case(name: str, raw: bytes) -> dict:
    return {
        "name": name,
        "pdf_b64": base64.b64encode(raw).decode("ascii"),
        "pages": _record_images(raw),
    }


def main() -> None:
    cases = []

    # 1. Full-page large background image: single placement, has_large True.
    raw = _build_page(lambda p: p.insert_image(
        fitz.Rect(0.0, 0.0, 300.0, 400.0), stream=_png_bytes(60, 80, 230)))
    cases.append(_run_case("large_bg", raw))

    # 2. Several small non-overlapping images: has_large False.
    def _small(page: fitz.Page) -> None:
        page.insert_image(fitz.Rect(10.0, 10.0, 60.0, 60.0), stream=_png_bytes(5, 5, 120))
        page.insert_image(fitz.Rect(100.0, 100.0, 150.0, 150.0), stream=_png_bytes(5, 5, 90))
        page.insert_image(fitz.Rect(200.0, 250.0, 260.0, 310.0), stream=_png_bytes(6, 6, 200))

    raw = _build_page(_small)
    cases.append(_run_case("small_images", raw))

    # 3. Eight full-width strips: no single strip is large, but the tiled
    #    heuristic fires (8 >= min_count, all width ratio 1.0, merged bands
    #    cover the full page). has_large True via the tiled path.
    def _tiled(page: fitz.Page) -> None:
        for y in range(0, 400, 50):
            page.insert_image(
                fitz.Rect(0.0, float(y), 300.0, float(y + 50)), stream=_png_bytes(6, 1, 150))

    raw = _build_page(_tiled)
    cases.append(_run_case("tiled_bg", raw))

    # 4. Large image partially off-page + a small on-page + a fully off-page
    #    placement: fitz reports all three raw bboxes; has_large True via the
    #    page-clipped coverage of the partial image.
    def _offpage(page: fitz.Page) -> None:
        page.insert_image(fitz.Rect(-50.0, -50.0, 350.0, 350.0), stream=_png_bytes(10, 10, 100))
        page.insert_image(fitz.Rect(10.0, 10.0, 60.0, 60.0), stream=_png_bytes(5, 5, 80))
        page.insert_image(fitz.Rect(400.0, 400.0, 450.0, 450.0), stream=_png_bytes(5, 5, 200))

    raw = _build_page(_offpage)
    cases.append(_run_case("partial_offpage_bg", raw))

    # 5. Form XObject (a separate 100x100 page shown into a 300x400 rect)
    #    carrying a full-size image and a small one: placements recurse into the
    #    XObject in target page space. has_large True.
    def _xobject(page: fitz.Page) -> None:
        sub = fitz.open()
        sp = sub.new_page(width=100.0, height=100.0)
        sp.insert_image(fitz.Rect(0.0, 0.0, 100.0, 100.0), stream=_png_bytes(10, 10, 140))
        sp.insert_image(fitz.Rect(70.0, 70.0, 100.0, 100.0), stream=_png_bytes(3, 3, 70))
        page.show_pdf_page(fitz.Rect(0.0, 0.0, 300.0, 400.0), sub, 0, keep_proportion=False)
        sub.close()

    raw = _build_page(_xobject)
    cases.append(_run_case("xobject_bg", raw))

    # 6. Large masked (SMask) image: still a single fill_image placement on both
    #    sides (get_image_info excludes the mask itself). has_large True.
    def _masked(page: fitz.Page) -> None:
        mask = fitz.Pixmap(fitz.csGRAY, fitz.IRect(0, 0, 10, 10))
        mask.clear_with(128)
        page.insert_image(
            fitz.Rect(0.0, 0.0, 300.0, 300.0),
            stream=_png_bytes(10, 10, 200),
            mask=mask.tobytes("png"))

    raw = _build_page(_masked)
    cases.append(_run_case("masked_image", raw))

    # 7. Multiple placements with one large background: has_large True.
    def _multi(page: fitz.Page) -> None:
        page.insert_image(fitz.Rect(0.0, 0.0, 300.0, 400.0), stream=_png_bytes(6, 8, 180))
        page.insert_image(fitz.Rect(20.0, 20.0, 60.0, 60.0), stream=_png_bytes(4, 4, 90))
        page.insert_image(fitz.Rect(240.0, 340.0, 280.0, 380.0), stream=_png_bytes(4, 4, 60))

    raw = _build_page(_multi)
    cases.append(_run_case("multi_placements", raw))

    # 8. 90-degree-rotated page with a large image (slightly larger than the
    #    content box): get_image_info reports the content-space bbox, so the
    #    native collector must clear rotation. has_large True.
    def _rotated(page: fitz.Page) -> None:
        page.insert_image(fitz.Rect(0.0, 0.0, 320.0, 420.0), stream=_png_bytes(8, 8, 160))
        page.set_rotation(90)

    raw = _build_page(_rotated)
    cases.append(_run_case("rotated_page", raw))

    # 9. Golden PDFs: every page of each embedded file. `3.pdf` is excluded (see
    #    module docstring) — its parity is covered by the smoke test.
    for name in ["1.pdf", "2.pdf"]:
        golden = os.path.join(GOLDEN_ROOT, name)
        assert os.path.exists(golden), f"golden {golden} missing"
        cases.append(_run_case(f"golden_{name[:-4]}", open(golden, "rb").read()))

    corpus = {"schema": "retainpdf_image_rects_corpus_v1", "cases": cases}
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
