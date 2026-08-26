"""Optional native (Rust) backend for the source-background stage.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`build_clean_background_pdf` through the ported Rust stage. Without the native
module every call falls back to the pure-Python implementation, so importing
this module is always safe.

The native stage ports `stage.py::build_clean_background_pdf` restricted to
`page_specs=None` and `visual_profile=None`. Formula-region guard protection
and vector-text rect collection are full 7R-6 ports; calls with a non-empty
`page_specs`, a non-None `visual_profile`, or an instrumented (mocked) Python
stage fall back to pure Python.
"""

from __future__ import annotations

import json
from pathlib import Path

import services.rendering.source.background.stage as _stage
from services.rendering.policy import protect_formula_regions_in_redaction_items
from services.rendering.source.background.stage import _build_clean_background_pdf_python
from services.rendering.source.document_ops import save_optimized_pdf
from services.rendering.source.redaction import redact_source_text_areas
from services.rendering.source.vector_text import collect_vector_text_rects

try:
    from rendering_bridge import build_clean_background_pdf as _native_build_clean_background_pdf

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
    )


def _native_eligible(page_specs, visual_profile) -> bool:
    """True when the native stage can take the call: `page_specs=None` and
    `visual_profile=None` (the only non-ported paths), and the Python stage is
    not instrumented. Formula-guard protection and vector-text collection are
    full ports, so no per-page fallback remains."""
    if page_specs or visual_profile is not None:
        return False
    if _python_stage_instrumented():
        return False
    return True


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
    if not NATIVE:
        return _build_clean_background_pdf_python(
            source_pdf_path=source_pdf_path,
            translated_pages=translated_pages,
            output_pdf_path=output_pdf_path,
            redaction_strategy=redaction_strategy,
            page_specs=page_specs,
            source_text_precleaned_page_indices=source_text_precleaned_page_indices,
            visual_profile=visual_profile,
        )
    if not _native_eligible(page_specs, visual_profile):
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
    translated_json = json.dumps({str(k): v for k, v in sorted(translated_pages.items())})
    precleaned_json = json.dumps(sorted(source_text_precleaned_page_indices))
    out_bytes = _native_build_clean_background_pdf(
        source_bytes,
        translated_json,
        redaction_strategy,
        precleaned_json,
    )
    output_pdf_path.parent.mkdir(parents=True, exist_ok=True)
    output_pdf_path.write_bytes(out_bytes)
    return output_pdf_path
