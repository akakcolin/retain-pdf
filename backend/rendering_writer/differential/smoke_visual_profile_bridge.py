#!/usr/bin/env python3
"""Native bridge smoke test for the visual-profile sampling (Phase B2-4).

Exercises the three source primitives (`sample_page_color_fills`,
`extract_page_span_dicts`, `sample_foreground_colors`) plus the end-to-end
`sampler.build_document_visual_profile` routing and the prewarm entry point.

Parts:

1. Native availability: both shims (`source.background._native`,
   `visual_profile._native`) report NATIVE (maturin build installed).

2. Three-way primitive parity: for a synthetic two-page PDF with a doc-title
   foreground-probe item (thin dark stroke, no text), a doc-title red-span item,
   a non-title `_render_use_cover_fill` gray item, a non-title no-background
   item, a missing-bbox doc-title, an empty item_id, and an out-of-range page,
   each primitive's NATIVE result equals the shim re-invoked with NATIVE=False
   (fitz reference) equals a hand-written fitz loop, dict key by dict key. The
   foreground primitive compares `[r,g,b,confidence]` per probe (RGB within
   2/255 + 1e-9, confidence within 1e-3).

3. End-to-end parity: `build_document_visual_profile` NATIVE vs reference vs a
   manual fitz per-page `build_page_visual_profile` loop; per item the
   item_id/bbox/bbox_space/bbox_source/source_item_kind/method/warnings are
   exact and background_rgb/text_rgb within 2/255 + 1e-9, confidence within
   1e-3.

4. Prewarm parity: `apply_page_color_adapt_for_prewarm` NATIVE vs all-off
   (visual_profile + color_adapt shims both flipped); the empty item_id forces
   the color-adapt fallback path, so both shims matter.

5. Boundaries: missing bbox yields method == "page_fallback" (confidence 0.1,
   warnings ("missing_bbox",)), the out-of-range page is absent from the
   profile, non-doc-title items never use "foreground_pixels", span success
   yields a "*span_color" method (not foreground_pixels), and an empty item_id
   is dropped.

Run from backend/scripts:
    /Volumes/data/Projects/retain-pdf/.venv/bin/python ../rendering_writer/differential/smoke_visual_profile_bridge.py
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
from services.rendering.output.typst.color_adapt import DEFAULT_COVER_FILL  # noqa: E402
from services.rendering.source.background import _native as src_native  # noqa: E402
from services.rendering.source.background.fill import (  # noqa: E402
    LocalBackgroundSampler,
    _batch_sampler_clip_rect,
    sample_local_background_fill,
)
from services.rendering.source.prewarm_color_profile import (  # noqa: E402
    apply_page_color_adapt_for_prewarm,
)
from services.rendering.visual_profile import _native as vp_native  # noqa: E402
from services.rendering.visual_profile.contracts import (  # noqa: E402
    VISUAL_PROFILE_ALGORITHM_VERSION,
    DocumentVisualProfile,
)
from services.rendering.visual_profile.foreground import (  # noqa: E402
    sample_foreground_color_from_pixels,
)
from services.rendering.visual_profile.sampler import (  # noqa: E402
    DEFAULT_PAGE_BACKGROUND,
    _is_document_title,
    _item_needs_visual_profile_background,
    _item_rects,
    build_page_visual_profile,
)

PAGE_WIDTH = 612.0
PAGE_HEIGHT = 792.0
TOL = 2.0 / 255.0 + 1e-9


def _item(*, item_id: str, bbox: list[float], **extra) -> dict:
    item = {
        "item_id": item_id,
        "page_idx": 0,
        "block_type": "text",
        "block_kind": "text",
        "layout_role": "paragraph",
        "semantic_role": "paragraph",
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
    """Page 0 exercises the foreground probe (thin dark stroke, no text), the
    red-span path, the gray cover-fill path, and the no-background path; page 1
    exercises a second doc-title blue-span path. Geometry stays inside the page
    so `rect & page.rect` is a no-op."""
    doc = fitz.open()
    page = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)

    # p000-b001: thin dark "I" stroke for the foreground probe (vector, no text).
    stroke = page.new_shape()
    stroke.draw_rect(fitz.Rect(216, 204, 224, 236))
    stroke.finish(color=None, fill=(0.12, 0.12, 0.12))
    stroke.commit()

    # p000-b002: red text span.
    page.insert_text((20.0, 45.0), "RED TITLE", fontsize=16, fontname="helv", color=(0.8, 0.1, 0.1))

    # p000-b003: gray cover rect.
    gray = page.new_shape()
    gray.draw_rect(fitz.Rect(305, 35, 495, 65))
    gray.finish(color=None, fill=(0.8, 0.8, 0.8))
    gray.commit()

    page2 = doc.new_page(width=PAGE_WIDTH, height=PAGE_HEIGHT)
    page2.insert_text((45.0, 78.0), "BLUE TITLE", fontsize=16, fontname="helv", color=(0.1, 0.1, 0.8))

    raw = doc.tobytes(garbage=0)
    doc.close()
    return raw


def translated_pages() -> dict[int, list[dict]]:
    return {
        0: [
            _item(item_id="p000-b001", bbox=[200.0, 200.0, 240.0, 240.0], layout_role="title", semantic_role="title"),
            _item(item_id="p000-b002", bbox=[20.0, 30.0, 140.0, 60.0], layout_role="title", semantic_role="title"),
            _item(item_id="p000-b003", bbox=[300.0, 30.0, 500.0, 70.0], layout_role="heading", _render_use_cover_fill=True),
            _item(item_id="p000-b004", bbox=[300.0, 100.0, 500.0, 130.0], layout_role="paragraph"),
            _item(item_id="p000-b005", bbox=[], layout_role="title", semantic_role="title"),
            _item(item_id="", bbox=[10.0, 300.0, 60.0, 340.0], _render_use_cover_fill=True),
        ],
        1: [
            _item(item_id="p001-b001", bbox=[40.0, 50.0, 300.0, 90.0], layout_role="title", semantic_role="title"),
        ],
        99: [
            _item(item_id="p099-b001", bbox=[10.0, 10.0, 60.0, 40.0], layout_role="paragraph"),
        ],
    }


def background_rects_for(items: list[dict]) -> tuple[list[str], list[list[float]]]:
    """Mirror the native shim's per-page fills config: every item that needs a
    visual-profile background, mapped through `_item_rects`, in item order. The
    target rects equal the batch rects (one fill per background item)."""
    rects_by_item = _item_rects(items)
    item_ids: list[str] = []
    rects: list[list[float]] = []
    for item in items:
        item_id = str(item.get("item_id") or "")
        if not _item_needs_visual_profile_background(item):
            continue
        rect = rects_by_item.get(item_id)
        if rect is None:
            continue
        item_ids.append(item_id)
        rects.append([rect.x0, rect.y0, rect.x1, rect.y1])
    return item_ids, rects


def probe_config_for(page_idx: int, items: list[dict], fills_out) -> list[dict]:
    """Doc-title over-approximation probes with the item's sampled background
    (falls back to DEFAULT_PAGE_BACKGROUND), mirroring the native shim."""
    page_fills = fills_out.get(str(page_idx), {}).get("targets", {})
    ids, _rects = background_rects_for(items)
    fill_by_item_id: dict[str, tuple[float, float, float]] = {}
    for i, item_id in enumerate(ids):
        fill = page_fills.get(str(i))
        if fill is not None:
            fill_by_item_id[item_id] = (float(fill[0]), float(fill[1]), float(fill[2]))
    rects_by_item = _item_rects(items)
    probes: list[dict] = []
    for item in items:
        if not _is_document_title(item):
            continue
        rect = rects_by_item.get(str(item.get("item_id") or ""))
        if rect is None:
            continue
        item_id = str(item.get("item_id") or "")
        fill = fill_by_item_id.get(item_id, DEFAULT_PAGE_BACKGROUND)
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


def manual_foreground(src: Path, probes_by_page: dict[int, list[dict]]) -> dict[str, dict[str, list[float]]]:
    doc = fitz.open(src)
    try:
        result: dict[str, dict[str, list[float]]] = {}
        for page_idx, probes in probes_by_page.items():
            if not 0 <= page_idx < len(doc):
                continue
            page = doc[page_idx]
            page_out: dict[str, list[float]] = {}
            for i, probe in enumerate(probes):
                rect = fitz.Rect(probe["rect"])
                background = tuple(float(component) for component in probe["fill"])
                color, confidence = sample_foreground_color_from_pixels(page, rect, background)
                if color is not None:
                    page_out[str(i)] = [
                        float(color[0]),
                        float(color[1]),
                        float(color[2]),
                        float(confidence),
                    ]
            if page_out:
                result[str(page_idx)] = page_out
        return result
    finally:
        doc.close()


def manual_document(src: Path, pages: dict[int, list[dict]]) -> DocumentVisualProfile:
    doc = fitz.open(src)
    try:
        profiles = {}
        for page_index, items in pages.items():
            if not 0 <= page_index < len(doc):
                continue
            profiles[page_index] = build_page_visual_profile(doc[page_index], page_index, items)
        return DocumentVisualProfile(VISUAL_PROFILE_ALGORITHM_VERSION, profiles)
    finally:
        doc.close()


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


def assert_foreground_close(left, right, label: str) -> None:
    assert left.keys() == right.keys(), f"{label}: page keys {left.keys()} != {right.keys()}"
    for page_idx in left:
        l_page, r_page = left[page_idx], right[page_idx]
        assert l_page.keys() == r_page.keys(), f"{label}: probe keys on {page_idx}: {l_page.keys()} vs {r_page.keys()}"
        for probe_idx in l_page:
            l_entry, r_entry = l_page[probe_idx], r_page[probe_idx]
            for a, b in zip(l_entry[:3], r_entry[:3]):
                assert abs(a - b) <= TOL, f"{label}: probe {page_idx}.{probe_idx} rgb {a} != {b}"
            assert abs(l_entry[3] - r_entry[3]) <= 1e-3, (
                f"{label}: probe {page_idx}.{probe_idx} confidence {l_entry[3]} != {r_entry[3]}"
            )


def assert_profile_close(left: DocumentVisualProfile, right: DocumentVisualProfile, label: str) -> None:
    assert left.pages.keys() == right.pages.keys(), f"{label}: page keys {left.pages.keys()} != {right.pages.keys()}"
    for page_idx in left.pages:
        l_page, r_page = left.pages[page_idx], right.pages[page_idx]
        assert l_page.items.keys() == r_page.items.keys(), (
            f"{label}: item keys on page {page_idx}: {l_page.items.keys()} vs {r_page.items.keys()}"
        )
        for item_id in l_page.items:
            l_item, r_item = l_page.items[item_id], r_page.items[item_id]
            assert l_item.item_id == r_item.item_id, f"{label}: item id mismatch on {page_idx}"
            assert l_item.page_index == r_item.page_index, f"{label}: page index mismatch on {page_idx}"
            assert l_item.bbox == r_item.bbox, f"{label}: bbox {l_item.bbox} != {r_item.bbox} on {page_idx}.{item_id}"
            assert l_item.bbox_space == r_item.bbox_space, f"{label}: bbox_space on {page_idx}.{item_id}"
            assert l_item.bbox_source == r_item.bbox_source, f"{label}: bbox_source on {page_idx}.{item_id}"
            assert l_item.source_item_kind == r_item.source_item_kind, f"{label}: kind on {page_idx}.{item_id}"
            assert l_item.method == r_item.method, f"{label}: method {l_item.method} != {r_item.method} on {page_idx}.{item_id}"
            assert l_item.warnings == r_item.warnings, f"{label}: warnings on {page_idx}.{item_id}"
            assert abs(l_item.confidence - r_item.confidence) <= 1e-3, (
                f"{label}: confidence {l_item.confidence} != {r_item.confidence} on {page_idx}.{item_id}"
            )
            for a, b in zip(l_item.background_rgb, r_item.background_rgb):
                assert abs(a - b) <= TOL, f"{label}: background {page_idx}.{item_id} {a} != {b}"
            for a, b in zip(l_item.text_rgb, r_item.text_rgb):
                assert abs(a - b) <= TOL, f"{label}: text {page_idx}.{item_id} {a} != {b}"


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


def main() -> None:
    assert src_native.NATIVE, "source background native module not built"
    assert vp_native.NATIVE, "visual profile native module not built"

    raw = build_source_pdf()
    with tempfile.TemporaryDirectory(prefix="rps-vp-") as tmp:
        src = Path(tmp) / "in.pdf"
        src.write_bytes(raw)

        pages = translated_pages()
        fills_by_page: dict[int, dict[str, list[list[float]]]] = {}
        for page_idx in (0, 1, 99):
            _ids, rects = background_rects_for(pages[page_idx])
            fills_by_page[page_idx] = {"batch_rects": rects, "target_rects": rects}
        span_clips = {0: [None], 1: [None]}

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
        assert "99" not in fills_native, f"out-of-range page must be skipped in fills, got {fills_native.keys()}"
        for page_idx in (0, 1):
            assert fills_native[str(page_idx)]["batch"]["clip"] is None, (
                f"expected null batch clip on {page_idx}, got {fills_native[str(page_idx)]['batch']['clip']}"
            )
            assert fills_native[str(page_idx)]["targets"], f"expected resolved targets on {page_idx}"

        spans_native = src_native.extract_page_span_dicts(source_pdf_path=src, clips_by_page=span_clips)
        src_native.NATIVE = False
        try:
            spans_reference = src_native.extract_page_span_dicts(source_pdf_path=src, clips_by_page=span_clips)
        finally:
            src_native.NATIVE = was
        spans_manual = manual_spans(src, span_clips)
        assert spans_native == spans_reference == spans_manual, (
            f"span parity native {spans_native} != reference {spans_reference} != fitz {spans_manual}"
        )
        assert any("RED TITLE" in str(entry[5]) for entry in spans_native["0"]["0"])
        assert any("BLUE TITLE" in str(entry[5]) for entry in spans_native["1"]["0"])

        probes_by_page: dict[int, list[dict]] = {}
        for page_idx in (0, 1):
            probes = probe_config_for(page_idx, pages[page_idx], fills_reference)
            if probes:
                probes_by_page[page_idx] = probes
        assert len(probes_by_page[0]) == 2, f"expected 2 doc-title probes on page 0, got {len(probes_by_page[0])}"
        assert len(probes_by_page[1]) == 1, f"expected 1 doc-title probe on page 1, got {len(probes_by_page[1])}"

        foreground_native = src_native.sample_foreground_colors(source_pdf_path=src, probes_by_page=probes_by_page)
        src_native.NATIVE = False
        try:
            foreground_reference = src_native.sample_foreground_colors(source_pdf_path=src, probes_by_page=probes_by_page)
        finally:
            src_native.NATIVE = was
        foreground_manual = manual_foreground(src, probes_by_page)
        assert_foreground_close(foreground_native, foreground_reference, "foreground native vs reference")
        assert_foreground_close(foreground_reference, foreground_manual, "foreground reference vs fitz")
        assert "0" in foreground_native, f"expected foreground probe results on page 0, got {foreground_native}"
        assert "1" in foreground_native, f"expected foreground probe results on page 1, got {foreground_native}"

        # ---- end-to-end profile parity --------------------------------------
        profile_native = vp_native.build_document_visual_profile(source_pdf_path=src, pages=pages)
        was_vp = vp_native.NATIVE
        vp_native.NATIVE = False
        try:
            profile_reference = vp_native.build_document_visual_profile(source_pdf_path=src, pages=pages)
        finally:
            vp_native.NATIVE = was_vp
        profile_manual = manual_document(src, pages)
        assert_profile_close(profile_native, profile_reference, "profile native vs reference")
        assert_profile_close(profile_reference, profile_manual, "profile reference vs fitz")

        # ---- prewarm parity (flip both shims; empty item_id forces the
        #      color-adapt fallback path inside the prewarm) -------------------
        prewarm_native = apply_page_color_adapt_for_prewarm(src, pages)
        was_out = out_native.NATIVE
        out_native.NATIVE = False
        vp_native.NATIVE = False
        try:
            prewarm_reference = apply_page_color_adapt_for_prewarm(src, pages)
        finally:
            out_native.NATIVE = was_out
            vp_native.NATIVE = was_vp
        assert_colors_close(prewarm_native, prewarm_reference, "prewarm native vs reference")

        # ---- boundaries -----------------------------------------------------
        page0 = profile_native.pages[0].items
        assert set(page0.keys()) == {"p000-b001", "p000-b002", "p000-b003", "p000-b004", "p000-b005"}, (
            f"empty item_id must be dropped, got {page0.keys()}"
        )
        fallback = page0["p000-b005"]
        assert fallback.method == "page_fallback", f"missing bbox method {fallback.method}"
        assert fallback.confidence == 0.1, f"missing bbox confidence {fallback.confidence}"
        assert fallback.bbox == (0.0, 0.0, 0.0, 0.0), f"missing bbox bbox {fallback.bbox}"
        assert fallback.warnings == ("missing_bbox",), f"missing bbox warnings {fallback.warnings}"
        assert 99 not in profile_native.pages, f"out-of-range page must be absent, got {profile_native.pages.keys()}"
        for item_id in ("p000-b003", "p000-b004"):
            assert "foreground_pixels" not in page0[item_id].method, (
                f"non-doc-title {item_id} must not use foreground_pixels: {page0[item_id].method}"
            )
        assert page0["p000-b002"].method == "background_pixels+span_color", (
            f"span success method {page0['p000-b002'].method}"
        )
        assert "foreground_pixels" not in page0["p000-b002"].method, (
            f"span success must not use foreground_pixels: {page0['p000-b002'].method}"
        )
        assert page0["p000-b001"].method == "background_pixels+foreground_pixels", (
            f"foreground probe method {page0['p000-b001'].method}"
        )
        assert profile_native.pages[1].items["p001-b001"].method == "background_pixels+span_color", (
            f"page 1 span method {profile_native.pages[1].items['p001-b001'].method}"
        )

    print("all smoke tests pass")


if __name__ == "__main__":
    main()
