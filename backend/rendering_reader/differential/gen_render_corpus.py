#!/usr/bin/env python3
"""Render corpus generator for backend/rendering_reader (Phase 5B).

Replays the golden PDFs (resources/samples/golden-pdfs/{1,2}.pdf) through the
real production indent pipeline. For every text block that passes
`is_first_line_indent_candidate` it records fitz's own
`detect_first_line_indent_pt` (page-render path) and
`detect_first_line_indent_pt_with_displaylist` (display-list path), plus the
block geometry. For a capped, size-limited subset it also stores the rendered
grayscale pixels (fitz `get_pixmap`, `Matrix(2,2)`, `csGRAY`, `alpha=False`,
`clip=bbox & page.rect`) as base64 for both the page and display-list paths.

Writes backend/rendering_reader/tests/render_corpus.json.

Rust tests in render_diff.rs open the same PDFs with mupdf-rs, render the same
clips, and assert (1) pixel parity with the stored fitz pixels, (2) cross-path
parity between the two mupdf-rs renderers, and (3) result parity of
`rendering_core::first_line_indent` against the stored fitz indent values.

Deterministic: fixed PDFs, no RNG, stable page/block iteration, sort_keys +
indent serialization.

Run from backend/scripts:
    /tmp/rpdf-venv/bin/python ../rendering_reader/differential/gen_render_corpus.py
"""

import base64
import json
import os
import sys
from statistics import median

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
# -> backend/scripts (for services.*).
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

from services.rendering.layout.payload.first_line_indent import (  # noqa: E402
    INDENT_RENDER_SCALE,
    detect_first_line_indent_pt,
    detect_first_line_indent_pt_with_displaylist,
    is_first_line_indent_candidate,
)

REPO_ROOT = os.path.abspath(os.path.join(_HERE, "..", "..", ".."))
GOLDEN_SAMPLE_ROOT = os.path.join(REPO_ROOT, "resources", "samples", "golden-pdfs")
OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "render_corpus.json"))

GOLDEN_PDFS = [
    ("1.pdf", "editable-paper"),
    ("2.pdf", "pseudo-editable"),
]

# Pixel-subset budget: keep the checked-in JSON small. Blocks larger than
# MAX_PIXELS_PER_RENDER still get an indent entry, just no raw pixels.
MAX_PIXELS_PER_RENDER = 150_000
PIXEL_BUDGET_PER_PDF = 4


def page_text_width_med(page) -> float:
    widths = [b[2] - b[0] for b in page.get_text("blocks") if b[6] == 0]
    widths = [w for w in widths if w > 0]
    return median(widths) if widths else 0.0


def attach_pixels(doc, dl, page, page_rect, cand):
    """Render cand's clip via both fitz paths; attach pixels if small enough."""
    clip = fitz.Rect(cand["bbox"]) & page_rect
    if clip.is_empty or clip.width <= 0 or clip.height <= 0:
        return
    matrix = fitz.Matrix(INDENT_RENDER_SCALE, INDENT_RENDER_SCALE)
    pix_page = page.get_pixmap(matrix=matrix, colorspace=fitz.csGRAY, alpha=False, clip=clip)
    pix_dl = dl.get_pixmap(matrix=matrix, colorspace=fitz.csGRAY, alpha=False, clip=clip)
    if 0 < pix_page.width * pix_page.height <= MAX_PIXELS_PER_RENDER:
        cand["pixels"] = {
            "page": {
                "w": pix_page.width,
                "h": pix_page.height,
                "samples_b64": base64.b64encode(pix_page.samples).decode("ascii"),
            },
            "dl": {
                "w": pix_dl.width,
                "h": pix_dl.height,
                "samples_b64": base64.b64encode(pix_dl.samples).decode("ascii"),
            },
        }


def main():
    payload = {
        "schema": "retainpdf_render_corpus_v1",
        "render_scale": INDENT_RENDER_SCALE,
        "candidates": [],
    }
    for filename, category in GOLDEN_PDFS:
        path = os.path.join(GOLDEN_SAMPLE_ROOT, filename)
        if not os.path.exists(path):
            raise RuntimeError(f"golden sample not found: {path}")
        doc = fitz.open(path)
        # Track the per-page render context so pixel attachment can reuse the
        # already-built display list without reopening pages.
        per_page = {}
        candidates = []
        for idx in range(doc.page_count):
            page = doc.load_page(idx)
            page_rect = page.rect
            dl = page.get_displaylist()
            per_page[idx] = (page, page_rect, dl)
            ptwm = page_text_width_med(page)
            text_dict = page.get_text("dict")
            for block in text_dict["blocks"]:
                if block.get("type") != 0:
                    continue
                bbox = [float(v) for v in block["bbox"]]
                sizes = [s["size"] for line in block["lines"] for s in line["spans"]]
                font_size_pt = median(sizes) if sizes else 12.0
                text = "".join(
                    s["text"] for line in block["lines"] for s in line["spans"]
                )
                item = {
                    "bbox": bbox,
                    "source_text": text,
                    "lines": [],
                    "layout_role": "paragraph",
                    "semantic_role": "",
                    "tags": [],
                }
                if not is_first_line_indent_candidate(item, page_text_width_med=ptwm):
                    continue
                cand = {
                    "pdf": filename,
                    "page": idx,
                    "bbox": bbox,
                    "font_size_pt": font_size_pt,
                    "fitz_page_indent_pt": detect_first_line_indent_pt(
                        doc,
                        item,
                        page_idx=idx,
                        font_size_pt=font_size_pt,
                        page_text_width_med=ptwm,
                    ),
                    "fitz_dl_indent_pt": detect_first_line_indent_pt_with_displaylist(
                        doc,
                        dl,
                        item,
                        page_idx=idx,
                        font_size_pt=font_size_pt,
                        page_text_width_med=ptwm,
                    ),
                }
                candidates.append(cand)
        # Pixel subset: prefer nonzero-indent blocks (more interesting geometry),
        # then zero-indent ones, up to the per-pdf budget.
        nonzero = [c for c in candidates if c["fitz_page_indent_pt"] != 0.0]
        zero = [c for c in candidates if c["fitz_page_indent_pt"] == 0.0]
        picked = (nonzero + zero)[:PIXEL_BUDGET_PER_PDF]
        for cand in picked:
            page, page_rect, dl = per_page[cand["page"]]
            attach_pixels(doc, dl, page, page_rect, cand)
        payload["candidates"].extend(candidates)
        doc.close()
        print(f"generated {filename} ({category})")
    os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
    with open(OUT_PATH, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, ensure_ascii=False, sort_keys=True, indent=1)
    total = len(payload["candidates"])
    with_pix = sum(1 for c in payload["candidates"] if "pixels" in c)
    size = os.path.getsize(OUT_PATH)
    nonzero = sum(
        1
        for c in payload["candidates"]
        if c["fitz_page_indent_pt"] != 0.0 or c["fitz_dl_indent_pt"] != 0.0
    )
    print(
        f"wrote {total} candidates ({with_pix} with pixels, {nonzero} nonzero indent), "
        f"{size / 1024:.0f} KiB -> {OUT_PATH}"
    )


if __name__ == "__main__":
    main()
