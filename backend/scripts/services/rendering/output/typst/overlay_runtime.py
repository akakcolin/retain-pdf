from __future__ import annotations

from pathlib import Path
import re

import fitz

from services.rendering.output.typst.compiler import TypstCompileError
from services.rendering.output.typst.overlay_source_cache import PAGE_SIZE_TOLERANCE_PT


OVERLAY_STEM_RE = re.compile(r"\bbook-overlay-(\d{3,})\b")
OVERLAY_BLOCK_ID_RE = re.compile(r"\bp(\d+)_")


def can_use_pikepdf_book_overlay(
    *,
    apply_source_overlay: bool,
    use_typst_overlay_fill_only: bool,
    source_cleanup_strategy: str,
    source_text_precleaned_page_indices: frozenset[int],
    ordered_page_indices: list[int],
    translated_pages: dict[int, list[dict]],
) -> bool:
    if apply_source_overlay:
        return False
    if not ordered_page_indices:
        return False
    if use_typst_overlay_fill_only:
        return True
    return all(
        page_idx in source_text_precleaned_page_indices or not translated_pages.get(page_idx)
        for page_idx in ordered_page_indices
    )


def extract_failed_overlay_indices(
    exc: BaseException,
    page_specs: list[tuple[int, float, float, list[dict], str]],
) -> set[int]:
    details = str(exc)
    if isinstance(exc, TypstCompileError):
        details = "\n".join(
            part
            for part in (
                exc.stderr,
                exc.stdout,
                str(exc),
            )
            if part
        )

    candidates: set[int] = set()
    for match in OVERLAY_STEM_RE.finditer(details):
        candidates.add(int(match.group(1)))
    for match in OVERLAY_BLOCK_ID_RE.finditer(details):
        candidates.add(int(match.group(1)))

    max_index = len(page_specs) - 1
    return {index for index in candidates if 0 <= index <= max_index}


def overlay_pdf_size_mismatches(
    doc: fitz.Document | None,
    ordered_page_indices: list[int],
    overlay_pdf_path: Path,
    *,
    source_pdf_path: Path | None = None,
) -> list[dict[str, object]]:
    mismatches: list[dict[str, object]] = []
    if doc is None:
        if source_pdf_path is None:
            raise ValueError("overlay_pdf_size_mismatches requires a doc or a source_pdf_path")
        from services.rendering.layout._native import read_source_page_sizes

        source_sizes = read_source_page_sizes(
            source_pdf_path=source_pdf_path,
            page_indices=list(ordered_page_indices),
        )
        overlay_sizes = read_source_page_sizes(
            source_pdf_path=overlay_pdf_path,
            page_indices=list(range(len(ordered_page_indices))),
        )
        overlay_count = len(ordered_page_indices)
    else:
        source_sizes = {
            page_idx: (float(doc[page_idx].rect.width), float(doc[page_idx].rect.height))
            for page_idx in ordered_page_indices
        }
        overlay_doc = fitz.open(overlay_pdf_path)
        try:
            overlay_sizes = {
                overlay_page_idx: (
                    float(overlay_doc[overlay_page_idx].rect.width),
                    float(overlay_doc[overlay_page_idx].rect.height),
                )
                for overlay_page_idx in range(len(ordered_page_indices))
            }
            overlay_count = len(overlay_doc)
        finally:
            overlay_doc.close()
    for overlay_page_idx, page_idx in enumerate(ordered_page_indices):
        if overlay_page_idx >= overlay_count:
            mismatches.append(
                {
                    "page_index": page_idx,
                    "overlay_page_index": overlay_page_idx,
                    "reason": "overlay_page_missing",
                }
            )
            continue
        source_w, source_h = source_sizes[page_idx]
        overlay_w, overlay_h = overlay_sizes[overlay_page_idx]
        if (
            abs(source_w - overlay_w) > PAGE_SIZE_TOLERANCE_PT
            or abs(source_h - overlay_h) > PAGE_SIZE_TOLERANCE_PT
        ):
            mismatches.append(
                {
                    "page_index": page_idx,
                    "overlay_page_index": overlay_page_idx,
                    "source_page_width_pt": round(source_w, 3),
                    "source_page_height_pt": round(source_h, 3),
                    "overlay_page_width_pt": round(overlay_w, 3),
                    "overlay_page_height_pt": round(overlay_h, 3),
                }
            )
    return mismatches


__all__ = [
    "can_use_pikepdf_book_overlay",
    "extract_failed_overlay_indices",
    "overlay_pdf_size_mismatches",
]
