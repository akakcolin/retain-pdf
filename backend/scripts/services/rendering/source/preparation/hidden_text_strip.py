from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import fitz

from services.rendering.source.document_ops import page_is_pseudo_editable_scan


TEXT_SHOW_OPERATORS = {"Tj", "TJ", "'", '"'}


@dataclass(frozen=True)
class HiddenTextStripResult:
    changed: bool
    output_pdf_path: Path | None = None
    pages_changed: int = 0
    text_objects_removed: int = 0


def _page_hidden_text_scan_candidate(page: fitz.Page) -> bool:
    return page_is_pseudo_editable_scan(page)


def _collect_hidden_text_scan_pages(
    pdf_path: Path,
    *,
    start_page: int = 0,
    end_page: int = -1,
) -> set[int]:
    doc = fitz.open(pdf_path)
    try:
        if not doc:
            return set()
        start = max(0, start_page)
        stop = len(doc) - 1 if end_page < 0 else min(end_page, len(doc) - 1)
        if start > stop:
            return set()
        return {
            page_idx
            for page_idx in range(start, stop + 1)
            if _page_hidden_text_scan_candidate(doc[page_idx])
        }
    finally:
        doc.close()


def _analyze_text_object_visibility(
    text_ops: list[tuple],
    *,
    initial_render_mode: int = 0,
) -> tuple[bool, int]:
    render_mode = initial_render_mode
    saw_text_show = False
    all_text_show_is_hidden = True
    for operands, operator in text_ops:
        op = str(operator)
        if op == "Tr" and operands:
            try:
                render_mode = int(operands[0])
            except Exception:
                render_mode = 0
        if op in TEXT_SHOW_OPERATORS:
            saw_text_show = True
            if render_mode != 3:
                all_text_show_is_hidden = False
    return saw_text_show and all_text_show_is_hidden, render_mode


def _text_object_is_hidden(text_ops: list[tuple]) -> bool:
    hidden, _final_render_mode = _analyze_text_object_visibility(text_ops)
    return hidden


def build_hidden_text_stripped_pdf_copy(
    source_pdf_path: Path,
    output_pdf_path: Path,
    *,
    start_page: int = 0,
    end_page: int = -1,
) -> HiddenTextStripResult:
    """Fitz pre-scan (candidate pages) then delegate the per-page strip to the
    native bridge shim; the pre-scan stays Python because
    `page_is_pseudo_editable_scan` is a hard-boundary fitz primitive."""
    candidate_pages = _collect_hidden_text_scan_pages(
        source_pdf_path,
        start_page=start_page,
        end_page=end_page,
    )
    if not candidate_pages:
        return HiddenTextStripResult(changed=False)
    from services.rendering.source import _native

    return _native.build_hidden_text_stripped_pdf_copy(
        candidate_pages,
        source_pdf_path=source_pdf_path,
        output_pdf_path=output_pdf_path,
    )
