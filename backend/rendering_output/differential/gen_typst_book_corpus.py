#!/usr/bin/env python3
"""Deterministic corpus for the whole-book overlay/background source builders.

Runs the REAL production `build_typst_book_overlay_source` /
`build_typst_book_background_source` on prebuilt `RenderBlock` DTOs (the layout
pipeline `build_render_blocks` is out of scope) and writes
`tests/book_corpus.json`. Rust `tests/book_diff.rs` replays the same DTOs and
asserts byte-identical source.
"""

import dataclasses
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "scripts"))

from services.rendering.layout.model.models import RenderBlock
from services.rendering.output.typst import source_pages
from services.rendering.output.typst.source_builder import build_typst_book_background_source
from services.rendering.output.typst.source_builder import build_typst_book_overlay_source

WORK_DIR = "/render_work"
FONT_FAMILY = "Source Han Serif SC"

# The production book builders run the layout pipeline (`build_render_blocks`)
# on `translated_items`. That pipeline is out of scope for the Rust port, which
# receives prebuilt RenderBlocks, so feed those through the real source loop
# directly.
source_pages.build_render_blocks = lambda translated_items, **kwargs: translated_items


def rblock(block_id, **overrides):
    fields = dict(
        block_id=block_id,
        bbox=[10.0, 20.0, 210.0, 70.0],
        cover_bbox=[10.0, 20.0, 210.0, 70.0],
        inner_bbox=[12.0, 22.0, 208.0, 68.0],
        markdown_text="",
        plain_text="",
        render_kind="plain",
        font_size_pt=10.0,
        leading_em=1.4,
    )
    fields.update(overrides)
    return RenderBlock(**fields)


def dto_dict(blk):
    out = {}
    for f in dataclasses.fields(blk):
        value = getattr(blk, f.name)
        if isinstance(value, tuple):
            value = list(value)
        elif value is None:
            value = []
        out[f.name] = value
    return out


def main():
    overlay_pages = [
        (612.0, 792.0, [rblock("a", render_kind="plain", markdown_text="Alpha", plain_text="Alpha")]),
        (
            612.0,
            792.0,
            [
                rblock("b", render_kind="plain_line", markdown_text="Beta", plain_text="Beta", use_cover_fill=True),
                rblock(
                    "c",
                    render_kind="plain",
                    markdown_text=r"See $x^2$",
                    plain_text="See formula",
                    fit_to_box=True,
                    fit_single_line=True,
                    fit_min_font_size_pt=6.0,
                    fit_max_font_size_pt=12.0,
                ),
            ],
        ),
    ]
    background_pages = [
        (0, 612.0, 792.0, [rblock("a", markdown_text="BG A", plain_text="BG A")]),
        (3, 612.0, 792.0, [rblock("b", markdown_text="BG B", plain_text="BG B", preserve_line_breaks=True)]),
    ]

    cases = [
        {
            "name": "book_overlay_no_cover",
            "kind": "overlay",
            "font_family": FONT_FAMILY,
            "include_cover_rect": False,
            "page_specs": [[dto_dict(b) for b in blocks] for (_, _, blocks) in overlay_pages],
            "dims": [(w, h) for (w, h, _) in overlay_pages],
            "expected": build_typst_book_overlay_source(
                overlay_pages, font_family=FONT_FAMILY, include_cover_rect=False
            ),
        },
        {
            "name": "book_overlay_with_cover",
            "kind": "overlay",
            "font_family": FONT_FAMILY,
            "include_cover_rect": True,
            "page_specs": [[dto_dict(b) for b in blocks] for (_, _, blocks) in overlay_pages],
            "dims": [(w, h) for (w, h, _) in overlay_pages],
            "expected": build_typst_book_overlay_source(
                overlay_pages, font_family=FONT_FAMILY, include_cover_rect=True
            ),
        },
        {
            "name": "book_background",
            "kind": "background",
            "font_family": FONT_FAMILY,
            "work_dir": os.path.join(WORK_DIR, "work"),
            "source_pdf_path": os.path.join(WORK_DIR, "background.pdf"),
            "page_specs": [[dto_dict(b) for b in blocks] for (_, _, _, blocks) in background_pages],
            "dims": [(idx, w, h) for (idx, w, h, _) in background_pages],
            "expected": build_typst_book_background_source(
                os.path.join(WORK_DIR, "background.pdf"),
                background_pages,
                work_dir=os.path.join(WORK_DIR, "work"),
                font_family=FONT_FAMILY,
            ),
        },
    ]

    out_path = os.path.join(os.path.dirname(__file__), "..", "tests", "book_corpus.json")
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(cases, handle, ensure_ascii=False, indent=2, sort_keys=True)
    print(f"wrote {len(cases)} cases to {out_path}")


if __name__ == "__main__":
    main()
