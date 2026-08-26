#!/usr/bin/env python3
"""Background-image replacement corpus generator (Phase 7R-1).

Builds deterministic synthetic letter PDFs with manually constructed image
XObjects (FlateDecode DeviceRGB / DeviceGray / 1-bit masks, plus a tiled and a
no-image page), runs the REAL production `replace_background_image_page`, and
records:
  * the input PDF bytes (base64, trailer /ID pinned),
  * the page rect and the translated-item bboxes,
  * the detect facts (has_large / primary xref + placement rect),
  * the `changed` flag,
  * the rewritten background stream bytes (decoded) after replacement,
  * per-page words + ink_ratio for input and output.

The Rust replay re-derives the detect facts, rewrites the stream with the ported
`background::image_route`, asserts the `changed` flag, asserts the rewritten
decoded stream is byte-identical, and measures the same page facts.

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_image_route_corpus.py
"""

import base64
import json
import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.source.background.detect import (  # noqa: E402
    page_has_large_background_image,
    pick_primary_background_image,
)
from services.rendering.source.background.image_route import (  # noqa: E402
    replace_background_image_page,
)

OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "image_route_corpus.json"))

RENDER_SCALE = 2.0
INK_THRESHOLD = 250

PAGE_W = 612.0
PAGE_H = 792.0
DARK = (0.12, 0.12, 0.12)
TEXT = "The quick brown fox jumps over the lazy dog"

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


def ink_ratio(page: fitz.Page) -> float:
    pix = page.get_pixmap(
        matrix=fitz.Matrix(RENDER_SCALE, RENDER_SCALE),
        colorspace=fitz.csGRAY,
        alpha=False,
    )
    total = pix.width * pix.height
    if total == 0:
        return 0.0
    dark = sum(1 for b in pix.samples if b < INK_THRESHOLD)
    return dark / total


def page_words(page: fitz.Page) -> int:
    return len(page.get_text("words"))


def page_facts(doc: fitz.Document) -> list[dict]:
    return [{"words": page_words(doc[i]), "ink_ratio": ink_ratio(doc[i])} for i in range(doc.page_count)]


def _add_flate_image(doc: fitz.Document, page: fitz.Page, width: int, height: int, colorspace: str, raw: bytes) -> int:
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
    cxref = doc.get_new_xref()
    doc.update_object(cxref, "<< >>")
    doc.update_stream(cxref, ("q %f 0 0 %f 0 0 cm /Bg0 Do Q" % (PAGE_W, PAGE_H)).encode(), new=1)
    doc.xref_set_key(page.xref, "Contents", "%d 0 R" % cxref)
    return xref


def _add_text(page: fitz.Page, y: float, text: str) -> None:
    page.insert_text((60.0, y), text, fontsize=12, fontname="helv", color=DARK)


def build_image_page(colorspace: str, width: int, height: int, raw: bytes, text_y: float) -> bytes:
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    _add_flate_image(doc, page, width, height, colorspace, raw)
    _add_text(page, text_y, TEXT)
    out = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()
    return out


def rgb_white_with_dark_block(width: int, height: int) -> bytes:
    raw = bytearray(width * height * 3)
    for y in range(height):
        for x in range(width):
            o = (y * width + x) * 3
            if 10 <= x < width - 5 and 15 <= y < height - 10:
                raw[o:o + 3] = (60, 60, 60)
            else:
                raw[o:o + 3] = (250, 250, 250)
    return bytes(raw)


def gray_light_with_dark_block(width: int, height: int) -> bytes:
    raw = bytearray(width * height)
    for y in range(height):
        for x in range(width):
            raw[y * width + x] = 60 if (10 <= x < width - 5 and 15 <= y < height - 10) else 240
    return bytes(raw)


def make_rewrite_case(
    name: str,
    colorspace: str,
    raw: bytes,
    item_bboxes: list[list[float]],
    width: int = 120,
    height: int = 156,
) -> dict:
    raw_bytes = build_image_page(colorspace, width, height, raw, 740.0)
    d = fitz.open(stream=raw_bytes, filetype="pdf")
    p = d.load_page(0)
    page_rect = [float(v) for v in p.rect]
    has_large = page_has_large_background_image(p)
    primary = pick_primary_background_image(p)
    primary_xref = primary[0] if primary else None
    primary_rect = [float(v) for v in primary[1]] if primary else None
    input_facts = [{"words": page_words(p), "ink_ratio": ink_ratio(p)}]

    items = [{"bbox": list(b), "translated_text": TEXT} for b in item_bboxes]
    out_doc = fitz.open(stream=raw_bytes, filetype="pdf")
    out_page = out_doc.load_page(0)
    changed = replace_background_image_page(out_page, items)
    rewritten_b64 = None
    if changed and primary_xref is not None:
        rewritten_b64 = base64.b64encode(out_doc.xref_stream(primary_xref)).decode("ascii")
    output_facts = page_facts(out_doc)
    out_doc.close()
    d.close()

    return {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw_bytes).decode("ascii"),
        "item_bboxes": [list(b) for b in item_bboxes],
        "has_large": has_large,
        "primary_xref": primary_xref,
        "primary_rect": primary_rect,
        "changed": changed,
        "rewritten_b64": rewritten_b64,
        "expected_input": {"pages": input_facts},
        "expected_output": {"pages": output_facts},
    }


def make_tiled_case(name: str) -> dict:
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    res = int(doc.xref_get_key(page.xref, "Resources")[1].split()[0])
    w, h = 90, 90
    raw = bytes(bytearray([220, 225, 230]) * (w * h))
    for i in range(10):
        xref = doc.get_new_xref()
        doc.update_object(
            xref,
            "<< /Type /XObject /Subtype /Image /Width %d /Height %d /BitsPerComponent 8 "
            "/ColorSpace /DeviceRGB /Filter /FlateDecode >>" % (w, h),
        )
        doc.update_stream(xref, raw, new=1, compress=1)
        doc.xref_set_key(res, "XObject/Bg%d" % i, "%d 0 R" % xref)
    ops = "\n".join(
        "q %f 0 0 %f %f %f cm /Bg%d Do Q" % (600.0, 70.0, 6.0, 60.0 + i * 72.0, i)
        for i in range(10)
    )
    cxref = doc.get_new_xref()
    doc.update_object(cxref, "<< >>")
    doc.update_stream(cxref, ops.encode(), new=1)
    doc.xref_set_key(page.xref, "Contents", "%d 0 R" % cxref)
    _add_text(page, 740.0, TEXT)
    raw_bytes = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()

    d = fitz.open(stream=raw_bytes, filetype="pdf")
    p = d.load_page(0)
    page_rect = [float(v) for v in p.rect]
    has_large = page_has_large_background_image(p)
    primary = pick_primary_background_image(p)
    input_facts = [{"words": page_words(p), "ink_ratio": ink_ratio(p)}]

    items = [{"bbox": [100.0, 100.0, 400.0, 120.0], "translated_text": TEXT}]
    out_doc = fitz.open(stream=raw_bytes, filetype="pdf")
    out_page = out_doc.load_page(0)
    changed = replace_background_image_page(out_page, items)
    output_facts = page_facts(out_doc)
    out_doc.close()
    d.close()

    return {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw_bytes).decode("ascii"),
        "item_bboxes": [[100.0, 100.0, 400.0, 120.0]],
        "has_large": has_large,
        "primary_xref": primary[0] if primary else None,
        "primary_rect": [float(v) for v in primary[1]] if primary else None,
        "changed": changed,
        "rewritten_b64": None,
        "expected_input": {"pages": input_facts},
        "expected_output": {"pages": output_facts},
    }


def make_no_background_case(name: str) -> dict:
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    page.draw_rect(page.rect, color=None, fill=(0.95, 0.95, 0.95))
    _add_text(page, 740.0, TEXT)
    raw_bytes = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()

    d = fitz.open(stream=raw_bytes, filetype="pdf")
    p = d.load_page(0)
    page_rect = [float(v) for v in p.rect]
    has_large = page_has_large_background_image(p)
    primary = pick_primary_background_image(p)
    input_facts = [{"words": page_words(p), "ink_ratio": ink_ratio(p)}]

    items = [{"bbox": [100.0, 100.0, 400.0, 120.0], "translated_text": TEXT}]
    out_doc = fitz.open(stream=raw_bytes, filetype="pdf")
    out_page = out_doc.load_page(0)
    changed = replace_background_image_page(out_page, items)
    output_facts = page_facts(out_doc)
    out_doc.close()
    d.close()

    return {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw_bytes).decode("ascii"),
        "item_bboxes": [[100.0, 100.0, 400.0, 120.0]],
        "has_large": has_large,
        "primary_xref": primary[0] if primary else None,
        "primary_rect": [float(v) for v in primary[1]] if primary else None,
        "changed": changed,
        "rewritten_b64": None,
        "expected_input": {"pages": input_facts},
        "expected_output": {"pages": output_facts},
    }


def main() -> None:
    cases = []
    # RGB full-page background with a large dark region under the item rects.
    rgb = rgb_white_with_dark_block(120, 156)
    cases.append(make_rewrite_case("rgb_rewrite", "DeviceRGB", rgb, [[50.0, 100.0, 550.0, 600.0]]))

    # Gray full-page background with a dark region.
    gray = gray_light_with_dark_block(120, 156)
    cases.append(make_rewrite_case("gray_rewrite", "DeviceGray", gray, [[50.0, 100.0, 550.0, 600.0]]))

    # 1-bit mask: painting the item region sets MSB-first bits (byte-identity check).
    mask = bytes(bytearray([0x00]) * ((120 * 156 + 7) // 8))
    cases.append(make_rewrite_case("mask_rewrite", "mask", mask, [[50.0, 100.0, 550.0, 600.0]]))

    # Odd-width mask: PIL mode "1" row-pads to ceil(9/8) = 2 bytes per row; a
    # 9x104 mask needs 208 bytes. Guards the row-padding ported in image_route.
    mask_odd = bytes(bytearray([0x00]) * (2 * 104))
    cases.append(
        make_rewrite_case(
            "mask_rewrite_odd",
            "mask",
            mask_odd,
            [[50.0, 100.0, 550.0, 600.0]],
            width=9,
            height=104,
        )
    )

    cases.append(make_tiled_case("tiled_no_primary"))
    cases.append(make_no_background_case("no_background"))

    corpus = {
        "schema": "retainpdf_image_route_corpus_v1",
        "render_scale": RENDER_SCALE,
        "ink_threshold": INK_THRESHOLD,
        "cases": cases,
    }
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
