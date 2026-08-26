#!/usr/bin/env python3
"""Stage corpus generator (Phase 7R-4).

Builds deterministic multi-page synthetic letter PDFs (with TOC baked in),
runs the REAL production `build_clean_background_pdf`, and records:
  * the source PDF bytes (base64, trailer /ID pinned, TOC baked via set_toc),
  * the page rect,
  * `translated_pages` (page index -> item dicts, the stable serde DTO),
  * the redaction strategy and precleaned page indices,
  * the per-page `RedactionDiagnostics` (one entry per sorted translated page
    index; `None` for out-of-range / precleaned skips),
  * the output TOC outline list (count + [level, title, page] triples),
  * per-page words + ink_ratio for input and output.

The generator asserts the 7R-4 shim preconditions per case page before
recording, so a corpus case only captures a divergence-free shim hit:
  * `protect_formula_regions_in_redaction_items(items, items)` is identity on
    the engine-valid item set (config pinned via
    `apply_layout_tuning(source_cleanup_strategy="pikepdf_text_strip",
    default_text_overlay_cover_fill=False)` -> empty item policies; corpus
    formula items carry no translated text so they are invalid either way and
    their guard split never leaks into the valid set).
  * `collect_vector_text_rects(page, target_rects) == []` (corpus pages are
    built with `insert_text`, so there are no black-filled vector glyphs).

The Rust replay runs the ported `background::stage::build_clean_background_pdf`
(which calls the ported `copy_toc` and the per-page redaction engine), asserts
the exact per-page diagnostics + toc_entries, then measures the same output
facts and the output outline list.

Deterministic: fixed synthetic content, no RNG, stable iteration,
sort_keys + indent serialization.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/gen_stage_corpus.py
"""

import base64
import json
import os
import sys
import tempfile
from pathlib import Path

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from foundation.config.layout import apply_layout_tuning  # noqa: E402
from services.rendering.policy import (  # noqa: E402
    page_has_formula_region,
    protect_formula_regions_in_redaction_items,
)
from services.rendering.source.background.stage import build_clean_background_pdf  # noqa: E402
from services.rendering.source.cleanup.redaction_flow import (  # noqa: E402
    execute_redaction_flow,
)
from services.rendering.source.cleanup.text_extract import (  # noqa: E402
    extract_page_text_spans,
)
from services.rendering.source.cleanup.valid_items import (  # noqa: E402
    iter_valid_redaction_items,
)
from services.rendering.source.items import iter_valid_translated_items  # noqa: E402
from services.rendering.source.vector_text import collect_vector_text_rects  # noqa: E402

OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "stage_corpus.json"))

RENDER_SCALE = 2.0
INK_THRESHOLD = 250

PAGE_W = 612.0
PAGE_H = 792.0
DARK = (0.12, 0.12, 0.12)

FIXED_ID = (
    b"/ID[<30303030303030303030303030303030>"
    b"<30303030303030303030303030303030>]"
)

# Pin the cleanup policy so `build_render_page_policy` yields empty item
# policies -> `_apply_policy_fields_to_redaction_item` is a no-op.
apply_layout_tuning(
    source_cleanup_strategy="pikepdf_text_strip",
    default_text_overlay_cover_fill=False,
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


def build_source_pdf(pages_lines: list[list[tuple[float, str]]], toc: list[list]) -> bytes:
    """Letter PDF, one text line per (y, text) entry (font helv, 12pt), with an
    optional outline tree baked via `set_toc` and the trailer /ID pinned."""
    doc = fitz.open()
    for lines in pages_lines:
        page = doc.new_page(width=PAGE_W, height=PAGE_H)
        for y, text in lines:
            page.insert_text((60.0, y), text, fontsize=12, fontname="helv", color=DARK)
    if toc:
        doc.set_toc(toc)
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


def run_stage_case(
    name: str,
    source_bytes: bytes,
    translated_pages: dict[int, list[dict]],
    *,
    strategy: str | None,
    precleaned: list[int],
) -> dict:
    """Replicate the stage's per-page flow for diagnostics + shim asserts, then
    run the REAL `build_clean_background_pdf` for output facts + TOC."""
    d = fitz.open(stream=source_bytes, filetype="pdf")
    page_rect = [float(v) for v in d.load_page(0).rect]
    input_facts = page_facts(d)

    ordered = sorted(translated_pages)
    per_page_diagnostics: list[dict | None] = []
    for idx in ordered:
        if not (0 <= idx < len(d)):
            per_page_diagnostics.append(None)
            continue
        if idx in precleaned:
            per_page_diagnostics.append(None)
            continue
        page = d.load_page(idx)
        items = translated_pages[idx]
        protected = protect_formula_regions_in_redaction_items(items, items)

        valid_before = iter_valid_redaction_items(items)
        valid_after = iter_valid_redaction_items(protected)
        assert [v[0] for v in valid_before] == [v[0] for v in valid_after], (
            f"{name} p{idx}: protect changed valid rects"
        )
        assert [v[1] for v in valid_before] == [v[1] for v in valid_after], (
            f"{name} p{idx}: protect changed valid items"
        )

        target_rects = [fitz.Rect(v[0]) for v in iter_valid_translated_items(protected)]
        vector = collect_vector_text_rects(page, target_rects)
        assert vector == [], f"{name} p{idx}: vector text rects found {vector}"

        page_strategy = "visual_cover" if strategy is None and page_has_formula_region(items) else strategy
        diag = execute_redaction_flow(
            page,
            protected,
            fill_background=None,
            cover_only=False,
            strategy=page_strategy,
            visual_profile=None,
        )
        per_page_diagnostics.append(normalize_diagnostics(diag))
    d.close()

    with tempfile.TemporaryDirectory(prefix="rps-stage-") as tmp:
        in_path = Path(tmp) / "in.pdf"
        out_path = Path(tmp) / "out.pdf"
        in_path.write_bytes(source_bytes)
        build_clean_background_pdf(
            source_pdf_path=in_path,
            translated_pages=translated_pages,
            output_pdf_path=out_path,
            redaction_strategy=strategy,
            page_specs=None,
            source_text_precleaned_page_indices=frozenset(precleaned),
            visual_profile=None,
        )
        out = fitz.open(out_path)
        output_facts = page_facts(out)
        output_toc = [
            {"level": int(t[0]), "title": str(t[1]), "page": int(t[2])}
            for t in out.get_toc()
        ]
        toc_entries = len(output_toc)
        out.close()

    return {
        "name": name,
        "page_rect": page_rect,
        "source_pdf_b64": base64.b64encode(source_bytes).decode("ascii"),
        "translated_pages": {str(k): v for k, v in sorted(translated_pages.items())},
        "redaction_strategy": strategy,
        "precleaned_page_indices": precleaned,
        "toc_entries": toc_entries,
        "toc": output_toc,
        "per_page_diagnostics": per_page_diagnostics,
        "expected_input": {"pages": input_facts},
        "expected_output": {"pages": output_facts},
    }


def main() -> None:
    cases = []

    # (a) auto, no formula, 2 pages with TOC. Both pages safe-direct.
    pages_a = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    toc_a = [[1, "Intro", 1], [1, "Chapter Two", 2]]
    raw_a = build_source_pdf(pages_a, toc_a)
    d_a = fitz.open(stream=raw_a, filetype="pdf")
    items_a = {
        0: span_items_for(d_a.load_page(0), pages_a[0]),
        1: span_items_for(d_a.load_page(1), pages_a[1]),
    }
    d_a.close()
    cases.append(run_stage_case("auto_no_formula_toc", raw_a, items_a, strategy=None, precleaned=[]))

    # (b) explicit visual_cover, 2 pages.
    pages_b = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_b = build_source_pdf(pages_b, [])
    d_b = fitz.open(stream=raw_b, filetype="pdf")
    items_b = {
        0: span_items_for(d_b.load_page(0), pages_b[0]),
        1: span_items_for(d_b.load_page(1), pages_b[1]),
    }
    d_b.close()
    cases.append(run_stage_case("visual_cover_strategy", raw_b, items_b, strategy="visual_cover", precleaned=[]))

    # (c) visual_cover_and_remove_text, 2 pages.
    pages_c = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_c = build_source_pdf(pages_c, [])
    d_c = fitz.open(stream=raw_c, filetype="pdf")
    items_c = {
        0: span_items_for(d_c.load_page(0), pages_c[0]),
        1: span_items_for(d_c.load_page(1), pages_c[1]),
    }
    d_c.close()
    cases.append(run_stage_case(
        "visual_cover_and_remove_text_strategy",
        raw_c,
        items_c,
        strategy="visual_cover_and_remove_text",
        precleaned=[],
    ))

    # (d) precleaned page skipped: page 1 precleaned -> per-page diag None and
    #     the output page 1 facts equal the input page 1 facts.
    pages_d = [[(100.0, "Alpha")], [(100.0, "Gamma")]]
    raw_d = build_source_pdf(pages_d, [])
    d_d = fitz.open(stream=raw_d, filetype="pdf")
    items_d = {
        0: span_items_for(d_d.load_page(0), pages_d[0]),
        1: span_items_for(d_d.load_page(1), pages_d[1]),
    }
    d_d.close()
    cases.append(run_stage_case("precleaned_page_skip", raw_d, items_d, strategy=None, precleaned=[1]))

    # (e) auto with a formula item on page 0 -> page strategy flips to
    #     visual_cover. The formula item carries no translated text (invalid for
    #     the engine) and sits in an empty region, so the guard split is a no-op
    #     for the valid text items and the formula item never leaks in.
    pages_e = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    toc_e = [[1, "Intro", 1], [1, "Chapter Two", 2]]
    raw_e = build_source_pdf(pages_e, toc_e)
    d_e = fitz.open(stream=raw_e, filetype="pdf")
    formula_item = {
        "bbox": [60.0, 300.0, 300.0, 340.0],
        "translated_text": "",
        "block_type": "formula",
        "normalized_sub_type": "display_formula",
    }
    items_e = {
        0: span_items_for(d_e.load_page(0), pages_e[0]) + [dict(formula_item)],
        1: span_items_for(d_e.load_page(1), pages_e[1]),
    }
    d_e.close()
    cases.append(run_stage_case("auto_formula_flips_visual_cover", raw_e, items_e, strategy=None, precleaned=[]))

    corpus = {
        "schema": "retainpdf_stage_corpus_v1",
        "render_scale": RENDER_SCALE,
        "ink_threshold": INK_THRESHOLD,
        "cases": cases,
    }
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
