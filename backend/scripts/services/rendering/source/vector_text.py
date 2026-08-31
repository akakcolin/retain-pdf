"""Vector-text cleanup read (the drawing classifier), routed to the native
bridge (`source._native`).

Native-only: `collect_vector_text_rects` raises when the bridge is unavailable
or the page is in-memory. The pure-Python classifier
(`_looks_like_black_filled_glyph` / `_collect_vector_text_rects_python`) was
retired with the IN_MEMORY_PAGE boundary; the Rust classifier is the only
implementation (mupdf always RGB-converts fills, and only the native
`cs.n() == 3` gate matches Python's `len(fill) == 3`).
"""

from __future__ import annotations

from services.rendering.source import _native


def collect_vector_text_rects(page, target_rects):
    return _native.collect_vector_text_rects(page=page, target_rects=target_rects)
