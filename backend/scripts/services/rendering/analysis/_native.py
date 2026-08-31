"""Native (Rust) backend for `build_render_document_analysis`.

Builds the whole-document render analysis from a single bridge call
(`build_render_document_analysis`), which opens the PDF once and walks every
selected page through `page_snapshot` -> `build_render_page_profile` ->
`build_render_page_analysis` in Rust. The Python fitz reference
(`document.builder._build_render_document_analysis_python`) is retired; this
module is native-only and raises when the bridge is unavailable.

Documented divergences (the empty-`text_traces` fallback, since mupdf-rs exposes
no text-trace opacity/type-3 signal): `visible_text` / `editable_text` fall back
to `word_count >= 20`, and `hidden_text` is always false. This only diverges on
"hidden text and <20 words" pages — the routing fields (`kind` and everything
derived from it) stay parity.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering import _routing
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
    """`document.builder.build_render_document_analysis`, native-only. The
    bridge is mandatory: when it is unavailable (or the feature flag forces it
    off) this raises instead of running the retired Python reference; a native
    bridge failure also propagates after recording the fallback for D5."""
    if not _routing.routed("analysis", "build_render_document_analysis", NATIVE):
        raise RuntimeError(
            "analysis.build_render_document_analysis is native-only: the rendering_bridge "
            "build is required"
        )
    try:
        result = _build_native(source_pdf_path, translated_pages, start_page, end_page)
        _routing.record_native_hit("analysis", "build_render_document_analysis")
        return result
    except Exception as exc:
        _routing.record_fallback(
            "analysis",
            "build_render_document_analysis",
            _routing.FallbackReason.NATIVE_BRIDGE_ERROR,
        )
        raise


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
