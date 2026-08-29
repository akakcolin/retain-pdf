from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

import fitz

from services.rendering import _routing
from services.rendering.analysis.profile.builder import build_render_page_profile
from services.rendering.analysis.profile.models import RenderPageKind
from services.rendering.analysis.route.builder import build_render_page_route
from services.rendering.analysis.route.models import RenderPageRoute

try:
    from rendering_bridge import classify_render_page as _native_classify_render_page

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


@dataclass(frozen=True)
class RenderPageClassification:
    kind: RenderPageKind
    large_background_image: bool
    visible_text_traces: int
    hidden_text_traces: int
    drawing_count: int
    background_coverage_ratio: float
    route: RenderPageRoute | None = None


def classify_render_page(
    page: fitz.Page,
    *,
    background_threshold: float = 0.75,
) -> RenderPageClassification:
    """Per-page classification reference (fitz-backed). New callers with a file
    path should use :func:`classify_render_page_pdf` (native-routed)."""
    return _classify_render_page_python(page, background_threshold=background_threshold)


def classify_render_page_pdf(
    pdf_path: Path,
    page_index: int,
    *,
    background_threshold: float = 0.75,
) -> RenderPageClassification:
    """Per-page classification routed to the native bridge (B-A); the fitz
    reference only runs as the shim's fallback."""
    if not _routing.routed("analysis", "classify_render_page", NATIVE):
        return _classify_render_page_pdf_python(pdf_path, page_index, background_threshold)
    try:
        manifest = json.loads(
            _native_classify_render_page(pdf_path.read_bytes(), page_index, background_threshold)
        )
        _routing.record_native_hit("analysis", "classify_render_page")
        return _classification_from_manifest(manifest)
    except Exception as exc:
        _routing.record_fallback(
            "analysis",
            "classify_render_page",
            _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
        )
        print(
            f"classify_render_page native failed {type(exc).__name__}: {exc}; falling back to reference",
            flush=True,
        )
        return _classify_render_page_pdf_python(pdf_path, page_index, background_threshold)


def _classify_render_page_pdf_python(
    pdf_path: Path, page_index: int, background_threshold: float
) -> RenderPageClassification:
    doc = fitz.open(pdf_path)
    try:
        return _classify_render_page_python(doc[page_index], background_threshold=background_threshold)
    finally:
        doc.close()


def _classify_render_page_python(
    page: fitz.Page, *, background_threshold: float
) -> RenderPageClassification:
    profile = build_render_page_profile(page, background_threshold=background_threshold)
    route = build_render_page_route(profile)
    return RenderPageClassification(
        kind=profile.kind,
        large_background_image=profile.image_background.has_large_background,
        visible_text_traces=profile.text_layer.visible_traces,
        hidden_text_traces=profile.text_layer.hidden_traces,
        drawing_count=profile.vector_layer.drawing_count,
        background_coverage_ratio=profile.image_background.coverage_ratio,
        route=route,
    )


def _classification_from_manifest(manifest: dict) -> RenderPageClassification:
    route = manifest["route"]
    return RenderPageClassification(
        kind=manifest["kind"],
        large_background_image=bool(manifest["large_background_image"]),
        visible_text_traces=int(manifest["visible_text_traces"]),
        hidden_text_traces=int(manifest["hidden_text_traces"]),
        drawing_count=int(manifest["drawing_count"]),
        background_coverage_ratio=float(manifest["background_coverage_ratio"]),
        route=RenderPageRoute(
            redaction=route["redaction"],
            background=route["background"],
            compose=route["compose"],
            layout=route["layout"],
            reason=route["reason"],
        ),
    )
