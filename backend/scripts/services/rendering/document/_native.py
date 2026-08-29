"""Optional native (Rust) backend for `document.metadata` TOC copy.

`copy_toc` / `copy_toc_for_page_map` route through `rendering_bridge` when the
native module is built. The Rust side reads the source outline, remaps pages,
and re-writes the target outline in one pass, returning the target bytes plus
the entry count. A zero count means the target was left unchanged, so the shim
returns the caller's document untouched (no swap); otherwise a fresh document
re-opened from the native bytes is returned — the caller applies the established
swap pattern (like `show_pdf_page_on_doc`).
"""

from __future__ import annotations

import fitz

try:
    from rendering_bridge import copy_toc as _native_copy_toc
    from rendering_bridge import copy_toc_for_page_map as _native_copy_toc_for_page_map

    NATIVE = True
except ImportError:  # pragma: no cover - native build not present
    NATIVE = False


def copy_toc(
    source_doc: fitz.Document,
    target_doc: fitz.Document,
    *,
    start_page: int = 0,
    end_page: int | None = None,
) -> fitz.Document:
    """Native `metadata.copy_toc`. Returns the caller's `target_doc` when the
    outline was empty/unchanged (no swap); a fresh document otherwise."""
    result, count = _native_copy_toc(
        source_doc.tobytes(),
        target_doc.tobytes(),
        int(start_page or 0),
        int(end_page if end_page is not None else -1),
    )
    if count <= 0:
        return target_doc
    return fitz.open(stream=result, filetype="pdf")


def copy_toc_for_page_map(
    source_doc: fitz.Document,
    target_doc: fitz.Document,
    *,
    source_page_indices: list[int],
) -> fitz.Document:
    """Native `metadata.copy_toc_for_page_map`. Returns the caller's
    `target_doc` when the outline was empty/unchanged (no swap); a fresh
    document otherwise."""
    result, count = _native_copy_toc_for_page_map(
        source_doc.tobytes(),
        target_doc.tobytes(),
        [int(i) for i in source_page_indices],
    )
    if count <= 0:
        return target_doc
    return fitz.open(stream=result, filetype="pdf")
