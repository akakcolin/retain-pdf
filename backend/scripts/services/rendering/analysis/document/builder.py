from __future__ import annotations

from pathlib import Path

from services.rendering.analysis.document.models import RenderDocumentAnalysis
from services.rendering.analysis.document.models import RenderPageAnalysis
from services.rendering.analysis.profile.models import RenderPageProfile
from services.rendering.analysis.route.builder import build_render_page_route


def build_render_document_analysis(
    *,
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]] | None = None,
    start_page: int = 0,
    end_page: int = -1,
) -> RenderDocumentAnalysis:
    """Whole-document render analysis, routed through the native shim (native-only)."""
    from services.rendering.analysis._native import build_render_document_analysis as _impl

    return _impl(
        source_pdf_path=source_pdf_path,
        translated_pages=translated_pages,
        start_page=start_page,
        end_page=end_page,
    )


def build_render_page_analysis(profile: RenderPageProfile) -> RenderPageAnalysis:
    route = build_render_page_route(profile)
    return RenderPageAnalysis(
        page_index=profile.geometry.page_index,
        kind=profile.kind,
        redaction=route.redaction,
        background=route.background,
        compose=route.compose,
        layout=route.layout,
        reason=route.reason,
        has_large_background=profile.image_background.has_large_background,
        background_coverage_ratio=profile.image_background.coverage_ratio,
        visible_text=profile.text_layer.has_visible_text,
        hidden_text=profile.text_layer.has_hidden_text,
        editable_text=profile.text_layer.editable,
        drawing_count=profile.vector_layer.drawing_count,
        vector_heavy=profile.vector_layer.vector_heavy,
    )


__all__ = [
    "build_render_document_analysis",
    "build_render_page_analysis",
]
