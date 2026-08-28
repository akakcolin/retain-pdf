#!/usr/bin/env python3
"""Native bridge smoke test for the color-adaptation fitz primitives (Phase B2-3).

Exercises the three source primitives ported to Rust in
`rendering_bridge` (`sample_page_color_fills`, `extract_page_span_dicts`,
`sample_title_visual_colors`) plus the end-to-end
`apply_adaptive_overlay_colors_batch` and the prewarm entry point.

Parts:

1. Native availability: both shims (`source.background._native`,
   `output.typst._native`) report NATIVE (maturin build installed).

2. Three-way primitive parity: for a synthetic multi-page PDF with a
   needs-sampling gray block, a colored title span, a dark rect + light circle
   (visual probe), an explicit-white overlay item, a Path-B single-title page,
   and an out-of-range page, each primitive's NATIVE result equals the shim
   re-invoked with NATIVE=False (fitz reference) equals a hand-written fitz
   loop, dict key by dict key. Batch rects stay < 8 so `batch.clip` is null
   while `targets` still resolve.

3. End-to-end parity: `apply_adaptive_overlay_colors_batch` NATIVE vs
   reference vs a manual fitz per-page `apply_adaptive_overlay_colors` loop;
   per-item `_render_cover_fill`/`_render_text_color` within 2/255. The
   prewarm entry `apply_page_color_adapt_for_prewarm` agrees under NATIVE True
   and False.

4. Boundaries: empty item_id is dropped (defaults applied), out-of-range pages
   pass through as shallow copies, and the <8-rect batch gate yields a null
   clip while targets still sample.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_color_adapt_bridge.py
"""

import os
import sys
import tempfile
from pathlib import Path

os.environ["RETAIN_RENDER_PIXMAP_INDENT"] = "1"
os.environ["RETAIN_RENDER_TYPOGRAPHY_MEMORY"] = "0"

_HERE = os.path.dirname(os.path.abspath(__file__))
_SCRIPTS_DIR = os.path.abspath(os.path.join(_HERE, "..", "..", "scripts"))
sys.path.insert(0, _SCRIPTS_DIR)

import fitz  # noqa: E402

from services.rendering.output.typst import _native as out_native  # noqa: E402
from services.rendering.output.typst.color_adapt import (  # noqa: E402
    DEFAULT_COVER_FILL,
    apply_adaptive_overlay_colors,
    _item_needs_local_color_sampling,
    _item_uses_explicit_white_fill,
    _local_sampling_rects,
    is_title_like_block,
    title_text_color_from_visual_components,
)
from services.rendering.output.typst.color_adapt import cover_bbox  # noqa: E402
from services.rendering.source.background import _native as src_native  # noqa: E402
from services.rendering.source.background.fill import (  # noqa: E402
    LocalBackgroundSampler,
    _batch_sampler_clip_rect,
    sample_local_background_fill,
)
from services.rendering.source.prewarm_color_profile import (  # noqa: E402
    apply_page_color_adapt_for_prewarm,
)

PAGE_WIDTH = 612.0
PAGE_HEIGHT = 792.0
TOL = 2.0 / 255.0 + 1e-9


def _heading_item(*, item_id: str, bbox: list[float], **extra) -> dict:
    item = {
        "item_id": item_id,
        "page_idx": 0,
        "block_type": "text",
        "block_kind": "text",
        "layout_role": "heading",
        "semantic_role": "heading",
        "bbox": bbox,
        "lines": [],
        "source_text": "sample source text",
        "protected_source_text": "sample source text",
        "protected_translated_text": "示例译文",
        "formula_map": [],
    }
    item.update(extra)
    return item


def build_source_pdf() -> bytes:
    """Page 0 exercises the fill sampler + whole-page span sampler + visual
    probe + explicit-white overlay; page 1 exercises the Path-B per-item span
    clip. All geometry stays within the page so `rect & page.rect` is a no-op."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    gray = page.new_shape()
    gray.draw_rect(fitz.Rect(20, 20, 300, 300))
    gray.finish(color=None, fill=(0.8, 0.8, 0.8))
    gray.commit()
    page.insert_text((355.0, 62.0), "RED TITLE", fontsize=16, fontname="helv", color=(0.8, 0.1, 0.1))
    dark = page.new_shape()
    dark.draw_rect(fitz.Rect(348, 118, 542, 162))
    dark.finish(color=None, fill=(0.1, 0.1, 0.1))
    dark.commit()
    light = page.new_shape()
    light.draw_circle(fitz.Point(445, 140), 12)
    light.finish(color=None, fill=(1, 1, 1))
    light.commit()

    page2 = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page2.insert_text((45.0, 78.0), "BLUE HEADING", fontsize=16, fontname="helv", color=(0.1, 0.1, 0.8))

    # Page 2: /Rotate 90. fitz inserts/draws in the unrotated content space, and
    # `get_text("dict")` returns spans there too; production item bboxes are also
    # content-space. The native span extractor must clear the rotation the same
    # way fitz does, or the content-space clip never intersects the
    # display-space native spans and the title color silently defaults to black.
    page3 = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page3.set_rotation(90)
    page3.insert_text((355.0, 62.0), "ROT TITLE", fontsize=16, fontname="helv", color=(0.2, 0.6, 0.2))

    raw = doc.tobytes(garbage=0)
    doc.close()
    return raw


def translated_pages() -> dict[int, list[dict]]:
    return {
        0: [
            _heading_item(
                item_id="p000-b001",
                bbox=[26.0, 40.0, 130.0, 66.0],
                _render_use_cover_fill=True,
            ),
            _heading_item(item_id="p000-b002", bbox=[350.0, 40.0, 540.0, 66.0]),
            _heading_item(item_id="p000-b003", bbox=[350.0, 120.0, 540.0, 160.0]),
            _heading_item(item_id="p000-b004", bbox=[20.0, 180.0, 300.0, 210.0], _render_policy={"overlay_fill": "white"}),
        ],
        1: [
            _heading_item(item_id="p001-b001", bbox=[40.0, 50.0, 300.0, 90.0]),
        ],
        2: [
            # Single title on the rotated page → Path-B per-item span clip in
            # content space, covering the "ROT TITLE" green text at (355, 62).
            _heading_item(item_id="p002-b001", bbox=[350.0, 40.0, 540.0, 70.0]),
        ],
        99: [
            _heading_item(item_id="p099-b001", bbox=[10.0, 10.0, 60.0, 40.0]),
        ],
    }


def cover_rect(item: dict) -> fitz.Rect | None:
    bbox = cover_bbox(item)
    if len(bbox) != 4:
        return None
    rect = fitz.Rect(bbox)
    if rect.is_empty or rect.is_infinite:
        return None
    return rect


def fills_config(items: list[dict]) -> dict[str, list[list[float]]]:
    """Mirror `output/typst/_native.apply_adaptive_overlay_colors_batch`'s
    per-page fills_cfg (batch_rects + over-approximated target_rects)."""
    batch_rects = [[r.x0, r.y0, r.x1, r.y1] for r in _local_sampling_rects(items)]
    target_rects: list[list[float]] = []
    for item in items:
        if not _item_needs_local_color_sampling(item):
            continue
        rect = cover_rect(item)
        if rect is None:
            continue
        target_rects.append([rect.x0, rect.y0, rect.x1, rect.y1])
    for item in items:
        if _item_needs_local_color_sampling(item) or _item_uses_explicit_white_fill(item):
            continue
        if not is_title_like_block(item):
            continue
        rect = cover_rect(item)
        if rect is None:
            continue
        target_rects.append([rect.x0, rect.y0, rect.x1, rect.y1])
    return {"batch_rects": batch_rects, "target_rects": target_rects}


def span_clips_config(items: list[dict], page_rect: fitz.Rect) -> list[list[float] | None]:
    title_count = sum(1 for item in items if is_title_like_block(item))
    if title_count >= 2:
        return [None]
    clips: list[list[float] | None] = []
    for item in items:
        if not is_title_like_block(item):
            continue
        rect = cover_rect(item)
        if rect is None:
            continue
        clipped = rect & page_rect
        clips.append([clipped.x0, clipped.y0, clipped.x1, clipped.y1])
    return clips


def probe_config_from_fills(page_idx: int, items: list[dict], fills_out) -> list[dict]:
    page_fills = fills_out.get(str(page_idx), {}).get("targets", {})
    target_ids: list[str] = []
    target_rects: list[fitz.Rect] = []
    for item in items:
        if not _item_needs_local_color_sampling(item):
            continue
        rect = cover_rect(item)
        if rect is None:
            continue
        target_ids.append(str(item.get("item_id") or ""))
        target_rects.append(rect)
    for item in items:
        if _item_needs_local_color_sampling(item) or _item_uses_explicit_white_fill(item):
            continue
        if not is_title_like_block(item):
            continue
        rect = cover_rect(item)
        if rect is None:
            continue
        target_ids.append(str(item.get("item_id") or ""))
        target_rects.append(rect)
    fill_by_item_id: dict[str, tuple[float, float, float]] = {}
    for i, item_id in enumerate(target_ids):
        fill = page_fills.get(str(i))
        if fill is not None:
            fill_by_item_id[item_id] = (float(fill[0]), float(fill[1]), float(fill[2]))
    probes: list[dict] = []
    for item in items:
        if not is_title_like_block(item):
            continue
        rect = cover_rect(item)
        if rect is None:
            continue
        item_id = str(item.get("item_id") or "")
        fill = fill_by_item_id.get(item_id, DEFAULT_COVER_FILL)
        if not (fill[0] * 0.299 + fill[1] * 0.587 + fill[2] * 0.114 < 0.94):
            continue
        probes.append({"rect": [rect.x0, rect.y0, rect.x1, rect.y1], "fill": list(fill)})
    return probes


def manual_fills(src: Path, by_page: dict[int, dict[str, list[list[float]]]]) -> dict[str, dict[str, object]]:
    doc = fitz.open(src)
    try:
        result: dict[str, dict[str, object]] = {}
        for page_idx, cfg in by_page.items():
            if not 0 <= page_idx < len(doc):
                continue
            page = doc[page_idx]
            batch_rects = [fitz.Rect(rect) for rect in cfg["batch_rects"]]
            target_rects = [fitz.Rect(rect) for rect in cfg["target_rects"]]
            valid_count = sum(1 for rect in batch_rects if not rect.is_empty and not rect.is_infinite)
            clip_rect = _batch_sampler_clip_rect(page, batch_rects, allow_full_page=True)
            sampler = LocalBackgroundSampler.build(page, batch_rects)
            targets: dict[str, list[float]] = {}
            for i, rect in enumerate(target_rects):
                fill = sample_local_background_fill(page, rect, sampler=sampler)
                targets[str(i)] = [float(fill[0]), float(fill[1]), float(fill[2])]
            result[str(page_idx)] = {
                "batch": {
                    "count": valid_count,
                    "clip": (
                        [float(clip_rect.x0), float(clip_rect.y0), float(clip_rect.x1), float(clip_rect.y1)]
                        if clip_rect is not None
                        else None
                    ),
                },
                "targets": targets,
            }
        return result
    finally:
        doc.close()


def manual_spans(src: Path, clips_by_page: dict[int, list[list[float] | None]]) -> dict[str, dict[str, list[list[float]]]]:
    doc = fitz.open(src)
    try:
        result: dict[str, dict[str, list[list[float]]]] = {}
        for page_idx, clips in clips_by_page.items():
            if not 0 <= page_idx < len(doc):
                continue
            page = doc[page_idx]
            page_out: dict[str, list[list[float]]] = {}
            for i, clip in enumerate(clips):
                spans: list[list[float]] = []
                text = page.get_text("dict", clip=fitz.Rect(clip)) if clip is not None else page.get_text("dict")
                for block in text.get("blocks", []):
                    for line in block.get("lines", []):
                        for span in line.get("spans", []):
                            span_text = str(span.get("text") or "")
                            bbox = span.get("bbox")
                            if not isinstance(bbox, (list, tuple)) or len(bbox) < 4:
                                continue
                            rect = fitz.Rect(float(bbox[0]), float(bbox[1]), float(bbox[2]), float(bbox[3]))
                            if rect.is_empty or rect.is_infinite:
                                continue
                            color = span.get("color")
                            if not isinstance(color, int):
                                continue
                            spans.append(
                                [float(bbox[0]), float(bbox[1]), float(bbox[2]), float(bbox[3]), color, span_text]
                            )
                page_out[str(i)] = spans
            result[str(page_idx)] = page_out
        return result
    finally:
        doc.close()


def manual_visuals(src: Path, visuals_by_page: dict[int, list[dict]]) -> dict[str, dict[str, list[float]]]:
    doc = fitz.open(src)
    try:
        result: dict[str, dict[str, list[float]]] = {}
        for page_idx, visuals in visuals_by_page.items():
            if not 0 <= page_idx < len(doc):
                continue
            page = doc[page_idx]
            page_out: dict[str, list[float]] = {}
            for i, visual in enumerate(visuals):
                rect = fitz.Rect(visual["rect"])
                fill = tuple(float(component) for component in visual["fill"])
                color = title_text_color_from_visual_components(page, rect, fill)
                if color is not None:
                    page_out[str(i)] = [float(color[0]), float(color[1]), float(color[2])]
            if page_out:
                result[str(page_idx)] = page_out
        return result
    finally:
        doc.close()


def manual_batch(src: Path, pages: dict[int, list[dict]]) -> dict[int, list[dict]]:
    doc = fitz.open(src)
    try:
        results: dict[int, list[dict]] = {}
        for page_idx in sorted(pages):
            items = pages[page_idx]
            if not 0 <= page_idx < len(doc):
                results[page_idx] = list(items)
                continue
            results[page_idx] = apply_adaptive_overlay_colors(doc[page_idx], items)
        return results
    finally:
        doc.close()


def assert_colors_close(left: dict, right: dict, label: str) -> None:
    assert left.keys() == right.keys(), f"{label}: page keys {left.keys()} != {right.keys()}"
    for page_idx in left:
        l_items, r_items = left[page_idx], right[page_idx]
        assert len(l_items) == len(r_items), f"{label}: page {page_idx} item count {len(l_items)} != {len(r_items)}"
        for l_item, r_item in zip(l_items, r_items):
            assert l_item.get("item_id") == r_item.get("item_id"), f"{label}: item id mismatch on {page_idx}"
            for key in ("_render_cover_fill", "_render_text_color"):
                l_fill = l_item.get(key, DEFAULT_COVER_FILL)
                r_fill = r_item.get(key, DEFAULT_COVER_FILL)
                assert len(l_fill) == 3 and len(r_fill) == 3, f"{label}: {key} missing on {page_idx}"
                for a, b in zip(l_fill, r_fill):
                    assert abs(a - b) <= TOL, (
                        f"{label}: {key} {l_item.get('item_id')} {a} != {b} "
                        f"(diff {abs(a - b)}) on page {page_idx}"
                    )


def assert_fills_close(left, right, label: str) -> None:
    assert left.keys() == right.keys(), f"{label}: page keys {left.keys()} != {right.keys()}"
    for page_idx in left:
        l_page, r_page = left[page_idx], right[page_idx]
        assert l_page["batch"]["count"] == r_page["batch"]["count"], f"{label}: batch count on {page_idx}"
        assert l_page["batch"]["clip"] == r_page["batch"]["clip"], f"{label}: batch clip on {page_idx}"
        assert l_page["targets"].keys() == r_page["targets"].keys(), f"{label}: target keys on {page_idx}"
        for target_idx in l_page["targets"]:
            l_fill, r_fill = l_page["targets"][target_idx], r_page["targets"][target_idx]
            for a, b in zip(l_fill, r_fill):
                assert abs(a - b) <= TOL, f"{label}: target {target_idx} {a} != {b} on {page_idx}"


def main() -> None:
    assert src_native.NATIVE, "source background native module not built"
    assert out_native.NATIVE, "typst output native module not built"

    raw = build_source_pdf()
    with tempfile.TemporaryDirectory(prefix="rps-ca-") as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(raw)

        pages = translated_pages()
        doc = fitz.open(src)
        try:
            page_rects = {page_idx: doc[page_idx].rect for page_idx in (0, 1, 2)}
        finally:
            doc.close()

        fills_by_page = {page_idx: fills_config(pages[page_idx]) for page_idx in (0, 1, 2)}
        spans_by_page = {page_idx: span_clips_config(pages[page_idx], page_rects[page_idx]) for page_idx in (0, 1, 2)}

        # ---- 3-way primitive parity -----------------------------------------
        fills_native = src_native.sample_page_color_fills(source_pdf_path=src, by_page=fills_by_page)
        was = src_native.NATIVE
        src_native.NATIVE = False
        try:
            fills_reference = src_native.sample_page_color_fills(source_pdf_path=src, by_page=fills_by_page)
        finally:
            src_native.NATIVE = was
        fills_manual = manual_fills(src, fills_by_page)
        assert_fills_close(fills_native, fills_reference, "fills native vs reference")
        assert_fills_close(fills_reference, fills_manual, "fills reference vs fitz")

        for page_idx in (0, 1, 2):
            batch = fills_native[str(page_idx)]["batch"]
            assert batch["clip"] is None, f"expected null batch clip on {page_idx}, got {batch['clip']}"
            assert fills_native[str(page_idx)]["targets"], f"expected resolved targets on {page_idx}"

        spans_native = src_native.extract_page_span_dicts(source_pdf_path=src, clips_by_page=spans_by_page)
        src_native.NATIVE = False
        try:
            spans_reference = src_native.extract_page_span_dicts(source_pdf_path=src, clips_by_page=spans_by_page)
        finally:
            src_native.NATIVE = was
        spans_manual = manual_spans(src, spans_by_page)
        assert spans_native == spans_reference == spans_manual, (
            f"span parity native {spans_native} != reference {spans_reference} != fitz {spans_manual}"
        )

        # whole-page clip on page 0 yields the red title span; Path-B clips on
        # page 1 / page 2 (the latter a /Rotate 90 page) yield their headings.
        assert spans_native["0"]["0"], f"expected whole-page spans on page 0, got {spans_native.get('0')}"
        assert any("RED TITLE" in str(entry[5]) for entry in spans_native["0"]["0"])
        assert spans_native["1"]["0"], f"expected Path-B spans on page 1, got {spans_native.get('1')}"
        assert any("BLUE HEADING" in str(entry[5]) for entry in spans_native["1"]["0"])
        assert spans_native["2"]["0"], f"expected Path-B spans on rotated page 2, got {spans_native.get('2')}"
        assert any("ROT TITLE" in str(entry[5]) for entry in spans_native["2"]["0"])

        probes_by_page: dict[int, list[dict]] = {}
        for page_idx in (0, 1, 2):
            probes = probe_config_from_fills(page_idx, pages[page_idx], fills_reference)
            if probes:
                probes_by_page[page_idx] = probes
        visuals_native = src_native.sample_title_visual_colors(source_pdf_path=src, visuals_by_page=probes_by_page)
        src_native.NATIVE = False
        try:
            visuals_reference = src_native.sample_title_visual_colors(source_pdf_path=src, visuals_by_page=probes_by_page)
        finally:
            src_native.NATIVE = was
        visuals_manual = manual_visuals(src, probes_by_page)
        assert visuals_native.keys() == visuals_reference.keys() == visuals_manual.keys(), (
            f"visual page keys {visuals_native.keys()} vs {visuals_reference.keys()} vs {visuals_manual.keys()}"
        )
        for page_idx in visuals_native:
            l_page, r_page, m_page = visuals_native[page_idx], visuals_reference[page_idx], visuals_manual[page_idx]
            assert l_page.keys() == r_page.keys() == m_page.keys(), (
                f"visual keys on {page_idx}: {l_page.keys()} vs {r_page.keys()} vs {m_page.keys()}"
            )
            for visual_idx in l_page:
                for a, b, c in zip(l_page[visual_idx], r_page[visual_idx], m_page[visual_idx]):
                    assert abs(a - b) <= TOL and abs(a - c) <= TOL, (
                        f"visual {page_idx}.{visual_idx}: {a} vs {b} vs {c}"
                    )
        assert "0" in visuals_native, f"expected visual probe results on page 0, got {visuals_native}"

        # ---- end-to-end batch parity ---------------------------------------
        batch_native = out_native.apply_adaptive_overlay_colors_batch(source_pdf_path=src, pages=pages)
        was_out = out_native.NATIVE
        out_native.NATIVE = False
        try:
            batch_reference = out_native.apply_adaptive_overlay_colors_batch(source_pdf_path=src, pages=pages)
        finally:
            out_native.NATIVE = was_out
        batch_manual = manual_batch(src, pages)
        assert_colors_close(batch_native, batch_reference, "batch native vs reference")
        assert_colors_close(batch_reference, batch_manual, "batch reference vs fitz")

        # explicit expectations (fitz-derived, exact)
        assert batch_native[0][0]["_render_cover_fill"][0] >= 0.75, "p000-b001 gray cover"
        assert batch_native[0][1]["_render_cover_fill"] == (1, 1, 1), "p000-b002 white cover"
        assert batch_native[0][1]["_render_text_color"][0] >= 0.75, "p000-b002 red-ish text"
        assert batch_native[0][2]["_render_cover_fill"][0] <= 0.25, "p000-b003 dark cover"
        assert batch_native[0][2]["_render_text_color"][0] >= 0.75, "p000-b003 light text via probe"
        assert batch_native[0][3]["_render_cover_fill"] == (1, 1, 1), "p000-b004 white cover"
        assert batch_native[0][3]["_render_text_color"] == (0, 0, 0), "p000-b004 black text"
        assert batch_native[1][0]["_render_cover_fill"] == (1, 1, 1), "p001-b001 white cover"
        assert batch_native[1][0]["_render_text_color"][2] >= 0.6, "p001-b001 blue text via Path-B clip"
        # /Rotate 90 page: the content-space Path-B clip must intersect the native
        # span (rotation cleared) or the title color silently falls back to black.
        assert batch_native[2][0]["_render_cover_fill"] == (1, 1, 1), "p002-b001 white cover"
        assert batch_native[2][0]["_render_text_color"][1] >= 0.5, (
            f"p002-b001 green text via rotated Path-B clip, got {batch_native[2][0]['_render_text_color']}"
        )
        assert batch_native[2][0]["_render_text_color"][0] < 0.35, (
            f"p002-b001 green (not red) text, got {batch_native[2][0]['_render_text_color']}"
        )

        # ---- prewarm parity -------------------------------------------------
        prewarm_native = apply_page_color_adapt_for_prewarm(src, pages)
        out_native.NATIVE = False
        try:
            prewarm_reference = apply_page_color_adapt_for_prewarm(src, pages)
        finally:
            out_native.NATIVE = was_out
        assert_colors_close(prewarm_native, prewarm_reference, "prewarm native vs reference")

        # ---- boundaries -----------------------------------------------------
        empty_item = _heading_item(
            item_id="",
            bbox=[10.0, 300.0, 60.0, 340.0],
            _render_use_cover_fill=True,
        )
        oob_pages = dict(pages)
        oob_pages[0] = list(pages[0]) + [empty_item]
        oob_native = out_native.apply_adaptive_overlay_colors_batch(source_pdf_path=src, pages=oob_pages)
        out_native.NATIVE = False
        try:
            oob_reference = out_native.apply_adaptive_overlay_colors_batch(source_pdf_path=src, pages=oob_pages)
        finally:
            out_native.NATIVE = was_out
        assert_colors_close(oob_native, oob_reference, "empty item_id + oob parity")
        assert oob_native[99] == pages[99], "out-of-range page passes through as shallow copy"
        assert len(oob_native[0]) == 5, "empty item_id still yields an adapted item with defaults"
        assert oob_native[0][4].get("_render_cover_fill") == (1, 1, 1)
        assert oob_native[0][4].get("_render_text_color") == (0, 0, 0)

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
