#!/usr/bin/env python3
"""Redaction engine corpus generator (Phase 7R-2).

Builds deterministic synthetic letter PDFs with text lines placed at known
positions, runs the REAL production `execute_redaction_flow`, and records:
  * the input PDF bytes (base64, trailer /ID pinned),
  * the page rect,
  * the translated-item dicts (the stable serde DTO surface),
  * the strategy / cover_only flags,
  * the normalized `RedactionDiagnostics` (full field set),
  * per-page words + ink_ratio for input and output.

Each item is designed so the 7R-2 safe-direct-only matcher is deterministic:
  * safe-direct items: `bbox` == an exact fitz text-span rect -> production
    `safe_direct_redaction_rect` returns `expand_word_rect(span)` (asserted),
    so the item removes its span's text.
  * cover items: `bbox` over an empty region (no span center inside) and no
    `source_text` -> production returns `[]` (asserted), so the item is a
    whole-bbox cover. The generator asserts these per-item outcomes before
    running the flow, so a corpus case only records a divergence-free shim hit.

The Rust replay runs the ported `background::redaction::execute_redaction_flow`,
asserts the exact diagnostics, and measures the same page facts.

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_redaction_corpus.py
"""

import base64
import json
import os
import sys

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.source.cleanup.redaction_flow import (  # noqa: E402
    execute_redaction_flow,
)
from services.rendering.source.cleanup.text_extract import (  # noqa: E402
    _extract_page_text_spans_python as extract_page_text_spans,
)
from services.rendering.source.cleanup.text_matching import (  # noqa: E402
    item_removable_text_rects,
)

OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "redaction_corpus.json"))

RENDER_SCALE = 2.0
INK_THRESHOLD = 250

PAGE_W = 612.0
PAGE_H = 792.0
DARK = (0.12, 0.12, 0.12)

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


def build_text_page(lines: list[tuple[float, str]]) -> bytes:
    """Letter page with one text line per (y, text) entry (font helv, 12pt)."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_W, height=PAGE_H)
    for y, text in lines:
        page.insert_text((60.0, y), text, fontsize=12, fontname="helv", color=DARK)
    raw = _pin_pdf_id(doc.tobytes(garbage=0))
    doc.close()
    return raw


def span_items_for(page: fitz.Page, y_text_pairs: list[tuple[float, str]]) -> list[dict]:
    """One item per line whose bbox is the exact fitz span rect of that line."""
    spans = extract_page_text_spans(page)
    by_y: dict[float, tuple[fitz.Rect, str]] = {}
    for rect, text in spans:
        key = round(rect.y0, 1)
        by_y[key] = (rect, text)
    items = []
    for y, text in y_text_pairs:
        rect = None
        for key in (round(y - 9.0, 1), round(y - 8.0, 1)):
            if key in by_y:
                rect = by_y[key][0]
                break
        if rect is None:
            keys = sorted(by_y.keys(), key=lambda k: abs(k - y))
            rect = by_y[keys[0]][0]
        items.append({"bbox": [rect.x0, rect.y0, rect.x1, rect.y1], "translated_text": text})
    return items


def normalize_diagnostics(d: dict) -> dict:
    return {
        "items": int(d.get("items", 0)),
        "raw_removable_rects": int(d.get("raw_removable_rects", 0)),
        "merged_removable_rects": int(d.get("merged_removable_rects", 0)),
        "cover_rects": int(d.get("cover_rects", 0)),
        "fast_page_cover_only": bool(d.get("fast_page_cover_only", False)),
        "item_fast_cover_count": int(d.get("item_fast_cover_count", 0)),
        "route": str(d.get("route", "")),
        "strategy": str(d.get("strategy", "")),
        "uses_pymupdf_redaction": bool(d.get("uses_pymupdf_redaction", False)),
        "legacy_pdf_write_reason": str(d.get("legacy_pdf_write_reason", "")),
        "visual_profile_cover_rects": int(d.get("visual_profile_cover_rects", 0)),
        "auto_text_cleanup_math_protected": bool(
            d.get("auto_text_cleanup_math_protected", False)
        ),
        "auto_text_cleanup_items_skipped": int(
            d.get("auto_text_cleanup_items_skipped", 0)
        ),
    }


def run_case(
    name: str,
    raw_bytes: bytes,
    items: list[dict],
    *,
    strategy: str | None,
    cover_only: bool,
    expected_removable: list[bool] | None = None,
) -> dict:
    """Run production on a copy, assert the per-item removable outcomes, record."""
    d = fitz.open(stream=raw_bytes, filetype="pdf")
    p = d.load_page(0)
    page_rect = [float(v) for v in p.rect]
    input_facts = [{"words": page_words(p), "ink_ratio": ink_ratio(p)}]

    # Assert each item's safe-direct outcome (7R-2 shim contract).
    if expected_removable is not None:
        assert len(expected_removable) == len(items), f"{name}: expected_removable length"
        for (bbox, expected) in zip(items, expected_removable):
            rect = fitz.Rect(bbox["bbox"])
            produced = item_removable_text_rects(p, bbox, rect)
            if expected:
                assert produced, f"{name}: expected removable rect, got {produced}"
            else:
                assert not produced, f"{name}: expected cover, got {produced}"

    out_doc = fitz.open(stream=raw_bytes, filetype="pdf")
    out_page = out_doc.load_page(0)
    diagnostics = execute_redaction_flow(
        out_page,
        items,
        cover_only=cover_only,
        strategy=strategy,
    )
    output_facts = page_facts(out_doc)
    out_doc.close()
    d.close()

    return {
        "name": name,
        "page_rect": page_rect,
        "input_pdf_b64": base64.b64encode(raw_bytes).decode("ascii"),
        "items": items,
        "strategy": strategy,
        "cover_only": cover_only,
        "expected_diagnostics": normalize_diagnostics(diagnostics),
        "expected_input": {"pages": input_facts},
        "expected_output": {"pages": output_facts},
    }


def make_case(name: str, strategy: str | None, cover_only: bool, lines, items, expected_removable) -> dict:
    raw = build_text_page(lines)
    return run_case(
        name,
        raw,
        items,
        strategy=strategy,
        cover_only=cover_only,
        expected_removable=expected_removable,
    )


def main() -> None:
    cases = []

    # 1. Auto: two safe-direct lines -> both spans removed.
    lines_a = [(100.0, "Alpha"), (150.0, "Beta")]
    d_a = fitz.open(stream=build_text_page(lines_a), filetype="pdf")
    items_a = span_items_for(d_a.load_page(0), lines_a)
    cases.append(make_case("auto_safe_direct_two", None, False, lines_a, items_a, [True, True]))

    # 2. Auto: single safe-direct line.
    lines_b = [(200.0, "Gamma")]
    d_b = fitz.open(stream=build_text_page(lines_b), filetype="pdf")
    items_b = span_items_for(d_b.load_page(0), lines_b)
    cases.append(make_case("auto_safe_direct_single", None, False, lines_b, items_b, [True]))

    # 3. Auto: one cover item over an empty region (no span center inside).
    cover_item = {"bbox": [100.0, 400.0, 500.0, 420.0], "translated_text": "Covered"}
    cases.append(make_case("auto_cover", None, False, [(100.0, "Delta")], [cover_item], [False]))

    # 4. Auto: one safe-direct line + one cover item.
    lines_d = [(100.0, "Alpha"), (150.0, "Beta")]
    d_d = fitz.open(stream=build_text_page(lines_d), filetype="pdf")
    items_d = span_items_for(d_d.load_page(0), lines_d) + [dict(cover_item)]
    cases.append(make_case("auto_mixed", None, False, lines_d, items_d, [True, True, False]))

    # 5. Auto: one safe-direct line + one force-visual-cover-only item (skipped
    #    risky in auto -> whole-bbox cover, auto_text_cleanup_items_skipped=1).
    risky_item = {
        "bbox": [100.0, 500.0, 500.0, 520.0],
        "translated_text": "Risky",
        "_force_visual_cover_only": True,
    }
    cases.append(make_case("auto_skipped_risky", None, False, lines_b, items_b + [risky_item], [True, False]))

    # 6. Visual cover: two items, covers only (no text removal).
    lines_e = [(100.0, "Alpha"), (150.0, "Beta")]
    d_e = fitz.open(stream=build_text_page(lines_e), filetype="pdf")
    items_e = span_items_for(d_e.load_page(0), lines_e)
    cases.append(make_case("visual_cover_two", "visual_cover", False, lines_e, items_e, [True, True]))

    # 7. Visual cover + remove text: two items, covers + redaction.
    cases.append(make_case(
        "visual_cover_and_remove_text_two",
        "visual_cover_and_remove_text",
        False,
        lines_e,
        items_e,
        [True, True],
    ))

    # 8. cover_only=True with strategy None resolves to visual_cover.
    cases.append(make_case("cover_only_resolves_visual_cover", None, True, lines_b, items_b, [True]))

    # 9-10. No valid items -> empty result (strategy None vs explicit).
    empty_items = [{"bbox": [], "translated_text": "x"}, {"bbox": [0, 0, 0, 0], "translated_text": "y"}]
    cases.append(make_case("empty_auto", None, False, [(100.0, "Alpha")], empty_items, None))
    cases.append(make_case("empty_visual", "visual_cover", False, [(100.0, "Alpha")], empty_items, None))

    # --- Phase 7R-3 full text-span matcher cases ---------------------------------
    # Each item below deliberately breaks safe-direct (bbox much bigger than the
    # span) so the block / word / whole-bbox layers are exercised deterministically.

    # 11. Word-hit: a long line, item bbox around ONE word only. The block center
    #     (mid-line) falls outside the item bbox, so the block layer misses and the
    #     word layer removes just that word.
    long_line = [(100.0, "The quick brown fox jumps over the lazy dog")]
    d_word = fitz.open(stream=build_text_page(long_line), filetype="pdf")
    p_word = d_word.load_page(0)
    quick_rect = next(fitz.Rect(w[:4]) for w in p_word.get_text("words") if w[4] == "quick")
    word_item = {
        "bbox": [quick_rect.x0 - 2.0, quick_rect.y0 - 4.0, quick_rect.x1 + 2.0, quick_rect.y1 + 4.0],
        "translated_text": "快速",
        "source_text": "quick",
    }
    cases.append(make_case("textmatch_word_hit", None, False, long_line, [word_item], [True]))

    # 12. Block-hit: item bbox == the line block inflated horizontally, so safe-direct
    #     fails on size while the block center stays inside -> the block layer returns
    #     the whole block expanded, removing all its words.
    line_block = [(150.0, "Alpha Beta Gamma")]
    d_block = fitz.open(stream=build_text_page(line_block), filetype="pdf")
    p_block = d_block.load_page(0)
    block_rect = next(fitz.Rect(b[:4]) for b in p_block.get_text("blocks") if b[6] == 0)
    block_item = {
        "bbox": [block_rect.x0 - 8.0, block_rect.y0 - 2.0, block_rect.x1 + 8.0, block_rect.y1 + 2.0],
        "translated_text": "阿尔法 贝塔",
        "source_text": "Alpha Beta",
    }
    cases.append(make_case("textmatch_block_hit", None, False, line_block, [block_item], [True]))

    # 13. No match: source_text words absent from the page -> block/word layers both
    #     miss -> [] removable -> whole-bbox cover (text stays in the layer).
    nomatch_line = [(200.0, "Delta Epsilon")]
    d_nm = fitz.open(stream=build_text_page(nomatch_line), filetype="pdf")
    p_nm = d_nm.load_page(0)
    nm_block = next(fitz.Rect(b[:4]) for b in p_nm.get_text("blocks") if b[6] == 0)
    nomatch_item = {
        "bbox": [nm_block.x0 - 8.0, nm_block.y0 - 2.0, nm_block.x1 + 8.0, nm_block.y1 + 2.0],
        "translated_text": "德尔塔",
        "source_text": "zzzzzz",
    }
    cases.append(make_case("textmatch_no_match_cover", None, False, nomatch_line, [nomatch_item], [False]))

    # 14. Whole-bbox fallback: source_text normalizes to zero words ("!!!") but the
    #     item contains two owned words ("brown fox", block center outside) -> with
    #     empty source_words and pdf_words >= 2 the item removes its whole bbox.
    d_wb = fitz.open(stream=build_text_page(long_line), filetype="pdf")
    p_wb = d_wb.load_page(0)
    brown_rect = next(fitz.Rect(w[:4]) for w in p_wb.get_text("words") if w[4] == "brown")
    fox_rect = next(fitz.Rect(w[:4]) for w in p_wb.get_text("words") if w[4] == "fox")
    bbox_item = {
        "bbox": [brown_rect.x0 - 2.0, brown_rect.y0 - 4.0, fox_rect.x1 + 2.0, fox_rect.y1 + 4.0],
        "translated_text": "棕色狐狸",
        "source_text": "!!!",
    }
    cases.append(make_case("textmatch_whole_bbox_fallback", None, False, long_line, [bbox_item], [True]))

    corpus = {
        "schema": "retainpdf_redaction_corpus_v1",
        "render_scale": RENDER_SCALE,
        "ink_threshold": INK_THRESHOLD,
        "cases": cases,
    }
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
