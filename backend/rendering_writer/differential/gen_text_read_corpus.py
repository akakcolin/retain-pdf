#!/usr/bin/env python3
"""Text-read corpus generator (Phase B2-9).

Records the pure-Python reference outputs of the `source/cleanup` text-read
family for deterministic synthetic PDFs and every page of the golden PDFs
`resources/samples/golden-pdfs/1.pdf` and `2.pdf` (the 19 MB `3.pdf` is
excluded — its parity is covered by the file-backed smoke test):

  * the source PDF bytes (base64, trailer /ID pinned),
  * the page rect,
  * per-page `text_spans`/`text_blocks` (fitz `get_text("dict")` spans and
    `get_text("blocks")` text blocks as `[x0,y0,x1,y1,text]`), `math_rects`
    (deduped math-font span rects), and `span_heights` (non-math span heights
    > 0.5) — all computed from the `_python` reference implementations (the
    deterministic, non-routed oracles).

The Rust replay (`tests/text_read_diff.rs`) runs
`rendering_reader::PdfDocument::{page_text_spans, page_text_blocks,
page_math_rects, page_span_heights}` on the same bytes and asserts it
reproduces the recorded values positionally (rect within 0.01 pt, text / height
exact). Order matters: both fitz and the native collectors iterate the same
mupdf stext page (blocks -> lines -> spans).

Synthetic coverage pins the parity surface: single-font multi-line text (block
text "\n" joins), font/size/color/flag-driven span splits on one line, a
math-named font (STIX Two Math) mixed with a regular font (the math-rect /
non-math-height filters), and a 90-degree-rotated page (fitz get_text clears
/rotate, so the native collectors must too). Ligatures (PRESERVE_LIGATURES) are
covered by the golden PDFs, which carry ﬁ/ﬂ/ﬀ/ﬃ on nearly every page.

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_text_read_corpus.py
"""

import base64
import json
import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

# Corpus lives next to its replay (`rendering_reader/tests/text_read_diff.rs`).
OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "..", "rendering_reader", "tests", "text_read_corpus.json"))
GOLDEN_ROOT = os.path.abspath(
    os.path.join(_HERE, "..", "..", "..", "resources", "samples", "golden-pdfs")
)
# A system font whose embedded PostScript name ("STIXTwoMath-Regular") contains
# "math" — `is_special_math_font` fires on it, giving the math-rect filter a
# deterministic real-world target. Regeneration requires it; the committed
# corpus embeds the font inside the case PDF.
STIX_MATH_FONT = "/System/Library/Fonts/Supplemental/STIXTwoMath.otf"

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
    """Open a blank 300x400 page, run `draw(page)`, pin /ID, return bytes."""
    doc = fitz.open()
    page = doc.new_page(width=300.0, height=400.0)
    draw(page)
    raw = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()
    return raw


def _record_text(raw: bytes) -> dict:
    """Record the reference text-read outputs (the `_python` oracles) per page."""
    from services.rendering.source.cleanup.text_extract import (
        _extract_page_text_blocks_python,
        _extract_page_text_spans_python,
    )
    from services.rendering.source.cleanup.math_spans import (
        _collect_page_math_protection_rects_python,
        _collect_page_non_math_span_heights_python,
    )

    doc = fitz.open(stream=raw, filetype="pdf")
    pages = {}
    for idx in range(doc.page_count):
        page = doc.load_page(idx)
        pages[str(idx)] = {
            "page_rect": [float(v) for v in page.rect],
            "text_spans": [
                [float(v) for v in rect] + [text]
                for rect, text in _extract_page_text_spans_python(page)
            ],
            "text_blocks": [
                [float(v) for v in rect] + [text]
                for rect, text in _extract_page_text_blocks_python(page)
            ],
            "math_rects": [
                [float(v) for v in rect]
                for rect in _collect_page_math_protection_rects_python(page)
            ],
            "span_heights": [
                float(h) for h in _collect_page_non_math_span_heights_python(page)
            ],
        }
    doc.close()
    return pages


def _run_case(name: str, raw: bytes) -> dict:
    return {
        "name": name,
        "pdf_b64": base64.b64encode(raw).decode("ascii"),
        "pages": _record_text(raw),
    }


def main() -> None:
    cases = []

    # 1. Single font, three lines close enough to form one block: block text is
    #    the "\n"-joined line texts; spans one per line; heights all > 0.5.
    def _simple(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Alpha line one", fontsize=14)
        page.insert_text((20.0, 82.0), "Beta line two", fontsize=14)
        page.insert_text((20.0, 104.0), "Gamma line three", fontsize=14)

    cases.append(_run_case("simple_text", _build_page(_simple)))

    # 2. Two fonts on the same line: span split by font name.
    def _fonts(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Helvetica ", fontname="helv", fontsize=14)
        page.insert_text((95.0, 60.0), "Times", fontname="tiro", fontsize=14)

    cases.append(_run_case("multi_font", _build_page(_fonts)))

    # 3. Two colors on the same line: span split by color.
    def _colors(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "black ", fontname="helv", fontsize=14)
        page.insert_text((60.0, 60.0), "red", fontname="helv", fontsize=14, color=(1.0, 0.0, 0.0))

    cases.append(_run_case("multi_color", _build_page(_colors)))

    # 4. Two sizes on the same line: span split by size.
    def _sizes(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "small ", fontname="helv", fontsize=12)
        page.insert_text((70.0, 60.0), "large", fontname="helv", fontsize=20)

    cases.append(_run_case("multi_size", _build_page(_sizes)))

    # 5. Regular + bold + italic on the same line: span splits by char flags.
    def _styled(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "reg ", fontname="helv", fontsize=14)
        page.insert_text((55.0, 60.0), "bold ", fontname="hebo", fontsize=14)
        page.insert_text((105.0, 60.0), "italic", fontname="heit", fontsize=14)

    cases.append(_run_case("styled_text", _build_page(_styled)))

    # 6. A math-named font (STIX Two Math) mixed with Helvetica: the math rects
    #    filter keeps the STIX span; the non-math heights filter keeps Helvetica.
    def _math(page: fitz.Page) -> None:
        with open(STIX_MATH_FONT, "rb") as f:
            stix = f.read()
        page.insert_font(fontname="STIXTwoMath", fontbuffer=stix)
        page.insert_text((20.0, 60.0), "x + y = z", fontname="STIXTwoMath", fontsize=16)
        page.insert_text((20.0, 90.0), "plain words", fontname="helv", fontsize=14)

    raw = _build_page(_math)
    cases.append(_run_case("math_font", raw))

    # 7. 90-degree-rotated page: fitz get_text clears /rotate, so spans/blocks
    #    are in the 300x400 content space; the native collectors must too.
    def _rotated(page: fitz.Page) -> None:
        page.insert_text((20.0, 60.0), "Rotated text", fontsize=16)
        page.set_rotation(90)

    cases.append(_run_case("rotated_page", _build_page(_rotated)))

    # 8. Golden PDFs: every page of each embedded file. `3.pdf` is excluded (see
    #    module docstring). The goldens carry the ligature-rich real-world text
    #    that guards PRESERVE_LIGATURES and the multi-font span grouping.
    for name in ["1.pdf", "2.pdf"]:
        golden = os.path.join(GOLDEN_ROOT, name)
        assert os.path.exists(golden), f"golden {golden} missing"
        cases.append(_run_case(f"golden_{name[:-4]}", open(golden, "rb").read()))

    corpus = {"schema": "retainpdf_text_read_corpus_v1", "cases": cases}
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
