from __future__ import annotations

from pathlib import Path

from services.rendering.layout.model.models import RenderPageSpec
from services.rendering.visual_profile import VisualProfileRuntime


def build_clean_background_pdf(
    *,
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]],
    output_pdf_path: Path,
    redaction_strategy: str | None = None,
    page_specs: list[RenderPageSpec] | None = None,
    source_text_precleaned_page_indices: frozenset[int] = frozenset(),
    visual_profile: VisualProfileRuntime | None = None,
) -> Path:
    """Route to the native Rust stage (see `_native.py`)."""
    from services.rendering.source.background import _native

    return _native.build_clean_background_pdf(
        source_pdf_path=source_pdf_path,
        translated_pages=translated_pages,
        output_pdf_path=output_pdf_path,
        redaction_strategy=redaction_strategy,
        page_specs=page_specs,
        source_text_precleaned_page_indices=source_text_precleaned_page_indices,
        visual_profile=visual_profile,
    )
