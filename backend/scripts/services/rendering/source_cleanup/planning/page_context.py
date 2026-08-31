"""Page-scoped context for source-cleanup planning.

`PlanningPageContext` bundles every primitive the planning path consumes per
page — page rect, bboxlog entries, content-stream size, form-xobject flag, and
the inverse page transformation matrix — so `planner` (and its resolver / items
helpers) can run from a context instead of a live fitz.Page. The native bridge
(`_native.build_page_contexts`) builds contexts with mupdf-rs; the shared per-page
helper `_build_context_from_fitz` here backs the fitz planner wrappers and tests.
Page rect and bboxlog entries are pure `Rect` / `Matrix` (attribute-compatible
with fitz) so both paths carry the same data types and downstream math is identical.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field

import fitz

from services.rendering.source.rects import Matrix
from services.rendering.source.rects import Rect
from services.rendering.source.rects import coerce
from services.rendering.source.rects import inverse_affine
from services.rendering.source_cleanup.planning.page_probe import page_content_stream_size
from services.rendering.source_cleanup.planning.page_probe import page_has_form_xobjects


@dataclass(frozen=True)
class PlanningPageContext:
    page_index: int
    page_rect: Rect
    bboxlog_entries: tuple[tuple[str, Rect], ...] = ()
    content_stream_size: int = 0
    has_form_xobjects: bool = False
    inverse_ctm: Matrix = field(default_factory=Matrix)


def inverse_ctm_from_ctm(ctm: object) -> Matrix:
    """`~page.transformation_matrix` from a raw `[a,b,c,d,e,f]` ctm.

    Returns a pure `Matrix` (attribute-compatible with `fitz.Matrix`, so
    consumers reading `.a`..`.f` keep working) computed with the same inverse
    formula fitz uses for `page.transformation_matrix`, so the native path (ctm
    from the bridge) and the reference path (fitz page) agree numerically.
    """
    return inverse_affine(ctm)


def decode_bboxlog_entries(raw: object) -> tuple[tuple[str, Rect], ...]:
    """Normalize bboxlog entries to `(kind, Rect)` tuples.

    Accepts both the bridge shape (`[kind, [x0, y0, x1, y1]]`) and the fitz
    shape (`(kind, (x0, y0, x1, y1))`). Entries whose rect is empty (including
    the move-only-path sentinel) are dropped, matching the reference consumers
    (`page_bboxlog_rect_groups` / `bboxlog_rect`).
    """
    entries: list[tuple[str, Rect]] = []
    for entry in raw or ():
        kind = _entry_kind(entry)
        rect = _entry_rect(entry)
        if kind is None or rect is None:
            continue
        entries.append((kind, rect))
    return tuple(entries)


def _entry_kind(entry: object) -> str | None:
    try:
        kind = str(entry[0]).strip()
    except Exception:
        return None
    return kind or None


def _entry_rect(entry: object) -> Rect | None:
    try:
        value = entry[1]
        rect = Rect(*(float(item) for item in value))
    except Exception:
        return None
    return None if rect.is_empty else rect


def _build_context_from_fitz(doc: fitz.Document | None, page: fitz.Page) -> PlanningPageContext:
    """Reference context builder. `doc` supplies content-stream size and the
    form-xobject flag; `doc=None` yields the geometry-only context used by
    wrappers that only need ctm / bboxlog / rect."""
    try:
        bboxlog = page.get_bboxlog()
    except Exception:
        bboxlog = ()
    if doc is not None:
        content_stream_size = page_content_stream_size(doc, page)
        has_form_xobjects = page_has_form_xobjects(page)
    else:
        content_stream_size = 0
        has_form_xobjects = False
    return PlanningPageContext(
        page_index=page.number,
        page_rect=coerce(page.rect),
        bboxlog_entries=decode_bboxlog_entries(bboxlog),
        content_stream_size=content_stream_size,
        has_form_xobjects=has_form_xobjects,
        inverse_ctm=inverse_ctm_from_ctm(page.transformation_matrix),
    )
