"""Optional native (Rust) backend for the source-background stage.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`build_clean_background_pdf` through the ported Rust stage. Without the native
module every call falls back to the pure-Python implementation, so importing
this module is always safe.

The native stage ports `stage.py::build_clean_background_pdf`. Page-spec
replacement (`redaction_items_from_layout_blocks`) and the visual-profile fill
matching (`background_fill_for_item`) run in Rust: this shim sends the ORIGINAL
translated items plus the raw `RenderPageSpec` JSON (`render_page_spec_to_bridge`)
and a flat first-wins fill map (`visual_profile_fill_map`), and the Rust side
applies the replacement + per-item fill before redaction. Formula-region guard
protection and vector-text rect collection are full 7R-6 ports, and the
`text_layer_only`/`text_redaction` subroutes are full 7R-7 ports. Calls with an
instrumented (mocked) Python stage fall back to pure Python.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering import _routing
import services.rendering.source.background.stage as _stage
from services.rendering.policy import protect_formula_regions_in_redaction_items
from services.rendering.source.background.redaction_items import (
    redaction_items_from_layout_blocks,
)
from services.rendering.source.background.stage import _build_clean_background_pdf_python
from services.rendering.source.document_ops import save_optimized_pdf
from services.rendering.source.redaction import redact_source_text_areas
from services.rendering.source.vector_text import collect_vector_text_rects

try:
    from rendering_bridge import build_clean_background_pdf as _native_build_clean_background_pdf
    from rendering_bridge import extract_page_span_dicts as _native_extract_page_span_dicts
    from rendering_bridge import sample_page_color_fills as _native_sample_page_color_fills
    from rendering_bridge import sample_title_visual_colors as _native_sample_title_visual_colors
    from rendering_bridge import sample_foreground_colors as _native_sample_foreground_colors

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def _python_stage_instrumented() -> bool:
    """True when the pure-Python stage's functions have been replaced (e.g.
    mocked in white-box tests) — the caller is exercising the Python
    orchestration, so route there instead of the native stage."""
    return (
        _stage.collect_vector_text_rects is not collect_vector_text_rects
        or _stage.protect_formula_regions_in_redaction_items
        is not protect_formula_regions_in_redaction_items
        or _stage.redact_source_text_areas is not redact_source_text_areas
        or _stage.save_optimized_pdf is not save_optimized_pdf
        or _stage.redaction_items_from_layout_blocks
        is not redaction_items_from_layout_blocks
    )


def _native_eligible(
    redaction_strategy: str | None,
) -> tuple[bool, _routing.FallbackReason | None]:
    """True when the native stage can take the call: every strategy (auto /
    visual_cover / visual_cover_and_remove_text / text_layer_only / text_redaction)
    resolves to a ported route, and the Python stage is not instrumented.
    `page_specs` and `visual_profile` no longer gate native — the shim
    precomputes them into the item DTO. Returns the reason alongside False so
    the routing layer can log it."""
    if _python_stage_instrumented():
        return False, _routing.FallbackReason.STAGE_INSTRUMENTED
    return True, None


def render_page_spec_to_bridge(spec) -> dict:
    """Deterministic bridge-facing serialization of a `RenderPageSpec` (fixed
    key order), shared with the differential generator so the corpus records
    exactly what the bridge receives. The Rust DTO consumes only `page_index`
    and `blocks`; the remaining keys are carried for fidelity."""
    return {
        "page_index": spec.page_index,
        "page_width_pt": spec.page_width_pt,
        "page_height_pt": spec.page_height_pt,
        "background_pdf_path": (
            str(spec.background_pdf_path) if spec.background_pdf_path else None
        ),
        "blocks": [
            {
                "block_id": b.block_id,
                "background_rect": list(b.background_rect),
                "content_rect": list(b.content_rect),
                "content_kind": b.content_kind,
                "content_text": b.content_text,
                "plain_text": b.plain_text,
            }
            for b in spec.blocks
        ],
    }


def visual_profile_fill_map(visual_profile) -> dict[str, list[float]]:
    """Flat first-wins `{item_id: [r, g, b]}` fill table extracted from the
    visual profile (mirrors `VisualProfileRuntime.item()`'s first-page-wins;
    not `colors_by_item_id()`'s last-wins). Empty when the profile is None or
    not loaded. The Rust side does the per-item key-order matching."""
    if visual_profile is None or not visual_profile.loaded:
        return {}
    fills: dict[str, list[float]] = {}
    for page in visual_profile.profile.pages.values():
        for item_id, item in page.items.items():
            if item_id not in fills:
                fills[item_id] = list(item.background_rgb)
    return fills


def build_clean_background_pdf(
    *,
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]],
    output_pdf_path: Path,
    redaction_strategy: str | None = None,
    page_specs=None,
    source_text_precleaned_page_indices=frozenset(),
    visual_profile=None,
) -> Path:
    if not _routing.routed("background", "build_clean_background_pdf", NATIVE):
        return _build_clean_background_pdf_python(
            source_pdf_path=source_pdf_path,
            translated_pages=translated_pages,
            output_pdf_path=output_pdf_path,
            redaction_strategy=redaction_strategy,
            page_specs=page_specs,
            source_text_precleaned_page_indices=source_text_precleaned_page_indices,
            visual_profile=visual_profile,
        )
    eligible, reason = _native_eligible(redaction_strategy)
    if not eligible:
        _routing.record_fallback("background", "build_clean_background_pdf", reason)
        return _build_clean_background_pdf_python(
            source_pdf_path=source_pdf_path,
            translated_pages=translated_pages,
            output_pdf_path=output_pdf_path,
            redaction_strategy=redaction_strategy,
            page_specs=page_specs,
            source_text_precleaned_page_indices=source_text_precleaned_page_indices,
            visual_profile=visual_profile,
        )

    source_bytes = source_pdf_path.read_bytes()
    translated_json = json.dumps(translated_pages)
    page_specs_json = json.dumps([render_page_spec_to_bridge(s) for s in page_specs or []])
    fill_map_json = json.dumps(visual_profile_fill_map(visual_profile))
    precleaned_json = json.dumps(sorted(source_text_precleaned_page_indices))
    out_bytes = _native_build_clean_background_pdf(
        source_bytes,
        translated_json,
        redaction_strategy,
        precleaned_json,
        page_specs_json,
        fill_map_json,
    )
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    output_pdf_path.write_bytes(out_bytes)
    _routing.record_native_hit("background", "build_clean_background_pdf")
    return output_pdf_path


def sample_page_color_fills(
    *,
    source_pdf_path: Path,
    by_page: dict[int, dict[str, list[list[float]]]],
) -> dict[str, dict[str, object]]:
    """Batch local-background fill sampling per target rect (RGB, scale 2.0),
    routed to the native bridge when built; otherwise the pure-Python reference.
    `by_page[page_idx] = {"batch_rects": [[x0,y0,x1,y1], ...], "target_rects":
    [[...], ...]}`; the batch rects form the `LocalBackgroundSampler` clip
    (a `< BACKGROUND_CLIP_SAMPLER_MIN_RECTS` gate), the target rects are every
    rect the caller may read (extra results are ignored). Returns
    `{"<page>": {"batch": {"count": N, "clip": [...] | None}, "targets":
    {"<i>": [r,g,b]}}}` (keys are strings)."""
    if _routing.routed("background", "sample_page_color_fills", NATIVE):
        config_json = json.dumps(
            {
                str(page_idx): {
                    "batch_rects": cfg["batch_rects"],
                    "target_rects": cfg["target_rects"],
                }
                for page_idx, cfg in by_page.items()
            }
        )
        result = json.loads(_native_sample_page_color_fills(source_pdf_path.read_bytes(), config_json))
        _routing.record_native_hit("background", "sample_page_color_fills")
        return result
    return _sample_page_color_fills_python(source_pdf_path=source_pdf_path, by_page=by_page)


def extract_page_span_dicts(
    *,
    source_pdf_path: Path,
    clips_by_page: dict[int, list[list[float] | None]],
) -> dict[str, dict[str, list[list[float]]]]:
    """Structured-text span dicts per page and per clip, routed to the native
    bridge when built; otherwise the pure-Python reference. `clips_by_page`
    lists per-page clips (`None` = whole page). Returns
    `{"<page>": {"<i>": [[x0,y0,x1,y1, color_int, "text"], ...]}}` (keys are
    strings); unreadable pages are omitted."""
    if _routing.routed("background", "extract_page_span_dicts", NATIVE):
        clips_json = json.dumps(
            {str(page_idx): [clip for clip in clips] for page_idx, clips in clips_by_page.items()}
        )
        result = json.loads(_native_extract_page_span_dicts(source_pdf_path.read_bytes(), clips_json))
        _routing.record_native_hit("background", "extract_page_span_dicts")
        return result
    return _extract_page_span_dicts_python(source_pdf_path=source_pdf_path, clips_by_page=clips_by_page)


def sample_title_visual_colors(
    *,
    source_pdf_path: Path,
    visuals_by_page: dict[int, list[dict]],
) -> dict[str, dict[str, list[float]]]:
    """Title foreground colors probed from a 3x RGB render of each visual's
    rect, routed to the native bridge when built; otherwise the pure-Python
    reference. `visuals_by_page[page_idx]` is a list of
    `{"rect": [x0,y0,x1,y1], "fill": [r,g,b]}`. Returns
    `{"<page>": {"<i>": [r,g,b]}}`; a failed probe is an absent `<i>` key."""
    if _routing.routed("background", "sample_title_visual_colors", NATIVE):
        visuals_json = json.dumps(
            {
                str(page_idx): [{"rect": v["rect"], "fill": v["fill"]} for v in visuals]
                for page_idx, visuals in visuals_by_page.items()
            }
        )
        result = json.loads(_native_sample_title_visual_colors(source_pdf_path.read_bytes(), visuals_json))
        _routing.record_native_hit("background", "sample_title_visual_colors")
        return result
    return _sample_title_visual_colors_python(source_pdf_path=source_pdf_path, visuals_by_page=visuals_by_page)


def sample_foreground_colors(
    *,
    source_pdf_path: Path,
    probes_by_page: dict[int, list[dict]],
) -> dict[str, dict[str, list[float]]]:
    """Foreground color + confidence probed from a 3x RGB render of each probe
    rect (clipped to the page bounds) against its background fill, routed to the
    native bridge when built; otherwise the pure-Python reference.
    `probes_by_page[page_idx]` is a list of `{"rect": [x0,y0,x1,y1], "fill":
    [r,g,b]}`. Returns `{"<page>": {"<i>": [r,g,b, confidence]}}`; a probe that
    fails (rect outside the page / render failure / no text-like foreground) is
    an absent `<i>` key, a bad page is an absent page key."""
    if _routing.routed("background", "sample_foreground_colors", NATIVE):
        probes_json = json.dumps(
            {
                str(page_idx): [{"rect": p["rect"], "fill": p["fill"]} for p in probes]
                for page_idx, probes in probes_by_page.items()
            }
        )
        result = json.loads(
            _native_sample_foreground_colors(source_pdf_path.read_bytes(), probes_json)
        )
        _routing.record_native_hit("background", "sample_foreground_colors")
        return result
    return _sample_foreground_colors_python(source_pdf_path=source_pdf_path, probes_by_page=probes_by_page)


def _sample_foreground_colors_python(
    *,
    source_pdf_path: Path,
    probes_by_page: dict[int, list[dict]],
) -> dict[str, dict[str, list[float]]]:
    import fitz

    from services.rendering.visual_profile.foreground import sample_foreground_color_from_pixels

    source_doc = fitz.open(source_pdf_path)
    try:
        result: dict[str, dict[str, list[float]]] = {}
        for page_idx, probes in probes_by_page.items():
            if page_idx < 0 or page_idx >= len(source_doc):
                continue
            page = source_doc[page_idx]
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
        source_doc.close()


def _sample_page_color_fills_python(
    *,
    source_pdf_path: Path,
    by_page: dict[int, dict[str, list[list[float]]]],
) -> dict[str, dict[str, object]]:
    import fitz

    from services.rendering.source.background.fill import LocalBackgroundSampler
    from services.rendering.source.background.fill import _batch_sampler_clip_rect
    from services.rendering.source.background.fill import sample_local_background_fill

    source_doc = fitz.open(source_pdf_path)
    try:
        result: dict[str, dict[str, object]] = {}
        for page_idx, cfg in by_page.items():
            if page_idx < 0 or page_idx >= len(source_doc):
                continue
            page = source_doc[page_idx]
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
        source_doc.close()


def _extract_page_span_dicts_python(
    *,
    source_pdf_path: Path,
    clips_by_page: dict[int, list[list[float] | None]],
) -> dict[str, dict[str, list[list[float]]]]:
    import fitz

    source_doc = fitz.open(source_pdf_path)
    try:
        result: dict[str, dict[str, list[list[float]]]] = {}
        for page_idx, clips in clips_by_page.items():
            if page_idx < 0 or page_idx >= len(source_doc):
                continue
            page = source_doc[page_idx]
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
        source_doc.close()


def _sample_title_visual_colors_python(
    *,
    source_pdf_path: Path,
    visuals_by_page: dict[int, list[dict]],
) -> dict[str, dict[str, list[float]]]:
    import fitz

    from services.rendering.output.typst.color_adapt import title_text_color_from_visual_components

    source_doc = fitz.open(source_pdf_path)
    try:
        result: dict[str, dict[str, list[float]]] = {}
        for page_idx, visuals in visuals_by_page.items():
            if page_idx < 0 or page_idx >= len(source_doc):
                continue
            page = source_doc[page_idx]
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
        source_doc.close()
