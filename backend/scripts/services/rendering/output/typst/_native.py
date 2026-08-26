"""Optional native (Rust) backend for the Typst output layer.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes the Typst source
emitter through the ported Rust implementation. Without the native module every
function falls back to the pure-Python implementation, so importing this module
is always safe.

Native output is identical to the Python emitter modulo numeric literal
formatting (e.g. `260pt` vs `260.0pt`); both parse to the same Typst value.
"""

from __future__ import annotations

import json
from pathlib import Path

from foundation.config import fonts
from services.rendering.output.typst.emitter import build_typst_source_from_page_specs

try:
    from rendering_bridge import clean_background as _native_clean_background
    from rendering_bridge import emit_typst_source as _native_emit_typst_source

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def _block_to_dict(block) -> dict:
    text_color = tuple(block.text_color or (0, 0, 0))
    cover_fill = tuple(block.cover_fill or (1, 1, 1))
    return {
        "block_id": block.block_id,
        "page_index": block.page_index,
        "background_rect": list(block.background_rect),
        "content_rect": list(block.content_rect),
        "content_kind": block.content_kind,
        "content_text": block.content_text,
        "plain_text": block.plain_text,
        "math_map": block.math_map or [],
        "font_size_pt": block.font_size_pt,
        "leading_em": block.leading_em,
        "font_weight": block.font_weight,
        "fit_to_box": block.fit_to_box,
        "fit_single_line": block.fit_single_line,
        "fit_min_font_size_pt": block.fit_min_font_size_pt,
        "fit_max_font_size_pt": block.fit_max_font_size_pt,
        "fit_min_leading_em": block.fit_min_leading_em,
        "fit_max_height_pt": block.fit_max_height_pt,
        "fit_target_width_pt": block.fit_target_width_pt,
        "fit_target_height_pt": block.fit_target_height_pt,
        "fit_shift_up_pt": block.fit_shift_up_pt,
        "first_line_indent_pt": block.first_line_indent_pt,
        "justify_text": block.justify_text,
        "text_color": [text_color[0], text_color[1], text_color[2]],
        "cover_fill": [cover_fill[0], cover_fill[1], cover_fill[2]],
        "use_cover_fill": block.use_cover_fill,
        "skip_reason": block.skip_reason,
        "preserve_line_breaks": block.preserve_line_breaks,
        "preserved_line_boxes": block.preserved_line_boxes or [],
        "toc_entries": block.toc_entries or [],
    }


def _page_spec_to_dict(spec) -> dict:
    return {
        "page_index": spec.page_index,
        "page_width_pt": spec.page_width_pt,
        "page_height_pt": spec.page_height_pt,
        "background_pdf_path": str(spec.background_pdf_path) if spec.background_pdf_path else None,
        "blocks": [_block_to_dict(b) for b in spec.blocks],
    }


def emit_typst_source(
    *,
    background_pdf_path: Path,
    page_specs: list,
    work_dir: Path,
    font_family: str = fonts.TYPST_DEFAULT_FONT_FAMILY,
) -> str:
    """`emitter.build_typst_source_from_page_specs`, routed to the native Rust
    emitter when the module is built; otherwise the pure-Python emitter."""
    if NATIVE:
        payload = json.dumps([_page_spec_to_dict(spec) for spec in page_specs])
        return _native_emit_typst_source(
            payload,
            str(background_pdf_path),
            str(work_dir),
            font_family,
        )
    return build_typst_source_from_page_specs(
        background_pdf_path=background_pdf_path,
        page_specs=page_specs,
        work_dir=work_dir,
        font_family=font_family,
    )
