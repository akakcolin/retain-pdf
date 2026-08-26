#!/usr/bin/env python3
"""Background-image detection corpus generator (Phase 7R-1).

Builds deterministic synthetic letter PDFs with manually constructed image
XObjects (FlateDecode DeviceRGB / DeviceGray / 1-bit masks, or none), runs the
REAL production `page_has_large_background_image` / `pick_primary_background_image`
/ `page_has_tiled_background_images`, and records:
  * the input PDF bytes (base64, trailer /ID pinned),
  * the page rect,
  * the detect facts: has_large / tiled / primary xref + placement rect.

The Rust replay walks the page content stream (CTM at each `Do`) to reproduce
the same placement rects and asserts the detect facts exactly.

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_detect_corpus.py
"""

import base64
import json
import os
import sys
import zlib

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.source.background.detect import (  # noqa: E402
    page_has_large_background_image,
    page_has_tiled_background_images,
    pick_primary_background_image,
)

OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "detect_corpus.json"))

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
        if c == 0x28:  # '(' literal string
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
        elif c == 0x3C:  # '<' hex string
            j = raw.find(b">", i)
            if j < 0:
                return -1
            i = j + 1
        elif c == 0x5D:  # ']'
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


def _add_flate_image(doc: fitz.Document, page: fitz.Page, width: int, height: int, colorspace: str, raw: bytes) -> int:
    """Manually construct an image XObject with a FlateDecode stream and place
    it full-page. Returns the image xref."""
    xref = doc.get_new_xref()
    if colorspace == "mask":
        obj = (
            "<< /Type /XObject /Subtype /Image /Width %d /Height %d "
            "/BitsPerComponent 1 /ImageMask true /Filter /FlateDecode >>"
            % (width, height)
        )
    else:
        obj = (
            "<< /Type /XObject /Subtype /Image /Width %d /Height %d "
            "/BitsPerComponent 8 /ColorSpace /%s /Filter /FlateDecode >>"
            % (width, height, colorspace)
        )
    doc.update_object(xref, obj)
    doc.update_stream(xref, raw, new=1, compress=1)
    res = int(doc.xref_get_key(page.xref, "Resources")[1].split()[0])
    doc.xref_set_key(res, "XObject/Bg0", "%d 0 R" % xref)
    ops = "q %f 0 0 %f 0 0 cm /Bg0 Do Q" % (PAGE_W, PAGE_H)
    cxref = doc.get_new_xref()
    doc.update_object(cxref, "<< >>")
    doc.update_stream(cxref, ops.encode(), new=1)
    doc.xref_set_key(page.xref, "Contents", "%d 0 R" % cxref)
    return xref


def _place_images(doc: fitz.Document, page: fitz.Page, placements: list[tuple[str, float, float, float, float]]) -> None:
    """Write one content stream placing each `(name, x0, y0, w, h)` image."""
    ops = "\n".join(
        "q %f 0 0 %f %f %f cm /%s Do Q" % (w, h, x0, y0, name)
        for name, x0, y0, w, h in placements
    )
    cxref = doc.get_new_xref()
    doc.update_object(cxref, "<< >>")
    doc.update_stream(cxref, ops.encode(), new=1)
    doc.xref_set_key(page.xref, "Contents", "%d 0 R" % cxref)


def make_fullpage_image_case(name: str, colorspace: str, width: int, height: int, raw: bytes) -> dict:
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    _add_flate_image(doc, page, width, height, colorspace, raw)
    raw_bytes = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()

    d = fitz.open(stream=raw_bytes, filetype="pdf")
    p = d.load_page(0)
    page_rect = [float(v) for v in p.rect]
    has_large = page_has_large_background_image(p)
    tiled = page_has_tiled_background_images(p)
    primary = pick_primary_background_image(p)
    record = {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw_bytes).decode("ascii"),
        "has_large": has_large,
        "tiled": tiled,
        "primary_xref": primary[0] if primary else None,
        "primary_rect": [float(v) for v in primary[1]] if primary else None,
    }
    d.close()
    return record


def make_tiled_case(name: str) -> dict:
    """10 page-wide image strips stacked vertically (>= 8 page-wide tiles)."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    res = int(doc.xref_get_key(page.xref, "Resources")[1].split()[0])
    w, h = 90, 90
    raw = bytes(bytearray([220, 225, 230]) * (w * h))
    xrefs = []
    for i in range(10):
        xref = doc.get_new_xref()
        doc.update_object(
            xref,
            "<< /Type /XObject /Subtype /Image /Width %d /Height %d /BitsPerComponent 8 "
            "/ColorSpace /DeviceRGB /Filter /FlateDecode >>" % (w, h),
        )
        doc.update_stream(xref, raw, new=1, compress=1)
        doc.xref_set_key(res, "XObject/Bg%d" % i, "%d 0 R" % xref)
        xrefs.append(xref)
    # tiles spanning most of the page width, stacked vertically
    _place_images(
        doc,
        page,
        [("Bg%d" % i, 6.0, 60.0 + i * 72.0, 600.0, 70.0) for i in range(10)],
    )
    raw_bytes = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()

    d = fitz.open(stream=raw_bytes, filetype="pdf")
    p = d.load_page(0)
    page_rect = [float(v) for v in p.rect]
    has_large = page_has_large_background_image(p)
    tiled = page_has_tiled_background_images(p)
    primary = pick_primary_background_image(p)
    record = {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw_bytes).decode("ascii"),
        "has_large": has_large,
        "tiled": tiled,
        "primary_xref": primary[0] if primary else None,
        "primary_rect": [float(v) for v in primary[1]] if primary else None,
    }
    d.close()
    return record


def make_no_image_case(name: str) -> dict:
    """Vector fill background only (no image XObjects)."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    page.draw_rect(page.rect, color=None, fill=(0.95, 0.95, 0.95))
    raw_bytes = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()

    d = fitz.open(stream=raw_bytes, filetype="pdf")
    p = d.load_page(0)
    page_rect = [float(v) for v in p.rect]
    record = {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw_bytes).decode("ascii"),
        "has_large": page_has_large_background_image(p),
        "tiled": page_has_tiled_background_images(p),
        "primary_xref": None,
        "primary_rect": None,
    }
    d.close()
    return record


def make_small_image_case(name: str) -> dict:
    """One small image placement that does not cover the page."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    res = int(doc.xref_get_key(page.xref, "Resources")[1].split()[0])
    w, h = 24, 24
    raw = bytes(bytearray([220, 225, 230]) * (w * h))
    xref = doc.get_new_xref()
    doc.update_object(
        xref,
        "<< /Type /XObject /Subtype /Image /Width %d /Height %d /BitsPerComponent 8 "
        "/ColorSpace /DeviceRGB /Filter /FlateDecode >>" % (w, h),
    )
    doc.update_stream(xref, raw, new=1, compress=1)
    doc.xref_set_key(res, "XObject/Bg0", "%d 0 R" % xref)
    _place_images(doc, page, [("Bg0", 40.0, 60.0, 200.0, 90.0)])
    raw_bytes = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()

    d = fitz.open(stream=raw_bytes, filetype="pdf")
    p = d.load_page(0)
    page_rect = [float(v) for v in p.rect]
    record = {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw_bytes).decode("ascii"),
        "has_large": page_has_large_background_image(p),
        "tiled": page_has_tiled_background_images(p),
        "primary_xref": None,
        "primary_rect": None,
    }
    d.close()
    return record


def main() -> None:
    cases = []
    # Full-page RGB background image (primary detected, not tiled).
    rgb = bytearray(120 * 156 * 3)
    for y in range(156):
        for x in range(120):
            o = (y * 120 + x) * 3
            rgb[o:o + 3] = (250, 250, 250)
    cases.append(make_fullpage_image_case("fullpage_rgb", "DeviceRGB", 120, 156, bytes(rgb)))

    # Full-page gray background image.
    gray = bytes(bytearray([250]) * (120 * 156))
    cases.append(make_fullpage_image_case("fullpage_gray", "DeviceGray", 120, 156, gray))

    # Full-page 1-bit image mask.
    mask = bytes(bytearray([0x00]) * ((120 * 156 + 7) // 8))
    cases.append(make_fullpage_image_case("fullpage_mask", "mask", 120, 156, mask))

    cases.append(make_tiled_case("tiled"))
    cases.append(make_no_image_case("no_image"))
    cases.append(make_small_image_case("small_image"))

    corpus = {
        "schema": "retainpdf_detect_corpus_v1",
        "cases": cases,
    }
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
