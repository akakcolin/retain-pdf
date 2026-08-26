#!/usr/bin/env python3
"""Deterministic corpus for the whole-book pikepdf overlay merge.

Runs the REAL production pipeline on a synthesized base PDF:
  1. synthesize a deterministic base PDF (fitz text, trailer `/ID` stripped),
  2. build the whole-book overlay source (`build_typst_book_overlay_source`)
     and compile it with the real typst CLI (`compile_typst_book_overlay_pdf`),
  3. merge with the real production `overlay_pdf_pages_with_pikepdf`,
  4. record per-page words/ink_ratio facts of the merged output plus the base
     and overlay PDFs (b64).

Rust `tests/merge_diff.rs` replays the same inputs through the ported merge
(`merge::overlay_pdf_pages`) and asserts semantic-fact equivalence.
"""

import base64
import json
import os
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "scripts"))

import fitz

from services.rendering.layout.model.models import RenderBlock
from services.rendering.output.typst import source_pages
from services.rendering.output.typst.compiler import compile_typst_book_overlay_pdf
from services.rendering.document.pikepdf_overlay import overlay_pdf_pages_with_pikepdf

# The production book builders run the layout pipeline (`build_render_blocks`)
# on `translated_items`. That pipeline is out of scope for the Rust port, which
# receives prebuilt RenderBlocks, so feed those through the real source loop
# directly.
source_pages.build_render_blocks = lambda translated_items, **kwargs: translated_items

RENDER_SCALE = 2.0
INK_THRESHOLD = 250
FONT_FAMILY = "Source Han Serif SC"

# typst embeds a /CreationDate (second granularity) unless pinned; fix it so the
# compiled overlay PDF is byte-identical across runs. The env var propagates to
# the typst CLI subprocess the production compiler spawns.
os.environ["SOURCE_DATE_EPOCH"] = "0"

ID_PATTERN = re.compile(rb"/ID\s*\[<[0-9A-Fa-f]+><[0-9A-Fa-f]+>\]")


def rblock(block_id, **overrides):
    fields = dict(
        block_id=block_id,
        bbox=[10.0, 20.0, 300.0, 60.0],
        cover_bbox=[10.0, 20.0, 300.0, 60.0],
        inner_bbox=[14.0, 26.0, 296.0, 54.0],
        markdown_text="",
        plain_text="",
        render_kind="plain",
        font_size_pt=14.0,
        leading_em=1.4,
    )
    fields.update(overrides)
    return RenderBlock(**fields)


def synth_base(path, sizes, texts):
    """Deterministic base PDF: fitz pages with text, trailer `/ID` stripped."""
    doc = fitz.open()
    for (width, height), text in zip(sizes, texts):
        page = doc.new_page(width=width, height=height)
        page.insert_text((72, 100), text, fontsize=12, fontname="helv")
    doc.save(path, garbage=3, deflate=True)
    doc.close()
    data = open(path, "rb").read()
    data = ID_PATTERN.sub(b"", data)
    with open(path, "wb") as handle:
        handle.write(data)


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


def page_facts(doc: fitz.Document) -> list[dict]:
    facts = []
    for idx in range(doc.page_count):
        page = doc[idx]
        facts.append({"words": len(page.get_text("words")), "ink_ratio": ink_ratio(page)})
    return facts


def dto_dict(blk):
    out = {}
    for f in __import__("dataclasses").fields(blk):
        value = getattr(blk, f.name)
        if isinstance(value, tuple):
            value = list(value)
        elif value is None:
            value = []
        out[f.name] = value
    return out


def run_case(name, sizes, texts, block_specs, source_page_indices):
    tmp = tempfile.mkdtemp(prefix="rp_merge_corpus_")
    base_path = os.path.join(tmp, "base.pdf")
    synth_base(base_path, sizes, texts)

    page_specs = [
        (width, height, [rblock(bid, **over) for (bid, over) in blocks])
        for (width, height), blocks in zip(sizes, block_specs)
    ]
    overlay_pdf = compile_typst_book_overlay_pdf(
        page_specs,
        stem="overlay",
        font_family=FONT_FAMILY,
        include_cover_rect=False,
        work_dir=Path(tmp) / "book-overlays",
    )
    merged_path = os.path.join(tmp, "merged.pdf")
    res = overlay_pdf_pages_with_pikepdf(
        source_pdf_path=Path(base_path),
        overlay_pdf_path=Path(overlay_pdf),
        output_pdf_path=Path(merged_path),
        source_page_indices=source_page_indices,
    )
    assert res.pages_merged == len(source_page_indices), res

    merged_doc = fitz.open(merged_path)
    try:
        expected = {"pages": page_facts(merged_doc)}
    finally:
        merged_doc.close()

    with open(base_path, "rb") as handle:
        base_b64 = base64.b64encode(handle.read()).decode("ascii")
    with open(overlay_pdf, "rb") as handle:
        overlay_b64 = base64.b64encode(handle.read()).decode("ascii")

    return {
        "name": name,
        "base_pdf_b64": base_b64,
        "overlay_pdf_b64": overlay_b64,
        "source_page_indices": list(source_page_indices),
        "expected": expected,
    }


def main():
    letter = (612.0, 792.0)
    cases = [
        run_case(
            "merge_single_page",
            [letter],
            ["BASE PAGE 0 header"],
            [[("a", dict(markdown_text="Overlay Alpha", plain_text="Overlay Alpha"))]],
            [0],
        ),
        run_case(
            "merge_multi_page",
            [letter, letter, letter],
            ["BASE PAGE 0 header", "BASE PAGE 1 header", "BASE PAGE 2 header"],
            [
                [("a", dict(markdown_text="Overlay Alpha", plain_text="Overlay Alpha"))],
                [("b", dict(markdown_text="Overlay Beta", plain_text="Overlay Beta"))],
                [("c", dict(markdown_text="Overlay Gamma", plain_text="Overlay Gamma"))],
            ],
            [0, 1, 2],
        ),
        run_case(
            "merge_reordered_skip",
            [letter, letter, letter],
            ["BASE PAGE 0 header", "BASE PAGE 1 header", "BASE PAGE 2 header"],
            [
                [("x", dict(markdown_text="Overlay Xray", plain_text="Overlay Xray"))],
                [("y", dict(markdown_text="Overlay Yankee", plain_text="Overlay Yankee"))],
            ],
            [2, 0],
        ),
    ]

    out_path = os.path.join(os.path.dirname(__file__), "..", "tests", "merge_corpus.json")
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(
            {
                "schema": "retainpdf_merge_corpus_v1",
                "render_scale": RENDER_SCALE,
                "ink_threshold": INK_THRESHOLD,
                "cases": cases,
            },
            handle,
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
    print(f"wrote {len(cases)} cases to {out_path}")


if __name__ == "__main__":
    main()
