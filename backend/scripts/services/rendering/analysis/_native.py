"""Optional native (Rust) backend for `build_render_document_analysis`.

Builds the whole-document render analysis from a single bridge call
(`build_render_document_analysis`), which opens the PDF once and walks every
selected page through `page_snapshot` -> `build_render_page_profile` ->
`build_render_page_analysis` in Rust. Without the native module — or when a
native call fails — `build_render_document_analysis` falls back to the
pure-Python reference `document.builder._build_render_document_analysis_python`,
so importing this module is always safe.

Documented divergences (the empty-`text_traces` fallback, since mupdf-rs exposes
no text-trace opacity/type-3 signal): `visible_text` / `editable_text` fall back
to `word_count >= 20`, and `hidden_text` is always false. This only diverges on
"hidden text and <20 words" pages — the routing fields (`kind` and everything
derived from it) stay parity.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering.analysis.document.models import RenderDocumentAnalysis

try:
    from rendering_bridge import build_render_document_analysis as _native_build_render_document_analysis
    from rendering_bridge import read_page_geometry as _native_read_page_geometry

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def build_render_document_analysis(
    *,
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]] | None = None,
    start_page: int = 0,
    end_page: int = -1,
) -> RenderDocumentAnalysis:
    """`document.builder.build_render_document_analysis`, routed to the native
    bridge when built; otherwise the pure-Python reference. Any native failure
    falls back to the reference (analysis is advisory — a fallback result is
    better than raising)."""
    if not NATIVE:
        return _build_python(source_pdf_path, translated_pages, start_page, end_page)
    try:
        return _build_native(source_pdf_path, translated_pages, start_page, end_page)
    except Exception as exc:
        print(
            f"render document analysis native failed {type(exc).__name__}: {exc}; falling back to reference",
            flush=True,
        )
        return _build_python(source_pdf_path, translated_pages, start_page, end_page)


def _build_python(
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]] | None,
    start_page: int,
    end_page: int,
) -> RenderDocumentAnalysis:
    # Lazy: builder's public delegator imports this shim, so a top-level
    # reference import here would be circular.
    from services.rendering.analysis.document.builder import _build_render_document_analysis_python

    return _build_render_document_analysis_python(
        source_pdf_path=source_pdf_path,
        translated_pages=translated_pages,
        start_page=start_page,
        end_page=end_page,
    )


def _build_native(
    source_pdf_path: Path,
    translated_pages: dict[int, list[dict]] | None,
    start_page: int,
    end_page: int,
) -> RenderDocumentAnalysis:
    pdf_bytes = source_pdf_path.read_bytes()
    page_count = int(json.loads(_native_read_page_geometry(pdf_bytes))["page_count"])
    selected = _selected_page_indices(
        page_count=page_count,
        translated_pages=translated_pages,
        start_page=start_page,
        end_page=end_page,
    )
    if not selected:
        return RenderDocumentAnalysis(pages={})
    config = {
        str(page_idx): [
            [float(v) for v in item.get("bbox", [])]
            for item in (translated_pages or {}).get(page_idx, [])
        ]
        for page_idx in selected
    }
    manifest = json.loads(_native_build_render_document_analysis(pdf_bytes, json.dumps(config)))
    analysis = RenderDocumentAnalysis.from_manifest(manifest)
    return analysis if analysis is not None else RenderDocumentAnalysis(pages={})


def _selected_page_indices(
    *,
    page_count: int,
    translated_pages: dict[int, list[dict]] | None,
    start_page: int,
    end_page: int,
) -> list[int]:
    if translated_pages:
        return sorted(page_idx for page_idx in translated_pages if 0 <= page_idx < page_count)
    # Lazy: same selection logic as the reference builder, copied to avoid a
    # shim -> translation circular import at module load.
    from services.translation.public import resolve_page_range

    resolved_start, resolved_stop = resolve_page_range(page_count, start_page, end_page)
    return list(range(resolved_start, resolved_stop + 1))
