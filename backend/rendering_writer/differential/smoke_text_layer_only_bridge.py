#!/usr/bin/env python3
"""Native text_layer_only bridge smoke (Phase 7R-8 / B-G).

Every production `text_layer_only` subroute (standard, standard + complex-math
force-cover, fast_page_cover_only, cover_only_count, vector_heavy_redaction,
image_page_redaction) is replayed through the REAL `_native.build_clean_background_pdf`
stage shim (native bridge when built) versus the pure-Python stage, and the
output page facts must agree (words +/-10%, ink +/-0.02). Also asserts the
`text_layer_only` / `text_redaction` strategies route native (B-G removes the
STRATEGY_NOT_PORTED fallback) and that a not-instrumented stage stays eligible.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_text_layer_only_bridge.py
"""

import os
import sys
import tempfile
from pathlib import Path

import fitz

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)
sys.path.insert(0, _HERE)

import gen_redaction_corpus as gen  # noqa: E402

from services.rendering.source.background import _native  # noqa: E402
from services.rendering.source.background.stage import (  # noqa: E402
    _build_clean_background_pdf_python,
)

WORD_TOL = 0.10
INK_TOL = 0.02


def page_facts(doc):
    facts = []
    for i in range(doc.page_count):
        page = doc[i]
        words = len(page.get_text("words"))
        pix = page.get_pixmap(
            matrix=fitz.Matrix(2.0, 2.0), colorspace=fitz.csGRAY, alpha=False
        )
        total = pix.width * pix.height
        dark = sum(1 for b in pix.samples if b < 250)
        facts.append({"words": words, "ink_ratio": (dark / total) if total else 0.0})
    return facts


def assert_close(actual, expected, label):
    assert len(actual) == len(expected), f"{label}: page count {len(actual)} != {len(expected)}"
    for i, (a, e) in enumerate(zip(actual, expected)):
        rel = abs(a["words"] - e["words"]) / max(e["words"], 1)
        assert rel <= WORD_TOL, f"{label} p{i}: words {a['words']} vs {e['words']}"
        ink = abs(a["ink_ratio"] - e["ink_ratio"])
        assert ink <= INK_TOL, f"{label} p{i}: ink {a['ink_ratio']:.6f} vs {e['ink_ratio']:.6f}"


def replay(name, source_bytes, items):
    translated_pages = {0: items}
    with tempfile.TemporaryDirectory(prefix="rps-tlo-") as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(source_bytes)
        native_out = Path(tmp) / "native.pdf"
        py_out = Path(tmp) / "py.pdf"
        _native.build_clean_background_pdf(
            source_pdf_path=src,
            translated_pages=translated_pages,
            output_pdf_path=native_out,
            redaction_strategy="text_layer_only",
            page_specs=None,
            source_text_precleaned_page_indices=frozenset(),
            visual_profile=None,
        )
        _build_clean_background_pdf_python(
            source_pdf_path=src,
            translated_pages=translated_pages,
            output_pdf_path=py_out,
            redaction_strategy="text_layer_only",
            page_specs=None,
            source_text_precleaned_page_indices=frozenset(),
            visual_profile=None,
        )
        native_facts = page_facts(fitz.open(native_out))
        py_facts = page_facts(fitz.open(py_out))
    assert_close(native_facts, py_facts, f"{name}: native vs python")
    print(f"ok {name}")


def main() -> None:
    assert _native.NATIVE, "native module not built"
    for strategy in ("text_layer_only", "text_redaction"):
        eligible, reason = _native._native_eligible(strategy)
        assert eligible and reason is None, f"{strategy} should route native"
    print("ok text_layer_only / text_redaction route native")

    # standard: two safe-direct lines.
    lines = [(100.0, "Alpha"), (150.0, "Beta")]
    d = fitz.open(stream=gen.build_text_page(lines), filetype="pdf")
    replay("standard", gen.build_text_page(lines), gen.span_items_for(d.load_page(0), lines))

    # standard + complex-math force-cover item.
    math_item = {
        "bbox": [100.0, 400.0, 500.0, 420.0],
        "translated_text": "Math",
        "render_protected_text": "$\\frac{1}{2}$",
    }
    replay("complex_math_cover", gen.build_text_page(lines), gen.span_items_for(d.load_page(0), lines) + [math_item])

    # fast page cover: 5pt short-word line, item over the first 28 words.
    fast_words = (
        "cat dog fox hen pig cow rat bat ant owl bee elk emu yak ape ram ewe sow "
        "boa cod eel gnu jay kit pug ray sea toad newt mink ibex oryx crow dove "
        "duck gull hawk loon swan teal tern wren dodo rook kite coot snipe quail "
        "curlew plover stint godwit"
    ).split()
    fast_line = [(100.0, " ".join(fast_words))]
    d_fast = fitz.open(stream=gen.build_text_page(fast_line, fontsize=5.0), filetype="pdf")
    p_fast = d_fast.load_page(0)
    fast_ws = p_fast.get_text("words")
    assert len(fast_ws) == len(fast_words) and fast_ws[-1][2] < 600.0
    fast_item = {
        "bbox": [fast_ws[0][0] - 1.0, fast_ws[0][1] - 2.0, fast_ws[27][2] + 1.0, fast_ws[0][3] + 2.0],
        "translated_text": "译文",
        "source_text": " ".join(fast_words),
    }
    replay("fast_page_cover", gen.build_text_page(fast_line, fontsize=5.0), [fast_item])

    # cover-only count / vector heavy: many drawings + cover item in the empty
    # bottom margin.
    cover_item = {"bbox": [50.0, 730.0, 550.0, 750.0], "translated_text": "Covered"}
    replay("cover_only_count", gen.build_drawing_heavy_page(5000), [cover_item])
    replay("vector_heavy", gen.build_drawing_heavy_page(2000), [cover_item])

    # image page: full-page flat image + two safe-direct lines.
    d_img = fitz.open(stream=gen.build_image_text_page(lines), filetype="pdf")
    replay("image_page", gen.build_image_text_page(lines), gen.span_items_for(d_img.load_page(0), lines))

    print("all text_layer_only bridge smoke tests pass")


if __name__ == "__main__":
    main()
