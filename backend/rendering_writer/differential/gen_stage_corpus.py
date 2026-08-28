#!/usr/bin/env python3
"""Stage corpus generator (Phase 7R-4, extended in 7R-7/7R-8).

Builds deterministic multi-page synthetic letter PDFs (with TOC baked in),
runs the pure-Python production reference
`_build_clean_background_pdf_python` (the same reference `stage.py` routes
to when the native bridge is absent), and records:
  * the source PDF bytes (base64, trailer /ID pinned, TOC baked via set_toc),
  * the page rect,
  * `translated_pages` (page index -> item dicts, the stable serde DTO — the
    ORIGINAL items, pre-page-spec-replacement/pre-fill, exactly what the 7R-8
    bridge receives),
  * `page_specs` (raw `RenderPageSpec` JSON via
    `_native.render_page_spec_to_bridge`, recorded when present),
  * `visual_profile_fill_map` (flat first-wins fill table via
    `_native.visual_profile_fill_map`, recorded when non-empty),
  * `expected_replaced_pages` (the inline page-spec-replaced + profile-filled
    reference computation, recorded when it differs from `translated_pages`),
  * the redaction strategy and precleaned page indices,
  * the per-page `RedactionDiagnostics` (one entry per sorted translated page
    index; `None` for out-of-range / precleaned skips),
  * the output TOC outline list (count + [level, title, page] triples),
  * per-page words + ink_ratio for input and output.

The generator asserts the shim preconditions per case page before recording,
so a corpus case only captures a divergence-free shim hit:
  * `protect_formula_regions_in_redaction_items(items, formula_source)` is
    identity on the engine-valid item set (config pinned via
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
from services.rendering.layout.model.models import RenderLayoutBlock, RenderPageSpec  # noqa: E402
from services.rendering.policy import (  # noqa: E402
    page_has_formula_region,
    protect_formula_regions_in_redaction_items,
)
from services.rendering.source.background._native import (  # noqa: E402
    render_page_spec_to_bridge,
    visual_profile_fill_map,
)
from services.rendering.source.background.redaction_items import (  # noqa: E402
    redaction_items_from_layout_blocks,
)
from services.rendering.source.background.stage import (  # noqa: E402
    _build_clean_background_pdf_python,
)
from services.rendering.source.cleanup.redaction_flow import (  # noqa: E402
    execute_redaction_flow,
)
from services.rendering.source.cleanup.text_extract import (  # noqa: E402
    _extract_page_text_spans_python as extract_page_text_spans,
)
from services.rendering.source.cleanup.valid_items import (  # noqa: E402
    iter_valid_redaction_items,
)
from services.rendering.source.items import iter_valid_translated_items  # noqa: E402
from services.rendering.source.vector_text import collect_vector_text_rects  # noqa: E402
from services.rendering.visual_profile import VisualProfileRuntime  # noqa: E402
from services.rendering.visual_profile.contracts import (  # noqa: E402
    VISUAL_PROFILE_ALGORITHM_VERSION,
    DocumentVisualProfile,
    ItemVisualProfile,
    PageVisualProfile,
)

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


def make_item_profile(item_id: str, background_rgb: tuple[float, float, float]) -> ItemVisualProfile:
    return ItemVisualProfile(
        item_id=item_id,
        page_index=0,
        bbox=(0.0, 0.0, 0.0, 0.0),
        bbox_space="page",
        bbox_source="corpus",
        source_item_kind="text",
        background_rgb=background_rgb,
        text_rgb=(0.0, 0.0, 0.0),
        confidence=0.9,
        method="corpus",
    )


def make_visual_profile(
    item_fills: dict[str, tuple[float, float, float]],
) -> VisualProfileRuntime:
    items = {
        item_id: make_item_profile(item_id, fill) for item_id, fill in item_fills.items()
    }
    profile = DocumentVisualProfile(
        algorithm=VISUAL_PROFILE_ALGORITHM_VERSION,
        pages={
            0: PageVisualProfile(
                page_index=0,
                background_rgb=(1.0, 1.0, 1.0),
                items=items,
            )
        },
    )
    return VisualProfileRuntime(
        path=None,
        profile=profile,
        diagnostics={"loaded": True, "page_count": 1, "item_count": len(items)},
    )


def make_not_loaded_visual_profile() -> VisualProfileRuntime:
    profile = DocumentVisualProfile(algorithm=VISUAL_PROFILE_ALGORITHM_VERSION, pages={})
    return VisualProfileRuntime(
        path=None,
        profile=profile,
        diagnostics={"loaded": False, "reason": "empty_pages"},
    )


def make_block(
    block_id: str,
    cover_bbox: list[float],
    plain_text: str,
    *,
    page_index: int = 0,
    content_kind: str = "text",
    content_text: str | None = None,
) -> RenderLayoutBlock:
    return RenderLayoutBlock(
        block_id=block_id,
        page_index=page_index,
        background_rect=list(cover_bbox),
        content_rect=list(cover_bbox),
        content_kind=content_kind,
        content_text=content_text if content_text is not None else plain_text,
        plain_text=plain_text,
        math_map=[],
        font_size_pt=12.0,
        leading_em=1.2,
    )


def make_page_spec(page_index: int, blocks: list[RenderLayoutBlock]) -> RenderPageSpec:
    return RenderPageSpec(
        page_index=page_index,
        page_width_pt=PAGE_W,
        page_height_pt=PAGE_H,
        background_pdf_path=None,
        blocks=blocks,
    )


def page_spec_from_dict(d: dict) -> RenderPageSpec:
    """Reconstruct a `RenderPageSpec` from a `render_page_spec_to_bridge` dict
    (used by the smoke replay to feed the shim the exact corpus page specs)."""
    return RenderPageSpec(
        page_index=int(d["page_index"]),
        page_width_pt=float(d.get("page_width_pt", PAGE_W)),
        page_height_pt=float(d.get("page_height_pt", PAGE_H)),
        background_pdf_path=d.get("background_pdf_path"),
        blocks=[
            RenderLayoutBlock(
                block_id=b["block_id"],
                page_index=int(d["page_index"]),
                background_rect=list(b["background_rect"]),
                content_rect=list(b["content_rect"]),
                content_kind=b.get("content_kind", "text"),
                content_text=b.get("content_text", b.get("plain_text", "")),
                plain_text=b.get("plain_text", ""),
                math_map=[],
                font_size_pt=float(b.get("font_size_pt", 12.0)),
                leading_em=float(b.get("leading_em", 1.2)),
            )
            for b in d["blocks"]
        ],
    )


def visual_profile_from_fill_map(fill_map: dict[str, list[float]]) -> VisualProfileRuntime:
    """Reconstruct a loaded `VisualProfileRuntime` from a flat
    `visual_profile_fill_map` (first-wins; pages collapsed into page 0, which is
    faithful for `background_fill_for_item`). Empty map -> not-loaded profile."""
    if not fill_map:
        return make_not_loaded_visual_profile()
    items = {
        item_id: make_item_profile(item_id, tuple(fill)) for item_id, fill in fill_map.items()
    }
    profile = DocumentVisualProfile(
        algorithm=VISUAL_PROFILE_ALGORITHM_VERSION,
        pages={
            0: PageVisualProfile(
                page_index=0,
                background_rgb=(1.0, 1.0, 1.0),
                items=items,
            )
        },
    )
    return VisualProfileRuntime(
        path=None,
        profile=profile,
        diagnostics={"loaded": True, "page_count": 1, "item_count": len(items)},
    )


def _as_str_keyed(pages: dict[int, list[dict]]) -> dict[str, list[dict]]:
    return {str(k): v for k, v in sorted(pages.items())}


def _reference_replaced_pages(
    translated_pages: dict[int, list[dict]],
    page_specs: list[RenderPageSpec] | None,
    visual_profile: VisualProfileRuntime | None,
) -> dict[int, list[dict]]:
    """Inline reference of the 7R-8 native conversion (the exact logic
    `page_specs::apply_page_specs_and_fills` ports): replace each page's items
    with `redaction_items_from_layout_blocks` when it has a spec, then inject
    the per-item `_visual_profile_fill` the engine reads. Mirrors the old
    `_prepare_native_pages`, now recorded against the ORIGINAL items."""
    specs_by_page = {spec.page_index: spec for spec in page_specs or []}
    out: dict[int, list[dict]] = {}
    for page_index, page_items in sorted(translated_pages.items()):
        items = page_items
        spec = specs_by_page.get(page_index)
        if spec is not None:
            items = redaction_items_from_layout_blocks(items, spec.blocks)
        if visual_profile is not None:
            filled: list[dict] = []
            for item in items:
                fill = visual_profile.background_fill_for_item(item)
                filled.append(item if fill is None else {**item, "_visual_profile_fill": list(fill)})
            items = filled
        out[page_index] = items
    return out


def run_stage_case(
    name: str,
    source_bytes: bytes,
    translated_pages: dict[int, list[dict]],
    *,
    strategy: str | None,
    precleaned: list[int],
    page_specs: list[RenderPageSpec] | None = None,
    visual_profile: VisualProfileRuntime | None = None,
) -> dict:
    """Replicate the stage's per-page flow for diagnostics + shim asserts, then
    run the pure-Python production reference `_build_clean_background_pdf_python`
    for output facts + TOC. Formula detection reads the ORIGINAL items
    (`stage.py` passes `translated_pages[page_index]` to
    `page_has_formula_region` and the guard reference), redaction uses the
    page-spec-replaced items. The recorded `translated_pages` are the ORIGINAL
    items; the page-spec replacement + profile fill are recorded separately as
    `page_specs`/`visual_profile_fill_map` plus the inline
    `expected_replaced_pages` reference — exactly what the 7R-8 bridge receives
    and what the Rust side replays."""
    d = fitz.open(stream=source_bytes, filetype="pdf")
    page_rect = [float(v) for v in d.load_page(0).rect]
    input_facts = page_facts(d)
    specs_by_page = {spec.page_index: spec for spec in page_specs or []}

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
        redaction_items = translated_pages[idx]
        formula_source = translated_pages[idx]
        spec = specs_by_page.get(idx)
        if spec is not None:
            redaction_items = redaction_items_from_layout_blocks(
                redaction_items, spec.blocks
            )
        protected = protect_formula_regions_in_redaction_items(
            redaction_items, formula_source
        )

        valid_before = iter_valid_redaction_items(redaction_items)
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

        page_strategy = (
            "visual_cover"
            if strategy is None and page_has_formula_region(formula_source)
            else strategy
        )
        diag = execute_redaction_flow(
            page,
            protected,
            fill_background=None,
            cover_only=False,
            strategy=page_strategy,
            visual_profile=visual_profile,
        )
        per_page_diagnostics.append(normalize_diagnostics(diag))
    d.close()

    with tempfile.TemporaryDirectory(prefix="rps-stage-") as tmp:
        in_path = Path(tmp) / "in.pdf"
        out_path = Path(tmp) / "out.pdf"
        in_path.write_bytes(source_bytes)
        _build_clean_background_pdf_python(
            source_pdf_path=in_path,
            translated_pages=translated_pages,
            output_pdf_path=out_path,
            redaction_strategy=strategy,
            page_specs=page_specs,
            source_text_precleaned_page_indices=frozenset(precleaned),
            visual_profile=visual_profile,
        )
        out = fitz.open(out_path)
        output_facts = page_facts(out)
        output_toc = [
            {"level": int(t[0]), "title": str(t[1]), "page": int(t[2])}
            for t in out.get_toc()
        ]
        toc_entries = len(output_toc)
        out.close()

    expected_replaced_pages = _reference_replaced_pages(
        translated_pages, page_specs, visual_profile
    )
    case: dict[str, object] = {
        "name": name,
        "page_rect": page_rect,
        "source_pdf_b64": base64.b64encode(source_bytes).decode("ascii"),
        "translated_pages": _as_str_keyed(translated_pages),
        "redaction_strategy": strategy,
        "precleaned_page_indices": precleaned,
        "toc_entries": toc_entries,
        "toc": output_toc,
        "per_page_diagnostics": per_page_diagnostics,
        "expected_input": {"pages": input_facts},
        "expected_output": {"pages": output_facts},
    }
    if page_specs:
        case["page_specs"] = [render_page_spec_to_bridge(s) for s in page_specs]
    fills = visual_profile_fill_map(visual_profile)
    if fills:
        case["visual_profile_fill_map"] = fills
    if expected_replaced_pages != translated_pages:
        case["expected_replaced_pages"] = _as_str_keyed(expected_replaced_pages)
    return case


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

    # (f) visual_cover with a loaded profile that hits item 0 only -> solid
    #     cover for it (visual_profile_cover_rects=1), sampled white cover for
    #     item 1 (which carries no ids).
    pages_f = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_f = build_source_pdf(pages_f, [])
    d_f = fitz.open(stream=raw_f, filetype="pdf")
    items_f = {
        0: span_items_for(d_f.load_page(0), pages_f[0]),
        1: span_items_for(d_f.load_page(1), pages_f[1]),
    }
    d_f.close()
    items_f[0][0]["item_id"] = "line-0"
    profile_f = make_visual_profile({"line-0": (0.85, 0.87, 0.9)})
    cases.append(run_stage_case(
        "visual_profile_partial_hit",
        raw_f,
        items_f,
        strategy="visual_cover",
        precleaned=[],
        visual_profile=profile_f,
    ))

    # (g) loaded profile that misses every item id -> all sampled covers; the
    #     profile branch is active but contributes 0 solid covers, matching the
    #     not-loaded / no-profile else branch.
    pages_g = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_g = build_source_pdf(pages_g, [])
    d_g = fitz.open(stream=raw_g, filetype="pdf")
    items_g = {
        0: span_items_for(d_g.load_page(0), pages_g[0]),
        1: span_items_for(d_g.load_page(1), pages_g[1]),
    }
    d_g.close()
    profile_g = make_visual_profile({"ghost-item": (0.9, 0.9, 0.9)})
    cases.append(run_stage_case(
        "visual_profile_all_miss",
        raw_g,
        items_g,
        strategy="visual_cover",
        precleaned=[],
        visual_profile=profile_g,
    ))

    # (h) profile runtime present but not loaded (empty pages) -> else branch,
    #     byte-equivalent to no profile at all.
    pages_h = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_h = build_source_pdf(pages_h, [])
    d_h = fitz.open(stream=raw_h, filetype="pdf")
    items_h = {
        0: span_items_for(d_h.load_page(0), pages_h[0]),
        1: span_items_for(d_h.load_page(1), pages_h[1]),
    }
    d_h.close()
    cases.append(run_stage_case(
        "visual_profile_not_loaded",
        raw_h,
        items_h,
        strategy="visual_cover",
        precleaned=[],
        visual_profile=make_not_loaded_visual_profile(),
    ))

    # (i) page 0 replaced by a single block -> 1 redaction item covering both
    #     lines; formula_source keeps the 2 original items (formula_source_pages
    #     is recorded because the maps differ).
    pages_i = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_i = build_source_pdf(pages_i, [])
    d_i = fitz.open(stream=raw_i, filetype="pdf")
    items_i = {
        0: span_items_for(d_i.load_page(0), pages_i[0]),
        1: span_items_for(d_i.load_page(1), pages_i[1]),
    }
    d_i.close()
    specs_i = [make_page_spec(0, [make_block("block-0", [50.0, 85.0, 320.0, 200.0], "Alpha Beta")])]
    cases.append(run_stage_case(
        "page_specs_replace_items",
        raw_i,
        items_i,
        strategy="visual_cover",
        precleaned=[],
        page_specs=specs_i,
    ))

    # (j) page 0 original items carry a formula item -> strategy flips to
    #     visual_cover even though the redacted (page-spec replaced) items have
    #     no formula: formula detection reads formula_source_pages. The block
    #     cover does not overlap the expanded guard, so the guard split stays
    #     identity and the single replaced item is covered whole.
    pages_j = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_j = build_source_pdf(pages_j, [])
    d_j = fitz.open(stream=raw_j, filetype="pdf")
    formula_item_j = {
        "bbox": [250.0, 300.0, 350.0, 320.0],
        "translated_text": "",
        "block_type": "formula",
        "normalized_sub_type": "display_formula",
    }
    items_j = {
        0: span_items_for(d_j.load_page(0), pages_j[0]) + [dict(formula_item_j)],
        1: span_items_for(d_j.load_page(1), pages_j[1]),
    }
    d_j.close()
    specs_j = [make_page_spec(0, [make_block("block-0", [50.0, 85.0, 320.0, 200.0], "Alpha Beta")])]
    cases.append(run_stage_case(
        "page_specs_formula_source_flip",
        raw_j,
        items_j,
        strategy=None,
        precleaned=[],
        page_specs=specs_j,
    ))

    # (k) full book_renderer scenario: page_specs replacement + formula source
    #     flip + profile fill on the replaced block item -> solid profile cover
    #     on page 0 (visual_profile_cover_rects=1).
    pages_k = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_k = build_source_pdf(pages_k, [])
    d_k = fitz.open(stream=raw_k, filetype="pdf")
    formula_item_k = {
        "bbox": [250.0, 300.0, 350.0, 320.0],
        "translated_text": "",
        "block_type": "formula",
        "normalized_sub_type": "display_formula",
    }
    items_k = {
        0: span_items_for(d_k.load_page(0), pages_k[0]) + [dict(formula_item_k)],
        1: span_items_for(d_k.load_page(1), pages_k[1]),
    }
    d_k.close()
    specs_k = [make_page_spec(0, [make_block("block-0", [50.0, 85.0, 320.0, 200.0], "Alpha Beta")])]
    profile_k = make_visual_profile({"block-0": (0.82, 0.84, 0.86)})
    cases.append(run_stage_case(
        "page_specs_formula_profile_combo",
        raw_k,
        items_k,
        strategy=None,
        precleaned=[],
        page_specs=specs_k,
        visual_profile=profile_k,
    ))

    # (l) page-spec block_id "item-<item_id>": the block merges the source item
    #     matched by its item_id (source_items_by_id), preserving the source's
    #     non-overridden fields (raw_block_type / normalized_sub_type) and the
    #     profile fill hits via source_item_id on the replaced item.
    pages_l = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_l = build_source_pdf(pages_l, [])
    d_l = fitz.open(stream=raw_l, filetype="pdf")
    items_l = {
        0: span_items_for(d_l.load_page(0), pages_l[0]),
        1: span_items_for(d_l.load_page(1), pages_l[1]),
    }
    d_l.close()
    items_l[0][0]["item_id"] = "line-0"
    items_l[0][0]["raw_block_type"] = "paragraph"
    items_l[0][0]["normalized_sub_type"] = "body"
    specs_l = [make_page_spec(0, [make_block("item-line-0", [50.0, 85.0, 320.0, 200.0], "Alpha Beta")])]
    profile_l = make_visual_profile({"line-0": (0.82, 0.84, 0.86)})
    cases.append(run_stage_case(
        "page_specs_merge_by_id_source_fields",
        raw_l,
        items_l,
        strategy="visual_cover",
        precleaned=[],
        page_specs=specs_l,
        visual_profile=profile_l,
    ))

    # (m) block_id "item-<index>" with a digit suffix: the by-id lookup misses
    #     (source items carry no item_id) so the by-index fallback merges the
    #     Nth source item. Distinct normalized_sub_type on the two source items
    #     makes the selected index observable in the replaced output.
    pages_m = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_m = build_source_pdf(pages_m, [])
    d_m = fitz.open(stream=raw_m, filetype="pdf")
    items_m = {
        0: span_items_for(d_m.load_page(0), pages_m[0]),
        1: span_items_for(d_m.load_page(1), pages_m[1]),
    }
    d_m.close()
    items_m[0][0]["normalized_sub_type"] = "first"
    items_m[0][1]["normalized_sub_type"] = "second"
    specs_m = [make_page_spec(0, [make_block("item-1", [50.0, 85.0, 320.0, 200.0], "Alpha Beta")])]
    cases.append(run_stage_case(
        "page_specs_merge_by_index_fallback",
        raw_m,
        items_m,
        strategy="visual_cover",
        precleaned=[],
        page_specs=specs_m,
    ))

    # (n) markdown content_kind: protected_translated_text = content_text
    #     (differs from plain_text / source_text), exercising
    #     render_block_protected_text's markdown branch.
    pages_n = [[(100.0, "Alpha"), (150.0, "Beta")], [(100.0, "Gamma")]]
    raw_n = build_source_pdf(pages_n, [])
    d_n = fitz.open(stream=raw_n, filetype="pdf")
    items_n = {
        0: span_items_for(d_n.load_page(0), pages_n[0]),
        1: span_items_for(d_n.load_page(1), pages_n[1]),
    }
    d_n.close()
    specs_n = [make_page_spec(
        0,
        [make_block(
            "block-0",
            [50.0, 85.0, 320.0, 200.0],
            "Alpha Beta",
            content_kind="markdown",
            content_text="**Alpha Beta**",
        )],
    )]
    cases.append(run_stage_case(
        "page_specs_markdown_protected_text",
        raw_n,
        items_n,
        strategy="visual_cover",
        precleaned=[],
        page_specs=specs_n,
    ))

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
