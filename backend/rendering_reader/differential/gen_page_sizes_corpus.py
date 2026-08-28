#!/usr/bin/env python3
"""Page-sizes corpus generator (Phase B2-1).

Builds deterministic synthetic multi-page PDFs (varied sizes, rotations,
cropboxes) and records per-page `rect` / `cropbox` / `rotation` (fitz) plus the
source PDF bytes. The Rust replay (`tests/page_sizes_diff.rs`) opens the same
bytes via the `PdfDocument` trait and asserts `page_rect` / `page_cropbox` /
`page_rotation` match — the contract the bridge `read_page_sizes` entry (and the
layout `_native.read_source_page_sizes` shim) relies on for the
`build_render_page_specs` size lookup.

Deterministic: fixed content, no RNG, stable iteration, sort_keys + indent.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_reader/differential/gen_page_sizes_corpus.py
"""

import base64
import json
import os

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
OUT_PATH = os.path.abspath(os.path.join(_HERE, "..", "tests", "page_sizes_corpus.json"))


def build_pdf(pages) -> bytes:
    """`pages`: list of `(width, height, rotation, cropbox|None)` tuples."""
    doc = fitz.open()
    for width, height, rotation, cropbox in pages:
        page = doc.new_page(width=width, height=height)
        if rotation:
            page.set_rotation(rotation)
        if cropbox is not None:
            page.set_cropbox(fitz.Rect(*cropbox))
        page.insert_text((72.0, 72.0), f"page {len(doc)}", fontsize=12, fontname="helv")
    raw = doc.tobytes(garbage=0)
    doc.close()
    return raw


def record(name: str, pages) -> dict:
    raw = build_pdf(pages)
    d = fitz.open(stream=raw, filetype="pdf")
    facts: dict[str, dict] = {}
    for idx in range(d.page_count):
        p = d.load_page(idx)
        facts[str(idx)] = {
            "rect": [float(v) for v in p.rect],
            "cropbox": [float(v) for v in p.cropbox],
            "rotation": int(p.rotation),
        }
    d.close()
    return {
        "name": name,
        "pdf_b64": base64.b64encode(raw).decode("ascii"),
        "pages": facts,
    }


def main() -> None:
    cases = [
        record("letter_2page", [(612.0, 792.0, 0, None), (612.0, 792.0, 0, None)]),
        record("rotated_90", [(612.0, 792.0, 90, None), (612.0, 792.0, 0, None)]),
        record("a4_rotated_270", [(595.0, 842.0, 270, None)]),
        record("mixed_sizes", [(612.0, 792.0, 0, None), (595.0, 842.0, 0, None)]),
        record("cropbox_modified", [(612.0, 792.0, 0, (36.0, 36.0, 576.0, 756.0))]),
        record(
            "three_page_mixed_rotated",
            [
                (612.0, 792.0, 0, None),
                (595.0, 842.0, 90, None),
                (612.0, 792.0, 180, (36.0, 36.0, 576.0, 756.0)),
            ],
        ),
    ]
    corpus = {"schema": "retainpdf_page_sizes_corpus_v1", "cases": cases}
    with open(OUT_PATH, "w") as f:
        json.dump(corpus, f, indent=2, sort_keys=True)
    print(f"wrote {OUT_PATH} with {len(cases)} cases")


if __name__ == "__main__":
    main()
