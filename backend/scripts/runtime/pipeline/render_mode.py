from __future__ import annotations

import math
from pathlib import Path

from services.rendering.analysis.document.builder import build_render_document_analysis
from services.rendering.contracts import RenderDocumentAnalysis


def is_pseudo_editable_scan_pdf(source_pdf_path: Path, start_page: int, end_page: int) -> bool:
    """Auto-mode probe for pseudo-editable-scan PDFs, sampled on the leading
    pages. Routed through the native render analysis; the fitz reference only
    runs as the shim's fallback."""
    return is_pseudo_editable_scan_analysis(_sample_pdf_analysis(source_pdf_path, start_page, end_page))


def is_editable_pdf(source_pdf_path: Path, start_page: int, end_page: int) -> bool:
    """Auto-mode probe for editable-text PDFs, sampled on the leading pages.
    Routed through the native render analysis; the fitz reference only runs as
    the shim's fallback."""
    return is_editable_analysis(_sample_pdf_analysis(source_pdf_path, start_page, end_page))


def _sample_pdf_analysis(
    source_pdf_path: Path, start_page: int, end_page: int
) -> RenderDocumentAnalysis:
    sample_end = start_page + 2 if end_page < 0 else min(end_page, start_page + 2)
    return build_render_document_analysis(
        source_pdf_path=source_pdf_path,
        translated_pages=None,
        start_page=start_page,
        end_page=sample_end,
    )


def is_pseudo_editable_scan_analysis(analysis: RenderDocumentAnalysis) -> bool:
    pages = list(analysis.pages.values())[:3]
    sampled = len(pages)
    pseudo_scan_pages = sum(1 for page in pages if page.kind == "pseudo_editable_scan")
    return sampled > 0 and pseudo_scan_pages >= max(1, math.ceil(sampled / 2))


def is_editable_analysis(analysis: RenderDocumentAnalysis) -> bool:
    pages = list(analysis.pages.values())[:3]
    sampled = len(pages)
    editable_pages = sum(1 for page in pages if page.editable_text and page.kind == "editable_text")
    pseudo_scan_pages = sum(1 for page in pages if page.kind == "pseudo_editable_scan")
    if sampled == 0 or pseudo_scan_pages >= sampled:
        return False
    return editable_pages >= max(1, math.ceil(sampled / 2))


def resolve_effective_render_mode(
    *,
    render_mode: str,
    source_pdf_path: Path,
    start_page: int,
    end_page: int,
    translated_pages_map: dict[int, list[dict]] | None = None,
    document_analysis: RenderDocumentAnalysis | None = None,
) -> str:
    if render_mode != "auto":
        return render_mode

    if not translated_pages_map:
        print("auto render mode selected: overlay (no translated pages map)")
        return "overlay"

    if document_analysis is not None:
        if is_pseudo_editable_scan_analysis(document_analysis):
            print(
                "auto render mode selected: typst_visual "
                "(pseudo-editable scan or tiled image background PDF)"
            )
            return "typst_visual"
        if not is_editable_analysis(document_analysis):
            print("auto render mode selected: typst_visual (non-editable PDF)")
            return "typst_visual"
        print("auto render mode selected: overlay (editable PDF default route)")
        return "overlay"

    if is_pseudo_editable_scan_pdf(source_pdf_path, start_page, end_page):
        print(
            "auto render mode selected: typst_visual "
            "(pseudo-editable scan or tiled image background PDF)"
        )
        return "typst_visual"
    if not is_editable_pdf(source_pdf_path, start_page, end_page):
        print("auto render mode selected: typst_visual (non-editable PDF)")
        return "typst_visual"

    print("auto render mode selected: overlay (editable PDF default route)")
    return "overlay"
