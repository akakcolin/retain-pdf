"""Optional native (Rust) backend for the layout source-page-size read.

Build the pyo3 module with maturin from `backend/rendering_bridge` and
`import rendering_bridge` succeeds; this module then routes
`read_source_page_sizes` (the fitz `page.rect` size lookup used by
`build_render_page_specs`) through the Rust `rendering_bridge.read_page_sizes`.
`read_source_page_sizes` is native-only: when the native path is unavailable
(routing gate off) the call raises instead of falling back to a Python
reference, so the fitz size read is fully retired.
"""

from __future__ import annotations

import json
from pathlib import Path

from services.rendering import _routing

try:
    from rendering_bridge import read_page_sizes as _native_read_page_sizes

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def read_source_page_sizes(*, source_pdf_path: Path, page_indices: list[int]) -> dict[int, tuple[float, float]]:
    """`page_specs.build_render_page_specs`' source-page size lookup, routed to
    the native bridge. Native-only: without the bridge the call fails closed."""
    if not _routing.routed("layout", "read_source_page_sizes", NATIVE):
        raise RuntimeError("layout.read_source_page_sizes is native-only: the rendering_bridge read_page_sizes is required")
    try:
        raw = json.loads(_native_read_page_sizes(source_pdf_path.read_bytes(), json.dumps(page_indices)))
    except Exception:
        _routing.record_fallback("layout", "read_source_page_sizes", _routing.FallbackReason.NATIVE_BRIDGE_ERROR)
        raise
    _routing.record_native_hit("layout", "read_source_page_sizes")
    return {
        int(idx): (rect[2] - rect[0], rect[3] - rect[1])
        for idx, rect in raw.items()
    }
