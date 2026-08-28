from __future__ import annotations

import fitz

from services.rendering.source_cleanup.pdf.constants import BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD


def page_has_form_xobjects(page: fitz.Page) -> bool:
    try:
        return bool(page.get_xobjects())
    except Exception:
        return True


def page_content_stream_size(doc: fitz.Document, page: fitz.Page) -> int:
    try:
        content_xrefs = page.get_contents() or []
    except Exception:
        return 0
    total = 0
    for xref in content_xrefs:
        try:
            total += len(doc.xref_stream(xref) or b"")
        except Exception:
            continue
        if total >= BBOX_TEXT_STRIP_CONTENT_STREAM_SIZE_THRESHOLD:
            return total
    return total
