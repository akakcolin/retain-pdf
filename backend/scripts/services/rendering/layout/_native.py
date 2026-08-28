"""Optional native (Rust) backend for the layout source-page-size read.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`read_source_page_sizes` (the fitz `page.rect` size lookup used by
`build_render_page_specs`) through the Rust `rendering_bridge.read_page_sizes`.
Without the native module every call falls back to the pure-Python reference
`page_specs._read_source_page_sizes_python`, so importing this module is always
safe.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering.layout.page_specs import _read_source_page_sizes_python

try:
    from rendering_bridge import read_page_sizes as _native_read_page_sizes

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def read_source_page_sizes(*, source_pdf_path: Path, page_indices: list[int]) -> dict[int, tuple[float, float]]:
    """`page_specs.build_render_page_specs`' source-page size lookup, routed to
    the native bridge when built; otherwise the pure-Python reference."""
    if not NATIVE:
        return _read_source_page_sizes_python(
            source_pdf_path=source_pdf_path,
            page_indices=page_indices,
        )
    raw = json.loads(_native_read_page_sizes(source_pdf_path.read_bytes(), json.dumps(page_indices)))
    return {
        int(idx): (rect[2] - rect[0], rect[3] - rect[1])
        for idx, rect in raw.items()
    }
