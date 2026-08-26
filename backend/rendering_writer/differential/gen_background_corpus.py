#!/usr/bin/env python3
"""Background-fill corpus generator for backend/rendering_writer (Phase 5D-7).

Builds deterministic synthetic letter PDFs (flat gray / colored / near-white /
gradient backgrounds with dark text lines), runs the REAL production
`sample_local_background_fill` / `draw_white_covers`, and records:
  * the input PDF bytes (base64, trailer /ID pinned),
  * the cover rects (page space),
  * the per-rect fill colors, quantized to integer 0..255,
  * the batched `LocalBackgroundSampler` fills for the ≥8-rect case,
  * per-page words + ink_ratio for input and after `draw_white_covers`.

The Rust replay renders the same base PDF with mupdf-rs (byte-identical to fitz:
both wrap MuPDF), recomputes fills via the ported `background::fill` functions,
and asserts each channel within ±2/255 plus the semantic output facts
(words ±10% rel, ink ±0.02 abs).

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_background_corpus.py
"""

import base64
import io
import json
import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.source.background.fill import (  # noqa: E402
    LocalBackgroundSampler,
    draw_white_covers,
    sample_local_background_fill,
)

OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "background_corpus.json"))

RENDER_SCALE = 2.0
INK_THRESHOLD = 250

PAGE_W = 612.0
PAGE_H = 792.0
DARK = (0.12, 0.12, 0.12)
TEXT = "The quick brown fox jumps over the lazy dog and the mindful river"


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
    facts = []
    for idx in range(doc.page_count):
        page = doc[idx]
        facts.append({"words": page_words(page), "ink_ratio": ink_ratio(page)})
    return facts


def quantize(fill: tuple[float, float, float]) -> list[int]:
    return [int(round(c * 255)) for c in fill]


FIXED_ID = (
    b"/ID[<30303030303030303030303030303030>"
    b"<30303030303030303030303030303030>]"
)


def _array_end(raw: bytes, open_bracket: int) -> int:
    """Index of the `]` closing the array at `open_bracket`, skipping literal
    strings `(...)` (with backslash escapes) and hex strings `<...>`."""
    i = open_bracket + 1
    n = len(raw)
    while i < n:
        c = raw[i]
        if c == 0x28:  # '(' literal string
            i += 1
            while i < n:
                c2 = raw[i]
                if c2 == 0x5C:  # backslash escapes next byte
                    i += 2
                    continue
                if c2 == 0x29:  # ')'
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
    """Replace every trailer /ID array with a fixed value (determinism)."""
    start = 0
    while True:
        idx = raw.find(b"/ID", start)
        if idx < 0:
            return raw
        j = idx + 3
        while j < len(raw) and raw[j] in b" \t\r\n":
            j += 1
        if j < len(raw) and raw[j] == 0x5B:  # '['
            end = _array_end(raw, j)
            if end > 0:
                raw = raw[:j] + FIXED_ID + raw[end + 1 :]
                start = j + len(FIXED_ID)
                continue
        start = idx + 3


def build_page(background: tuple | None, text_rects: list[fitz.Rect]) -> tuple[bytes, list[fitz.Rect]]:
    """One letter page with an optional full-page background fill and one text
    line per rect, top-anchored inside the rect. Returns (pinned bytes, rects)."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    if background is not None:
        page.draw_rect(page.rect, color=None, fill=background)
    rects: list[fitz.Rect] = []
    for idx, rect in enumerate(text_rects):
        y = rect.y0 + 12.0
        page.insert_text((rect.x0 + 6.0, y), TEXT, fontsize=12, fontname="helv", color=DARK)
        rects.append(fitz.Rect(rect))
    raw = doc.tobytes()
    doc.close()
    return _pin_pdf_id(raw), rects


def row_rects(count: int, x0: float, y_start: float, w: float, h: float, gap: float) -> list[fitz.Rect]:
    return [fitz.Rect(x0, y_start + i * gap, x0 + w, y_start + i * gap + h) for i in range(count)]


def make_case(
    name: str,
    rects: list[fitz.Rect],
    background: tuple | None,
    *,
    record_sampler: bool = False,
) -> dict:
    base_bytes, rects = build_page(background, rects)
    page = fitz.open(stream=base_bytes, filetype="pdf").load_page(0)
    page_rect = fitz.Rect(page.rect)

    free_fills = [quantize(sample_local_background_fill(page, r)) for r in rects]

    sampler_fills = None
    if record_sampler:
        sampler = LocalBackgroundSampler.build(page, rects)
        if sampler is not None:
            sampler_fills = [
                quantize(sampler.sample_local_background_fill(r)) for r in rects
            ]

    input_facts = {"pages": [{"words": page_words(page), "ink_ratio": ink_ratio(page)}]}

    out_doc = fitz.open(stream=base_bytes, filetype="pdf")
    out_page = out_doc.load_page(0)
    draw_white_covers(out_page, rects)
    output_facts = {"pages": page_facts(out_doc)}

    return {
        "name": name,
        "page_count": 1,
        "page_rect": [float(v) for v in page_rect],
        "base_pdf_b64": base64.b64encode(base_bytes).decode("ascii"),
        "rects": [[float(v) for v in r] for r in rects],
        "free_fills": free_fills,
        "sampler_fills": sampler_fills,
        "expected_input": input_facts,
        "expected_output": output_facts,
    }


def main() -> None:
    cases = []

    # Flat light-gray background + text; dominant non-white fill (~0.949).
    # 10 rects (>= 8) with 34pt spacing: the union clip area stays <= 35% of
    # the page (needed for LocalBackgroundSampler.build to succeed without the
    # 24-rect full-page fallback), and adjacent outer rects (rect +/- 6pt) do
    # not overlap, so covers stay draw-order-independent.
    cases.append(
        make_case(
            "gray_text",
            row_rects(10, 110.0, 120.0, 320.0, 14.0, 34.0),
            (0.95, 0.95, 0.95),
            record_sampler=True,
        )
    )

    # Colored background + text; dominant colored fill.
    cases.append(
        make_case(
            "colored_text",
            row_rects(5, 120.0, 140.0, 300.0, 14.0, 80.0),
            (0.85, 0.60, 0.40),
        )
    )

    # Near-white background: dominant rejected (max channel >= 0.98), so the
    # fill comes from the clean outer border (channel medians ~0.984) and the
    # covers push dark text above the ink threshold → strong output signal.
    cases.append(
        make_case(
            "near_white_text",
            row_rects(6, 130.0, 130.0, 280.0, 14.0, 72.0),
            (0.984, 0.984, 0.984),
        )
    )

    # Vertical gradient background (white → 0.90 gray): outer border is
    # high-spread, so the fill comes from the trimmed robust median.
    def gradient_bg(page: fitz.Page, a: tuple, b: tuple) -> None:
        page.draw_rect(
            page.rect,
            color=None,
            fill=a,
            fill_opacity=0.0,
        )
        # fitz has no gradient helper; emulate a smooth vertical ramp with
        # horizontal bands (each 4pt tall). Enough bands → smooth at scale 2.
        steps = 90
        for i in range(steps):
            t = i / max(steps - 1, 1)
            band = fitz.Rect(0, i * PAGE_H / steps, PAGE_W, (i + 1) * PAGE_H / steps)
            color = tuple(a[j] + (b[j] - a[j]) * t for j in range(3))
            page.draw_rect(band, color=None, fill=color)

    gdoc = fitz.open()
    gpage = gdoc.new_page(width=PAGE_W, height=PAGE_H)
    gradient_bg(gpage, (1.0, 1.0, 1.0), (0.90, 0.90, 0.90))
    g_rects = row_rects(4, 150.0, 180.0, 260.0, 14.0, 120.0)
    for rect in g_rects:
        gpage.insert_text(
            (rect.x0 + 6.0, rect.y0 + 12.0),
            TEXT,
            fontsize=12,
            fontname="helv",
            color=DARK,
        )
    grad_raw = _pin_pdf_id(gdoc.tobytes())
    gdoc.close()
    gpage2 = fitz.open(stream=grad_raw, filetype="pdf").load_page(0)
    g_free = [quantize(sample_local_background_fill(gpage2, r)) for r in g_rects]
    g_in = {"pages": [{"words": page_words(gpage2), "ink_ratio": ink_ratio(gpage2)}]}
    gout_doc = fitz.open(stream=grad_raw, filetype="pdf")
    draw_white_covers(gout_doc.load_page(0), g_rects)
    cases.append(
        {
            "name": "gradient_text",
            "page_count": 1,
            "page_rect": [0.0, 0.0, PAGE_W, PAGE_H],
            "base_pdf_b64": base64.b64encode(grad_raw).decode("ascii"),
            "rects": [[float(v) for v in r] for r in g_rects],
            "free_fills": g_free,
            "sampler_fills": None,
            "expected_input": g_in,
            "expected_output": {"pages": page_facts(gout_doc)},
        }
    )

    corpus = {
        "schema": "retainpdf_background_corpus_v1",
        "render_scale": RENDER_SCALE,
        "ink_threshold": INK_THRESHOLD,
        "cases": cases,
    }
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
