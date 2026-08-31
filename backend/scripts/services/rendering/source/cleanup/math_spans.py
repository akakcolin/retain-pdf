"""Math-font text-read primitives routed to the native bridge (`source._native`).

Native-only: `collect_page_math_protection_rects` /
`collect_page_non_math_span_heights` raise when the bridge is unavailable or
the page is in-memory (the pure-Python `_collect_*_python` references were
retired with the IN_MEMORY_PAGE boundary).

The `_native` import is lazy (circular-import gotcha shared with
`detect`/`text_extract`).
"""

from __future__ import annotations


def collect_page_math_protection_rects(page):
    from services.rendering.source import _native

    return _native.collect_page_math_protection_rects(page=page)


def collect_page_non_math_span_heights(page):
    from services.rendering.source import _native

    return _native.collect_page_non_math_span_heights(page=page)
