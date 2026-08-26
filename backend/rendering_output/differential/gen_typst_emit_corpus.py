#!/usr/bin/env python3
"""Deterministic emit corpus for `build_typst_source_from_page_specs`.

Runs the REAL production emitter on `RenderPageSpec` DTOs constructed directly
(the layout pipeline `build_render_blocks` is out of scope; its output shape is
`RenderLayoutBlock`, which we stand in for deterministically) and writes
`tests/emit_corpus.json`. Rust `tests/emit_diff.rs` replays the same DTOs and
asserts byte-identical Typst source.
"""

import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "scripts"))

from services.rendering.layout.model.models import RenderLayoutBlock, RenderPageSpec
from services.rendering.output.typst.emitter import build_typst_source_from_page_specs

WORK_DIR = "/render_work"


def rblock(block_id, **overrides):
    fields = dict(
        block_id=block_id,
        page_index=0,
        background_rect=[10.0, 20.0, 210.0, 70.0],
        content_rect=[12.0, 22.0, 208.0, 68.0],
        content_kind="plain",
        content_text="",
        plain_text="",
        math_map=[],
        font_size_pt=10.0,
        leading_em=1.4,
    )
    fields.update(overrides)
    return RenderLayoutBlock(**fields)


def page_spec(page_index, blocks, page_width=612.0, page_height=792.0):
    for block in blocks:
        block.page_index = page_index
    return RenderPageSpec(
        page_index=page_index,
        page_width_pt=page_width,
        page_height_pt=page_height,
        background_pdf_path="background.pdf",
        blocks=blocks,
    )


def main():
    cases = []

    def add(name, work_dir, background_path, page_specs):
        expected = build_typst_source_from_page_specs(
            background_pdf_path=background_path,
            page_specs=page_specs,
            work_dir=work_dir,
            font_family="Source Han Serif SC",
        )
        cases.append(
            {
                "name": name,
                "work_dir": work_dir,
                "background_pdf_path": background_path,
                "page_specs": [
                    {
                        "page_index": spec.page_index,
                        "page_width_pt": spec.page_width_pt,
                        "page_height_pt": spec.page_height_pt,
                        "background_pdf_path": spec.background_pdf_path,
                        "blocks": [
                            {
                                **{
                                    f.name: (
                                        list(getattr(block, f.name))
                                        if isinstance(getattr(block, f.name), tuple)
                                        else ([] if getattr(block, f.name) is None else getattr(block, f.name))
                                    )
                                    for f in __import__("dataclasses").fields(block)
                                }
                            }
                            for block in spec.blocks
                        ],
                    }
                    for spec in page_specs
                ],
                "expected": expected,
            }
        )

    add(
        "single_page_two_blocks",
        WORK_DIR,
        os.path.join(WORK_DIR, "background.pdf"),
        [
            page_spec(
                0,
                [
                    rblock("b0", content_kind="plain", content_text="Hello **world**", plain_text="Hello world"),
                    rblock(
                        "b1",
                        content_kind="plain",
                        content_text=r"See $x^2$ and $\frac{a}{b}$",
                        plain_text="See formula",
                        font_size_pt=12.0,
                        math_map=[
                            {"formula_text": r"x^2", "latex": r"x^2"},
                            {"formula_text": r"\frac{a}{b}", "latex": r"\frac{a}{b}"},
                        ],
                        use_cover_fill=True,
                        cover_fill=(0.95, 0.95, 1.0),
                    ),
                ],
            )
        ],
    )
    add(
        "two_pages_fit_and_toc",
        WORK_DIR,
        os.path.join(WORK_DIR, "background.pdf"),
        [
            page_spec(
                0,
                [
                    rblock(
                        "f0",
                        content_kind="plain",
                        content_text="Fit this line exactly",
                        plain_text="Fit this line exactly",
                        fit_to_box=True,
                        fit_single_line=True,
                        fit_min_font_size_pt=6.0,
                        fit_max_font_size_pt=12.0,
                    ),
                    rblock(
                        "t0",
                        content_kind="plain",
                        content_text="TOC",
                        plain_text="TOC",
                        toc_entries=[
                            {"title": "Chapter One", "page_label": "12", "bbox": [10.0, 20.0, 200.0, 30.0], "number": "1", "level": 1},
                        ],
                    ),
                ],
            ),
            page_spec(
                1,
                [
                    rblock(
                        "b0",
                        content_kind="plain",
                        content_text="long\nlines\npreserved",
                        plain_text="long\nlines\npreserved",
                        preserve_line_breaks=True,
                    ),
                    rblock(
                        "b1",
                        content_kind="plain_line",
                        content_text="Single line",
                        plain_text="Single line",
                        justify_text=True,
                    ),
                ],
            ),
        ],
    )
    add(
        "sibling_workdir_relpath",
        os.path.join(WORK_DIR, "work"),
        os.path.join(WORK_DIR, "background.pdf"),
        [
            page_spec(
                0,
                [
                    rblock(
                        "b0",
                        content_kind="plain",
                        content_text="X" * 60,
                        plain_text="Y" * 60,
                        first_line_indent_pt=8.0,
                    ),
                ],
            )
        ],
    )

    out_path = os.path.join(os.path.dirname(__file__), "..", "tests", "emit_corpus.json")
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(cases, handle, ensure_ascii=False, indent=2, sort_keys=True)
    print(f"wrote {len(cases)} cases to {out_path}")


if __name__ == "__main__":
    main()
