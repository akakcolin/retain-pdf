"""Optional native (Rust) backend for the visual-profile sampler.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`sampler.build_document_visual_profile` through the ported Rust primitives
(`source.background._native` fills/spans/foreground). Without the native module
every call falls back to the pure-Python implementation in `sampler.py`, so
importing this module is always safe.

The native path opens the source PDF three times (once per primitive): batch
local-background fills for every item that needs one, whole-page span dicts
when any item needs span text color, and foreground probes for doc-title items
(over-approximation; the probe result is ignored when span sampling succeeds).
Page validity is taken from the fills response (`batch` entry presence), which
the `sample_page_color_fills` bridge emits for every readable page.
"""

from __future__ import annotations

from pathlib import Path

from services.rendering import _routing

try:
    from rendering_bridge import sample_foreground_colors as _native_sample_foreground_colors

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def build_document_visual_profile(
    *,
    source_pdf_path: Path,
    pages: dict[int, list[dict]],
):
    """`sampler.build_document_visual_profile`, routed to the native primitives
    when the module is built; otherwise the pure-Python reference in `sampler`."""
    if not _routing.routed("visual_profile", "build_document_visual_profile", NATIVE):
        from services.rendering.visual_profile.sampler import build_document_visual_profile as _reference

        return _reference(source_pdf_path=source_pdf_path, pages=pages)

    from services.rendering.source.background import _native as _source_native
    from services.rendering.source.rects import Rect
    from services.rendering.visual_profile.contracts import DocumentVisualProfile
    from services.rendering.visual_profile.contracts import PageVisualProfile
    from services.rendering.visual_profile.contracts import VISUAL_PROFILE_ALGORITHM_VERSION
    from services.rendering.visual_profile.sampler import DEFAULT_PAGE_BACKGROUND
    from services.rendering.visual_profile.sampler import _fallback_item_profile
    from services.rendering.visual_profile.sampler import _is_document_title
    from services.rendering.visual_profile.sampler import _item_needs_visual_profile_background
    from services.rendering.visual_profile.sampler import _item_rects
    from services.rendering.visual_profile.sampler import _page_needs_span_text_color
    from services.rendering.visual_profile.sampler import _sample_item_profile_with_data
    from services.rendering.visual_profile.text_spans import PageSpanColorSampler
    from services.rendering.visual_profile.text_spans import SpanColorSample

    def decode_spans(entries: list) -> list[SpanColorSample]:
        samples: list[SpanColorSample] = []
        for entry in entries:
            span_text = str(entry[5])
            if not span_text.strip():
                continue
            color_int = int(entry[4])
            samples.append(
                SpanColorSample(
                    rect=Rect(float(entry[0]), float(entry[1]), float(entry[2]), float(entry[3])),
                    text=span_text,
                    rgb=((color_int >> 16) & 255, (color_int >> 8) & 255, color_int & 255),
                )
            )
        return samples

    page_indices = sorted(pages)
    fills_cfg: dict[int, dict[str, object]] = {}
    span_cfg: dict[int, list[None]] = {}
    for page_idx in page_indices:
        items = pages[page_idx]
        item_rects = _item_rects(items)
        background_ids: list[str] = []
        background_rects: list[Rect] = []
        for item in items:
            item_id = str(item.get("item_id") or "")
            if not _item_needs_visual_profile_background(item):
                continue
            rect = item_rects.get(item_id)
            if rect is None:
                continue
            background_ids.append(item_id)
            background_rects.append(rect)
        fills_cfg[page_idx] = {
            "batch_rects": [[r.x0, r.y0, r.x1, r.y1] for r in background_rects],
            "target_rects": [[r.x0, r.y0, r.x1, r.y1] for r in background_rects],
            "target_ids": background_ids,
        }
        if _page_needs_span_text_color(items):
            span_cfg[page_idx] = [None]

    fills_raw = _source_native.sample_page_color_fills(
        source_pdf_path=source_pdf_path,
        by_page={
            page_idx: {
                "batch_rects": cfg["batch_rects"],
                "target_rects": cfg["target_rects"],
            }
            for page_idx, cfg in fills_cfg.items()
        },
    )
    spans_raw = _source_native.extract_page_span_dicts(
        source_pdf_path=source_pdf_path,
        clips_by_page=span_cfg,
    )

    probe_cfg: dict[int, list[dict]] = {}
    probe_ids: dict[int, list[str]] = {}
    for page_idx in page_indices:
        cfg = fills_cfg.get(page_idx)
        if cfg is None:
            continue
        page_fills = fills_raw.get(str(page_idx), {}).get("targets", {})
        fill_by_item_id: dict[str, tuple[float, float, float]] = {}
        for i, item_id in enumerate(cfg["target_ids"]):
            key = str(i)
            if key in page_fills:
                fill = page_fills[key]
                fill_by_item_id[item_id] = (float(fill[0]), float(fill[1]), float(fill[2]))
        item_rects = _item_rects(pages[page_idx])
        probes: list[dict] = []
        ids: list[str] = []
        for item in pages[page_idx]:
            if not _is_document_title(item):
                continue
            rect = item_rects.get(str(item.get("item_id") or ""))
            if rect is None:
                continue
            item_id = str(item.get("item_id") or "")
            fill = fill_by_item_id.get(item_id, DEFAULT_PAGE_BACKGROUND)
            probes.append({"rect": [rect.x0, rect.y0, rect.x1, rect.y1], "fill": list(fill)})
            ids.append(item_id)
        if probes:
            probe_cfg[page_idx] = probes
            probe_ids[page_idx] = ids

    probes_raw = _source_native.sample_foreground_colors(
        source_pdf_path=source_pdf_path,
        probes_by_page=probe_cfg,
    )

    page_profiles: dict[int, PageVisualProfile] = {}
    for page_idx in page_indices:
        cfg = fills_cfg.get(page_idx)
        page_fills = fills_raw.get(str(page_idx))
        if cfg is None or page_fills is None or "batch" not in page_fills:
            continue
        items = pages[page_idx]
        targets = page_fills.get("targets", {})
        fill_by_item_id: dict[str, tuple[float, float, float]] = {}
        for i, item_id in enumerate(cfg["target_ids"]):
            key = str(i)
            if key in targets:
                fill = targets[key]
                fill_by_item_id[item_id] = (float(fill[0]), float(fill[1]), float(fill[2]))
        span_sampler = None
        if _page_needs_span_text_color(items):
            samples = decode_spans(spans_raw.get(str(page_idx), {}).get("0", []))
            if samples:
                span_sampler = PageSpanColorSampler(samples)
        page_probes = probes_raw.get(str(page_idx), {})
        foreground_by_item_id: dict[str, tuple[float, float, float]] = {}
        foreground_confidence_by_item_id: dict[str, float] = {}
        for i, item_id in enumerate(probe_ids.get(page_idx, [])):
            key = str(i)
            if key in page_probes:
                entry = page_probes[key]
                foreground_by_item_id[item_id] = (
                    float(entry[0]),
                    float(entry[1]),
                    float(entry[2]),
                )
                foreground_confidence_by_item_id[item_id] = float(entry[3])
        item_rects = _item_rects(items)
        profiles = {}
        for item in items:
            item_id = str(item.get("item_id") or "")
            if not item_id:
                continue
            rect = item_rects.get(item_id)
            if rect is None:
                profiles[item_id] = _fallback_item_profile(
                    page_index=page_idx,
                    item=item,
                    item_id=item_id,
                )
                continue
            profiles[item_id] = _sample_item_profile_with_data(
                page_index=page_idx,
                item=item,
                item_id=item_id,
                rect=rect,
                background=fill_by_item_id.get(item_id, DEFAULT_PAGE_BACKGROUND),
                span_sampler=span_sampler,
                foreground_color=foreground_by_item_id.get(item_id),
                foreground_confidence=foreground_confidence_by_item_id.get(item_id, 0.0),
            )
        page_profiles[page_idx] = PageVisualProfile(
            page_index=page_idx,
            background_rgb=DEFAULT_PAGE_BACKGROUND,
            items=profiles,
        )
    _routing.record_native_hit("visual_profile", "build_document_visual_profile")
    return DocumentVisualProfile(
        algorithm=VISUAL_PROFILE_ALGORITHM_VERSION,
        pages=page_profiles,
    )
