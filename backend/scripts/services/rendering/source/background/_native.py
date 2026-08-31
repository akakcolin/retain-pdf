"""Optional native (Rust) backend for the source-background stage.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`build_clean_background_pdf` through the ported Rust stage. The stage + sampler
primitives are native-only: they raise when the bridge is unavailable instead
of running the retired pure-Python references.

The native stage ports `stage.py::build_clean_background_pdf`. Page-spec
replacement (`redaction_items_from_layout_blocks`) and the visual-profile fill
matching (`background_fill_for_item`) run in Rust: this shim sends the ORIGINAL
translated items plus the raw `RenderPageSpec` JSON (`render_page_spec_to_bridge`)
and a flat first-wins fill map (`visual_profile_fill_map`), and the Rust side
applies the replacement + per-item fill before redaction. Formula-region guard
protection and vector-text rect collection are full 7R-6 ports, and the
`text_layer_only`/`text_redaction` subroutes are full 7R-7 ports.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering import _routing

try:
    from rendering_bridge import build_clean_background_pdf as _native_build_clean_background_pdf
    from rendering_bridge import extract_page_span_dicts as _native_extract_page_span_dicts
    from rendering_bridge import sample_page_color_fills as _native_sample_page_color_fills
    from rendering_bridge import sample_title_visual_colors as _native_sample_title_visual_colors
    from rendering_bridge import sample_foreground_colors as _native_sample_foreground_colors

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


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
        raise RuntimeError(
            "background.build_clean_background_pdf is native-only: the "
            "rendering_bridge build_clean_background_pdf is required"
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
    raise RuntimeError(
        "background.sample_page_color_fills is native-only: the rendering_bridge "
        "sample_page_color_fills is required"
    )


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
    raise RuntimeError(
        "background.extract_page_span_dicts is native-only: the rendering_bridge "
        "extract_page_span_dicts is required"
    )


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
    raise RuntimeError(
        "background.sample_title_visual_colors is native-only: the rendering_bridge "
        "sample_title_visual_colors is required"
    )


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
    raise RuntimeError(
        "background.sample_foreground_colors is native-only: the rendering_bridge "
        "sample_foreground_colors is required"
    )
