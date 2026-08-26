#!/usr/bin/env python3
"""Deterministic single-block corpus for the `build_typst_block` differential.

Runs the REAL production `build_typst_block` on a battery of representative
`RenderBlock` instances (covering every render_kind / fit branch) and writes
`tests/block_corpus.json`. Rust `tests/block_diff.rs` replays the same DTOs and
asserts byte-identical output.
"""

import dataclasses
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "scripts"))

from services.rendering.layout.model.models import RenderBlock
from services.rendering.output.typst.block_renderer import build_typst_block

BASE_BBOX = [10.0, 20.0, 210.0, 70.0]


def block(**overrides):
    fields = dict(
        block_id="blk_1",
        bbox=BASE_BBOX,
        cover_bbox=BASE_BBOX,
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
    """Dataclass to JSON in the exact shape the Rust serde DTO expects."""
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
    cases = []

    def add(name, blk):
        cases.append(
            {
                "name": name,
                "block": dto_dict(blk),
                "expected": {
                    "include_fill_false": build_typst_block(blk.block_id, blk, include_fill=False),
                    "include_fill_true": build_typst_block(blk.block_id, blk, include_fill=True),
                },
            }
        )

    add(
        "plain_short",
        block(
            render_kind="plain",
            plain_text="Hello world",
            markdown_text="Hello **world**",
            text_color=(0.1, 0.2, 0.3),
        ),
    )
    add(
        "plain_line_short",
        block(render_kind="plain_line", plain_text="Single line", markdown_text="Single *line*"),
    )
    add(
        "plain_long_fit",
        block(
            render_kind="plain",
            plain_text="X" * 60,
            markdown_text="Y" * 60,
            font_size_pt=11.5,
            first_line_indent_pt=8.0,
            justify_text=True,
            text_color=(0.9, 0.1, 0.2),
        ),
    )
    add(
        "formula_insets",
        block(
            render_kind="plain",
            markdown_text=r"See $x^2$ and $\frac{a}{b}$ here",
            plain_text="See formula here",
            font_size_pt=12.0,
            math_map=[
                {"formula_text": r"x^2", "latex": r"x^2"},
                {"formula_text": r"\frac{a}{b}", "latex": r"\frac{a}{b}"},
            ],
        ),
    )
    add(
        "long_inline_math_risk",
        block(
            render_kind="plain",
            markdown_text="$" + "k" * 70 + "$",
            plain_text="L" * 70,
            font_size_pt=12.0,
            fit_to_box=True,
            fit_min_font_size_pt=8.0,
            fit_max_font_size_pt=12.0,
            fit_min_leading_em=1.1,
        ),
    )
    add(
        "cover_fill",
        block(
            render_kind="plain",
            markdown_text="Covered",
            plain_text="Covered",
            cover_fill=(1.0, 0.0, 0.0),
            use_cover_fill=True,
        ),
    )
    add(
        "preserve_line_breaks",
        block(
            render_kind="plain",
            markdown_text="line one\n\nline two\nline three",
            plain_text="line one\nline two\nline three",
            preserve_line_breaks=True,
            math_map=[],
        ),
    )
    add(
        "preserved_line_boxes",
        block(
            render_kind="plain",
            markdown_text="boxed lines",
            plain_text="boxed lines",
            preserve_line_breaks=True,
            preserved_line_boxes=[
                {"text": "First line", "bbox": [10.0, 20.0, 200.0, 30.0]},
                {"text": "Second line", "bbox": [10.0, 32.0, 205.0, 42.0]},
            ],
        ),
    )
    add(
        "toc_entries",
        block(
            render_kind="plain",
            markdown_text="TOC",
            plain_text="TOC",
            toc_entries=[
                {"title": "Chapter One", "page_label": "12", "bbox": [10.0, 20.0, 200.0, 30.0], "number": "1", "level": 1},
                {"title": "Deep", "page_label": "45", "bbox": [10.0, 32.0, 200.0, 42.0], "number": "1.2", "level": 2},
            ],
        ),
    )
    add(
        "fit_single_line",
        block(
            render_kind="plain",
            markdown_text="Fit this line exactly",
            plain_text="Fit this line exactly",
            fit_to_box=True,
            fit_single_line=True,
            fit_min_font_size_pt=6.0,
            fit_max_font_size_pt=12.0,
            fit_target_width_pt=150.0,
            fit_target_height_pt=40.0,
            fit_shift_up_pt=2.5,
        ),
    )
    add(
        "fit_markdown",
        block(
            render_kind="plain",
            markdown_text="Fit to box markdown",
            plain_text="Fit to box",
            fit_to_box=True,
            fit_min_font_size_pt=8.0,
            fit_max_font_size_pt=12.0,
            fit_min_leading_em=1.0,
            fit_max_height_pt=45.0,
            fit_target_width_pt=160.0,
            fit_target_height_pt=40.0,
            first_line_indent_pt=6.0,
            justify_text=True,
        ),
    )

    out_path = os.path.join(os.path.dirname(__file__), "..", "tests", "block_corpus.json")
    with open(out_path, "w", encoding="utf-8") as handle:
        json.dump(cases, handle, ensure_ascii=False, indent=2, sort_keys=True)
    print(f"wrote {len(cases)} cases to {out_path}")


if __name__ == "__main__":
    main()
